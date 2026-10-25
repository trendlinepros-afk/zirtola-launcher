//! Background jobs (install, update, verify/repair, uninstall) and their
//! progress events.
//!
//! Events:
//! * `job-progress`  `{ game_id, kind, phase, total_bytes, done_bytes, network_bytes,
//!                      reused_bytes, download_bytes, speed_bps, eta_s }`
//! * `job-finished`  `{ game_id, kind, ok, paused, code, message }`

use crate::state::{AppState, CmdError, Job, JobKind};
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use zirtola_core::install::{self, ApplyOptions, InstallError, InstallRecord, Phase, Progress};

pub const PLATFORM: &str = "win64";

#[derive(Serialize, Clone)]
struct ProgressEvent<'a> {
    game_id: &'a str,
    kind: JobKind,
    #[serde(flatten)]
    p: &'a Progress,
    speed_bps: f64,
    eta_s: Option<f64>,
}

#[derive(Serialize, Clone)]
pub struct FinishedEvent {
    pub game_id: String,
    pub kind: JobKind,
    pub ok: bool,
    pub paused: bool,
    pub code: Option<String>,
    pub message: Option<String>,
}

struct Meter {
    primed: bool,
    last_emit: Instant,
    last_sample: Instant,
    last_net: u64,
    last_done: u64,
    speed: f64,
    work_rate: f64,
}

fn progress_emitter(app: AppHandle, game_id: String, kind: JobKind) -> install::ProgressFn {
    let meter = Mutex::new(Meter {
        primed: false,
        last_emit: Instant::now() - std::time::Duration::from_secs(1),
        last_sample: Instant::now(),
        last_net: 0,
        last_done: 0,
        speed: 0.0,
        work_rate: 0.0,
    });
    Arc::new(move |p: &Progress| {
        let mut m = meter.lock().unwrap();
        let now = Instant::now();
        if !m.primed {
            // Start measuring from wherever a resumed job begins.
            m.primed = true;
            m.last_sample = now;
            m.last_net = p.network_bytes;
            m.last_done = p.done_bytes;
        }
        let dt = now.duration_since(m.last_sample).as_secs_f64();
        if dt >= 0.5 {
            let inst = p.network_bytes.saturating_sub(m.last_net) as f64 / dt;
            let work = p.done_bytes.saturating_sub(m.last_done) as f64 / dt;
            // Exponential moving averages keep the numbers readable.
            m.speed = if m.speed == 0.0 {
                inst
            } else {
                m.speed * 0.7 + inst * 0.3
            };
            m.work_rate = if m.work_rate == 0.0 {
                work
            } else {
                m.work_rate * 0.7 + work * 0.3
            };
            m.last_sample = now;
            m.last_net = p.network_bytes;
            m.last_done = p.done_bytes;
        }
        let important = matches!(p.phase, Phase::Done | Phase::Committing | Phase::Preparing);
        if !important && now.duration_since(m.last_emit).as_millis() < 200 {
            return;
        }
        m.last_emit = now;
        let remaining = p.total_bytes.saturating_sub(p.done_bytes) as f64;
        let eta = (m.work_rate > 1.0).then(|| remaining / m.work_rate);
        let _ = app.emit(
            "job-progress",
            ProgressEvent {
                game_id: &game_id,
                kind,
                p,
                speed_bps: m.speed,
                eta_s: eta,
            },
        );
    })
}

fn finish(app: &AppHandle, ev: FinishedEvent) {
    app.state::<AppState>()
        .jobs
        .lock()
        .unwrap()
        .remove(&ev.game_id);
    let _ = app.emit("job-finished", ev);
}

/// Register a job; fails if the game already has one running.
pub fn begin(
    state: &AppState,
    game_id: &str,
    kind: JobKind,
) -> Result<CancellationToken, CmdError> {
    let mut jobs = state.jobs.lock().unwrap();
    if let Some(j) = jobs.get(game_id) {
        return Err(CmdError::new(
            "busy",
            format!("This game is busy ({:?}).", j.kind).to_lowercase(),
        ));
    }
    let cancel = CancellationToken::new();
    jobs.insert(
        game_id.to_string(),
        Job {
            kind,
            cancel: cancel.clone(),
        },
    );
    Ok(cancel)
}

pub fn spawn_install(
    app: AppHandle,
    game_id: String,
    root: PathBuf,
    kind: JobKind,
    cancel: CancellationToken,
) {
    tauri::async_runtime::spawn(async move {
        let res = run_install(&app, &game_id, &root, kind, cancel).await;
        let ev = match res {
            Ok(()) => FinishedEvent {
                game_id,
                kind,
                ok: true,
                paused: false,
                code: None,
                message: None,
            },
            Err(JobError::Paused) => FinishedEvent {
                game_id,
                kind,
                ok: false,
                paused: true,
                code: Some("paused".into()),
                message: None,
            },
            Err(JobError::Failed(e)) => FinishedEvent {
                game_id,
                kind,
                ok: false,
                paused: false,
                code: Some(e.code),
                message: Some(e.message),
            },
        };
        finish(&app, ev);
    });
}

