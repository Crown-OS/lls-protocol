use core::time::Duration;

use crate::clock::Instant;

/// One network path to the peer, named by the driver: a local socket and a peer address.
///
/// The session never sees an address. The driver maps each `(socket, peer address)` it hears an
/// authenticated datagram on to a `PathId` and hands it in, so migration logic stays sans-I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PathId(pub u8);

/// A path challenge to send: a probe flagged `PATH_CHALLENGE` carrying `token`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathChallenge {
    pub path: PathId,
    pub token: u32,
}

#[derive(Debug, Clone, Copy)]
struct Validation {
    path: PathId,
    token: u32,
    sent_at: Instant,
    attempts: u8,
}

/// Moves the session to a new path only once the peer has proven it answers there.
///
/// An authenticated datagram from an unfamiliar path makes it a candidate — authenticated, so
/// an off-path attacker cannot trigger this — and a challenge goes out on it. Traffic switches
/// only when the matching acknowledgement comes back on that same path, which is what stops a
/// replayed datagram from redirecting the stream somewhere the peer is not.
#[derive(Debug)]
pub struct PathManager {
    active: PathId,
    validation: Option<Validation>,
    next_token: u32,
}

impl PathManager {
    const RETRY_INTERVAL: Duration = Duration::from_millis(200);
    const MAX_ATTEMPTS: u8 = 5;

    pub const fn new(active: PathId) -> Self {
        Self {
            active,
            validation: None,
            next_token: 0x9E37_79B9,
        }
    }

    pub const fn active(&self) -> PathId {
        self.active
    }

    pub fn validating(&self) -> Option<PathId> {
        self.validation.map(|validation| validation.path)
    }

    /// Notes an authenticated arrival, returning a challenge if it came from a new path.
    pub fn observe_arrival(&mut self, path: PathId, now: Instant) -> Option<PathChallenge> {
        if path == self.active || self.validating() == Some(path) {
            return None;
        }
        Some(self.begin_validation(path, now))
    }

    /// Starts validating `path` now, as the driver asks when the local network changes.
    pub const fn begin_validation(&mut self, path: PathId, now: Instant) -> PathChallenge {
        let token = self.take_token();
        self.validation = Some(Validation {
            path,
            token,
            sent_at: now,
            attempts: 1,
        });
        PathChallenge { path, token }
    }

    /// A challenge acknowledgement arrived; returns the new active path if it completes one.
    pub fn on_challenge_ack(&mut self, path: PathId, token: u32) -> Option<PathId> {
        let validation = self.validation?;
        if validation.path != path || validation.token != token {
            return None;
        }
        self.validation = None;
        self.active = path;
        Some(path)
    }

    /// The challenge to resend, if the last one has gone unanswered too long.
    pub fn poll(&mut self, now: Instant) -> Option<PathChallenge> {
        let validation = self.validation.as_mut()?;
        if now.saturating_elapsed_since(validation.sent_at) < Self::RETRY_INTERVAL {
            return None;
        }
        if validation.attempts >= Self::MAX_ATTEMPTS {
            self.validation = None;
            return None;
        }
        validation.attempts += 1;
        validation.sent_at = now;
        Some(PathChallenge {
            path: validation.path,
            token: validation.token,
        })
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.validation
            .map(|validation| validation.sent_at.saturating_add(Self::RETRY_INTERVAL))
    }

    /// Xorshift, so tokens are unrelated to one another. They travel encrypted, so they only
    /// have to be distinct, not secret.
    const fn take_token(&mut self) -> u32 {
        let mut token = self.next_token;
        token ^= token << 13;
        token ^= token >> 17;
        token ^= token << 5;
        self.next_token = token;
        token
    }
}
