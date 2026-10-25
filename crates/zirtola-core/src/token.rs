//! Launch tokens: short-lived EdDSA JWTs the website issues for one account and
//! one game. The launcher passes the token to the game in the
//! `ZIRTOLA_SESSION_TOKEN` environment variable; the game verifies it offline
//! against the website's published `launch` key (or online via
//! `POST /v1/launch-tokens/verify`). See `docs/API.md` and
//! `examples/license-check` for a reference verifier.

use crate::b64;
use crate::keys::TrustedKeys;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchClaims {
    /// Issuer, e.g. `https://zirtola.com`.
    pub iss: String,
    /// Opaque account id.
    pub sub: String,
    /// Always `zirtola-game:<game_id>`.
    pub aud: String,
    /// Game id the token is valid for.
    pub game: String,
    /// Entitlement kind: `purchase`, `key`, `grant`.
    pub ent: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    /// Display name for in-game UI (optional).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

/// Verify a launch token for `game_id`. `leeway_s` tolerates clock skew.
pub fn verify_launch_token(
    token: &str,
    keys: &TrustedKeys,
    game_id: &str,
    now_unix: i64,
    leeway_s: i64,
) -> anyhow::Result<LaunchClaims> {
    let mut parts = token.split('.');
    let (h, p, s) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => anyhow::bail!("token is not a JWS compact serialization"),
    };
    let header: Header = serde_json::from_slice(&b64::decode_any(h)?)?;
    if header.alg != "EdDSA" {
        anyhow::bail!("unexpected token alg {}", header.alg);
    }
    let signing_input = format!("{h}.{p}");
    keys.verify(&header.kid, "launch", signing_input.as_bytes(), s)?;
    let claims: LaunchClaims = serde_json::from_slice(&b64::decode_any(p)?)?;
    if claims.game != game_id || claims.aud != format!("zirtola-game:{game_id}") {
        anyhow::bail!("token is for a different game");
    }
    if now_unix > claims.exp + leeway_s {
        anyhow::bail!("token expired");
    }
    if claims.iat > now_unix + leeway_s {
        anyhow::bail!("token issued in the future");
    }
    Ok(claims)
}

/// Mint a launch token (used by tests and tooling; the website does this in production).
pub fn sign_launch_token(
    claims: &LaunchClaims,
    kid: &str,
    sk: &ed25519_dalek::SigningKey,
) -> anyhow::Result<String> {
    use ed25519_dalek::Signer;
    let header = serde_json::json!({ "alg": "EdDSA", "typ": "JWT", "kid": kid });
    let h = b64::encode_url(&serde_json::to_vec(&header)?);
    let p = b64::encode_url(&serde_json::to_vec(claims)?);
    let sig = sk.sign(format!("{h}.{p}").as_bytes());
    Ok(format!("{h}.{p}.{}", b64::encode_url(&sig.to_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate;

    fn claims(game: &str, exp: i64) -> LaunchClaims {
        LaunchClaims {
            iss: "https://example.test".into(),
            sub: "acct_1".into(),
            aud: format!("zirtola-game:{game}"),
            game: game.into(),
            ent: "purchase".into(),
            iat: 1000,
            exp,
            jti: "j".into(),
            name: None,
        }
    }

    #[test]
    fn verifies_and_rejects() {
        let (sk, jwk) = generate("l1", Some("launch"));
        let mut keys = TrustedKeys::new();
        keys.add_jwk(&jwk).unwrap();
        let t = sign_launch_token(&claims("dirt-digger", 2000), "l1", &sk).unwrap();
        assert!(verify_launch_token(&t, &keys, "dirt-digger", 1500, 30).is_ok());
        assert!(verify_launch_token(&t, &keys, "bush-pilot", 1500, 30).is_err());
        assert!(verify_launch_token(&t, &keys, "dirt-digger", 3000, 30).is_err());
        let mut bad = t.clone();
        bad.replace_range(bad.len() - 4.., "AAAA");
        assert!(verify_launch_token(&bad, &keys, "dirt-digger", 1500, 30).is_err());
    }
}