enum JobError {
    Paused,
    Failed(CmdError),
}

impl From<CmdError> for JobError {
    fn from(e: CmdError) -> Self {
        JobError::Failed(e)
    }
}
impl From<std::io::Error> for JobError {
    fn from(e: std::io::Error) -> Self {
        JobError::Failed(e.into())
    }
}
impl From<anyhow::Error> for JobError {
    fn from(e: anyhow::Error) -> Self {
        JobError::Failed(e.into())
    }
}

fn map_install_error(e: InstallError) -> JobError {
    match e {
        InstallError::Cancelled => JobError::Paused,
        InstallError::InsufficientSpace { needed, available } => JobError::Failed(CmdError::new(
            "disk_full",
            format!(
                "Not enough disk space: {:.1} GB needed, {:.1} GB free.",
                needed as f64 / 1e9,
                available as f64 / 1e9
            ),
        )),
        InstallError::FileInUse(p) => JobError::Failed(CmdError::new(
            "file_in_use",
            format!("{p} is in use. Close the game and try again."),
        )),
        InstallError::ChunkUrlExpired => {
            JobError::Failed(CmdError::new("link_expired", "The download link expired."))
        }
        InstallError::Other(e) => JobError::Failed(e.into()),
    }
}

async fn run_install(
    app: &AppHandle,
    game_id: &str,
    root: &std::path::Path,
    kind: JobKind,
    cancel: CancellationToken,
) -> Result<(), JobError> {
    let state = app.state::<AppState>();
    let progress = progress_emitter(app.clone(), game_id.to_string(), kind);
    let (branch, parallel) = {
        let reg = state.registry.lock().await;
        let branch = reg
            .get(game_id)
            .map(|r| r.branch.clone())
            .unwrap_or_else(|| "live".into());
        (branch, state.settings.lock().await.parallel_downloads)
    };

    // Remember the folder right away so a paused first install can be resumed.
    {
        let mut reg = state.registry.lock().await;
        let prev = reg.get(game_id).cloned();
        reg.put(InstallRecord {
            game_id: game_id.to_string(),
            path: root.to_path_buf(),
            branch: branch.clone(),
            version: prev.as_ref().and_then(|p| p.version.clone()),
            installed_at: prev.and_then(|p| p.installed_at),
        });
        reg.save()?;
    }

    for _attempt in 0..3 {
        let resp = state
            .authed(|at| {
                let api = state.api.clone();
                let gid = game_id.to_string();
                let branch = branch.clone();
                async move { api.manifest(&at, &gid, PLATFORM, &branch, None).await }
            })
            .await?;
        let manifest = {
            let keys = state.keys.read().await;
            resp.signed_manifest.open(&keys).map_err(|e| {
                CmdError::new(
                    "bad_manifest",
                    format!("The build manifest failed verification: {e:#}"),
                )
            })?
        };
        if manifest.game_id != game_id || manifest.platform != PLATFORM {
            return Err(CmdError::new(
                "bad_manifest",
                "The server sent a manifest for a different game.",
            )
            .into());
        }
        let opts = ApplyOptions {
            parallel,
            http: state.dl_http.clone(),
            chunk_url_template: resp.chunk_url_template.clone(),
            force_rebuild: HashSet::new(),
            cancel: cancel.clone(),
            progress: Some(progress.clone()),
            space_margin: 1 << 30,
        };
        match install::apply(root, &resp.signed_manifest, &manifest, opts).await {
            Ok(()) => {
                let mut reg = state.registry.lock().await;
                reg.put(InstallRecord {
                    game_id: game_id.to_string(),
                    path: root.to_path_buf(),
                    branch: branch.clone(),
                    version: Some(manifest.version.clone()),
                    installed_at: Some(chrono::Utc::now().to_rfc3339()),
                });
                reg.save()?;
                return Ok(());
            }
            Err(InstallError::ChunkUrlExpired) => continue, // fetch a fresh link and resume
            Err(e) => return Err(map_install_error(e)),
        }
    }
    Err(CmdError::new(
        "link_expired",
        "The download server kept rejecting the download link.",
    )
    .into())
}

