//! Crash reports for games started from the launcher.
//!
//! Unreal Engine writes a folder per crash (`Saved/Crashes/UECC-Windows-<guid>_0000/`
//! with `<Project>.log`, `CrashContext.runtime-xml` and `UEMinidump.dmp`). For
//! packaged games that is under `%LOCALAPPDATA%\<Project>\Saved\Crashes`; some
//! setups write next to the game instead. The launcher watches those places:
//! when a game it started exits abnormally or leaves a new crash folder, it asks
//! the player whether to send a report (Settings → "Offer to send crash reports").
//! At start-up it also offers recent crash folders it has not seen yet.

use serde::Serialize;
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use zirtola_core::manifest::Manifest;

/// Total cap for a report zip.
pub const MAX_ZIP_BYTES: u64 = 48 * 1024 * 1024;
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DUMP_BYTES: u64 = 40 * 1024 * 1024;
const MAX_OTHER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct CrashFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashCandidate {
    pub id: String,
    pub game_id: String,
    pub game_title: String,
    pub game_version: String,
    pub build_id: String,
    pub exit_code: Option<i32>,
    /// First line of UE's error message, if a crash folder was found.
    pub error: String,
    pub files: Vec<CrashFile>,
    pub detected_at: String,
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// The project name UE uses for its Saved folder: the executable name without
/// "-Win64-Shipping" style suffixes.
fn project_names(m: &Manifest) -> Vec<String> {
    let exe = Path::new(&m.launch.executable);
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = stem.split('-').next().unwrap_or(&stem).to_string();
    let mut v = vec![stem.clone()];
    if base != stem {
        v.push(base);
    }
    // The first folder in the build is usually the project folder.
    if let Some(first) = m.files.iter().find_map(|f| {
        let mut parts = f.path.split('/');
        let a = parts.next()?;
        parts.next().map(|_| a.to_string())
    }) {
        if first != "Engine" && !v.contains(&first) {
            v.push(first);
        }
    }
    v
}

/// Folders where this game's crash folders can appear.
pub fn crash_roots(install_root: &Path, m: &Manifest) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    for name in project_names(m) {
        if let Some(l) = &local {
            roots.push(l.join(&name).join("Saved").join("Crashes"));
        }
        roots.push(install_root.join(&name).join("Saved").join("Crashes"));
    }
    roots.push(install_root.join("Saved").join("Crashes"));
    roots
}

/// Crash folders under `roots`, with their modification time.
pub fn list_crash_dirs(roots: &[PathBuf]) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for r in roots {
        let Ok(rd) = std::fs::read_dir(r) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let t = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                out.push((p, t));
            }
        }
    }
    out.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    out
}

fn tag(xml: &str, name: &str) -> String {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let Some(a) = xml.find(&open) else {
        return String::new();
    };
    let rest = &xml[a + open.len()..];
    let Some(b) = rest.find(&close) else {
        return String::new();
    };
    rest[..b]
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

/// UE's error message from CrashContext.runtime-xml (first line).
pub fn error_message(dir: &Path) -> String {
    let xml = std::fs::read_to_string(dir.join("CrashContext.runtime-xml")).unwrap_or_default();
    tag(&xml, "ErrorMessage")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

pub fn files_of(dir: &Path) -> Vec<CrashFile> {
    let mut v: Vec<CrashFile> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| CrashFile {
                    name: e.file_name().to_string_lossy().to_string(),
                    size: e.metadata().map(|m| m.len()).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Zip the crash folder: logs (last 8 MB), crash context and other small text
/// files, and the minidump if it is under 40 MB. Total stays under 48 MB.
pub fn zip_crash_dir(dir: &Path) -> anyhow::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut total = 0u64;
        for f in files_of(dir) {
            let lower = f.name.to_ascii_lowercase();
            let (limit, tail) = if lower.ends_with(".log") {
                (MAX_LOG_BYTES, true)
            } else if lower.ends_with(".dmp") {
                (MAX_DUMP_BYTES, false)
            } else if lower.ends_with(".xml")
                || lower.ends_with(".runtime-xml")
                || lower.ends_with(".txt")
                || lower.ends_with(".json")
            {
                (MAX_OTHER_BYTES, true)
            } else {
                continue;
            };
            if !tail && f.size > limit {
                continue; // too big a dump: skip rather than truncate a binary
            }
            let take = f.size.min(limit);
            if total + take > MAX_ZIP_BYTES {
                continue;
            }
            let mut file = std::fs::File::open(dir.join(&f.name))?;
            if f.size > take {
                file.seek(SeekFrom::Start(f.size - take))?;
            }
            let mut data = Vec::with_capacity(take as usize);
            file.take(take).read_to_end(&mut data)?;
            zip.start_file(f.name.as_str(), opts)?;
            zip.write_all(&data)?;
            total += take;
        }
        zip.finish()?;
    }
    Ok(buf.into_inner())
}

/// Crash folders for these roots created after `since` and not in `seen`.
pub fn new_crash_dirs(
    roots: &[PathBuf],
    since: SystemTime,
    seen: &HashSet<String>,
) -> Vec<PathBuf> {
    list_crash_dirs(roots)
        .into_iter()
        .filter(|(p, t)| {
            *t + Duration::from_secs(5) >= since
                && !seen.contains(
                    &p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                )
        })
        .map(|(p, _)| p)
        .collect()
}

pub fn seen_key(dir: &Path) -> String {
    dir.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}
