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

    /// The packet rate of Opus frames of `frame_ms`, for the frame sizes Opus defines from
    /// 10 ms up; a 60 ms frame rounds down to 16 Hz, which errs towards a longer budget.
    pub const fn from_opus_frame_ms(frame_ms: u8) -> Option<Self> {
        match frame_ms {
            10 | 20 | 40 | 60 => Some(Self::from_hz_literal(1_000 / frame_ms as u16)),
            _ => None,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_frame_sizes_map_to_their_packet_rates() {
        let rates = [10, 20, 40, 60].map(|ms| FrameRate::from_opus_frame_ms(ms).map(FrameRate::hz));
        assert_eq!(rates, [Some(100), Some(50), Some(25), Some(16)]);
        assert_eq!(
            FrameRate::from_opus_frame_ms(10),
            Some(FrameRate::OPUS_10_MS)
        );
        assert_eq!(FrameRate::from_opus_frame_ms(0), None);
        assert_eq!(FrameRate::from_opus_frame_ms(5), None);
    }
}