/// Verify every file; if anything is damaged or missing, repair it from the
/// same version's manifest (copying good chunks, downloading the rest).
pub fn spawn_verify(app: AppHandle, game_id: String, root: PathBuf, cancel: CancellationToken) {
    tauri::async_runtime::spawn(async move {
        let kind = JobKind::Verify;
        let res = run_verify(&app, &game_id, &root, cancel).await;
        let ev = match res {
            Ok(msg) => FinishedEvent {
                game_id,
                kind,
                ok: true,
                paused: false,
                code: None,
                message: Some(msg),
            },
            Err(JobError::Paused) => FinishedEvent {
                game_id,
                kind,
                ok: false,
                paused: true,
                code: Some("paused".into()),
                message: None,
            },
            Err(JobError::Failed(e)) => FinishedEvent {
                game_id,
                kind,
                ok: false,
                paused: false,
                code: Some(e.code),
                message: Some(e.message),
            },
        };
        finish(&app, ev);
    });
}

async fn run_verify(
    app: &AppHandle,
    game_id: &str,
    root: &std::path::Path,
    cancel: CancellationToken,
) -> Result<String, JobError> {
    let state = app.state::<AppState>();
    let progress = progress_emitter(app.clone(), game_id.to_string(), JobKind::Verify);
    let Some((_, installed)) = install::installed_manifest(root)? else {
        return Err(CmdError::new(
            "not_installed",
            "No completed install was found in that folder.",
        )
        .into());
    };
    let report =
        match install::verify(root, &installed, Some(progress.clone()), cancel.clone()).await {
            Ok(r) => r,
            Err(_) if cancel.is_cancelled() => return Err(JobError::Paused),
            Err(e) => return Err(e.into()),
        };
    if report.is_ok() {
        return Ok(format!("All {} files are intact.", installed.files.len()));
    }
    let broken: HashSet<String> = report
        .damaged
        .iter()
        .chain(report.missing.iter())
        .cloned()
        .collect();
    let count = broken.len();
    let branch = state
        .registry
        .lock()
        .await
        .get(game_id)
        .map(|r| r.branch.clone())
        .unwrap_or_else(|| "live".into());
    let parallel = state.settings.lock().await.parallel_downloads;
    for _attempt in 0..3 {
        let version = installed.version.clone();
        let resp = state
            .authed(|at| {
                let api = state.api.clone();
                let gid = game_id.to_string();
                let branch = branch.clone();
                let version = version.clone();
                async move {
                    api.manifest(&at, &gid, PLATFORM, &branch, Some(&version))
                        .await
                }
            })
            .await?;
        let manifest = {
            let keys = state.keys.read().await;
            resp.signed_manifest
                .open(&keys)
                .map_err(|e| CmdError::new("bad_manifest", format!("{e:#}")))?
        };
        if manifest.build_id != installed.build_id {
            return Err(CmdError::new(
                "version_gone",
                "This version is no longer available. Update the game instead.",
            )
            .into());
        }
        let opts = ApplyOptions {
            parallel,
            http: state.dl_http.clone(),
            chunk_url_template: resp.chunk_url_template.clone(),
            force_rebuild: broken.clone(),
            cancel: cancel.clone(),
            progress: Some(progress.clone()),
            space_margin: 256 << 20,
        };
        match install::apply(root, &resp.signed_manifest, &manifest, opts).await {
            Ok(()) => return Ok(format!("Repaired {count} file(s).")),
            Err(InstallError::ChunkUrlExpired) => continue,
            Err(e) => return Err(map_install_error(e)),
        }
    }
    Err(CmdError::new(
        "link_expired",
        "The download server kept rejecting the download link.",
    )
    .into())
}

pub fn spawn_uninstall(app: AppHandle, game_id: String, root: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let kind = JobKind::Uninstall;
        let _ = app.emit(
            "job-progress",
            ProgressEvent {
                game_id: &game_id,
                kind,
                p: &Progress {
                    phase: Phase::Uninstalling,
                    total_bytes: 1,
                    done_bytes: 0,
                    network_bytes: 0,
                    reused_bytes: 0,
                    download_bytes: 0,
                },
                speed_bps: 0.0,
                eta_s: None,
            },
        );
        let r = tauri::async_runtime::spawn_blocking({
            let root = root.clone();
            move || install::uninstall(&root)
        })
        .await;
        let res: Result<(), CmdError> = match r {
            Ok(Ok(())) => {
                let state = app.state::<AppState>();
                let mut reg = state.registry.lock().await;
                reg.remove(&game_id);
                reg.save().map_err(CmdError::from)
            }
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(CmdError::new("error", e.to_string())),
        };
        let ev = FinishedEvent {
            game_id,
            kind,
            ok: res.is_ok(),
            paused: false,
            code: res.as_ref().err().map(|e| e.code.clone()),
            message: res.err().map(|e| e.message),
        };
        finish(&app, ev);
    });
}
