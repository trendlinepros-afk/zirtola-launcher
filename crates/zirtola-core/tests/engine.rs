//! End-to-end tests of the install engine against a tiny in-process chunk server:
//! fresh install, pause/resume, patching (only changed chunks fetched),
//! verify + repair, expired links and uninstall.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use zirtola_core::install::{self, ApplyOptions, InstallError};
use zirtola_core::keys::generate;
use zirtola_core::manifest::{Chunk, FileEntry, LaunchSpec, Manifest, SignedManifest, SCHEMA};
use zirtola_core::TrustedKeys;

const CHUNK: usize = 64 * 1024;

#[derive(Default)]
struct Server {
    chunks: Mutex<HashMap<String, Vec<u8>>>,
    hits: Mutex<Vec<String>>,
    expired: AtomicBool,
    delay_ms: AtomicU64,
}

async fn serve(server: Arc<Server>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let server = server.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf);
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let hash = path
                    .trim_start_matches("/c/")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let delay = server.delay_ms.load(Ordering::Relaxed);
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                if server.expired.load(Ordering::Relaxed) {
                    let _ = sock.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
                let body = server.chunks.lock().unwrap().get(&hash).cloned();
                match body {
                    Some(b) => {
                        server.hits.lock().unwrap().push(hash);
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            b.len()
                        );
                        let _ = sock.write_all(head.as_bytes()).await;
                        let _ = sock.write_all(&b).await;
                    }
                    None => {
                        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    }
                }
            });
        }
    });
    format!("http://{addr}/c/{{hash}}")
}

fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.extend_from_slice(&x.to_le_bytes());
    }
    out.truncate(len);
    out
}

fn hex256(d: &[u8]) -> String {
    hex::encode(Sha256::digest(d))
}

/// Build a manifest from in-memory files, registering chunks with the server.
fn make_build(server: &Server, version: &str, files: &[(&str, Vec<u8>)]) -> Manifest {
    let mut entries = Vec::new();
    for (path, data) in files {
        let mut chunks = Vec::new();
        for (i, c) in data.chunks(CHUNK).enumerate() {
            let h = hex256(c);
            server.chunks.lock().unwrap().insert(h.clone(), c.to_vec());
            chunks.push(Chunk {
                hash: h,
                offset: (i * CHUNK) as u64,
                size: c.len() as u64,
            });
        }
        entries.push(FileEntry {
            path: path.to_string(),
            size: data.len() as u64,
            sha256: hex256(data),
            chunks,
        });
    }
    Manifest {
        schema: SCHEMA,
        game_id: "test-game".into(),
        version: version.into(),
        build_id: format!("build-{version}"),
        platform: "win64".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        launch: LaunchSpec {
            executable: files[0].0.into(),
            args: vec![],
            working_dir: None,
        },
        total_size: entries.iter().map(|e| e.size).sum(),
        files: entries,
    }
}

fn opts(template: &str, cancel: CancellationToken, force: HashSet<String>) -> ApplyOptions {
    ApplyOptions {
        parallel: 4,
        http: reqwest::Client::new(),
        chunk_url_template: template.to_string(),
        force_rebuild: force,
        cancel,
        progress: None,
        space_margin: 0,
    }
}

fn signed(m: &Manifest) -> (SignedManifest, TrustedKeys) {
    let (sk, jwk) = generate("k", Some("manifest"));
    let mut keys = TrustedKeys::new();
    keys.add_jwk(&jwk).unwrap();
    (SignedManifest::sign(m, "k", &sk).unwrap(), keys)
}

fn assert_files(root: &Path, files: &[(&str, Vec<u8>)]) {
    for (p, data) in files {
        let got = std::fs::read(root.join(p)).unwrap_or_else(|_| panic!("{p} missing"));
        assert!(got == *data, "{p} content differs");
    }
}

