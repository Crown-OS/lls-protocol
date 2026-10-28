use core::num::NonZeroU32;
use core::time::Duration;

/// Sampling clock of a media stream, in ticks per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockRate(NonZeroU32);

impl ClockRate {
    /// 90 kHz, the RTP video convention: an exact divisor of 24, 25, 30, 50, 60 and 90 fps.
    pub const VIDEO: Self = Self::from_hz_literal(90_000);
    /// 48 kHz, matching Opus.
    pub const AUDIO: Self = Self::from_hz_literal(48_000);

    const fn from_hz_literal(hz: u32) -> Self {
        match NonZeroU32::new(hz) {
            Some(rate) => Self(rate),
            None => Self(NonZeroU32::MIN),
        }
    }

    pub const fn new(hz: NonZeroU32) -> Self {
        Self(hz)
    }

    pub const fn hz(self) -> u32 {
        self.0.get()
    }
}

/// Capture instant of a frame, in [`ClockRate`] ticks, wrapping at `u32::MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaTimestamp(u32);

impl MediaTimestamp {
    pub const ZERO: Self = Self(0);

    pub const fn new(ticks: u32) -> Self {
        Self(ticks)
    }

    pub const fn ticks(self) -> u32 {
        self.0
    }

    pub fn from_duration(elapsed: Duration, rate: ClockRate) -> Self {
        let ticks = elapsed.as_nanos().saturating_mul(u128::from(rate.hz())) / 1_000_000_000;
        Self(u32::try_from(ticks & u128::from(u32::MAX)).unwrap_or(u32::MAX))
    }

    pub fn to_duration(self, rate: ClockRate) -> Duration {
        Duration::from_nanos(u64::from(self.0) * 1_000_000_000 / u64::from(rate.hz()))
    }

    /// Ticks elapsed since `earlier`, correct across a single wraparound.
    pub const fn ticks_since(self, earlier: Self) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }
}

/// Sender-local monotonic microseconds, wrapping roughly every 71 minutes.
///
/// Only differences are ever meaningful; the two endpoints' clocks are related by an offset the
/// session estimates separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonotonicMicros(u32);

impl MonotonicMicros {
    pub const ZERO: Self = Self(0);

    pub const fn new(micros: u32) -> Self {
        Self(micros)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn elapsed_since(self, earlier: Self) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }
}
