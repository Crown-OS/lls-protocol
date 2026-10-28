#![allow(
    clippy::cast_possible_truncation,
    reason = "the narrowing in `scaled_by` sits behind an explicit bound check, and `TryFrom` is \
              not callable from a `const fn` on stable"
)]

use core::num::NonZeroU32;
use core::time::Duration;

/// A media or pacing rate in bits per second.
///
/// A validated newtype rather than an enum of preset tiers, because bitrate has to be adjustable
/// at runtime for the congestion controller to do anything at all. The tiers survive as
/// associated constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bitrate(NonZeroU32);

impl Bitrate {
    pub const LOW: Self = Self::from_kilobits_literal(6_000);
    pub const STANDARD: Self = Self::from_kilobits_literal(12_000);
    pub const HIGH: Self = Self::from_kilobits_literal(24_000);

    pub const fn from_bits_per_second(bits: NonZeroU32) -> Self {
        Self(bits)
    }

    pub const fn from_kilobits_per_second(kilobits: u32) -> Option<Self> {
        match NonZeroU32::new(kilobits.saturating_mul(1_000)) {
            Some(bits) => Some(Self(bits)),
            None => None,
        }
    }

    pub const fn bits_per_second(self) -> u32 {
        self.0.get()
    }

    pub const fn bytes_per_second(self) -> u32 {
        self.0.get() / 8
    }

    /// How long this rate takes to put `bytes` on the wire.
    pub fn transmission_time(self, bytes: u32) -> Duration {
        let bits = u64::from(bytes).saturating_mul(8);
        Duration::from_micros(bits.saturating_mul(1_000_000) / u64::from(self.bits_per_second()))
    }

    /// Bytes this rate delivers in `span`.
    pub fn bytes_in(self, span: Duration) -> u64 {
        u64::from(self.bytes_per_second())
            .saturating_mul(span.as_micros().min(u128::from(u64::MAX)) as u64)
            / 1_000_000
    }

    #[must_use]
    pub const fn scaled_by(self, numerator: u32, denominator: u32) -> Self {
        if denominator == 0 {
            return self;
        }
        let scaled = (self.0.get() as u64).saturating_mul(numerator as u64) / denominator as u64;
        let clamped = if scaled > u32::MAX as u64 {
            u32::MAX
        } else {
            scaled as u32
        };
        match NonZeroU32::new(clamped) {
            Some(bits) => Self(bits),
            None => Self(NonZeroU32::MIN),
        }
    }

    const fn from_kilobits_literal(kilobits: u32) -> Self {
        match NonZeroU32::new(kilobits * 1_000) {
            Some(bits) => Self(bits),
            None => Self(NonZeroU32::MIN),
        }
    }
}
