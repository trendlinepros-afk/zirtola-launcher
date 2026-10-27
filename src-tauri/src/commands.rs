//! Commands the UI calls through `invoke`.

use crate::auth;
use crate::jobs::{self, PLATFORM};
use crate::state::{dev_mode, AppState, CmdError, CmdResult, JobKind, Settings};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;
use tokio_util::sync::CancellationToken;
use zirtola_core::api::{CatalogGame, Me, RedeemResult};
use zirtola_core::install::{self, valid_game_id, Journal};

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    api_base: String,
    dev_mode: bool,
    store_url: Option<String>,
    account_url: Option<String>,
    support_url: Option<String>,
    /// Oldest launcher the website still supports; below it the update is required.
    min_launcher_version: Option<String>,
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let cfg = state.config().await.ok();
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        api_base: state.api.base().to_string(),
        dev_mode: dev_mode(),
        store_url: cfg.as_ref().map(|c| c.store_url.clone()),
        account_url: cfg.as_ref().and_then(|c| c.account_url.clone()),
        support_url: cfg.as_ref().and_then(|c| c.support_url.clone()),
        min_launcher_version: cfg.as_ref().and_then(|c| c.min_launcher_version.clone()),
    })
}

// ---------------------------------------------------------------- account

#[tauri::command]
pub async fn restore_session(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<Me>> {
    let me = state.restore_session().await?;
    if me.is_some() {
        tauri::async_runtime::spawn(crate::reports::report_hardware_if_needed(app.clone()));
        tauri::async_runtime::spawn(crate::playtime::flush(app));
    }
    Ok(me)
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Me> {
    let cfg = state.config().await?;
    let cancel = CancellationToken::new();
    if let Some(old) = state.login_cancel.lock().unwrap().replace(cancel.clone()) {
        old.cancel();
    }
    let tokens = auth::sign_in_browser(&state.api, &cfg, cancel)
        .await
        .map_err(|e| match e.downcast::<zirtola_core::api::ApiError>() {
            Ok(api) => CmdError::from(api),
            Err(e) => CmdError::new("sign_in_failed", format!("{e:#}")),
        })?;
    let me = state.start_session(tokens).await?;
    crate::playtime::clear_cache();
    tauri::async_runtime::spawn(crate::reports::report_hardware_if_needed(app.clone()));
    tauri::async_runtime::spawn(crate::playtime::flush(app));
    Ok(me)
}

#[tauri::command]
pub fn cancel_sign_in(state: State<'_, AppState>) {
    if let Some(c) = state.login_cancel.lock().unwrap().take() {
        c.cancel();
    }
}

#[tauri::command]
pub async fn sign_out(state: State<'_, AppState>) -> CmdResult<()> {
    let sess = state.session.lock().await.take();
    if let (Some(s), Ok(cfg)) = (sess, state.config().await) {
        if let (Some(ep), Some(rt)) = (&cfg.revocation_endpoint, &s.tokens.refresh_token) {
            let _ = state.api.revoke(ep, &cfg.client_id, rt).await;
        }
    }
    auth::clear_refresh_token(state.api.base());
    crate::playtime::clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn redeem_key(state: State<'_, AppState>, key: String) -> CmdResult<RedeemResult> {
    let key = key.trim().to_uppercase();
    if key.len() < 5 || key.len() > 64 {
        return Err(CmdError::new(
            "invalid_key",
            "That doesn't look like a Zirtola key.",
        ));
    }
    state
        .authed(|at| {
            let api = state.api.clone();
            let key = key.clone();
            async move { api.redeem_key(&at, &key).await }
        })
        .await
}

// ---------------------------------------------------------------- library

#[derive(Serialize)]
pub struct InstallInfo {
    path: PathBuf,
    /// Installed version (None if the first install has not finished).
    version: Option<String>,
    latest_version: Option<String>,
    update_available: bool,
    /// Version of an interrupted install/update that can be resumed.
    paused_version: Option<String>,
    folder_missing: bool,
    /// The installed version is older than a required (mandatory) release:
    /// it must update before Play.
    update_required: bool,
    min_version: Option<String>,
    required_reason: Option<String>,
}

#[derive(Serialize)]
pub struct LibraryGame {
    game: CatalogGame,
    owned: bool,
    entitlement: Option<String>,
    /// Channels the licence allows ("live" = stable, "beta").
    branches: Vec<String>,
    /// Channel this install follows.
    branch: String,
    install: Option<InstallInfo>,
    job: Option<JobKind>,
    /// Active play time (website totals plus runs not sent yet).
    playtime_seconds: u64,
    /// The same, rounded down to the quarter hour.
    hours_played: f64,
}

#[tauri::command]
pub async fn library(state: State<'_, AppState>) -> CmdResult<Vec<LibraryGame>> {
    let catalog = state.api.catalog().await?;
    let signed_in = state.session.lock().await.is_some();
    let ents = if signed_in {
        state
            .authed(|at| {
                let api = state.api.clone();
                async move { api.entitlements(&at).await }
            })
            .await?
            .entitlements
    } else {
        vec![]
    };
    // A game installed from a channel the account can no longer use (beta
    // access ended, or a different account signed in) follows stable again,
    // so it keeps getting updates instead of silently checking a channel it
    // can't see.
    if signed_in {
        let mut reg = state.registry.lock().await;
        let mut changed = false;
        for e in &ents {
            if let Some(rec) = reg.get(&e.game_id).cloned() {
                if rec.branch != "live" && !e.branches.iter().any(|b| b == &rec.branch) {
                    let mut rec = rec;
                    rec.branch = "live".into();
                    reg.put(rec);
                    changed = true;
                }
            }
        }
        if changed {
            let _ = reg.save();
        }
    }
    let registry = state.registry.lock().await.installs.clone();
    let played = crate::playtime::active_seconds(&state).await;

    let mut out = Vec::new();
    for g in catalog.games {
        if !valid_game_id(&g.id) {
            continue;
        }
        let ent = ents.iter().find(|e| e.game_id == g.id);
        let install = match registry.get(&g.id) {
            None => None,
            Some(rec) => {
                let on_disk = install::installed_manifest(&rec.path).ok().flatten();
                let paused = Journal::load(&rec.path).map(|j| j.version);
                let version = on_disk.as_ref().map(|(_, m)| m.version.clone());
                Some(InstallInfo {
                    folder_missing: !rec.path.exists(),
                    path: rec.path.clone(),
                    version,
                    latest_version: None,
                    update_available: false,
                    paused_version: paused,
                    update_required: false,
                    min_version: None,
                    required_reason: None,
                })
            }
        };
        out.push(LibraryGame {
            owned: ent.is_some(),
            entitlement: ent.map(|e| e.kind.clone()),
            branches: ent.map(|e| e.branches.clone()).unwrap_or_default(),
            branch: registry
                .get(&g.id)
                .map(|r| r.branch.clone())
                .unwrap_or_else(|| "live".into()),
            job: state.job_running(&g.id),
            playtime_seconds: played.get(&g.id).copied().unwrap_or(0),
            hours_played: zirtola_core::api::quarter_hours(played.get(&g.id).copied().unwrap_or(0)),
            game: g,
            install,
        });
    }

    // Update check for installed, owned games (in parallel).
    let checks = out
        .iter()
        .enumerate()
        .filter(|(_, g)| g.owned && g.install.as_ref().is_some_and(|i| i.version.is_some()))
        .map(|(i, g)| {
            let branch = registry
                .get(&g.game.id)
                .map(|r| r.branch.clone())
                .unwrap_or_else(|| "live".into());
            let id = g.game.id.clone();
            let st = &state;
            async move {
                let r = st
                    .authed(|at| {
                        let api = st.api.clone();
                        let id = id.clone();
                        let branch = branch.clone();
                        async move { api.latest_build(&at, &id, PLATFORM, &branch).await }
                    })
                    .await;
                (i, r)
            }
        });
    for (i, r) in futures::future::join_all(checks).await {
        if let (Ok(latest), Some(inst)) = (r, out[i].install.as_mut()) {
            inst.update_available = inst.version.as_deref() != Some(latest.version.as_str());
            if let (Some(min), Some(v)) = (latest.min_version.as_deref(), inst.version.as_deref()) {
                inst.update_required = zirtola_core::api::compare_versions(v, min) == std::cmp::Ordering::Less;
            }
            inst.min_version = latest.min_version.clone();
            inst.required_reason = latest.mandatory_reason.clone().filter(|r| !r.is_empty());
            inst.latest_version = Some(latest.version);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- folders & disk

fn folder_name(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        "Game".into()
    } else {
        s
    }
}

#[tauri::command]
pub async fn suggest_install_dir(
    state: State<'_, AppState>,
    title: String,
    parent: Option<String>,
) -> CmdResult<String> {
    let base = match parent {
        Some(p) => PathBuf::from(p),
        None => state.settings.lock().await.library_dir.clone(),
    };
    Ok(base
        .join(folder_name(&title))
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a library folder")
        .pick_folder(move |p| {
            let _ = tx.send(p);
        });
    let picked = rx
        .await
        .map_err(|_| CmdError::new("error", "dialog closed"))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

#[derive(Serialize)]
pub struct DiskInfo {
    free_bytes: u64,
}

#[tauri::command]
pub fn disk_info(path: String) -> CmdResult<DiskInfo> {
    Ok(DiskInfo {
        free_bytes: install::available_space(Path::new(&path))?,
    })
}

/// Check that `path` is a sensible place to install `game_id`.
fn check_install_dir(path: &Path, game_id: &str) -> CmdResult<()> {
    if !path.is_absolute() || path.parent().is_none() {
        return Err(CmdError::new(
            "bad_folder",
            "Choose a folder, not the root of a drive.",
        ));
    }
    if path.is_file() {
        return Err(CmdError::new("bad_folder", "That path is a file."));
    }
    if path.exists() {
        if let Some((_, m)) = install::installed_manifest(path)? {
            if m.game_id != game_id {
                return Err(CmdError::new(
                    "bad_folder",
                    "That folder contains a different game.",
                ));
            }
            return Ok(()); // existing install of this game: adopt and update/repair it
        }
        if Journal::load(path).is_some() {
            return Ok(()); // interrupted install: resume it
        }
        let non_empty = std::fs::read_dir(path)?.next().is_some();
        if non_empty {
            return Err(CmdError::new(
                "bad_folder",
                "That folder is not empty. Choose an empty folder or a new one.",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- jobs

#[tauri::command]
pub async fn install_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
    path: String,
) -> CmdResult<()> {
    if !valid_game_id(&game_id) {
        return Err(CmdError::new("bad_game", "Unknown game."));
    }
    state.access_token().await?;
    let root = PathBuf::from(path);
    check_install_dir(&root, &game_id)?;
    std::fs::create_dir_all(&root)
        .map_err(|e| CmdError::new("bad_folder", format!("Could not create the folder: {e}")))?;
    let cancel = jobs::begin(&state, &game_id, JobKind::Install)?;
    jobs::spawn_install(app, game_id, root, JobKind::Install, cancel);
    Ok(())
}

async fn install_root(state: &AppState, game_id: &str) -> CmdResult<PathBuf> {
    state
        .registry
        .lock()
        .await
        .get(game_id)
        .map(|r| r.path.clone())
        .ok_or_else(|| CmdError::new("not_installed", "This game is not installed."))
}

/// Update to the latest version, or resume a paused install/update.
#[tauri::command]
pub async fn update_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
) -> CmdResult<()> {
    let root = install_root(&state, &game_id).await?;
    state.access_token().await?;
    let kind = if install::installed_manifest(&root)?.is_some() {
        JobKind::Update
    } else {
        JobKind::Install
    };
    let cancel = jobs::begin(&state, &game_id, kind)?;
    jobs::spawn_install(app, game_id, root, kind, cancel);
    Ok(())
}

#[tauri::command]
pub async fn verify_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
) -> CmdResult<()> {
    let root = install_root(&state, &game_id).await?;
    let cancel = jobs::begin(&state, &game_id, JobKind::Verify)?;
    jobs::spawn_verify(app, game_id, root, cancel);
    Ok(())
}

#[tauri::command]
pub async fn uninstall_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
) -> CmdResult<()> {
    let root = install_root(&state, &game_id).await?;
    jobs::begin(&state, &game_id, JobKind::Uninstall)?;
    jobs::spawn_uninstall(app, game_id, root);
    Ok(())
}

/// "Also delete saves?" after uninstalling. Saves never live in the install
/// folder: the catalogue names the folder (e.g. %LOCALAPPDATA%\GranitePit\Saved)
/// and this deletes it only after the player ticked the box, and only if it
/// resolves inside the player's own profile folders.
#[tauri::command]
pub async fn delete_game_saves(state: State<'_, AppState>, game_id: String) -> CmdResult<String> {
    if !valid_game_id(&game_id) {
        return Err(CmdError::new("bad_game", "Unknown game."));
    }
    let catalog = state.api.catalog().await?;
    let game = catalog
        .games
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| CmdError::new("bad_game", "Unknown game."))?;
    let raw = game
        .save_location
        .ok_or_else(|| CmdError::new("no_saves", "This game doesn't say where it keeps its saves."))?;
    let dir = crate::saves::resolve_save_dir(&raw).map_err(|e| CmdError::new("bad_save_dir", e))?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| CmdError::new("delete_failed", format!("Couldn't delete {}: {e}", dir.display())))?;
    }
    Ok(dir.display().to_string())
}

/// Where a game keeps its saves on this PC (for the uninstall dialog).
#[tauri::command]
pub fn resolve_save_location(raw: String) -> CmdResult<String> {
    crate::saves::resolve_save_dir(&raw)
        .map(|p| p.display().to_string())
        .map_err(|e| CmdError::new("bad_save_dir", e))
}

/// Pause (cancel) the running job. Installs and updates resume from where they stopped.
#[tauri::command]
pub fn pause_job(state: State<'_, AppState>, game_id: String) -> CmdResult<()> {
    if let Some(j) = state.jobs.lock().unwrap().get(&game_id) {
        if j.kind == JobKind::Uninstall {
            return Err(CmdError::new("busy", "Uninstall can't be paused."));
        }
        j.cancel.cancel();
    }
    Ok(())
}

/// Forget a game whose folder was deleted outside the launcher.
#[tauri::command]
pub async fn forget_install(state: State<'_, AppState>, game_id: String) -> CmdResult<()> {
    let mut reg = state.registry.lock().await;
    reg.remove(&game_id);
    reg.save()?;
    Ok(())
}

#[tauri::command]
pub async fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
    title: Option<String>,
) -> CmdResult<()> {
    if state.job_running(&game_id).is_some() {
        return Err(CmdError::new(
            "busy",
            "Wait for the current task to finish.",
        ));
    }
    let root = install_root(&state, &game_id).await?;
    let Some((_, m)) = install::installed_manifest(&root)? else {
        return Err(CmdError::new(
            "not_installed",
            "This game is not installed.",
        ));
    };
    let device_id = state.device_id();
    // A hardware label, not the computer name (which often contains the user's name).
    let device_name = state.device_label();
    let branch = state
        .registry
        .lock()
        .await
        .get(&game_id)
        .map(|r| r.branch.clone())
        .unwrap_or_else(|| "live".into());
    let version = m.version.clone();
    let token = state
        .authed(|at| {
            let api = state.api.clone();
            let gid = game_id.clone();
            let (did, dname, ver, br) = (device_id.clone(), device_name.clone(), version.clone(), branch.clone());
            async move { api.launch_token(&at, &gid, &did, &dname, &ver, &br).await }
        })
        .await?;
    // The run's playtime id, also given to the game so its career-stats
    // reports can name the run (docs/GAME_STATS_API.md).
    let play_session_id = crate::playtime::new_session_id();
    let child = install::launch_with_env(
        &root,
        &m,
        &token.token,
        state.api.base(),
        &[("ZIRTOLA_PLAY_SESSION_ID", play_session_id.as_str())],
    )
    .map_err(|e| CmdError::new("launch_failed", format!("{e:#}")))?;
    let title = title.unwrap_or_else(|| m.game_id.clone());
    let user_id = crate::playtime::current_user(&state).await;
    crate::reports::watch_game(app, child, root, m, title, user_id, play_session_id);
    Ok(())
}

// ---------------------------------------------------------------- devices, channels, changelog

/// Deactivate another PC so this one can play (the player confirmed in the UI).
#[tauri::command]
pub async fn deactivate_device(state: State<'_, AppState>, activation_id: String) -> CmdResult<()> {
    if activation_id.is_empty() || activation_id.len() > 64 {
        return Err(CmdError::new("bad_request", "Unknown device."));
    }
    state
        .authed(|at| {
            let api = state.api.clone();
            let id = activation_id.clone();
            async move { api.deactivate_device(&at, &id).await }
        })
        .await
}

/// "View license key": fetched live for the signed-in account; never stored.
#[tauri::command]
pub async fn game_license(state: State<'_, AppState>, game_id: String) -> CmdResult<serde_json::Value> {
    if !valid_game_id(&game_id) {
        return Err(CmdError::new("bad_game", "Unknown game."));
    }
    let device = state.device_id();
    state
        .authed(|at| {
            let api = state.api.clone();
            let (gid, dev) = (game_id.clone(), device.clone());
            async move { api.license(&at, &gid, &dev).await }
        })
        .await
}

/// Release notes for a game on its channel, newest first.
#[tauri::command]
pub async fn game_changelog(state: State<'_, AppState>, game_id: String) -> CmdResult<serde_json::Value> {
    if !valid_game_id(&game_id) {
        return Err(CmdError::new("bad_game", "Unknown game."));
    }
    let branch = state
        .registry
        .lock()
        .await
        .get(&game_id)
        .map(|r| r.branch.clone())
        .unwrap_or_else(|| "live".into());
    Ok(state.api.changelog(&game_id, &branch).await?)
}

/// Switch a game between the stable ("live") and "beta" channels. The next
/// library refresh offers that channel's build as an update.
#[tauri::command]
pub async fn set_branch(state: State<'_, AppState>, game_id: String, branch: String) -> CmdResult<()> {
    if branch != "live" && branch != "beta" {
        return Err(CmdError::new("bad_request", "Unknown channel."));
    }
    if state.job_running(&game_id).is_some() {
        return Err(CmdError::new("busy", "Wait for the current task to finish."));
    }
    let mut reg = state.registry.lock().await;
    let Some(mut rec) = reg.get(&game_id).cloned() else {
        return Err(CmdError::new("not_installed", "Install the game first."));
    };
    rec.branch = branch;
    reg.put(rec);
    reg.save()?;
    Ok(())
}

// ---------------------------------------------------------------- hardware & crash reports

/// Exactly what "Share hardware info" sends (shown in Settings).
#[tauri::command]
pub async fn hardware_report(app: AppHandle) -> CmdResult<serde_json::Value> {
    Ok(serde_json::to_value(crate::reports::collect(&app).await).unwrap_or_default())
}

#[tauri::command]
pub async fn pending_crashes(app: AppHandle) -> CmdResult<Vec<crate::crash::CrashCandidate>> {
    Ok(crate::reports::pending_crashes(&app).await)
}

#[tauri::command]
pub async fn send_crash_report(app: AppHandle, id: String, comment: String) -> CmdResult<String> {
    crate::reports::send(&app, &id, &comment).await
}

#[tauri::command]
pub fn dismiss_crash(app: AppHandle, id: String) {
    crate::reports::dismiss(&app, &id);
}

// ---------------------------------------------------------------- misc

#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    let u = url::Url::parse(&url).map_err(|_| CmdError::new("bad_url", "Invalid link."))?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err(CmdError::new("bad_url", "Only web links can be opened."));
    }
    // Debug builds only: automated tests switch the browser off and read the
    // URL from the log. Compiled out of release builds, which always open it.
    #[cfg(debug_assertions)]
    if std::env::var_os("ZIRTOLA_NO_BROWSER").is_some() {
        eprintln!("ZIRTOLA_OPEN_URL={u}");
        return Err(CmdError::new(
            "browser_suppressed",
            format!("Test mode: the browser is switched off (ZIRTOLA_NO_BROWSER). Would open {u}"),
        ));
    }
    open::that_detached(u.as_str())?;
    Ok(())
}

/// A `zirtola://` link the launcher was started with (Windows passes it as an argument).
#[tauri::command]
pub fn startup_deep_link() -> Option<String> {
    deep_link_arg(std::env::args())
}

pub fn deep_link_arg<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter()
        .find(|a| a.starts_with("zirtola://") && a.len() < 512)
}

#[tauri::command]
pub async fn open_install_folder(state: State<'_, AppState>, game_id: String) -> CmdResult<()> {
    let root = install_root(&state, &game_id).await?;
    open::that_detached(&root)?;
    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<()> {
    if !settings.library_dir.is_absolute() {
        return Err(CmdError::new(
            "bad_folder",
            "The library folder must be a full path.",
        ));
    }
    let mut s = settings;
    s.parallel_downloads = s.parallel_downloads.clamp(1, 32);
    let turned_on = s.share_hardware_info && !state.settings.lock().await.share_hardware_info;
    *state.settings.lock().await = s;
    state.save_settings().await?;
    if turned_on {
        tauri::async_runtime::spawn(crate::reports::report_hardware_if_needed(app));
    }
    Ok(())
}

// ---------------------------------------------------------------- launcher self-update

#[derive(Serialize)]
pub struct LauncherUpdate {
    version: String,
    notes: Option<String>,
    date: Option<String>,
    /// A required launcher update (release record "mandatory"): no "Later".
    mandatory: bool,
    mandatory_reason: Option<String>,
}

/// The launcher's own update feed is on the website, for signed-in accounts
/// that own a game: `{api_base}/v1/launcher/update`, with the launcher's
/// access token. Signed out there is nothing to check (sign-in comes first:
/// the launcher can't do anything without it). Every update is still verified
/// with the updater public key built into the launcher.
async fn launcher_updater(app: &AppHandle, state: &AppState) -> Result<Option<tauri_plugin_updater::Updater>, CmdError> {
    if state.session.lock().await.is_none() {
        return Ok(None);
    }
    let at = state.access_token().await?;
    let endpoint = url::Url::parse(&format!("{}/v1/launcher/update", state.api.base()))
        .map_err(|e| CmdError::new("updater", e.to_string()))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|b| b.header("Authorization", format!("Bearer {at}")))
        .and_then(|b| b.build())
        .map_err(|e| CmdError::new("updater", e.to_string()))?;
    Ok(Some(updater))
}

#[tauri::command]
pub async fn check_launcher_update(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<LauncherUpdate>> {
    let Some(updater) = launcher_updater(&app, &state).await? else {
        return Ok(None);
    };
    match updater.check().await {
        Ok(Some(u)) => Ok(Some(LauncherUpdate {
            version: u.version.clone(),
            notes: u.body.clone(),
            date: u.date.map(|d| d.to_string()),
            mandatory: u.raw_json.get("mandatory").and_then(|v| v.as_bool()).unwrap_or(false),
            mandatory_reason: u.raw_json.get("mandatory_reason").and_then(|v| v.as_str()).map(str::to_string).filter(|s| !s.is_empty()),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(CmdError::new("updater", e.to_string())),
    }
}

#[tauri::command]
pub async fn install_launcher_update(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if !state.jobs.lock().unwrap().is_empty() {
        return Err(CmdError::new(
            "busy",
            "Pause or finish downloads before updating the launcher.",
        ));
    }
    let Some(updater) = launcher_updater(&app, &state).await? else {
        return Err(CmdError::new("signed_out", "Sign in to update the launcher."));
    };
    let Some(update) = updater
        .check()
        .await
        .map_err(|e| CmdError::new("updater", e.to_string()))?
    else {
        return Ok(());
    };
    let mut downloaded: u64 = 0;
    let handle = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = handle.emit(
                    "launcher-update-progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| CmdError::new("updater", e.to_string()))?;
    app.restart();
}

pub fn cancel_all_jobs(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let jobs = state.jobs.lock().unwrap();
    for j in jobs.values() {
        j.cancel.cancel();
    }
    !jobs.is_empty()
}

// ---------------------------------------------------------------- requirements (prereqs.rs)

/// What a game needs on this PC, one row per item. With a game id, the disk
/// check uses its download size and the VC++ installer it ships.
#[tauri::command]
pub async fn prereqs_check(state: State<'_, AppState>, game_id: Option<String>, need_bytes: Option<u64>) -> CmdResult<Vec<crate::prereqs::Item>> {
    let input = prereq_input(&state, game_id.as_deref(), need_bytes).await;
    Ok(tauri::async_runtime::spawn_blocking(move || crate::prereqs::check(&input))
        .await
        .map_err(|e| CmdError::new("prereqs", e.to_string()))?)
}

/// Install the given items (only after the player said yes on the
/// requirements screen). Progress arrives as "prereq-progress" events.
#[tauri::command]
pub async fn prereqs_install(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>, game_id: Option<String>) -> CmdResult<()> {
    let input = prereq_input(&state, game_id.as_deref(), None).await;
    for id in ids {
        let handle = app.clone();
        let r = crate::prereqs::install(&id, &state.dl_http, input.game_root.clone(), move |p| {
            let _ = handle.emit("prereq-progress", &p);
        })
        .await;
        if let Err(message) = r {
            let _ = app.emit(
                "prereq-progress",
                &crate::prereqs::Progress { id: id.clone(), phase: "error", done: 0, total: 0, message: Some(message.clone()) },
            );
            return Err(CmdError::new("prereq_failed", message));
        }
    }
    Ok(())
}

async fn prereq_input(state: &AppState, game_id: Option<&str>, need_bytes: Option<u64>) -> crate::prereqs::CheckInput {
    let rec = match game_id {
        Some(id) if valid_game_id(id) => state.registry.lock().await.get(id).cloned(),
        _ => None,
    };
    let installed = rec.as_ref().is_some_and(|r| install::installed_manifest(&r.path).ok().flatten().is_some());
    crate::prereqs::CheckInput {
        need_bytes: if installed { 0 } else { need_bytes.unwrap_or(0) },
        install_dir: match &rec {
            Some(r) => r.path.clone(),
            None => state.settings.lock().await.library_dir.clone(),
        },
        game_root: rec.filter(|_| installed).map(|r| r.path),
    }
}
