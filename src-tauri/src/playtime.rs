//! Playtime: how long each game runs, and how much of that is active play.
//!
//! While a game the launcher started is running, a tracker thread wakes every
//! 15 seconds. Wall time always counts towards `total_seconds`; it counts
//! towards `active_seconds` only when the game's window is in front and the
//! player used the keyboard, mouse or a controller in the last 5 minutes.
//! Every 5 minutes and when the game exits the run's running totals are
//! queued in `playtime-pending.json` and sent to `POST /v1/playtime`; reports
//! that can't be sent (offline, signed out) stay queued and go with the next
//! flush. The website keeps the largest values per session, so re-sending is
//! always safe. See docs/API.md → Playtime.

use crate::state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use zirtola_core::api::{GamePlaytime, PlaytimeReport};

const TICK: Duration = Duration::from_secs(15);
const REPORT_EVERY: Duration = Duration::from_secs(5 * 60);
pub const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);
/// A gap much longer than a tick means the PC slept; that isn't play.
const MAX_STEP: Duration = Duration::from_secs(60);
const MAX_BATCH: usize = 50;

/// A queued report and the account that played it (reports are only ever
/// sent while that account is signed in).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Queued {
    user_id: String,
    report: PlaytimeReport,
}

static PENDING_LOCK: Mutex<()> = Mutex::new(());
static FLUSHING: AtomicBool = AtomicBool::new(false);
/// The website's latest totals for the signed-in player.
static TOTALS: Mutex<Vec<GamePlaytime>> = Mutex::new(Vec::new());

fn pending_path(state: &AppState) -> PathBuf {
    state.data_dir.join("playtime-pending.json")
}

fn load_pending(state: &AppState) -> HashMap<String, Queued> {
    std::fs::read(pending_path(state))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_pending(state: &AppState, pending: &HashMap<String, Queued>) {
    if let Ok(bytes) = serde_json::to_vec(pending) {
        let _ = std::fs::write(pending_path(state), bytes);
    }
}

fn queue(state: &AppState, user_id: &str, report: PlaytimeReport) {
    let _g = PENDING_LOCK.lock().unwrap();
    let mut pending = load_pending(state);
    pending.insert(
        report.session_id.clone(),
        Queued {
            user_id: user_id.to_string(),
            report,
        },
    );
    save_pending(state, &pending);
}

fn new_session_id() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 18];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    zirtola_core::b64::encode_url(&buf)
}

/// A running game's tracker. Call [`Tracker::finish`] when the game exits.
pub struct Tracker {
    stop: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Tracker {
    /// Record the final totals and send them. Blocks only for the last tick.
    pub fn finish(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

pub fn start(app: AppHandle, user_id: String, game_id: String, root: PathBuf) -> Tracker {
    let (stop, rx) = mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        let session_id = new_session_id();
        let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut probe = activity::Probe::new(&root);
        let (mut active, mut total) = (Duration::ZERO, Duration::ZERO);
        let mut last = Instant::now();
        let mut last_report = Instant::now();
        loop {
            let finished = !matches!(rx.recv_timeout(TICK), Err(mpsc::RecvTimeoutError::Timeout));
            let step = last.elapsed().min(MAX_STEP);
            last = Instant::now();
            total += step;
            if probe.active() {
                active += step;
            }
            if finished || last_report.elapsed() >= REPORT_EVERY {
                last_report = Instant::now();
                let state = app.state::<AppState>();
                queue(
                    &state,
                    &user_id,
                    PlaytimeReport {
                        game_id: game_id.clone(),
                        session_id: session_id.clone(),
                        started_at: started_at.clone(),
                        active_seconds: active.as_secs(),
                        total_seconds: total.as_secs(),
                        ended: finished,
                    },
                );
                tauri::async_runtime::spawn(flush(app.clone()));
            }
            if finished {
                break;
            }
        }
    });
    Tracker { stop, thread }
}

/// Send queued reports (signed in only). Accepted reports leave the queue,
/// as do ones the website refuses for good (bad or unknown game); the rest
/// stay for the next try. Updates the cached totals and tells the UI.
pub async fn flush(app: AppHandle) {
    if FLUSHING.swap(true, Ordering::SeqCst) {
        return;
    }
    flush_inner(&app).await;
    FLUSHING.store(false, Ordering::SeqCst);
}

async fn flush_inner(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(user_id) = current_user(&state).await else {
        return;
    };
    let batch: Vec<PlaytimeReport> = {
        let _g = PENDING_LOCK.lock().unwrap();
        load_pending(&state)
            .into_values()
            .filter(|q| q.user_id == user_id)
            .map(|q| q.report)
            .take(MAX_BATCH)
            .collect()
    };
    if batch.is_empty() {
        return;
    }
    let sent = batch.clone();
    let res = state
        .authed(|at| {
            let api = state.api.clone();
            let reports = sent.clone();
            async move { api.post_playtime(&at, &reports).await }
        })
        .await;
    let Ok(p) = res else { return };
    {
        let _g = PENDING_LOCK.lock().unwrap();
        let mut pending = load_pending(&state);
        for r in &p.results {
            let permanent = matches!(r.error.as_deref(), Some("bad_request" | "not_found"));
            if !(r.ok || permanent) {
                continue;
            }
            // Only drop it if no newer heartbeat replaced it meanwhile.
            let unchanged = batch.iter().find(|b| b.session_id == r.session_id)
                == pending.get(&r.session_id).map(|q| &q.report);
            if unchanged {
                pending.remove(&r.session_id);
            }
        }
        save_pending(&state, &pending);
    }
    *TOTALS.lock().unwrap() = p.games;
    let _ = app.emit("playtime-updated", ());
}

/// Active seconds per game: the website's totals plus anything still queued.
pub async fn active_seconds(state: &AppState) -> HashMap<String, u64> {
    let Some(user_id) = current_user(state).await else {
        return HashMap::new();
    };
    let fetched = state
        .authed(|at| {
            let api = state.api.clone();
            async move { api.playtime(&at).await }
        })
        .await;
    if let Ok(p) = fetched {
        *TOTALS.lock().unwrap() = p.games;
    }
    let mut out: HashMap<String, u64> = TOTALS
        .lock()
        .unwrap()
        .iter()
        .map(|g| (g.game_id.clone(), g.active_seconds))
        .collect();
    let _g = PENDING_LOCK.lock().unwrap();
    for q in load_pending(state)
        .values()
        .filter(|q| q.user_id == user_id)
    {
        *out.entry(q.report.game_id.clone()).or_default() += q.report.active_seconds;
    }
    out
}

pub async fn current_user(state: &AppState) -> Option<String> {
    state.session.lock().await.as_ref().map(|s| s.me.id.clone())
}

/// Forget the cached totals (sign-out / account switch).
pub fn clear_cache() {
    TOTALS.lock().unwrap().clear();
}

mod activity {
    use super::IDLE_AFTER;
    use std::path::Path;