#[tokio::test]
async fn install_patch_verify_repair_uninstall() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Game");

    // v1: three files, ~1.3 MB.
    let v1_files = vec![
        ("Game.exe", bytes(1, 150_000)),
        ("Content/Paks/pak0.pak", bytes(2, 900_000)),
        ("Content/old.bin", bytes(3, 200_000)),
    ];
    let v1 = make_build(&server, "1.0.0", &v1_files);
    let (env1, keys) = signed(&v1);
    assert_eq!(env1.open(&keys).unwrap().version, "1.0.0");

    install::apply(
        &root,
        &env1,
        &v1,
        opts(&template, CancellationToken::new(), HashSet::new()),
    )
    .await
    .unwrap();
    assert_files(&root, &v1_files);
    assert!(!root.join(".zirtola/pending.json").exists());
    assert_eq!(
        install::installed_manifest(&root)
            .unwrap()
            .unwrap()
            .1
            .version,
        "1.0.0"
    );
    let first_hits = server.hits.lock().unwrap().len();
    assert_eq!(
        first_hits,
        v1.files.iter().map(|f| f.chunks.len()).sum::<usize>()
    );

    // v2: rewrite one chunk in the pak, drop old.bin, add new.bin; exe unchanged.
    let mut pak2 = v1_files[1].1.clone();
    pak2[CHUNK * 3 + 10..CHUNK * 3 + 500].fill(0xAB);
    let v2_files = vec![
        ("Game.exe", v1_files[0].1.clone()),
        ("Content/Paks/pak0.pak", pak2),
        ("Content/new.bin", bytes(4, 70_000)),
    ];
    let v2 = make_build(&server, "1.0.1", &v2_files);
    let (env2, _) = signed(&v2);
    server.hits.lock().unwrap().clear();
    install::apply(
        &root,
        &env2,
        &v2,
        opts(&template, CancellationToken::new(), HashSet::new()),
    )
    .await
    .unwrap();
    assert_files(&root, &v2_files);
    assert!(
        !root.join("Content/old.bin").exists(),
        "removed file should be deleted"
    );
    let patch_hits = server.hits.lock().unwrap().len();
    // 1 changed pak chunk + 2 chunks of new.bin (70 KB).
    assert_eq!(patch_hits, 3, "patch should only download changed chunks");

    // Corrupt one chunk on disk and delete a file; verify finds both, repair fixes them.
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(root.join("Content/Paks/pak0.pak"))
            .unwrap();
        f.seek(SeekFrom::Start((CHUNK * 7 + 3) as u64)).unwrap();
        f.write_all(b"corruption").unwrap();
    }
    std::fs::remove_file(root.join("Content/new.bin")).unwrap();
    let report = install::verify(&root, &v2, None, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.damaged, vec!["Content/Paks/pak0.pak".to_string()]);
    assert_eq!(report.missing, vec!["Content/new.bin".to_string()]);

    server.hits.lock().unwrap().clear();
    let force: HashSet<String> = report
        .damaged
        .iter()
        .chain(report.missing.iter())
        .cloned()
        .collect();
    install::apply(
        &root,
        &env2,
        &v2,
        opts(&template, CancellationToken::new(), force),
    )
    .await
    .unwrap();
    assert_files(&root, &v2_files);
    // new.bin's 2 chunks + the one corrupted pak chunk; the rest of the pak is copied locally.
    assert_eq!(server.hits.lock().unwrap().len(), 3);
    assert!(install::verify(&root, &v2, None, CancellationToken::new())
        .await
        .unwrap()
        .is_ok());

    install::uninstall(&root).unwrap();
    assert!(
        !root.exists(),
        "install folder should be removed when empty"
    );
}

