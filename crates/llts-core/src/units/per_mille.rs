#![allow(
    clippy::cast_possible_truncation,
    reason = "every narrowing here sits behind an explicit bound check on the line above it, and \
              `TryFrom` is not callable from a `const fn` on stable"
)]

/// A ratio in thousandths, the shared representation behind every proportion in the engine.
///
/// Integer rather than `f32` so that a repair rate derived from a loss estimate is exactly
/// reproducible across the two devices and across a replayed test run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct PerMille(u16);

impl PerMille {
    pub const ZERO: Self = Self(0);
    pub const WHOLE: Self = Self(1_000);

    /// Clamps rather than rejecting: a caller asking for 120 % wants "all of it".
    pub const fn from_per_mille(value: u16) -> Self {
        Self(if value > Self::WHOLE.0 {
            Self::WHOLE.0
        } else {
            value
        })
    }

    pub const fn from_percent(percent: u8) -> Self {
        Self::from_per_mille((percent as u16).saturating_mul(10))
    }

    /// The proportion `numerator / denominator`, saturating at one whole.
    pub const fn from_ratio(numerator: u64, denominator: u64) -> Self {
        if denominator == 0 {
            return Self::ZERO;
        }
        let scaled = numerator.saturating_mul(Self::WHOLE.0 as u64) / denominator;
        if scaled > Self::WHOLE.0 as u64 {
            Self::WHOLE
        } else {
            Self(scaled as u16)
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// This proportion of `count`, rounded up so a non-zero ratio never yields nothing.
    pub const fn apply_ceil(self, count: u32) -> u32 {
        if self.0 == 0 {
            return 0;
        }
        let scaled = (count as u64).saturating_mul(self.0 as u64);
        let rounded = scaled.div_ceil(Self::WHOLE.0 as u64);
        if rounded > u32::MAX as u64 {
            u32::MAX
        } else {
            rounded as u32
        }
    }

    pub const fn scaled_by(self, numerator: u16, denominator: u16) -> Self {
        if denominator == 0 {
            return Self::ZERO;
        }
        let scaled = (self.0 as u32).saturating_mul(numerator as u32) / denominator as u32;
        Self::from_per_mille(if scaled > u16::MAX as u32 {
            u16::MAX
        } else {
            scaled as u16
        })
    }

    pub const fn clamp_between(self, low: Self, high: Self) -> Self {
        if self.0 < low.0 {
            low
        } else if self.0 > high.0 {
            high
        } else {
            self
        }
    }

    pub const fn max_of(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }
}
