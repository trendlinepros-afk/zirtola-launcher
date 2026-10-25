//! The launcher's list of installed games (`%APPDATA%\com.zirtola.launcher\installs.json`).
//! The install folder itself is the source of truth for what is on disk; the
//! registry only remembers where each game lives.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRecord {
    pub game_id: String,
    pub path: PathBuf,
    #[serde(default = "live")]
    pub branch: String,
    /// Installed version, or None while the first install is still in progress.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
}

fn live() -> String {
    "live".into()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub installs: BTreeMap<String, InstallRecord>,
    #[serde(skip)]
    file: PathBuf,
}

impl Registry {
    pub fn load(file: &Path) -> Self {
        let mut r: Registry = std::fs::read(file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        r.file = file.to_path_buf();
        r
    }

    pub fn save(&self) -> std::io::Result<()> {
        super::write_atomic(
            &self.file,
            &serde_json::to_vec_pretty(self).unwrap_or_default(),
        )
    }

    pub fn get(&self, game_id: &str) -> Option<&InstallRecord> {
        self.installs.get(game_id)
    }

    pub fn put(&mut self, rec: InstallRecord) {
        self.installs.insert(rec.game_id.clone(), rec);
    }

    pub fn remove(&mut self, game_id: &str) -> Option<InstallRecord> {
        self.installs.remove(game_id)
    }
}