#[tokio::test]
async fn pause_and_resume() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Game");
    let files = vec![
        ("Game.exe", bytes(9, 40 * CHUNK)),
        ("data.pak", bytes(10, 20 * CHUNK)),
    ];
    let m = make_build(&server, "1.0.0", &files);
    let (env, _) = signed(&m);
    let total_chunks: usize = m.files.iter().map(|f| f.chunks.len()).sum();

    server.delay_ms.store(40, Ordering::Relaxed);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c2.cancel();
    });
    let r = install::apply(&root, &env, &m, opts(&template, cancel, HashSet::new())).await;
    assert!(matches!(r, Err(InstallError::Cancelled)));
    assert!(
        root.join(".zirtola/pending.json").exists(),
        "journal kept for resume"
    );
    assert!(!root.join("Game.exe").exists(), "nothing committed yet");
    let journal = install::Journal::load(&root).unwrap();
    let done: usize = journal.done.values().map(|v| v.len()).sum();
    assert!(
        done > 0 && done < total_chunks,
        "some but not all chunks done ({done})"
    );

    server.delay_ms.store(0, Ordering::Relaxed);
    // Let requests that were in flight at cancel time finish on the server side
    // so they are not counted as resume traffic.
    tokio::time::sleep(Duration::from_millis(150)).await;
    server.hits.lock().unwrap().clear();
    install::apply(
        &root,
        &env,
        &m,
        opts(&template, CancellationToken::new(), HashSet::new()),
    )
    .await
    .unwrap();
    assert_files(&root, &files);
    let resumed = server.hits.lock().unwrap().len();
    assert!(
        resumed <= total_chunks - done,
        "resume re-downloaded finished chunks ({resumed})"
    );
}

#[tokio::test]
async fn expired_link_is_reported() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("Game.exe", bytes(5, 3 * CHUNK))];
    let m = make_build(&server, "1.0.0", &files);
    let (env, _) = signed(&m);
    server.expired.store(true, Ordering::Relaxed);
    let r = install::apply(
        &dir.path().join("G"),
        &env,
        &m,
        opts(&template, CancellationToken::new(), HashSet::new()),
    )
    .await;
    assert!(matches!(r, Err(InstallError::ChunkUrlExpired)));
}

#[tokio::test]
async fn insufficient_space_is_refused() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("Game.exe", bytes(6, CHUNK))];
    let m = make_build(&server, "1.0.0", &files);
    let (env, _) = signed(&m);
    let mut o = opts(&template, CancellationToken::new(), HashSet::new());
    o.space_margin = u64::MAX / 2;
    let r = install::apply(&dir.path().join("G"), &env, &m, o).await;
    assert!(matches!(r, Err(InstallError::InsufficientSpace { .. })));
}