    /// Decides whether the player is actively playing right now.
    pub struct Probe {
        #[cfg(windows)]
        inner: win::Probe,
    }

    impl Probe {
        pub fn new(_root: &Path) -> Self {
            Probe {
                #[cfg(windows)]
                inner: win::Probe::new(_root),
            }
        }

        /// The game is in front and the player gave input within IDLE_AFTER.
        /// Elsewhere (development builds on other systems) every tick counts.
        pub fn active(&mut self) -> bool {
            #[cfg(windows)]
            {
                self.inner.active()
            }
            #[cfg(not(windows))]
            {
                let _ = IDLE_AFTER;
                true
            }
        }
    }

    #[cfg(windows)]
    mod win {
        use super::IDLE_AFTER;
        use std::path::Path;
        use std::time::Instant;
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        pub struct Probe {
            /// Install folder, lower-case with a trailing separator.
            root: String,
            pads: [Option<u32>; 4],
            last_pad: Option<Instant>,
        }

        impl Probe {
            pub fn new(root: &Path) -> Self {
                let mut r = root.to_string_lossy().to_lowercase().replace('/', "\\");
                if !r.ends_with('\\') {
                    r.push('\\');
                }
                Probe {
                    root: r,
                    pads: [None; 4],
                    last_pad: None,
                }
            }

            pub fn active(&mut self) -> bool {
                // Poll the controllers every tick so a change is noticed even
                // when the game isn't in front at this moment.
                let pad = self.pad_recently_used();
                self.game_in_front()
                    && (pad || keyboard_mouse_idle_ms() < IDLE_AFTER.as_millis() as u32)
            }

            /// Any program from the game's install folder owns the foreground
            /// window (games often run a second .exe after a bootstrapper).
            fn game_in_front(&self) -> bool {
                unsafe {
                    let hwnd = GetForegroundWindow();
                    if hwnd.0.is_null() {
                        return false;
                    }
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    if pid == 0 {
                        return false;
                    }
                    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                        return false;
                    };
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    let ok = QueryFullProcessImageNameW(
                        h,
                        PROCESS_NAME_WIN32,
                        PWSTR(buf.as_mut_ptr()),
                        &mut len,
                    )
                    .is_ok();
                    let _ = CloseHandle(h);
                    ok && String::from_utf16_lossy(&buf[..len as usize])
                        .to_lowercase()
                        .starts_with(&self.root)
                }
            }

            /// XInput's packet number changes whenever a controller's state
            /// does, so a different number since the last tick means input.
            fn pad_recently_used(&mut self) -> bool {
                for (i, seen) in self.pads.iter_mut().enumerate() {
                    let mut st = XINPUT_STATE::default();
                    let connected = unsafe { XInputGetState(i as u32, &mut st) } == 0;
                    let now = connected.then_some(st.dwPacketNumber);
                    if let (Some(prev), Some(cur)) = (*seen, now) {
                        if prev != cur {
                            self.last_pad = Some(Instant::now());
                        }
                    }
                    *seen = now;
                }
                self.last_pad.is_some_and(|t| t.elapsed() < IDLE_AFTER)
            }
        }

        fn keyboard_mouse_idle_ms() -> u32 {
            unsafe {
                let mut lii = LASTINPUTINFO {
                    cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                    dwTime: 0,
                };
                if !GetLastInputInfo(&mut lii).as_bool() {
                    return 0;
                }
                GetTickCount().wrapping_sub(lii.dwTime)
            }
        }
    }
}
