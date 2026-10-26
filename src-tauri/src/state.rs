//! Shared launcher state: API client, trusted keys, session, settings, installs, jobs.

use crate::auth::{self, Tokens};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use zirtola_core::api::{ApiClient, ApiError, Me, ServiceConfig};
use zirtola_core::install::Registry;
use zirtola_core::TrustedKeys;

/// Manifest/launch keys shipped inside the launcher. Production keys go here
/// (public keys only); see README "Signing keys".
const PINNED_KEYS: &str = include_str!("../keys/trusted-keys.json");

/// Builds that trust keys served by a loopback API (the mock server) and honour
/// a runtime `ZIRTOLA_API_BASE`. Never true for customer release builds.
pub fn dev_mode() -> bool {
    cfg!(debug_assertions) || option_env!("ZIRTOLA_DEV_BUILD").is_some()
}

pub fn api_base() -> String {
    if dev_mode() {
        if let Ok(v) = std::env::var("ZIRTOLA_API_BASE") {
            return v;
        }
    }
    match option_env!("ZIRTOLA_API_BASE") {
        Some(v) => v.to_string(),
        None if dev_mode() => "http://127.0.0.1:8787".to_string(),
        None => "https://zirtola.com/api/launcher".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Default parent folder for new installs.
    pub library_dir: PathBuf,
    /// Concurrent chunk downloads.
    pub parallel_downloads: usize,
    /// Install launcher updates found at start-up (a required update, below the
    /// website's min_launcher_version, installs regardless).
    pub auto_update_launcher: bool,
    /// Start game updates found at start-up and before Play.
    pub auto_update_games: bool,
    /// Send the hardware summary (see hardware.rs) to the website.
    pub share_hardware_info: bool,
    /// Ask to send a crash report when a game crashes.
    pub offer_crash_reports: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        Settings {
            library_dir: PathBuf::from(format!("{drive}\\Zirtola Games")),
            parallel_downloads: 8,
            auto_update_launcher: true,
            auto_update_games: true,
            share_hardware_info: true,
            offer_crash_reports: true,
        }
    }
}

pub struct Session {
    pub tokens: Tokens,
    pub me: Me,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Install,
    Update,
    Verify,
    Uninstall,
}

pub struct Job {
    pub kind: JobKind,
    pub cancel: CancellationToken,
}

pub struct AppState {
    pub api: ApiClient,
    pub dl_http: reqwest::Client,
    pub data_dir: PathBuf,
    pub config: RwLock<Option<ServiceConfig>>,
    pub keys: RwLock<TrustedKeys>,
    pub session: Mutex<Option<Session>>,
    pub registry: Mutex<Registry>,
    pub settings: Mutex<Settings>,
    pub jobs: std::sync::Mutex<HashMap<String, Job>>,
    pub login_cancel: std::sync::Mutex<Option<CancellationToken>>,
    /// Crash reports waiting for the player's decision, by id.
    pub crashes: std::sync::Mutex<HashMap<String, crate::crash::CrashCandidate>>,
    /// Last collected hardware report (for the device label and Settings).
    pub hardware: std::sync::Mutex<Option<crate::hardware::HardwareReport>>,
}

#[derive(Debug, Serialize)]
pub struct CmdError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl CmdError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        CmdError {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
}

impl From<anyhow::Error> for CmdError {
    fn from(e: anyhow::Error) -> Self {
        if let Some(api) = e.downcast_ref::<ApiError>() {
            return CmdError::new(api.code(), api.to_string());
        }
        CmdError::new("error", format!("{e:#}"))
    }
}

