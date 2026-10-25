//! Sign-in: OAuth 2.0 authorization code + PKCE in the system browser with a
//! loopback redirect (RFC 8252 / RFC 7636). The launcher is a public client:
//! it holds no client secret. The refresh token lives in Windows Credential
//! Manager; the access token only in memory.

use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use zirtola_core::api::{ApiClient, ServiceConfig, TokenResponse};
use zirtola_core::b64;

const KEYRING_SERVICE: &str = "Zirtola Launcher";

fn keyring_entry(api_base: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("refresh-token@{api_base}"))
}

pub fn load_refresh_token(api_base: &str) -> Option<String> {
    keyring_entry(api_base).ok()?.get_password().ok()
}

pub fn store_refresh_token(api_base: &str, token: &str) {
    if let Ok(e) = keyring_entry(api_base) {
        if let Err(err) = e.set_password(token) {
            log::warn!("could not store refresh token: {err}");
        }
    }
}

pub fn clear_refresh_token(api_base: &str) {
    if let Ok(e) = keyring_entry(api_base) {
        let _ = e.delete_credential();
    }
}

fn random_b64(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    b64::encode_url(&buf)
}

pub struct Tokens {
    pub access_token: String,
    pub expires_at: Instant,
    pub refresh_token: Option<String>,
}

impl From<TokenResponse> for Tokens {
    fn from(t: TokenResponse) -> Self {
        Tokens {
            access_token: t.access_token,
            expires_at: Instant::now() + Duration::from_secs(t.expires_in.max(30)),
            refresh_token: t.refresh_token,
        }
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

const DONE_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Zirtola</title>
<style>body{margin:0;height:100vh;display:grid;place-items:center;background:#0f1216;color:#e8ecf1;font:16px system-ui,Segoe UI,sans-serif}
.c{text-align:center}.m{width:56px;height:56px;margin:0 auto 18px;border-radius:14px;background:#e3a33b;color:#0f1216;font:800 32px/56px system-ui}</style></head>
<body><div class="c"><div class="m">Z</div><h2>__TITLE__</h2><p>__BODY__</p></div></body></html>"#;

fn page(title: &str, body: &str) -> String {
    let html = DONE_PAGE
        .replace("__TITLE__", title)
        .replace("__BODY__", body);
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{}",
        html.len(),
        html
    )
}

/// Run the full browser sign-in and return fresh tokens.
pub async fn sign_in_browser(
    api: &ApiClient,
    cfg: &ServiceConfig,
    cancel: CancellationToken,
) -> anyhow::Result<Tokens> {
    let verifier = random_b64(48);
    let challenge = b64::encode_url(&Sha256::digest(verifier.as_bytes()));
    let state = random_b64(24);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut auth = url::Url::parse(&cfg.authorization_endpoint)?;
    auth.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &cfg.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", &cfg.scopes.join(" "))
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    if !matches!(auth.scheme(), "https" | "http") {
        anyhow::bail!("authorization endpoint must be http(s)");
    }
    if crate::state::dev_mode() && std::env::var_os("ZIRTOLA_NO_BROWSER").is_some() {
        // Development: print the URL instead of opening it, so a test browser can use it.
        eprintln!("ZIRTOLA_AUTHORIZE_URL={auth}");
    } else {
        open::that_detached(auth.as_str())?;
    }

    let deadline = tokio::time::sleep(Duration::from_secs(600));
    tokio::pin!(deadline);
    let code = loop {
        let (mut sock, _) = tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("sign-in cancelled"),
            _ = &mut deadline => anyhow::bail!("sign-in timed out"),
            r = listener.accept() => r?,
        };
        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(Duration::from_secs(10), sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let target = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("");
        let Some(query) = target.strip_prefix("/callback?") else {
            let _ = sock
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        if query_param(query, "state").as_deref() != Some(state.as_str()) {
            let _ = sock
                .write_all(page("Sign-in failed", "The response did not match this sign-in attempt. Try again from the launcher.").as_bytes())
                .await;
            continue;
        }
        if let Some(err) = query_param(query, "error") {
            let _ = sock
                .write_all(
                    page(
                        "Sign-in cancelled",
                        "You can close this tab and return to the launcher.",
                    )
                    .as_bytes(),
                )
                .await;
            anyhow::bail!("sign-in was not completed ({err})");
        }
        let Some(code) = query_param(query, "code") else {
            continue;
        };
        let _ = sock
            .write_all(
                page(
                    "You're signed in",
                    "You can close this tab and return to the Zirtola Launcher.",
                )
                .as_bytes(),
            )
            .await;
        let _ = sock.shutdown().await;
        break code;
    };

    let t = api
        .token(
            &cfg.token_endpoint,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", &redirect_uri),
                ("client_id", &cfg.client_id),
                ("code_verifier", &verifier),
            ],
        )
        .await?;
    Ok(t.into())
}

pub async fn refresh(
    api: &ApiClient,
    cfg: &ServiceConfig,
    refresh_token: &str,
) -> Result<Tokens, zirtola_core::api::ApiError> {
    let t = api
        .token(
            &cfg.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &cfg.client_id),
            ],
        )
        .await?;
    let mut tokens: Tokens = t.into();
    // Servers that do not rotate refresh tokens omit it; keep the old one.
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.to_string());
    }
    Ok(tokens)
}
