use core::time::Duration;

use llts_core::clock::Instant;
use llts_signaling::discovery::{EPOCH_SECONDS, Epoch};

/// The two clocks a node reads, sampled together by the runtime for every call.
///
/// The monotonic `instant` drives every timer; wall-clock `unix_ms` only names discovery
/// epochs, stamps clipboards and dates pairing offers, so a stepped wall clock never stalls a
/// retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub instant: Instant,
    pub unix_ms: u64,
}

impl Now {
    pub const fn new(instant: Instant, unix_ms: u64) -> Self {
        Self { instant, unix_ms }
    }

    pub const fn unix_seconds(self) -> u64 {
        self.unix_ms / 1_000
    }

    pub const fn epoch(self) -> Epoch {
        Epoch::from_unix_seconds(self.unix_seconds())
    }

    /// When, on the monotonic clock, the discovery epoch after this one begins.
    pub const fn next_epoch_at(self) -> Instant {
        let epoch_ms = EPOCH_SECONDS * 1_000;
        let into_epoch = self.unix_ms % epoch_ms;
        self.instant
            .saturating_add(Duration::from_millis(epoch_ms - into_epoch))
    }
}
