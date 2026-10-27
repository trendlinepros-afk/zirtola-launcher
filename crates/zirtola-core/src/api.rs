//! Typed client for the Zirtola website API. The contract is documented in
//! `docs/API.md`; `mock-server/` implements it for local development.

use crate::keys::KeySet;
use crate::manifest::SignedManifest;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

pub const USER_AGENT: &str = concat!("ZirtolaLauncher/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub device_authorization_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub store_url: String,
    #[serde(default)]
    pub account_url: Option<String>,
    #[serde(default)]
    pub support_url: Option<String>,
    /// Launchers older than this must self-update before signing in.
    #[serde(default)]
    pub min_launcher_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameArt {
    /// Accent colour for the card, `#rrggbb`.
    pub accent: String,
    /// Optional cover image URL (the launcher ships built-in art for known games).
    #[serde(default)]
    pub cover_url: Option<String>,
}

/// A price in minor units (cents), e.g. `{ "amount": 1999, "currency": "usd" }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub amount: i64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogGame {
    pub id: String,
    pub title: String,
    pub tagline: String,
    pub description: String,
    /// `released`, `early_access`, `coming_soon`.
    pub status: String,
    pub store_url: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub install_size: Option<u64>,
    pub art: GameArt,
    /// Shown on the "Buy on the website" button when the website provides it.
    #[serde(default)]
    pub price: Option<Price>,
    /// `Some(false)`: priced but not on sale yet ("Coming soon").
    #[serde(default)]
    pub sales_open: Option<bool>,
    /// The website's checkout page for this game, if it sells it directly.
    #[serde(default)]
    pub checkout_url: Option<String>,
    /// Store-page bullet points.
    #[serde(default)]
    pub features: Vec<String>,
    /// Still in early access: the game page offers "Report a bug".
    #[serde(default)]
    pub early_access: Option<bool>,
    /// The game's bug-report forum on the website.
    #[serde(default)]
    pub bug_report_url: Option<String>,
    /// Link to the website's system requirements, with a one-line minimum.
    #[serde(default)]
    pub requirements: Option<Requirements>,
    /// Where the game keeps saves and settings, outside the install folder,
    /// e.g. `%LOCALAPPDATA%\GranitePit\Saved`. Shown when uninstalling.
    #[serde(default)]
    pub save_location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirements {
    pub url: String,
    #[serde(default)]
    pub minimum_target: Option<String>,
    #[serde(default)]
    pub minimum_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub games: Vec<CatalogGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entitlement {
    pub game_id: String,
    /// `purchase`, `key` (redeemed key), `grant` (studio granted).
    pub kind: String,
    pub granted_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default = "default_branches")]
    pub branches: Vec<String>,
    /// Add-on (DLC) skus owned for this game; never needed to install it.
    #[serde(default)]
    pub addons: Vec<String>,
}

