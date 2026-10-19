//! Authentication: the `no_std` primitives live in [`orbien_proto::auth`]; the
//! replay window needs a monotonic clock and lives here.

pub use orbien_proto::auth::{
    compute_auth_digest, verify_auth_digest, AuthFailure, AUTH_SKEW_SECS,
};

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a digest stays in the replay window.
pub const REPLAY_TTL_SECS: u64 = (AUTH_SKEW_SECS as u64) * 2;

/// Hard cap on distinct digests held at once.
pub const REPLAY_MAX_ENTRIES: usize = 100_000;

/// Extended failure set for the server-side handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFailure {
    Auth(AuthFailure),
    Replay,
    Capacity,
}

impl LoginFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auth(failure) => failure.as_str(),
            Self::Replay => "authentication digest reused",
            Self::Capacity => "authentication replay cache full",
        }
    }
}

impl From<AuthFailure> for LoginFailure {
    fn from(value: AuthFailure) -> Self {
        Self::Auth(value)
    }
}

impl std::fmt::Display for LoginFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bounded, self-purging set of accepted digests.
#[derive(Debug, Default)]
pub struct ReplayCache {
    inner: Mutex<HashMap<String, Instant>>,
}

impl ReplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accept(&self, digest: &str) -> Result<(), LoginFailure> {
        self.accept_at(digest, Instant::now())
    }

    pub fn accept_at(&self, digest: &str, now: Instant) -> Result<(), LoginFailure> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, expires| *expires > now);

        if map.contains_key(digest) {
            return Err(LoginFailure::Replay);
        }
        if map.len() >= REPLAY_MAX_ENTRIES {
            return Err(LoginFailure::Capacity);
        }
        map.insert(
            digest.to_owned(),
            now + Duration::from_secs(REPLAY_TTL_SECS),
        );
        Ok(())
    }
}

/// Verifies a login digest and records it against replay.
pub fn verify_login(
    token: &str,
    auth_digest: &str,
    timestamp: i64,
    now_secs: i64,
    replay: &ReplayCache,
) -> Result<(), LoginFailure> {
    verify_auth_digest(token, auth_digest, timestamp, now_secs)?;
    if token.is_empty() {
        return Ok(());
    }
    replay.accept(auth_digest)
}

pub fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
