//! Trusted Ed25519 public keys, in a JWK-like key set:
//!
//! ```json
//! { "keys": [ { "kid": "2026-01", "kty": "OKP", "crv": "Ed25519", "x": "<base64url>", "use": "manifest" } ] }
//! ```
//!
//! `use` is `"manifest"` (build manifests) or `"launch"` (launch tokens). A key
//! without `use` is accepted for both. Keys are identified by `kid` so the
//! website can rotate keys without breaking older launchers.

use crate::b64;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kid: String,
    #[serde(default = "default_kty")]
    pub kty: String,
    #[serde(default = "default_crv")]
    pub crv: String,
    pub x: String,
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,
}

fn default_kty() -> String {
    "OKP".into()
}
fn default_crv() -> String {
    "Ed25519".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeySet {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("unknown signing key id '{0}'")]
    UnknownKey(String),
    #[error("key '{0}' is not allowed for {1}")]
    WrongUse(String, &'static str),
    #[error("malformed key or signature: {0}")]
    Malformed(String),
    #[error("signature does not verify")]
    BadSignature,
}

#[derive(Clone)]
struct Entry {
    key: VerifyingKey,
    use_: Option<String>,
}

/// Set of public keys the launcher trusts.
#[derive(Clone, Default)]
pub struct TrustedKeys {
    keys: HashMap<String, Entry>,
}

impl TrustedKeys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let mut t = Self::new();
        t.add_set(&serde_json::from_str(json)?)?;
        Ok(t)
    }

    pub fn add_set(&mut self, set: &KeySet) -> anyhow::Result<()> {
        for k in &set.keys {
            self.add_jwk(k)?;
        }
        Ok(())
    }

    pub fn add_jwk(&mut self, k: &Jwk) -> anyhow::Result<()> {
        if k.kty != "OKP" || k.crv != "Ed25519" {
            anyhow::bail!("key {} is not an Ed25519 OKP key", k.kid);
        }
        let raw = b64::decode_any(&k.x)?;
        let arr: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("key {} has wrong length", k.kid))?;
        let key = VerifyingKey::from_bytes(&arr)?;
        self.keys.insert(
            k.kid.clone(),
            Entry {
                key,
                use_: k.use_.clone(),
            },
        );
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Verify `sig_b64` over `data` with key `kid`, which must be allowed for `purpose`.
    pub fn verify(
        &self,
        kid: &str,
        purpose: &'static str,
        data: &[u8],
        sig_b64: &str,
    ) -> Result<(), KeyError> {
        let entry = self
            .keys
            .get(kid)
            .ok_or_else(|| KeyError::UnknownKey(kid.to_string()))?;
        if let Some(u) = &entry.use_ {
            if u != purpose {
                return Err(KeyError::WrongUse(kid.to_string(), purpose));
            }
        }
        let sig_raw = b64::decode_any(sig_b64).map_err(|e| KeyError::Malformed(e.to_string()))?;
        let sig =
            Signature::from_slice(&sig_raw).map_err(|e| KeyError::Malformed(e.to_string()))?;
        entry
            .key
            .verify(data, &sig)
            .map_err(|_| KeyError::BadSignature)
    }
}

/// Create a new signing key and its public JWK. Used by `zirtola-publish keygen`
/// and by the mock server's seeding step.
pub fn generate(kid: &str, use_: Option<&str>) -> (ed25519_dalek::SigningKey, Jwk) {
    let sk = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let jwk = public_jwk(&sk, kid, use_);
    (sk, jwk)
}

pub fn public_jwk(sk: &ed25519_dalek::SigningKey, kid: &str, use_: Option<&str>) -> Jwk {
    Jwk {
        kid: kid.to_string(),
        kty: default_kty(),
        crv: default_crv(),
        x: b64::encode_url(sk.verifying_key().as_bytes()),
        use_: use_.map(str::to_string),
    }
}
