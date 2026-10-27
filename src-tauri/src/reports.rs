//! Hardware reports and crash reports: collection, the player's consent, upload.

use crate::crash::{self, CrashCandidate};
use crate::hardware::{self, HardwareReport};
use crate::state::{AppState, CmdError, CmdResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use zirtola_core::install;
use zirtola_core::manifest::Manifest;

// ---------------------------------------------------------------- hardware

pub async fn collect(app: &AppHandle) -> HardwareReport {
    let state = app.state::<AppState>();
    let lib = state.settings.lock().await.library_dir.clone();
    let displays = app
        .get_webview_window("main")
        .and_then(|w| w.available_monitors().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|m| hardware::Display {
            width: m.size().width,
            height: m.size().height,
            scale: (m.scale_factor() * 100.0).round() / 100.0,
            refresh_hz: 0,
        })
        .collect();
    let version = app.package_info().version.to_string();
    let report =
        tauri::async_runtime::spawn_blocking(move || hardware::collect(displays, &lib, &version))
            .await
            .unwrap_or_else(|_| hardware::collect(vec![], Path::new("C:\\"), ""));
    *state.hardware.lock().unwrap() = Some(report.clone());
    report
}

#[derive(Serialize, Deserialize, Default)]
struct Sent {
    fingerprint: String,
    sent_at: i64,
    #[serde(default)]
    api_base: String,
}

/// Send the hardware report if sharing is on and it changed (or a week passed).
pub async fn report_hardware_if_needed(app: AppHandle) {
    let report = collect(&app).await;
    let state = app.state::<AppState>();
    if !state.settings.lock().await.share_hardware_info || state.session.lock().await.is_none() {
        return;
    }
    let path = state.data_dir.join("hardware-sent.json");
    let last: Sent = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let fp = report.fingerprint();
    let now = chrono::Utc::now().timestamp();
    let base = state.api.base().to_string();
    if last.fingerprint == fp && last.api_base == base && now - last.sent_at < 7 * 86_400 {
        return;
    }
    let mut body = serde_json::to_value(&report).unwrap_or_default();
    body["device_id"] = serde_json::Value::String(state.device_id());
    let res = state
        .authed(|at| {
            let api = state.api.clone();
            let body = body.clone();
            async move { api.post_hardware(&at, &body).await }
        })
        .await;
    match res {
        Ok(()) => {
            let _ = std::fs::write(
                &path,
                serde_json::to_vec(&Sent {
                    fingerprint: fp,
                    sent_at: now,
                    api_base: base,
                })
                .unwrap_or_default(),
            );
        }
        Err(e) => log::warn!("hardware report not sent: {}", e.message),
    }
}

// ---------------------------------------------------------------- crashes

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn candidate(
    m: &Manifest,
    title: &str,
    exit_code: Option<i32>,
    dir: Option<PathBuf>,
) -> CrashCandidate {
    let id = format!(
        "{}-{}",
        m.game_id,
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    CrashCandidate {
        id,
        game_id: m.game_id.clone(),
        game_title: title.to_string(),
        game_version: m.version.clone(),
        build_id: m.build_id.clone(),
        exit_code,
        error: dir.as_deref().map(crash::error_message).unwrap_or_default(),
        files: dir.as_deref().map(crash::files_of).unwrap_or_default(),
        detected_at: now_rfc3339(),
        dir,
    }
}

/// Wait for a game the launcher started; if it crashed, offer a report.
/// Also times the run for playtime when a player is signed in.
pub fn watch_game(
    app: AppHandle,
    mut child: std::process::Child,
    root: PathBuf,
    m: Manifest,
    title: String,
    user_id: Option<String>,
    play_session_id: String,
) {
    let tracker = user_id.map(|uid| {
        crate::playtime::start(app.clone(), uid, m.game_id.clone(), root.clone(), play_session_id)
    });
    let roots = crash::crash_roots(&root, &m);
    let before: std::collections::HashSet<String> = crash::list_crash_dirs(&roots)
        .into_iter()
        .map(|(p, _)| crash::seen_key(&p))
        .collect();
    let started = SystemTime::now();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        if let Some(t) = tracker {
            t.finish();
        }
        // Give UE's crash handler a moment to finish writing the folder.
        std::thread::sleep(Duration::from_secs(2));
        let state = app.state::<AppState>();
        let mut seen = state.crash_seen();
        seen.extend(before);
        let new_dir = crash::new_crash_dirs(&roots, started, &seen)
            .into_iter()
            .next();
        if new_dir.is_none() && code == Some(0) {
            return;
        }
        let c = candidate(&m, &title, code, new_dir);
        offer(&app, c);
    });
}

