//! Pre-shared-key authentication.
//!
//! The digest is `HMAC-SHA256(token, decimal(timestamp))`. Verification is
//! constant time (`hmac` compares the tags) and the timestamp window is checked
//! before the MAC so a clock-skewed peer does not consume hashing work.

use alloc::string::String;
use core::fmt;

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Accepted clock skew, in seconds, between peers.
pub const AUTH_SKEW_SECS: i64 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    EmptyDigest,
    InvalidDigest,
    TimestampSkew,
}

impl AuthFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyDigest => "empty authentication digest",
            Self::InvalidDigest => "invalid authentication digest",
            Self::TimestampSkew => "timestamp outside allowed window",
        }
    }
}

impl fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::error::Error for AuthFailure {}

pub fn compute_auth_digest(token: &str, timestamp: i64) -> String {
    let mut mac = new_mac(token);
    mac.update(timestamp_string(timestamp).as_bytes());
    crate::hex::encode(&mac.finalize().into_bytes())
}

/// Verifies a digest produced by [`compute_auth_digest`].
///
/// An empty `token` disables authentication, matching the `auth.token` config
/// semantics: the operator opted into an open endpoint.
pub fn verify_auth_digest(
    token: &str,
    auth_digest: &str,
    timestamp: i64,
    now_secs: i64,
) -> Result<(), AuthFailure> {
    if token.is_empty() {
        return Ok(());
    }
    if auth_digest.is_empty() {
        return Err(AuthFailure::EmptyDigest);
    }
    if now_secs.saturating_sub(timestamp).abs() > AUTH_SKEW_SECS {
        return Err(AuthFailure::TimestampSkew);
    }

    let mut expected = [0u8; 32];
    crate::hex::decode_into(auth_digest.as_bytes(), &mut expected)
        .ok_or(AuthFailure::InvalidDigest)?;

    let mut mac = new_mac(token);
    mac.update(timestamp_string(timestamp).as_bytes());
    mac.verify_slice(&expected)
        .map_err(|_| AuthFailure::InvalidDigest)
}

fn new_mac(token: &str) -> HmacSha256 {
    HmacSha256::new_from_slice(token.as_bytes()).expect("HMAC accepts arbitrary key lengths")
}

fn timestamp_string(timestamp: i64) -> String {
    let mut buf = itoa(timestamp);
    buf.shrink_to_fit();
    buf
}

/// Minimal decimal formatter so authentication does not depend on `core::fmt`.
fn itoa(value: i64) -> String {
    let mut buf = [0u8; 20];
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    let mut cursor = buf.len();
    loop {
        cursor -= 1;
        buf[cursor] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    if negative {
        cursor -= 1;
        buf[cursor] = b'-';
    }
    String::from_utf8_lossy(&buf[cursor..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_roundtrip_and_skew() {
        let now = 1_700_000_000;
        let digest = compute_auth_digest("s3cret", now);
        assert_eq!(digest.len(), 64);
        assert!(verify_auth_digest("s3cret", &digest, now, now).is_ok());
        assert_eq!(
            verify_auth_digest("wrong", &digest, now, now),
            Err(AuthFailure::InvalidDigest)
        );
        assert_eq!(
            verify_auth_digest("s3cret", &digest, now, now + AUTH_SKEW_SECS + 1),
            Err(AuthFailure::TimestampSkew)
        );
        assert_eq!(
            verify_auth_digest("s3cret", "", now, now),
            Err(AuthFailure::EmptyDigest)
        );
        assert!(verify_auth_digest("", "", now, now).is_ok());
    }

    #[test]
    fn negative_timestamps_format() {
        assert_eq!(itoa(0), "0");
        assert_eq!(itoa(-1), "-1");
        assert_eq!(itoa(i64::MIN), "-9223372036854775808");
        assert_eq!(itoa(i64::MAX), "9223372036854775807");
    }
}
