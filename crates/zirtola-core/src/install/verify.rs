//! Verify (re-hash every chunk on disk) and uninstall.

use super::*;
use crate::manifest::Manifest;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default, Serialize)]
pub struct VerifyReport {
    pub checked_bytes: u64,
    /// Files with at least one chunk that failed its hash or with the wrong size.
    pub damaged: Vec<String>,
    /// Files that do not exist.
    pub missing: Vec<String>,
}

impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.damaged.is_empty() && self.missing.is_empty()
    }
}

enum FileState {
    Ok,
    Damaged,
    Missing,
}

fn check_file(
    path: &Path,
    f: &crate::manifest::FileEntry,
    counter: &AtomicU64,
    cancel: &CancellationToken,
) -> std::io::Result<FileState> {
    use std::io::Read;
    let md = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(e) => return Err(e),
    };
    if md.len() != f.size {
        counter.fetch_add(f.size, Ordering::Relaxed);
        return Ok(FileState::Damaged);
    }
    let mut file = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut buf = Vec::new();
    let mut damaged = false;
    for ch in &f.chunks {
        if cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        buf.resize(ch.size as usize, 0);
        file.read_exact(&mut buf)?;
        if hex::encode(Sha256::digest(&buf)) != ch.hash {
            damaged = true;
        }
        counter.fetch_add(ch.size, Ordering::Relaxed);
    }
    Ok(if damaged {
        FileState::Damaged
    } else {
        FileState::Ok
    })
}

/// Re-hash every chunk of every file of `m` under `root`.
pub async fn verify(
    root: &Path,
    m: &Manifest,
    progress: Option<ProgressFn>,
    cancel: CancellationToken,
) -> anyhow::Result<VerifyReport> {
    let counter = Arc::new(AtomicU64::new(0));
    let total = m.total_size;
    let emit = |done: u64| {
        if let Some(p) = &progress {
            p(&Progress {
                phase: Phase::Verifying,
                total_bytes: total,
                done_bytes: done,
                network_bytes: 0,
                reused_bytes: 0,
                download_bytes: 0,
            });
        }
    };
    emit(0);

    let jobs = m.files.iter().cloned().map(|f| {
        let path = file_path(root, &f.path);
        let counter = counter.clone();
        let cancel = cancel.clone();
        async move {
            let path = path?;
            let res = tokio::task::spawn_blocking(move || {
                check_file(&path, &f, &counter, &cancel).map(|s| (f.path, s))
            })
            .await??;
            anyhow::Ok(res)
        }
    });
    // Two files at a time: enough to keep an NVMe busy without thrashing a hard drive.
    let mut stream = futures::stream::iter(jobs).buffer_unordered(2);
    let mut report = VerifyReport::default();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = tick.tick() => emit(counter.load(Ordering::Relaxed)),
            next = stream.next() => match next {
                None => break,
                Some(Err(e)) => {
                    if cancel.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }
                    return Err(e);
                }
                Some(Ok((path, FileState::Damaged))) => report.damaged.push(path),
                Some(Ok((path, FileState::Missing))) => report.missing.push(path),
                Some(Ok((_, FileState::Ok))) => {}
            }
        }
    }
    report.checked_bytes = counter.load(Ordering::Relaxed);
    emit(total);
    Ok(report)
}

/// Remove empty directories from `file`'s parent up to (not including) `root`.
pub(crate) fn prune_empty_dirs(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        if std::fs::remove_dir(d).is_err() {
            break; // not empty (or in use)
        }
        dir = d.parent();
    }
}

/// Delete the files of the installed build (and staging leftovers), then the
/// folder itself if nothing else is left in it. Files the manifest does not
/// list (e.g. a user's own mods or screenshots) are left alone.
pub fn uninstall(root: &Path) -> anyhow::Result<()> {
    let mut paths: Vec<String> = Vec::new();
    if let Some((_, m)) = installed_manifest(root)? {
        paths.extend(m.files.iter().map(|f| f.path.clone()));
    }
    if let Some(j) = super::apply::Journal::load(root) {
        paths.extend(j.files);
    }
    for rel in paths {
        let p = file_path(root, &rel)?;
        for candidate in [p.clone(), part_path(&p)] {
            match std::fs::remove_file(&candidate) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => anyhow::bail!("could not delete {}: {e}", candidate.display()),
            }
        }
        prune_empty_dirs(root, &p);
    }
    let sd = state_dir(root);
    if sd.exists() {
        std::fs::remove_dir_all(&sd)?;
    }
    let _ = std::fs::remove_dir(root);
    Ok(())
}
