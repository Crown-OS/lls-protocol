use super::PerMille;

/// Fraction of packets the receiver never saw, smoothed over a short window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct LossRate(PerMille);

impl LossRate {
    pub const NONE: Self = Self(PerMille::ZERO);

    pub const fn from_per_mille(value: u16) -> Self {
        Self(PerMille::from_per_mille(value))
    }

    pub const fn from_percent(percent: u8) -> Self {
        Self(PerMille::from_percent(percent))
    }

    pub const fn observed(lost: u64, expected: u64) -> Self {
        Self(PerMille::from_ratio(lost, expected))
    }

    pub const fn proportion(self) -> PerMille {
        self.0
    }

    pub const fn is_none(self) -> bool {
        self.0.is_zero()
    }
}
