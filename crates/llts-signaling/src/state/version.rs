use serde::{Deserialize, Serialize};

use crate::SignalingError;

/// A random value an owner draws once per process start.
///
/// It lets a peer tell "the owner restarted and its sequence began again" apart from "this update
/// is older than what I have", without the owner persisting a counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Incarnation(u64);

impl Incarnation {
    pub fn random() -> Result<Self, SignalingError> {
        Ok(Self(getrandom::u64()?))
    }

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

/// Which write of a document a copy reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version {
    pub incarnation: Incarnation,
    pub seq: u64,
}

impl Version {
    /// Whether a copy at this version should replace one cached at `cached`.
    ///
    /// Versions from different incarnations are not ordered: a new incarnation always wins,
    /// because the owner only ever runs one at a time.
    #[must_use]
    pub const fn supersedes(self, cached: Self) -> bool {
        self.incarnation.0 != cached.incarnation.0 || self.seq > cached.seq
    }
}
