//! Install / patch / verify / repair / uninstall engine.
//!
//! Layout of an install folder:
//!
//! ```text
//! <install root>/
//!   <game files...>
//!   .zirtola/
//!     manifest.json   signed manifest of the installed build (written last, on commit)
//!     pending.json    journal of an unfinished install or patch (resume point)
//! ```
//!
//! Every changed file is rebuilt next to the original as `<file>.zpart` from
//! chunks that are either copied from the current install (when a chunk with
//! the same hash already exists locally) or downloaded. Each chunk is hashed
//! before it is written. When every chunk is in place the `.zpart` files are
//! renamed over the originals, files that no longer exist in the new build are
//! deleted and the new manifest is written. Until that commit the old build
//! stays playable, and an interrupted job resumes from the journal.

mod apply;
mod registry;
mod verify;

pub use apply::{apply, ApplyOptions, InstallError, Journal};
pub use registry::{InstallRecord, Registry};
pub use verify::{uninstall, verify, VerifyReport};

use crate::manifest::{safe_relative_path, Manifest, SignedManifest};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STATE_DIR: &str = ".zirtola";
pub const PART_SUFFIX: &str = ".zpart";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Downloading,
    Verifying,
    Committing,
    Uninstalling,
    Done,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub phase: Phase,
    /// Bytes of work for this job (bytes to build, or bytes to verify).
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// Bytes fetched over the network so far in this run.
    pub network_bytes: u64,
    /// Bytes reused from the existing install so far in this run.
    pub reused_bytes: u64,
    /// Bytes that must actually be downloaded for this job (excludes reuse), best effort.
    pub download_bytes: u64,
}

pub type ProgressFn = std::sync::Arc<dyn Fn(&Progress) + Send + Sync>;

pub fn state_dir(root: &Path) -> PathBuf {
    root.join(STATE_DIR)
}

/// Load the installed build's manifest, if any. It was verified when it was
/// installed, so it is parsed without re-checking the signature.
pub fn installed_manifest(root: &Path) -> anyhow::Result<Option<(SignedManifest, Manifest)>> {
    let p = state_dir(root).join("manifest.json");
    if !p.exists() {
        return Ok(None);
    }
    let env: SignedManifest = serde_json::from_slice(&std::fs::read(&p)?)?;
    let bytes = crate::b64::decode_any(&env.manifest)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    m.validate()?;
    Ok(Some((env, m)))
}

/// Absolute path of a manifest file inside `root`. Refuses to go through a
/// symbolic link or junction inside the install folder, so a write or delete
/// can never land outside it (for example in the player's save folder).
pub fn file_path(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = safe_relative_path(rel)?;
    let mut cur = root.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(md) if is_link(&md) => anyhow::bail!(
                "refusing to follow a link inside the install folder: {}",
                cur.display()
            ),
            Ok(_) => {}
            Err(_) => break, // doesn't exist yet: nothing below it can be a link
        }
    }
    Ok(root.join(rel))
}

// On Windows this is true for symbolic links and junctions (name-surrogate
// reparse points) but not for OneDrive placeholders and other reparse points.
fn is_link(md: &std::fs::Metadata) -> bool {
    md.file_type().is_symlink()
}

pub fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(PART_SUFFIX);
    PathBuf::from(s)
}

/// Free bytes on the volume containing `path` (walks up to an existing ancestor).
pub fn available_space(path: &Path) -> anyhow::Result<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            anyhow::bail!("no existing ancestor for {}", path.display());
        }
    }
    Ok(fs2::available_space(&p)?)
}

/// Write `data` to `path` atomically (temp file + rename).
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = part_path(path);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Game ids are lowercase slugs; anything else is refused before it reaches a URL or a path.
pub fn valid_game_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Launch the installed game with a session token in its environment.
pub fn launch(
    root: &Path,
    m: &Manifest,
    session_token: &str,
    api_base: &str,
) -> anyhow::Result<std::process::Child> {
    let exe = file_path(root, &m.launch.executable)?;
    if !exe.is_file() {
        anyhow::bail!("game executable is missing; run Verify to repair the install");
    }
    let wd = match &m.launch.working_dir {
        Some(w) => file_path(root, w)?,
        None => exe.parent().unwrap_or(root).to_path_buf(),
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&m.launch.args)
        .arg("-zirtola")
        .current_dir(wd)
        .env("ZIRTOLA_SESSION_TOKEN", session_token)
        .env("ZIRTOLA_GAME_ID", &m.game_id)
        .env("ZIRTOLA_GAME_VERSION", &m.version)
        .env("ZIRTOLA_API_BASE", api_base);
    Ok(cmd.spawn()?)
}
