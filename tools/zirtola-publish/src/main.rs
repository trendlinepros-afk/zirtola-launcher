//! zirtola-publish: prepare a packaged game build for the launcher.
//!
//! ```text
//! zirtola-publish keygen --kid 2026-01 --use manifest --out manifest-key.json
//! zirtola-publish build  --game dirt-digger --version 1.2.0 --exe DirtDigger.exe \
//!                        --input "D:/Builds/DirtDigger/Windows" --store ./cdn/dirt-digger/chunks \
//!                        --key manifest-key.json --out ./cdn/dirt-digger/1.2.0.manifest.json
//! ```
//!
//! Files are split with content-defined chunking (FastCDC, 1/4/16 MiB
//! min/avg/max), so an edit inside a big .pak only changes the chunks around
//! the edit and a patch only downloads those. Chunks are stored by SHA-256 as
//! `<store>/<first two hex chars>/<hash>`; upload the store to the CDN as-is
//! (it only ever grows, chunks are immutable) and register the signed manifest
//! with the website.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zirtola_core::b64;
use zirtola_core::keys::{public_jwk, KeySet};
use zirtola_core::manifest::{Chunk, FileEntry, LaunchSpec, Manifest, SignedManifest, SCHEMA};

#[derive(Parser)]
#[command(
    version,
    about = "Chunk, hash and sign game builds for the Zirtola launcher"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an Ed25519 signing key (private JWK). Keep the output file secret.
    Keygen {
        #[arg(long)]
        kid: String,
        /// `manifest` or `launch`.
        #[arg(long = "use", default_value = "manifest")]
        use_: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Print the public key set (JWKS) for one or more private key files.
    Pubkey { keys: Vec<PathBuf> },
    /// Chunk a build folder into the store and write a signed manifest.
    Build {
        #[arg(long)]
        game: String,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "win64")]
        platform: String,
        /// Executable relative to the build folder.
        #[arg(long)]
        exe: String,
        /// Extra arguments passed to the game on launch.
        #[arg(long = "arg")]
        args: Vec<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        store: PathBuf,
        /// Private key file; or set ZIRTOLA_MANIFEST_KEY to the key JSON.
        #[arg(long)]
        key: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        /// Build id to record (default: a hash of the version and file contents).
        #[arg(long = "build-id")]
        build_id: Option<String>,
    },
}

/// Private key file format: a JWK with the private seed in `d`.
#[derive(Serialize, Deserialize)]
struct PrivateJwk {
    kid: String,
    kty: String,
    crv: String,
    #[serde(rename = "use")]
    use_: String,
    x: String,
    d: String,
}

fn load_key(path: Option<&Path>) -> Result<(String, SigningKey)> {
    let json = match path {
        Some(p) => {
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?
        }
        None => std::env::var("ZIRTOLA_MANIFEST_KEY")
            .context("pass --key or set ZIRTOLA_MANIFEST_KEY")?,
    };
    let k: PrivateJwk = serde_json::from_str(&json)?;
    let seed: [u8; 32] = b64::decode_any(&k.d)?
        .as_slice()
        .try_into()
        .context("private key must be 32 bytes")?;
    Ok((k.kid, SigningKey::from_bytes(&seed)))
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Keygen { kid, use_, out } => {
            if use_ != "manifest" && use_ != "launch" {
                anyhow::bail!("--use must be manifest or launch");
            }
            if out.exists() {
                anyhow::bail!("{} exists; refusing to overwrite a key", out.display());
            }
            let (sk, jwk) = zirtola_core::keys::generate(&kid, Some(&use_));
            let private = PrivateJwk {
                kid,
                kty: jwk.kty.clone(),
                crv: jwk.crv.clone(),
                use_,
                x: jwk.x.clone(),
                d: b64::encode_url(&sk.to_bytes()),
            };
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&out, serde_json::to_vec_pretty(&private)?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&KeySet { keys: vec![jwk] })?
            );
            eprintln!(
                "private key written to {} (do not commit it)",
                out.display()
            );
        }
        Cmd::Pubkey { keys } => {
            let mut set = KeySet::default();
            for p in keys {
                let k: PrivateJwk = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
                let (_, sk) = load_key(Some(&p))?;
                set.keys.push(public_jwk(&sk, &k.kid, Some(&k.use_)));
            }
            println!("{}", serde_json::to_string_pretty(&set)?);
        }
        Cmd::Build {
            game,
            version,
            platform,
            exe,
            args,
            input,
            store,
            key,
            out,
            build_id,
        } => {
            if !zirtola_core::install::valid_game_id(&game) {
                anyhow::bail!("game id must be a lowercase slug");
            }
            let (kid, sk) = load_key(key.as_deref())?;
            let mut m = build(&game, &version, &platform, &exe, args, &input, &store)?;
            if let Some(id) = build_id {
                if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || "+-._".contains(c)) {
                    anyhow::bail!("--build-id: up to 64 chars of letters, digits and + - . _");
                }
                m.build_id = id;
            }
            let env = SignedManifest::sign(&m, &kid, &sk)?;
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&out, serde_json::to_vec_pretty(&env)?)?;
            eprintln!(
                "{} {} ({}): {} files, {:.1} MiB, build {}",
                game,
                version,
                platform,
                m.files.len(),
                m.total_size as f64 / 1048576.0,
                m.build_id
            );
        }
    }
    Ok(())
}

