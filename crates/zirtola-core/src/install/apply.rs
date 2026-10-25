//! Install, patch and repair: bring an install folder to a target manifest.

use super::*;
use crate::manifest::{FileEntry, Manifest, SignedManifest};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("cancelled")]
    Cancelled,
    #[error("not enough disk space: {needed} bytes needed, {available} bytes free")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("the download link expired")]
    ChunkUrlExpired,
    #[error("{0} is in use. Close the game and try again.")]
    FileInUse(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        InstallError::Other(e.into())
    }
}

pub struct ApplyOptions {
    /// Concurrent chunk transfers.
    pub parallel: usize,
    /// HTTP client for chunk downloads (should have connect/read timeouts, no total timeout).
    pub http: reqwest::Client,
    /// URL template with `{hash}` and optionally `{prefix}`.
    pub chunk_url_template: String,
    /// Files to rebuild even if they look unchanged (repair after verify).
    pub force_rebuild: HashSet<String>,
    pub cancel: CancellationToken,
    pub progress: Option<ProgressFn>,
    /// Space to leave free on the volume after the job, in bytes.
    pub space_margin: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Journal {
    pub build_id: String,
    pub version: String,
    /// Set once every chunk is in place and renames have started.
    pub committing: bool,
    /// Files being rebuilt as `.zpart`.
    pub files: Vec<String>,
    /// Path -> indices of chunks already written and verified.
    pub done: HashMap<String, Vec<u32>>,
}

impl Journal {
    pub fn path(root: &Path) -> PathBuf {
        state_dir(root).join("pending.json")
    }
    pub fn load(root: &Path) -> Option<Journal> {
        std::fs::read(Self::path(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    }
    fn save(&self, root: &Path) -> std::io::Result<()> {
        write_atomic(
            &Self::path(root),
            &serde_json::to_vec(self).unwrap_or_default(),
        )
    }
}

struct Task {
    file: usize,
    index: u32,
    hash: String,
    offset: u64,
    size: u64,
}

struct Ctx {
    http: reqwest::Client,
    template: String,
    parts: Vec<Arc<std::fs::File>>,
    local: HashMap<String, (PathBuf, u64, u64)>,
    network: AtomicU64,
    reused: AtomicU64,
}

fn report(opts: &ApplyOptions, p: Progress) {
    if let Some(f) = &opts.progress {
        f(&p);
    }
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub(crate) fn read_at(path: &Path, offset: u64, size: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg_attr(unix, allow(unused_mut))]
fn write_at(f: &std::fs::File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        while !data.is_empty() {
            let n = f.seek_write(data, offset)?;
            if n == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            data = &data[n..];
            offset += n as u64;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(data, offset)
    }
}

impl Ctx {
    fn url_for(&self, hash: &str) -> String {
        self.template
            .replace("{hash}", hash)
            .replace("{prefix}", &hash[..2])
    }

    async fn fetch(&self, t: &Task) -> Result<Vec<u8>, InstallError> {
        let url = self.url_for(&t.hash);
        let mut last_err = String::new();
        for attempt in 0..5u32 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
            }
            let resp = match self.http.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = e.to_string();
                    continue;
                }
            };
            let status = resp.status().as_u16();
            if status == 401 || status == 403 {
                return Err(InstallError::ChunkUrlExpired);
            }
            if status == 404 {
                return Err(anyhow::anyhow!("chunk {} is missing on the server", t.hash).into());
            }
            if !resp.status().is_success() {
                last_err = format!("HTTP {status}");
                continue;
            }
            let mut buf = Vec::with_capacity(t.size as usize);
            let mut stream = resp.bytes_stream();
            let mut failed = false;
            while let Some(piece) = stream.next().await {
                match piece {
                    Ok(b) => {
                        if (buf.len() + b.len()) as u64 > t.size {
                            failed = true;
                            last_err = "chunk larger than expected".into();
                            break;
                        }
                        self.network.fetch_add(b.len() as u64, Ordering::Relaxed);
                        buf.extend_from_slice(&b);
                    }
                    Err(e) => {
                        failed = true;
                        last_err = e.to_string();
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            let data = buf;
            let hash = tokio::task::spawn_blocking(move || {
                let h = sha256_hex(&data);
                (h, data)
            })
            .await
            .map_err(anyhow::Error::from)?;
            if hash.0 == t.hash {
                return Ok(hash.1);
            }
            last_err = "chunk failed hash check".into();
        }
        Err(anyhow::anyhow!("could not download chunk {}: {}", &t.hash[..12], last_err).into())
    }

    /// Returns (file, index, size, reused).
    async fn run(self: Arc<Self>, t: Task) -> Result<(usize, u32, u64, bool), InstallError> {
        // 1. Reuse an identical chunk from the current install.
        if let Some((src, off, size)) = self.local.get(&t.hash).cloned() {
            if size == t.size {
                let want = t.hash.clone();
                let file = self.parts[t.file].clone();
                let offset = t.offset;
                let ok = tokio::task::spawn_blocking(move || -> bool {
                    match read_at(&src, off, size) {
                        Ok(data) if sha256_hex(&data) == want => {
                            write_at(&file, offset, &data).is_ok()
                        }
                        _ => false,
                    }
                })
                .await
                .unwrap_or(false);
                if ok {
                    self.reused.fetch_add(t.size, Ordering::Relaxed);
                    return Ok((t.file, t.index, t.size, true));
                }
            }
        }
        // 2. Download.
        let data = self.fetch(&t).await?;
        let file = self.parts[t.file].clone();
        let offset = t.offset;
        tokio::task::spawn_blocking(move || write_at(&file, offset, &data))
            .await
            .map_err(anyhow::Error::from)??;
        Ok((t.file, t.index, t.size, false))
    }
}

fn is_in_use(e: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33), or access denied while a process holds the file.
    matches!(e.raw_os_error(), Some(32) | Some(33) | Some(5))
        || e.kind() == std::io::ErrorKind::PermissionDenied
}

/// Bring `root` to `target`. Resumes an interrupted job for the same build.
pub async fn apply(
    root: &Path,
    target_env: &SignedManifest,
    target: &Manifest,
    opts: ApplyOptions,
) -> Result<(), InstallError> {
    let root = root.to_path_buf();
    let mut prog = Progress {
        phase: Phase::Preparing,
        total_bytes: 0,
        done_bytes: 0,
        network_bytes: 0,
        reused_bytes: 0,
        download_bytes: 0,
    };
    report(&opts, prog.clone());
    std::fs::create_dir_all(state_dir(&root))?;

    let current = installed_manifest(&root)?.map(|(_, m)| m);

    // Journal: resume the same build, otherwise clean up the old attempt.
    let mut journal = match Journal::load(&root) {
        Some(j) if j.build_id == target.build_id => j,
        Some(old) => {
            for f in &old.files {
                if let Ok(p) = file_path(&root, f) {
                    let _ = std::fs::remove_file(part_path(&p));
                }
            }
            Journal::default()
        }
        None => Journal::default(),
    };
    journal.build_id = target.build_id.clone();
    journal.version = target.version.clone();

    let build: Vec<&FileEntry> = if journal.committing {
        target
            .files
            .iter()
            .filter(|f| journal.files.contains(&f.path))
            .collect()
    } else {
        let mut v = Vec::new();
        for f in &target.files {
            let same_as_current = current
                .as_ref()
                .and_then(|c| c.file(&f.path))
                .map(|cf| cf.size == f.size && cf.chunks == f.chunks)
                .unwrap_or(false);
            let on_disk = file_path(&root, &f.path)?
                .metadata()
                .map(|m| m.is_file() && m.len() == f.size)
                .unwrap_or(false);
            if opts.force_rebuild.contains(&f.path) || !same_as_current || !on_disk {
                v.push(f);
            }
        }
        journal.files = v.iter().map(|f| f.path.clone()).collect();
        // Drop journal entries for files that are no longer being rebuilt.
        let keep: HashSet<&String> = journal.files.iter().collect();
        journal.done.retain(|k, _| keep.contains(k));
        v
    };

    if !journal.committing {
        // Chunks the current install already has, by hash.
        let mut local = HashMap::new();
        if let Some(c) = &current {
            for f in &c.files {
                let p = file_path(&root, &f.path)?;
                if p.is_file() {
                    for ch in &f.chunks {
                        local
                            .entry(ch.hash.clone())
                            .or_insert((p.clone(), ch.offset, ch.size));
                    }
                }
            }
        }

        // Work list, in file/offset order so writes stay close to each file's
        // written region (avoids NTFS zero-filling far ahead of the data).
        let mut tasks = Vec::new();
        let mut total = 0u64;
        let mut done = 0u64;
        let mut to_download = 0u64;
        let mut new_space = 0u64;
        let mut parts = Vec::with_capacity(build.len());
        for (i, f) in build.iter().enumerate() {
            total += f.size;
            let fin = file_path(&root, &f.path)?;
            let part = part_path(&fin);
            if !part.exists() {
                new_space += f.size;
            }
            let done_set: HashSet<u32> = journal
                .done
                .get(&f.path)
                .map(|v| v.iter().copied().collect())
                .unwrap_or_default();
            for (ci, ch) in f.chunks.iter().enumerate() {
                if done_set.contains(&(ci as u32)) {
                    done += ch.size;
                } else {
                    if !local.contains_key(&ch.hash) {
                        to_download += ch.size;
                    }
                    tasks.push(Task {
                        file: i,
                        index: ci as u32,
                        hash: ch.hash.clone(),
                        offset: ch.offset,
                        size: ch.size,
                    });
                }
            }
        }

        let available = available_space(&root)?;
        let needed = new_space + opts.space_margin;
        if needed > available {
            return Err(InstallError::InsufficientSpace { needed, available });
        }

        for f in &build {
            let fin = file_path(&root, &f.path)?;
            if let Some(dir) = fin.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let part = part_path(&fin);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&part)?;
            if file.metadata()?.len() != f.size {
                file.set_len(f.size)?;
            }
            parts.push(Arc::new(file));
        }

        prog.phase = Phase::Downloading;
        prog.total_bytes = total;
        prog.done_bytes = done;
        prog.download_bytes = to_download;
        report(&opts, prog.clone());
        journal.save(&root)?;

        let ctx = Arc::new(Ctx {
            http: opts.http.clone(),
            template: opts.chunk_url_template.clone(),
            parts,
            local,
            network: AtomicU64::new(0),
            reused: AtomicU64::new(0),
        });

        let parallel = opts.parallel.clamp(1, 32);
        let mut stream = futures::stream::iter(tasks.into_iter().map(|t| ctx.clone().run(t)))
            .buffer_unordered(parallel);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut last_save = Instant::now();
        let paths: Vec<String> = build.iter().map(|f| f.path.clone()).collect();

        loop {
            tokio::select! {
                _ = opts.cancel.cancelled() => {
                    journal.save(&root)?;
                    return Err(InstallError::Cancelled);
                }
                _ = tick.tick() => {
                    prog.network_bytes = ctx.network.load(Ordering::Relaxed);
                    prog.reused_bytes = ctx.reused.load(Ordering::Relaxed);
                    report(&opts, prog.clone());
                }
                next = stream.next() => {
                    match next {
                        None => break,
                        Some(Ok((file, index, size, _reused))) => {
                            journal.done.entry(paths[file].clone()).or_default().push(index);
                            prog.done_bytes += size;
                            if last_save.elapsed() > Duration::from_secs(2) {
                                journal.save(&root)?;
                                last_save = Instant::now();
                            }
                        }
                        Some(Err(e)) => {
                                    journal.save(&root)?;
                            return Err(e);
                        }
                    }
                }
            }
        }
        drop(stream);
        prog.network_bytes = ctx.network.load(Ordering::Relaxed);
        prog.reused_bytes = ctx.reused.load(Ordering::Relaxed);

        // Flush staged files and release handles before renaming.
        let ctx = Arc::try_unwrap(ctx)
            .map_err(|_| anyhow::anyhow!("internal: chunk tasks still running"))?;
        for f in ctx.parts {
            f.sync_all()?;
        }
        journal.committing = true;
        journal.save(&root)?;
    }