impl From<ApiError> for CmdError {
    fn from(e: ApiError) -> Self {
        let details = match &e {
            ApiError::Api { details, .. } => details.clone(),
            _ => None,
        };
        CmdError {
            details,
            ..CmdError::new(e.code(), e.to_string())
        }
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::new("io_error", e.to_string())
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

impl AppState {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let api = ApiClient::new(&api_base())?;
        let dl_http = reqwest::Client::builder()
            .user_agent(zirtola_core::api::USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(32)
            .build()?;
        let settings: Settings = std::fs::read(data_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(AppState {
            api,
            dl_http,
            registry: Mutex::new(Registry::load(&data_dir.join("installs.json"))),
            settings: Mutex::new(settings),
            data_dir,
            config: RwLock::new(None),
            keys: RwLock::new(TrustedKeys::from_json(PINNED_KEYS)?),
            session: Mutex::new(None),
            jobs: std::sync::Mutex::new(HashMap::new()),
            login_cancel: std::sync::Mutex::new(None),
            crashes: std::sync::Mutex::new(HashMap::new()),
            hardware: std::sync::Mutex::new(None),
        })
    }

    pub async fn save_settings(&self) -> std::io::Result<()> {
        let s = self.settings.lock().await.clone();
        std::fs::write(
            self.data_dir.join("settings.json"),
            serde_json::to_vec_pretty(&s).unwrap_or_default(),
        )
    }

    /// Service configuration (OAuth endpoints etc.), fetched once. In dev mode
    /// this also trusts the signing keys a loopback API publishes.
    pub async fn config(&self) -> Result<ServiceConfig, CmdError> {
        if let Some(c) = self.config.read().await.clone() {
            return Ok(c);
        }
        let cfg = self.api.config().await?;
        if dev_mode() && self.api.is_loopback() {
            match self.api.keys().await {
                Ok(set) => {
                    let mut k = self.keys.write().await;
                    if let Err(e) = k.add_set(&set) {
                        log::warn!("ignoring dev keys: {e}");
                    }
                }
                Err(e) => log::warn!("could not fetch dev keys: {e}"),
            }
        }
        *self.config.write().await = Some(cfg.clone());
        Ok(cfg)
    }

    /// A valid access token, refreshing it if needed.
    pub async fn access_token(&self) -> Result<String, CmdError> {
        self.access_token_inner(false).await
    }

    pub async fn access_token_inner(&self, force_refresh: bool) -> Result<String, CmdError> {
        let cfg = self.config().await?;
        let mut guard = self.session.lock().await;
        let Some(sess) = guard.as_mut() else {
            return Err(CmdError::new("signed_out", "Please sign in."));
        };
        if !force_refresh && sess.tokens.expires_at > Instant::now() + Duration::from_secs(60) {
            return Ok(sess.tokens.access_token.clone());
        }
        let Some(rt) = sess.tokens.refresh_token.clone() else {
            *guard = None;
            return Err(CmdError::new(
                "signed_out",
                "Your session ended. Please sign in again.",
            ));
        };
        match auth::refresh(&self.api, &cfg, &rt).await {
            Ok(t) => {
                if let Some(new_rt) = &t.refresh_token {
                    if *new_rt != rt {
                        auth::store_refresh_token(self.api.base(), new_rt);
                    }
                }
                sess.tokens = t;
                Ok(sess.tokens.access_token.clone())
            }
            Err(e) if e.status() == Some(400) || e.status() == Some(401) => {
                *guard = None;
                auth::clear_refresh_token(self.api.base());
                Err(CmdError::new(
                    "signed_out",
                    "Your session ended. Please sign in again.",
                ))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Run an authenticated API call, refreshing the token once on 401.
    pub async fn authed<T, F, Fut>(&self, f: F) -> Result<T, CmdError>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, ApiError>>,
    {
        let at = self.access_token().await?;
        match f(at).await {
            Err(e) if e.is_unauthorized() => {
                let at = self.access_token_inner(true).await?;
                Ok(f(at).await?)
            }
            r => Ok(r?),
        }
    }

    /// Try to restore a session from the stored refresh token.
    pub async fn restore_session(&self) -> Result<Option<Me>, CmdError> {
        if let Some(s) = self.session.lock().await.as_ref() {
            return Ok(Some(s.me.clone()));
        }
        let Some(rt) = auth::load_refresh_token(self.api.base()) else {
            return Ok(None);
        };
        let cfg = self.config().await?;
        let tokens = match auth::refresh(&self.api, &cfg, &rt).await {
            Ok(t) => t,
            Err(e) if e.status() == Some(400) || e.status() == Some(401) => {
                auth::clear_refresh_token(self.api.base());
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        self.start_session(tokens).await.map(Some)
    }

    pub async fn start_session(&self, tokens: Tokens) -> Result<Me, CmdError> {
        let me = self.api.me(&tokens.access_token).await?;
        if let Some(rt) = &tokens.refresh_token {
            auth::store_refresh_token(self.api.base(), rt);
        }
        *self.session.lock().await = Some(Session {
            tokens,
            me: me.clone(),
        });
        Ok(me)
    }

    /// Device id for licence activations: a salted hash of stable hardware ids
    /// (the motherboard's SMBIOS UUID, else Windows' MachineGuid), so it stays
    /// the same when the launcher is reinstalled or Windows updates. The raw
    /// ids never leave the PC. Falls back to a random id saved on disk.
    pub fn device_id(&self) -> String {
        if let Some(id) = crate::hardware::stable_device_id() {
            return id;
        }
        let path = self.data_dir.join("device-id");
        if let Ok(id) = std::fs::read_to_string(&path) {
            let id = id.trim().to_string();
            if id.len() >= 16 {
                return id;
            }
        }
        use rand::RngCore;
        let mut buf = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut buf);
        let id = zirtola_core::b64::encode_url(&buf);
        let _ = std::fs::write(&path, &id);
        id
    }

    /// Crash folders the player already decided about (sent or dismissed).
    pub fn crash_seen(&self) -> std::collections::HashSet<String> {
        std::fs::read(self.data_dir.join("crash-seen.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn mark_crash_seen(&self, key: &str) {
        let mut seen = self.crash_seen();
        seen.insert(key.to_string());
        let _ = std::fs::write(
            self.data_dir.join("crash-seen.json"),
            serde_json::to_vec(&seen).unwrap_or_default(),
        );
    }

    /// Non-identifying device label for the activations list ("Windows 11 Pro · RTX 3070").
    pub fn device_label(&self) -> String {
        self.hardware
            .lock()
            .unwrap()
            .as_ref()
            .map(|h| h.label())
            .unwrap_or_else(|| "Windows PC".into())
    }

    pub fn job_running(&self, game_id: &str) -> Option<JobKind> {
        self.jobs.lock().unwrap().get(game_id).map(|j| j.kind)
    }
}