fn build(
    game: &str,
    version: &str,
    platform: &str,
    exe: &str,
    args: Vec<String>,
    input: &Path,
    store: &Path,
) -> Result<Manifest> {
    let mut files = Vec::new();
    let mut new_chunks = 0usize;
    let mut new_bytes = 0u64;
    let mut entries: Vec<_> = walkdir::WalkDir::new(input)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| e.file_type().is_file())
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for e in entries {
        let rel = e
            .path()
            .strip_prefix(input)?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with(".zirtola/") {
            continue;
        }
        zirtola_core::manifest::safe_relative_path(&rel)?;
        let file = std::fs::File::open(e.path())?;
        let size = file.metadata()?.len();
        let mut whole = Sha256::new();
        let mut chunks = Vec::new();
        let cdc = fastcdc::v2020::StreamCDC::new(file, 1 << 20, 4 << 20, 16 << 20);
        for c in cdc {
            let c = c?;
            whole.update(&c.data);
            let hash = hex::encode(Sha256::digest(&c.data));
            let dest = store.join(&hash[..2]).join(&hash);
            if !dest.exists() {
                std::fs::create_dir_all(dest.parent().unwrap())?;
                let tmp = dest.with_extension("tmp");
                let mut f = std::fs::File::create(&tmp)?;
                f.write_all(&c.data)?;
                drop(f);
                std::fs::rename(&tmp, &dest)?;
                new_chunks += 1;
                new_bytes += c.length as u64;
            }
            chunks.push(Chunk {
                hash,
                offset: c.offset,
                size: c.length as u64,
            });
        }
        files.push(FileEntry {
            path: rel,
            size,
            sha256: hex::encode(whole.finalize()),
            chunks,
        });
    }
    if !files.iter().any(|f| f.path == exe) {
        anyhow::bail!("executable {exe} is not in the build folder");
    }

    let mut id = Sha256::new();
    id.update(format!("{game}\n{version}\n{platform}\n{exe}\n"));
    for f in &files {
        id.update(format!("{} {}\n", f.path, f.sha256));
    }
    let build_id = hex::encode(&id.finalize()[..12]);
    let total_size = files.iter().map(|f| f.size).sum();
    eprintln!(
        "stored {new_chunks} new chunks ({:.1} MiB) in {}",
        new_bytes as f64 / 1048576.0,
        store.display()
    );
    let m = Manifest {
        schema: SCHEMA,
        game_id: game.into(),
        version: version.into(),
        build_id,
        platform: platform.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        launch: LaunchSpec {
            executable: exe.into(),
            args,
            working_dir: None,
        },
        total_size,
        files,
    };
    m.validate()?;
    Ok(m)
}
