use core::num::NonZeroU16;
use core::time::Duration;

/// Frames per second, which sets the playout budget every other deadline is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameRate(NonZeroU16);

impl FrameRate {
    pub const LOW: Self = Self::from_hz_literal(30);
    pub const STANDARD: Self = Self::from_hz_literal(60);
    pub const SMOOTH: Self = Self::from_hz_literal(90);
    pub const SUPER_SMOOTH: Self = Self::from_hz_literal(120);
    /// Opus at its 10 ms frame size.
    pub const OPUS_10_MS: Self = Self::from_hz_literal(100);

    pub const fn from_hz(hz: NonZeroU16) -> Self {
        Self(hz)
    }

    pub const fn hz(self) -> u16 {
        self.0.get()
    }

    /// The wall-clock budget one frame has from capture to present.
    pub const fn period(self) -> Duration {
        Duration::from_nanos(1_000_000_000 / self.0.get() as u64)
    }

    const fn from_hz_literal(hz: u16) -> Self {
        match NonZeroU16::new(hz) {
            Some(rate) => Self(rate),
            None => Self(NonZeroU16::MIN),
        }
    }
}