fn default_branches() -> Vec<String> {
    vec!["live".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entitlements {
    pub entitlements: Vec<Entitlement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResult {
    pub entitlement: Entitlement,
    pub game_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestBuild {
    pub game_id: String,
    pub version: String,
    pub build_id: String,
    pub total_size: u64,
    pub released_at: String,
    #[serde(default)]
    pub notes: Option<String>,
    /// Installs older than this must update before playing (newest required release).
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub mandatory_reason: Option<String>,
}

/// -1/0/1 for dotted numeric versions ("1.10.0" > "1.9.3"); a pre-release sorts first.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        (core.split('.').map(|n| n.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>(), pre)
    };
    let (x, xp) = parse(a);
    let (y, yp) = parse(b);
    for i in 0..x.len().max(y.len()) {
        let o = x.get(i).copied().unwrap_or(0).cmp(&y.get(i).copied().unwrap_or(0));
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    match (xp, yp) {
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod version_tests {
    use super::compare_versions;
    use std::cmp::Ordering::*;
    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("1.10.0", "1.9.3"), Greater);
        assert_eq!(compare_versions("0.2.0", "0.2"), Equal);
        assert_eq!(compare_versions("1.0.0-beta.1", "1.0.0"), Less);
        assert_eq!(compare_versions("0.1.9", "0.2.0"), Less);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResponse {
    pub version: String,
    pub signed_manifest: SignedManifest,
    /// Chunk URL template containing `{hash}` (and optionally `{prefix}`, the
    /// first two hex chars). May carry a short-lived access token.
    pub chunk_url_template: String,
    pub chunk_url_expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchToken {
    pub token: String,
    pub expires_at: String,
}

/// One run of a game, as reported to `POST /v1/playtime`. The values are
/// running totals for the run, so re-sending a report is always safe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaytimeReport {
    pub game_id: String,
    /// The launcher's own id for this run.
    pub session_id: String,
    /// RFC 3339.
    pub started_at: String,
    /// Time the game was in front and the player gave input recently.
    pub active_seconds: u64,
    /// Wall time since the game started.
    pub total_seconds: u64,
    pub ended: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GamePlaytime {
    pub game_id: String,
    pub active_seconds: u64,
    #[serde(default)]
    pub total_seconds: u64,
    /// `active_seconds` rounded down to the quarter hour.
    #[serde(default)]
    pub hours: f64,
    #[serde(default)]
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Playtime {
    #[serde(default)]
    pub games: Vec<GamePlaytime>,
    /// Per-report outcome of a POST (absent on GET).
    #[serde(default)]
    pub results: Vec<PlaytimeResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaytimeResult {
    pub session_id: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// Hours played in quarter-hour steps, rounded down (899 s = 0, 900 s = 0.25),
/// the same rounding the website uses.
pub fn quarter_hours(seconds: u64) -> f64 {
    (seconds / 900) as f64 / 4.0
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("{message}")]
    Api {
        status: u16,
        code: String,
        message: String,
        /// The full error body (e.g. `active_devices` for `device_conflict`).
        details: Option<serde_json::Value>,
    },
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Api { status, .. } => Some(*status),
            ApiError::Network(e) => e.status().map(|s| s.as_u16()),
        }
    }
    pub fn code(&self) -> &str {
        match self {
            ApiError::Api { code, .. } => code,
            ApiError::Network(_) => "network_error",
        }
    }
    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(401)
    }
}

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// True when the API is on this machine (mock server / local website).
    pub fn is_loopback(&self) -> bool {
        url::Url::parse(&self.base)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .map(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]")
            .unwrap_or(false)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn decode<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, ApiError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp.json::<T>().await?);
        }
        let code = status.as_u16();
        let text = resp.text().await.unwrap_or_default();
        let (error, message) = match serde_json::from_str::<ErrorBody>(&text) {
            Ok(b) => {
                let m = b.message.unwrap_or_else(|| b.error.clone());
                (b.error, m)
            }
            Err(_) => (format!("http_{code}"), format!("HTTP {code}")),
        };
        Err(ApiError::Api {
            status: code,
            code: error,
            message,
            details: serde_json::from_str(&text).ok(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
    ) -> Result<T, ApiError> {
        let mut req = self.http.get(self.url(path));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        Self::decode(req.send().await?).await
    }

    async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &B,
    ) -> Result<T, ApiError> {
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        Self::decode(req.send().await?).await
    }

    // ---- public ----

    pub async fn config(&self) -> Result<ServiceConfig, ApiError> {
        self.get("/v1/config", None).await
    }

    pub async fn keys(&self) -> Result<KeySet, ApiError> {
        self.get("/v1/keys", None).await
    }

    pub async fn catalog(&self) -> Result<Catalog, ApiError> {
        self.get("/v1/catalog", None).await
    }

    // ---- OAuth token endpoint (form-encoded per RFC 6749) ----

    pub async fn token(
        &self,
        token_endpoint: &str,
        form: &[(&str, &str)],
    ) -> Result<TokenResponse, ApiError> {
        let resp = self.http.post(token_endpoint).form(form).send().await?;
        Self::decode(resp).await
    }

    pub async fn revoke(
        &self,
        endpoint: &str,
        client_id: &str,
        token: &str,
    ) -> Result<(), ApiError> {
        let resp = self
            .http
            .post(endpoint)
            .form(&[("token", token), ("client_id", client_id)])
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Self::decode::<serde_json::Value>(resp).await.map(|_| ())
        }
    }

    // ---- authenticated ----

    pub async fn me(&self, at: &str) -> Result<Me, ApiError> {
        self.get("/v1/me", Some(at)).await
    }

    pub async fn entitlements(&self, at: &str) -> Result<Entitlements, ApiError> {
        self.get("/v1/entitlements", Some(at)).await
    }

    pub async fn redeem_key(&self, at: &str, key: &str) -> Result<RedeemResult, ApiError> {
        self.post(
            "/v1/keys/redeem",
            Some(at),
            &serde_json::json!({ "key": key }),
        )
        .await
    }

    pub async fn latest_build(
        &self,
        at: &str,
        game_id: &str,
        platform: &str,
        branch: &str,
    ) -> Result<LatestBuild, ApiError> {
        self.get(
            &format!("/v1/games/{game_id}/latest?platform={platform}&branch={branch}"),
            Some(at),
        )
        .await
    }

    pub async fn manifest(
        &self,
        at: &str,
        game_id: &str,
        platform: &str,
        branch: &str,
        version: Option<&str>,
    ) -> Result<ManifestResponse, ApiError> {
        let mut path = format!("/v1/games/{game_id}/manifest?platform={platform}&branch={branch}");
        if let Some(v) = version {
            path.push_str("&version=");
            path.push_str(v);
        }
        self.get(&path, Some(at)).await
    }

    /// Deactivate one of the account's devices (frees the licence slot).
    pub async fn deactivate_device(&self, at: &str, activation_id: &str) -> Result<(), ApiError> {
        self.post::<_, serde_json::Value>(
            "/v1/activations/deactivate",
            Some(at),
            &serde_json::json!({ "activation_id": activation_id }),
        )
        .await
        .map(|_| ())
    }

    /// The account's licence for a game, including its key ("View license key").
    /// `device_id` lets the website mark which activation is this PC.
    pub async fn license(&self, at: &str, game_id: &str, device_id: &str) -> Result<serde_json::Value, ApiError> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/games/{game_id}/license")))
            .bearer_auth(at)
            .header("X-Zirtola-Device", device_id)
            .send()
            .await?;
        Self::decode(resp).await
    }

    /// Release notes per version, newest first (public).
    pub async fn changelog(&self, game_id: &str, branch: &str) -> Result<serde_json::Value, ApiError> {
        self.get(&format!("/v1/games/{game_id}/changelog?branch={branch}"), None)
            .await
    }

    /// Report this PC's hardware (only when the player allows it).
    pub async fn post_hardware(
        &self,
        at: &str,
        report: &serde_json::Value,
    ) -> Result<(), ApiError> {
        self.post::<_, serde_json::Value>("/v1/hardware", Some(at), report)
            .await
            .map(|_| ())
    }

    /// Upload a crash report: JSON metadata plus a zip of the crash folder.
    pub async fn post_crash(
        &self,
        at: &str,
        metadata: &serde_json::Value,
        zip: Vec<u8>,
    ) -> Result<String, ApiError> {
        let mut form = reqwest::multipart::Form::new().text("metadata", metadata.to_string());
        if !zip.is_empty() {
            let part = reqwest::multipart::Part::bytes(zip)
                .file_name("crash.zip")
                .mime_str("application/zip")?;
            form = form.part("file", part);
        }
        let resp = self
            .http
            .post(self.url("/v1/crashes"))
            .bearer_auth(at)
            .timeout(Duration::from_secs(300))
            .multipart(form)
            .send()
            .await?;
        let v: serde_json::Value = Self::decode(resp).await?;
        Ok(v.get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string())
    }

    /// The player's hours per game.
    pub async fn playtime(&self, at: &str) -> Result<Playtime, ApiError> {
        self.get("/v1/playtime", Some(at)).await
    }

    /// Report runs (one or several queued while offline); returns the
    /// updated totals and which reports were accepted.
    pub async fn post_playtime(
        &self,
        at: &str,
        reports: &[PlaytimeReport],
    ) -> Result<Playtime, ApiError> {
        self.post(
            "/v1/playtime",
            Some(at),
            &serde_json::json!({ "sessions": reports }),
        )
        .await
    }

    /// Request a launch token. `device_id` is the launcher's random install id
    /// (the server stores only a hash) and `device_name` a friendly PC name;
    /// together they let the studio see and reset a license's devices.
    pub async fn launch_token(
        &self,
        at: &str,
        game_id: &str,
        device_id: &str,
        device_name: &str,
        version: &str,
        branch: &str,
    ) -> Result<LaunchToken, ApiError> {
        // The installed version and channel let the website refuse installs
        // older than a required (mandatory) release.
        self.post(
            "/v1/launch-tokens",
            Some(at),
            &serde_json::json!({ "game_id": game_id, "device_id": device_id, "device_name": device_name, "version": version, "branch": branch }),
        )
        .await
    }
}

#[cfg(test)]
mod playtime_tests {
    use super::*;

    #[test]
    fn quarter_hours_round_down() {
        assert_eq!(quarter_hours(0), 0.0);
        assert_eq!(quarter_hours(899), 0.0);
        assert_eq!(quarter_hours(900), 0.25);
        assert_eq!(quarter_hours(3_599), 0.75);
        assert_eq!(quarter_hours(3_600), 1.0);
        assert_eq!(quarter_hours(19_400), 5.25);
    }

    #[test]
    fn playtime_response_parses() {
        let p: Playtime = serde_json::from_str(
            r#"{"hours_step":0.25,"results":[{"session_id":"abcdefgh","ok":true}],
                "games":[{"game_id":"dirt-simulator","active_seconds":19400,"total_seconds":20400,"hours":5.25,"last_played_at":null}]}"#,
        )
        .unwrap();
        assert_eq!(p.games[0].hours, 5.25);
        assert!(p.results[0].ok);
    }
}