    // Commit.
    prog.phase = Phase::Committing;
    report(&opts, prog.clone());
    for f in &build {
        let fin = file_path(&root, &f.path)?;
        let part = part_path(&fin);
        if part.exists() {
            if let Err(e) = std::fs::rename(&part, &fin) {
                if is_in_use(&e) {
                    return Err(InstallError::FileInUse(f.path.clone()));
                }
                return Err(e.into());
            }
        } else if !fin.is_file() {
            return Err(anyhow::anyhow!("staged file for {} is missing", f.path).into());
        }
    }
    if let Some(c) = &current {
        let keep: HashSet<&str> = target.files.iter().map(|f| f.path.as_str()).collect();
        for f in &c.files {
            if !keep.contains(f.path.as_str()) {
                let p = file_path(&root, &f.path)?;
                match std::fs::remove_file(&p) {
                    Ok(()) => super::verify::prune_empty_dirs(&root, &p),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) if is_in_use(&e) => return Err(InstallError::FileInUse(f.path.clone())),
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    write_atomic(
        &state_dir(&root).join("manifest.json"),
        &serde_json::to_vec_pretty(target_env).map_err(anyhow::Error::from)?,
    )?;
    let _ = std::fs::remove_file(Journal::path(&root));

    prog.phase = Phase::Done;
    prog.done_bytes = prog.total_bytes;
    report(&opts, prog);
    Ok(())
}
