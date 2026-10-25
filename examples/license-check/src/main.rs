//! Reference license check, and the stand-in "game" inside the mock builds.
//!
//! A real game does the same thing at startup (an Unreal version lives in
//! `docs/GAME_INTEGRATION.md`):
//!
//! 1. Read `ZIRTOLA_SESSION_TOKEN` and `ZIRTOLA_GAME_ID` from the environment.
//! 2. Verify the token's EdDSA signature with the website's `launch` public key
//!    (pinned in the game; this demo fetches it from a loopback API only).
//! 3. Check `aud`/`game` and `exp`. If anything fails, show "Please start the
//!    game from the Zirtola Launcher" (or fall back to an offline grace period).

use zirtola_core::token::verify_launch_token;
use zirtola_core::TrustedKeys;

/// Production launch keys would be pinned here (JWKS JSON). Empty until the website exists.
const PINNED_LAUNCH_KEYS: &str = r#"{ "keys": [] }"#;

#[tokio::main]
async fn main() {
    let game_id = std::env::var("ZIRTOLA_GAME_ID").unwrap_or_else(|_| "unknown".into());
    let version = std::env::var("ZIRTOLA_GAME_VERSION").unwrap_or_default();
    println!("==============================================");
    println!(" Zirtola license check  |  {game_id} {version}");
    println!("==============================================");
    let result = match check(&game_id).await {
        Ok(msg) => format!("LICENSE OK: {msg}"),
        Err(e) => format!("NOT LICENSED: {e}"),
    };
    if result.starts_with("LICENSE OK") {
        println!("\n {result}");
    } else {
        println!("\n {result}\n Please start the game from the Zirtola Launcher.");
    }
    // Leave a trace for automated tests (the console may close immediately).
    let log = std::env::temp_dir().join("zirtola-license-check.log");
    let _ = std::fs::write(
        &log,
        format!(
            "{} {game_id} {version}: {result}\n",
            chrono::Utc::now().to_rfc3339()
        ),
    );
    if std::env::args().any(|a| a == "--no-wait") {
        return;
    }
    println!("\n(This window stands in for the game. Press Enter to quit.)");
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}

async fn check(game_id: &str) -> anyhow::Result<String> {
    let token = std::env::var("ZIRTOLA_SESSION_TOKEN")
        .map_err(|_| anyhow::anyhow!("no session token (not started by the launcher)"))?;
    let mut keys = TrustedKeys::from_json(PINNED_LAUNCH_KEYS)?;
    if let Ok(base) = std::env::var("ZIRTOLA_API_BASE") {
        let api = zirtola_core::api::ApiClient::new(&base)?;
        if api.is_loopback() {
            keys.add_set(&api.keys().await?)?;
        }
    }
    let now = chrono::Utc::now().timestamp();
    let c = verify_launch_token(&token, &keys, game_id, now, 120)?;
    Ok(format!(
        "account {} ({}), entitlement '{}', token valid for {} more seconds",
        c.sub,
        c.name.unwrap_or_default(),
        c.ent,
        c.exp - now
    ))
}
