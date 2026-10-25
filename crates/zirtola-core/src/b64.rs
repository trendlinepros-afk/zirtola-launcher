//! Base64 helpers. All binary values on the wire are unpadded base64url.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

pub fn encode_url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Decode base64url, tolerating padding and the standard alphabet so that
/// hand-written test fixtures do not trip anyone up.
pub fn decode_any(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let trimmed = s.trim().trim_end_matches('=');
    URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| STANDARD.decode(s.trim()))
}