fn offer(app: &AppHandle, c: CrashCandidate) {
    let state = app.state::<AppState>();
    let offer =
        tauri::async_runtime::block_on(async { state.settings.lock().await.offer_crash_reports });
    if let Some(d) = &c.dir {
        state.mark_crash_seen(&crash::seen_key(d));
    }
    if !offer {
        return;
    }
    state
        .crashes
        .lock()
        .unwrap()
        .insert(c.id.clone(), c.clone());
    let _ = app.emit("game-crashed", c);
}

/// At start-up: recent crash folders of installed games the player has not decided about.
pub async fn pending_crashes(app: &AppHandle) -> Vec<CrashCandidate> {
    let state = app.state::<AppState>();
    if !state.settings.lock().await.offer_crash_reports {
        return vec![];
    }
    let installs = state.registry.lock().await.installs.clone();
    let seen = state.crash_seen();
    let week_ago = SystemTime::now() - Duration::from_secs(7 * 86_400);
    let mut out = Vec::new();
    for rec in installs.values() {
        let Ok(Some((_, m))) = install::installed_manifest(&rec.path) else {
            continue;
        };
        let roots = crash::crash_roots(&rec.path, &m);
        if let Some(dir) = crash::new_crash_dirs(&roots, week_ago, &seen)
            .into_iter()
            .next()
        {
            let title = m.game_id.clone();
            let c = candidate(&m, &title, None, Some(dir));
            state
                .crashes
                .lock()
                .unwrap()
                .insert(c.id.clone(), c.clone());
            out.push(c);
        }
    }
    out
}

pub async fn send(app: &AppHandle, id: &str, comment: &str) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let Some(c) = state.crashes.lock().unwrap().get(id).cloned() else {
        return Err(CmdError::new(
            "not_found",
            "That crash report is no longer available.",
        ));
    };
    let zip = match &c.dir {
        Some(d) => {
            let d = d.clone();
            tauri::async_runtime::spawn_blocking(move || crash::zip_crash_dir(&d))
                .await
                .map_err(|e| CmdError::new("error", e.to_string()))??
        }
        None => Vec::new(),
    };
    let share_hw = state.settings.lock().await.share_hardware_info;
    let hw = if share_hw {
        serde_json::to_value(collect(app).await).unwrap_or_default()
    } else {
        serde_json::json!({})
    };
    let meta = serde_json::json!({
        "source": "launcher",
        "game_id": c.game_id,
        "game_version": c.game_version,
        "build_id": c.build_id,
        "platform": "win64",
        "device_id": state.device_id(),
        "exit_code": c.exit_code,
        "comment": comment.chars().take(2000).collect::<String>(),
        "crash_guid": c.dir.as_deref().map(crash::seen_key).unwrap_or_default(),
        "hardware": hw,
    });
    let id_on_server = state
        .authed(|at| {
            let api = state.api.clone();
            let meta = meta.clone();
            let zip = zip.clone();
            async move { api.post_crash(&at, &meta, zip).await }
        })
        .await?;
    state.crashes.lock().unwrap().remove(id);
    Ok(id_on_server)
}

pub fn dismiss(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let removed = state.crashes.lock().unwrap().remove(id);
    if let Some(d) = removed.and_then(|c| c.dir) {
        state.mark_crash_seen(&crash::seen_key(&d));
    }
}
