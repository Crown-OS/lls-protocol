use core::time::Duration;

use llts_wire::MonotonicMicros;

use crate::util::duration::saturating_micros;

/// A point on the local monotonic clock, in microseconds since the session started.
///
/// The engine never reads a clock itself — every entry point takes the current [`Instant`] — so
/// tests drive time directly and a run is exactly reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    pub const SESSION_START: Self = Self(0);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn as_micros(self) -> u64 {
        self.0
    }

    pub const fn saturating_elapsed_since(self, earlier: Self) -> Duration {
        Duration::from_micros(self.0.saturating_sub(earlier.0))
    }

    pub const fn saturating_add(self, span: Duration) -> Self {
        Self(self.0.saturating_add(saturating_micros(span)))
    }

    pub const fn saturating_sub(self, span: Duration) -> Self {
        Self(self.0.saturating_sub(saturating_micros(span)))
    }

    pub const fn is_after(self, other: Self) -> bool {
        self.0 > other.0
    }

    /// This instant as it appears in a packet header.
    ///
    /// Wire timestamps are 32 bits and wrap about every 71 minutes. Only differences between them
    /// are ever meaningful — the peers' clocks share no origin — so the wrap is the intended
    /// behaviour rather than a range error.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the wire format is deliberately 32-bit and wrapping"
    )]
    pub const fn wire_micros(self) -> MonotonicMicros {
        MonotonicMicros::new(self.0 as u32)
    }
}
