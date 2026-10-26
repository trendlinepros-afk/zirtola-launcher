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

use zirtola_core::token::{verify_license, LicenseState};
use zirtola_core::TrustedKeys;

/// Production launch keys would be pinned here (JWKS JSON). Empty until the website exists.
const PINNED_LAUNCH_KEYS: &str = r#"{ "keys": [] }"#;

/// Test helper: with ZIRTOLA_TEST_CRASH=1 the stand-in game writes an
/// Unreal-style crash folder and exits with an error code, so the launcher's
/// crash reporting can be tried without a real crash.
fn fake_crash() -> ! {
    let exe = std::env::current_exe().ok();
    let project = exe
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Game".into());
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let guid = format!(
        "{:032X}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u128
    );
    let dir = base
        .join(&project)
        .join("Saved")
        .join("Crashes")
        .join(format!("UECC-Windows-{guid}_0000"));
    let _ = std::fs::create_dir_all(&dir);
    let error =
        "Unhandled Exception: EXCEPTION_ACCESS_VIOLATION reading address 0x0000000000000010";
    let stack = format!(
        "{project}!UExcavatorArm::TickHydraulics() [D:\\Build\\Source\\Machines\\ExcavatorArm.cpp:212]\n\
{project}!UHydraulicCylinder::ApplyForce() [D:\\Build\\Source\\Machines\\HydraulicCylinder.cpp:88]\n\
{project}!FPhysScene_Chaos::OnPhysicsStep()\n{project}!FEngineLoop::Tick()\nkernel32\nntdll"
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<FGenericCrashContext>\n<RuntimeProperties>\n\
<CrashGUID>UECC-Windows-{guid}_0000</CrashGUID>\n<GameName>UE-{project}</GameName>\n\
<BuildVersion>{}</BuildVersion>\n<ErrorMessage>{error}</ErrorMessage>\n<CallStack>{stack}</CallStack>\n\
</RuntimeProperties>\n</FGenericCrashContext>\n",
        std::env::var("ZIRTOLA_GAME_VERSION").unwrap_or_default()
    );
    let _ = std::fs::write(dir.join("CrashContext.runtime-xml"), xml);
    let _ = std::fs::write(
        dir.join(format!("{project}.log")),
        format!("[2026.10.25-12.00.00:000][  0]LogInit: Build: test\n[2026.10.25-12.00.05:000][310]LogWindows: Error: === Critical error: ===\n[2026.10.25-12.00.05:000][310]LogWindows: Error: {error}\n"),
    );
    // A small stand-in for the minidump (real ones are a few MB).
    let _ = std::fs::write(dir.join("UEMinidump.dmp"), vec![0x4d_u8; 256 * 1024]);
    eprintln!("wrote fake crash folder {}", dir.display());
    std::process::exit(3);
}

#[tokio::main]
async fn main() {
    if std::env::var_os("ZIRTOLA_TEST_CRASH").is_some() {
        fake_crash();
    }
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

/// Where the last good licence token is kept for offline play.
fn cache_path(game_id: &str) -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Zirtola").join("licences").join(format!("{game_id}.jwt"))
}

/// The licence check a game runs:
/// 1. Started by the launcher: verify the fresh token offline (signature, game,
///    expiry, clock skew) and keep it for offline play.
/// 2. Started without the launcher (or offline later): use the kept token while
///    its offline grace period (set by the website, 7 days by default) lasts.
/// 3. When online, also ask the website whether the licence is still owned and
///    this PC still activated; a deactivated PC stops at its next check.
async fn check(game_id: &str) -> anyhow::Result<String> {
    let mut keys = TrustedKeys::from_json(PINNED_LAUNCH_KEYS)?;
    let api = match std::env::var("ZIRTOLA_API_BASE") {
        Ok(base) => Some(zirtola_core::api::ApiClient::new(&base)?),
        Err(_) => None,
    };
    if let Some(api) = &api {
        if api.is_loopback() {
            keys.add_set(&api.keys().await?)?;
        }
    }
    let cache = cache_path(game_id);
    let token = match std::env::var("ZIRTOLA_SESSION_TOKEN") {
        Ok(t) => t,
        Err(_) => std::fs::read_to_string(&cache)
            .map_err(|_| anyhow::anyhow!("no licence on this PC (start the game from the launcher once)"))?,
    };
    let now = chrono::Utc::now().timestamp();
    let (c, state) = verify_license(&token, &keys, game_id, now, 120, 7 * 86_400)?;

    // Online re-check (best effort: offline play continues within the grace period).
    if let Some(api) = &api {
        let url = format!("{}/v1/launch-tokens/verify", api.base());
        if let Ok(resp) = api
            .http()
            .post(url)
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await
        {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if v.get("valid") == Some(&serde_json::Value::Bool(false)) {
                    let _ = std::fs::remove_file(&cache);
                    let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("licence not valid");
                    anyhow::bail!("the website says: {reason}");
                }
            }
        }
    }
    if state == LicenseState::Fresh {
        if let Some(dir) = cache.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&cache, &token);
    }
    Ok(match state {
        LicenseState::Fresh => format!(
            "account {} ({}), entitlement '{}', fresh token (valid {} more seconds, offline grace {} days)",
            c.sub,
            c.name.unwrap_or_default(),
            c.ent,
            c.exp - now,
            c.grace.unwrap_or(7 * 86_400) / 86_400
        ),
        LicenseState::Grace { remaining_s } => format!(
            "account {}, playing offline on the last licence check ({:.1} days of grace left)",
            c.sub,
            remaining_s as f64 / 86_400.0
        ),
    })
}
