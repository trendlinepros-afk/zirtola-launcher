//! Build manifests.
//!
//! A manifest describes one immutable build of one game for one platform: every
//! file, its size and SHA-256, and the content-addressed chunks it is made of.
//! Chunks are identified by the SHA-256 of their (uncompressed) bytes, so a
//! chunk that appears in two versions is downloaded once, and a patch only
//! fetches chunks the installed build does not already have.
//!
//! On the wire a manifest is wrapped in a [`SignedManifest`] envelope. The
//! signature covers the exact manifest bytes (base64-encoded in the envelope),
//! which sidesteps JSON canonicalisation entirely.

use crate::b64;
use crate::keys::TrustedKeys;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Lowercase hex SHA-256 of the chunk bytes.
    pub hash: String,
    /// Offset of the chunk in the file.
    pub offset: u64,
    /// Chunk length in bytes.
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path with forward slashes, e.g. `DirtDigger/Content/Paks/pak0.pak`.
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: String,
    #[serde(default)]
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSpec {
    /// Executable relative to the install root.
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory relative to the install root (default: the executable's folder).
    #[serde(default)]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub game_id: String,
    pub version: String,
    pub build_id: String,
    pub platform: String,
    pub created_at: String,
    pub launch: LaunchSpec,
    pub total_size: u64,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// Structural validation run after the signature check. Rejects anything
    /// that could make the installer write outside the install folder or loop
    /// forever on bad offsets.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.schema != SCHEMA {
            anyhow::bail!("unsupported manifest schema {}", self.schema);
        }
        safe_relative_path(&self.launch.executable)?;
        if let Some(w) = &self.launch.working_dir {
            safe_relative_path(w)?;
        }
        let mut total = 0u64;
        let mut seen = std::collections::HashSet::new();
        for f in &self.files {
            safe_relative_path(&f.path)?;
            if !seen.insert(f.path.to_ascii_lowercase()) {
                anyhow::bail!("duplicate path in manifest: {}", f.path);
            }
            let mut expect = 0u64;
            for c in &f.chunks {
                if c.offset != expect {
                    anyhow::bail!("chunks of {} are not contiguous", f.path);
                }
                if c.size == 0 || c.size > 256 * 1024 * 1024 {
                    anyhow::bail!("chunk size out of range in {}", f.path);
                }
                if c.hash.len() != 64 || !c.hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    anyhow::bail!("bad chunk hash in {}", f.path);
                }
                expect += c.size;
            }
            if expect != f.size {
                anyhow::bail!("chunks of {} do not cover the file", f.path);
            }
            total += f.size;
        }
        if total != self.total_size {
            anyhow::bail!("total_size does not match the files");
        }
        Ok(())
    }

    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }
}

/// Turn a manifest path into a safe relative [`PathBuf`]: no absolute paths,
/// drive letters, `..`, NTFS alternate data streams or reserved device names.
pub fn safe_relative_path(p: &str) -> anyhow::Result<PathBuf> {
    if p.is_empty() || p.contains('\\') || p.contains(':') || p.contains('\0') {
        anyhow::bail!("unsafe path in manifest: {p:?}");
    }
    let path = Path::new(p);
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
                const RESERVED: [&str; 22] = [
                    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
                    "LPT8", "LPT9",
                ];
                if RESERVED.contains(&stem.as_str()) || s.ends_with('.') || s.ends_with(' ') {
                    anyhow::bail!("unsafe path in manifest: {p:?}");
                }
                out.push(&*s);
            }
            _ => anyhow::bail!("unsafe path in manifest: {p:?}"),
        }
    }
    Ok(out)
}

/// The signed envelope served by `GET /v1/games/{id}/manifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedManifest {
    /// Key id of the signing key.
    pub kid: String,
    /// Base64url of the exact manifest JSON bytes.
    pub manifest: String,
    /// Base64url Ed25519 signature over the decoded manifest bytes.
    pub signature: String,
}

impl SignedManifest {
    /// Verify the signature and decode + validate the manifest.
    pub fn open(&self, keys: &TrustedKeys) -> anyhow::Result<Manifest> {
        let bytes = b64::decode_any(&self.manifest)?;
        keys.verify(&self.kid, "manifest", &bytes, &self.signature)?;
        let m: Manifest = serde_json::from_slice(&bytes)?;
        m.validate()?;
        Ok(m)
    }

    pub fn sign(m: &Manifest, kid: &str, sk: &ed25519_dalek::SigningKey) -> anyhow::Result<Self> {
        use ed25519_dalek::Signer;
        let bytes = serde_json::to_vec(m)?;
        let sig = sk.sign(&bytes);
        Ok(Self {
            kid: kid.to_string(),
            manifest: b64::encode_url(&bytes),
            signature: b64::encode_url(&sig.to_bytes()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{generate, KeySet};

    fn sample() -> Manifest {
        Manifest {
            schema: SCHEMA,
            game_id: "g".into(),
            version: "1.0.0".into(),
            build_id: "b".into(),
            platform: "win64".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            launch: LaunchSpec {
                executable: "Game.exe".into(),
                args: vec![],
                working_dir: None,
            },
            total_size: 10,
            files: vec![FileEntry {
                path: "Game.exe".into(),
                size: 10,
                sha256: "0".repeat(64),
                chunks: vec![Chunk {
                    hash: "a".repeat(64),
                    offset: 0,
                    size: 10,
                }],
            }],
        }
    }

    #[test]
    fn sign_and_open_roundtrip() {
        let (sk, jwk) = generate("k1", Some("manifest"));
        let mut keys = TrustedKeys::new();
        keys.add_set(&KeySet { keys: vec![jwk] }).unwrap();
        let env = SignedManifest::sign(&sample(), "k1", &sk).unwrap();
        assert_eq!(env.open(&keys).unwrap().version, "1.0.0");
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let (sk, jwk) = generate("k1", None);
        let mut keys = TrustedKeys::new();
        keys.add_jwk(&jwk).unwrap();
        let mut env = SignedManifest::sign(&sample(), "k1", &sk).unwrap();
        let mut m = sample();
        m.launch.executable = "evil.exe".into();
        env.manifest = b64::encode_url(&serde_json::to_vec(&m).unwrap());
        assert!(env.open(&keys).is_err());
    }

    #[test]
    fn launch_key_cannot_sign_manifests() {
        let (sk, jwk) = generate("k1", Some("launch"));
        let mut keys = TrustedKeys::new();
        keys.add_jwk(&jwk).unwrap();
        let env = SignedManifest::sign(&sample(), "k1", &sk).unwrap();
        assert!(env.open(&keys).is_err());
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for p in [
            "../x",
            "/abs",
            "C:/x",
            "a/../../b",
            "a\\b",
            "x:stream",
            "CON",
            "aux.txt",
            "a/nul",
            "trailing.",
            "",
        ] {
            assert!(safe_relative_path(p).is_err(), "{p} should be rejected");
        }
        assert!(safe_relative_path("Game/Content/Paks/pak0.pak").is_ok());
    }
}
