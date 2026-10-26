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
    /// Add-on skus the account owns for this game.
    #[serde(default)]
    pub addons: Vec<String>,
    /// Hash of the device the token was issued to.
    #[serde(default)]
    pub dev: Option<String>,
    /// Seconds after `iat` the game may keep running offline on this token
    /// (the website's LAUNCHER_OFFLINE_GRACE_DAYS).
    #[serde(default)]
    pub grace: Option<i64>,
}

/// Result of an offline licence check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseState {
    /// Token is within its normal lifetime (just started from the launcher).
    Fresh,
    /// Token's lifetime is over but the offline grace period still covers it.
    Grace { remaining_s: i64 },
}

/// Offline licence check for a game: like [`verify_launch_token`], but a token
/// whose short lifetime has passed is still accepted until `iat + grace`
/// (the token's own `grace` claim, or `default_grace_s`). Tampered tokens,
/// tokens for another game, tokens issued in the future (beyond `leeway_s` of
/// clock skew) and tokens past their grace period are refused.
pub fn verify_license(
    token: &str,
    keys: &TrustedKeys,
    game_id: &str,
    now_unix: i64,
    leeway_s: i64,
    default_grace_s: i64,
) -> anyhow::Result<(LaunchClaims, LicenseState)> {
    let claims = verify_signed_claims(token, keys, game_id)?;
    if claims.iat > now_unix + leeway_s {
        anyhow::bail!("licence issued in the future (check the PC's clock)");
    }
    if now_unix <= claims.exp + leeway_s {
        return Ok((claims, LicenseState::Fresh));
    }
    let grace = claims.grace.unwrap_or(default_grace_s).max(0);
    let until = claims.iat + grace;
    if now_unix <= until {
        let remaining_s = until - now_unix;
        return Ok((claims, LicenseState::Grace { remaining_s }));
    }
    anyhow::bail!("licence needs to be checked online again");
}

fn verify_signed_claims(token: &str, keys: &TrustedKeys, game_id: &str) -> anyhow::Result<LaunchClaims> {
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
    Ok(claims)
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
    let claims = verify_signed_claims(token, keys, game_id)?;
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
            addons: vec![],
            dev: Some("dev1".into()),
            grace: Some(7 * 86_400),
        }
    }

    fn keys_and_signer() -> (ed25519_dalek::SigningKey, TrustedKeys) {
        let (sk, jwk) = generate("l1", Some("launch"));
        let mut keys = TrustedKeys::new();
        keys.add_jwk(&jwk).unwrap();
        (sk, keys)
    }

    #[test]
    fn offline_grace_period() {
        let (sk, keys) = keys_and_signer();
        // iat 1000, exp 1300 (5 minutes), grace 7 days.
        let mut c = claims("dirt-simulator", 1300);
        c.iat = 1000;
        let t = sign_launch_token(&c, "l1", &sk).unwrap();
        assert_eq!(verify_license(&t, &keys, "dirt-simulator", 1200, 60, 0).unwrap().1, LicenseState::Fresh);
        // Two days later, offline: still fine, inside the grace period.
        let (_, st) = verify_license(&t, &keys, "dirt-simulator", 1000 + 2 * 86_400, 60, 0).unwrap();
        assert!(matches!(st, LicenseState::Grace { remaining_s } if remaining_s == 5 * 86_400));
        // Eight days later: must check online again.
        assert!(verify_license(&t, &keys, "dirt-simulator", 1000 + 8 * 86_400, 60, 0).is_err());
        // The strict check used right after launch refuses it after exp.
        assert!(verify_launch_token(&t, &keys, "dirt-simulator", 1000 + 2 * 86_400, 60).is_err());
    }

    #[test]
    fn default_grace_when_claim_missing() {
        let (sk, keys) = keys_and_signer();
        let mut c = claims("g", 1300);
        c.iat = 1000;
        c.grace = None;
        let t = sign_launch_token(&c, "l1", &sk).unwrap();
        assert!(verify_license(&t, &keys, "g", 1000 + 3 * 86_400, 60, 7 * 86_400).is_ok());
        assert!(verify_license(&t, &keys, "g", 1000 + 3 * 86_400, 60, 86_400).is_err());
    }

    #[test]
    fn clock_skew() {
        let (sk, keys) = keys_and_signer();
        let mut c = claims("g", 10_300);
        c.iat = 10_000;
        let t = sign_launch_token(&c, "l1", &sk).unwrap();
        // PC clock 90 s behind the server: accepted with 120 s leeway.
        assert!(verify_license(&t, &keys, "g", 9_910, 120, 0).is_ok());
        // PC clock an hour behind: token "from the future", refused.
        assert!(verify_license(&t, &keys, "g", 6_400, 120, 0).is_err());
    }

    #[test]
    fn tampered_claims_and_wrong_keys_are_refused() {
        let (sk, keys) = keys_and_signer();
        let t = sign_launch_token(&claims("g", 2000), "l1", &sk).unwrap();
        let parts: Vec<&str> = t.split('.').collect();
        // Change the payload (extend the grace period) but keep the signature.
        let mut evil = claims("g", 2000);
        evil.grace = Some(10 * 365 * 86_400);
        let p2 = b64::encode_url(&serde_json::to_vec(&evil).unwrap());
        let forged = format!("{}.{}.{}", parts[0], p2, parts[2]);
        assert!(verify_license(&forged, &keys, "g", 1500, 60, 0).is_err());
        // Signed by an attacker's own key using the same kid.
        let (other, _) = generate("l1", Some("launch"));
        let t2 = sign_launch_token(&claims("g", 2000), "l1", &other).unwrap();
        assert!(verify_license(&t2, &keys, "g", 1500, 60, 0).is_err());
        // A manifest-only key may not sign licences.
        let (msk, mjwk) = generate("m1", Some("manifest"));
        let mut mkeys = TrustedKeys::new();
        mkeys.add_jwk(&mjwk).unwrap();
        let t3 = sign_launch_token(&claims("g", 2000), "m1", &msk).unwrap();
        assert!(verify_license(&t3, &mkeys, "g", 1500, 60, 0).is_err());
        // Garbage, and a token for another game.
        assert!(verify_license("a.b", &keys, "g", 1500, 60, 0).is_err());
        assert!(verify_license(&t, &keys, "other-game", 1500, 60, 0).is_err());
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