/// Saves and settings must survive everything the launcher does to a game:
/// update, verify, repair and uninstall only add, replace or delete files the
/// manifest lists inside the install folder. Unknown files (a game writing
/// Saved/SaveGames/x.sav next to itself, a player's own mod) and anything
/// outside the folder (%LOCALAPPDATA%\GranitePit\Saved) are left byte for byte.
#[tokio::test]
async fn player_files_survive_update_verify_repair_and_uninstall() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Dirt Digger");
    let outside_saves = dir.path().join("LocalAppData/GranitePit/Saved/SaveGames");

    let v1_files = vec![
        ("GranitePit.exe", bytes(11, 120_000)),
        ("GranitePit/Content/Paks/pak0.pak", bytes(12, 600_000)),
        ("GranitePit/Content/Paks/old.pak", bytes(13, 90_000)),
        ("GranitePit/Binaries/Win64/GranitePit-Win64-Shipping.exe", bytes(14, 200_000)),
    ];
    let v1 = make_build(&server, "0.1.0", &v1_files);
    let (env1, _) = signed(&v1);
    install::apply(&root, &env1, &v1, opts(&template, CancellationToken::new(), HashSet::new()))
        .await
        .unwrap();

    // Files the launcher must never touch.
    let player_files: Vec<(&str, Vec<u8>)> = vec![
        ("Saved/SaveGames/x.sav", bytes(90, 33_333)),
        ("GranitePit/Saved/Config/Windows/GameUserSettings.ini", b"[ScalabilityGroups]\nsg.ResolutionQuality=100\n".to_vec()),
        ("GranitePit/Content/Paks/~mods/my_mod.pak", bytes(91, 4_096)),
    ];
    for (p, data) in &player_files {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, data).unwrap();
    }
    std::fs::create_dir_all(&outside_saves).unwrap();
    let outside_file = outside_saves.join("slot0.sav");
    std::fs::write(&outside_file, bytes(92, 10_000)).unwrap();
    let check_player_files = |when: &str| {
        for (p, data) in &player_files {
            let got = std::fs::read(root.join(p)).unwrap_or_else(|_| panic!("{p} deleted by {when}"));
            assert!(got == *data, "{p} changed by {when}");
        }
        assert_eq!(std::fs::read(&outside_file).unwrap(), bytes(92, 10_000), "outside save changed by {when}");
    };

    // Update: one pak changes, old.pak goes away (same folder as the mod).
    let mut pak = v1_files[1].1.clone();
    pak[CHUNK * 2..CHUNK * 2 + 64].fill(7);
    let v2_files = vec![
        ("GranitePit.exe", v1_files[0].1.clone()),
        ("GranitePit/Content/Paks/pak0.pak", pak),
        ("GranitePit/Binaries/Win64/GranitePit-Win64-Shipping.exe", bytes(15, 200_000)),
    ];
    let v2 = make_build(&server, "0.1.1", &v2_files);
    let (env2, _) = signed(&v2);
    install::apply(&root, &env2, &v2, opts(&template, CancellationToken::new(), HashSet::new()))
        .await
        .unwrap();
    assert_files(&root, &v2_files);
    assert!(!root.join("GranitePit/Content/Paks/old.pak").exists());
    check_player_files("update");

    // Verify reports the build as fine (unknown files are not "damage").
    let report = install::verify(&root, &v2, None, CancellationToken::new()).await.unwrap();
    assert!(report.is_ok(), "verify flagged: {report:?}");
    check_player_files("verify");

    // Repair after damage.
    std::fs::write(root.join("GranitePit.exe"), b"broken").unwrap();
    let report = install::verify(&root, &v2, None, CancellationToken::new()).await.unwrap();
    let force: HashSet<String> = report.damaged.iter().chain(report.missing.iter()).cloned().collect();
    assert_eq!(force.len(), 1);
    install::apply(&root, &env2, &v2, opts(&template, CancellationToken::new(), force))
        .await
        .unwrap();
    assert_files(&root, &v2_files);
    check_player_files("repair");

    // Uninstall removes only the build: the folder stays because player files are in it.
    install::uninstall(&root).unwrap();
    for (p, _) in &v2_files {
        assert!(!root.join(p).exists(), "{p} should be uninstalled");
    }
    check_player_files("uninstall");
}

/// A junction inside the install folder that points somewhere else (say, at
/// the save folder) is never followed: the update refuses instead of writing
/// or deleting through it.
#[cfg(windows)]
#[tokio::test]
async fn links_inside_the_install_folder_are_not_followed() {
    let server = Arc::new(Server::default());
    let template = serve(server.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Game");
    let elsewhere = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("save.sav"), b"precious").unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let st = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.join("Data"))
        .arg(&elsewhere)
        .output()
        .unwrap();
    assert!(st.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&st.stderr));

    let files = vec![("Game.exe", bytes(21, 1000)), ("Data/save.sav", bytes(22, 1000))];
    let m = make_build(&server, "1.0.0", &files);
    let (env, _) = signed(&m);
    let r = install::apply(&root, &env, &m, opts(&template, CancellationToken::new(), HashSet::new())).await;
    let err = format!("{:?}", r.err().expect("must not write through a junction"));
    assert!(err.contains("refusing to follow a link"), "unexpected error: {err}");
    assert_eq!(std::fs::read(elsewhere.join("save.sav")).unwrap(), b"precious");
    install::uninstall(&root).unwrap();
    assert_eq!(std::fs::read(elsewhere.join("save.sav")).unwrap(), b"precious");
}
