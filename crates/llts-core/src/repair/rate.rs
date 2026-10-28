use crate::units::PerMille;

/// Parity as a proportion of a frame's data shards.
///
/// Capped at [`RepairRate::MAX`]: past half again as many bytes, a link is better served by
/// dropping resolution than by sending the same picture twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct RepairRate(PerMille);

impl RepairRate {
    pub const NONE: Self = Self(PerMille::ZERO);
    /// The 20 % that Sunshine ships as its `fec_percentage` default.
    pub const SUNSHINE_DEFAULT: Self = Self(PerMille::from_percent(20));
    pub const MAX: Self = Self(PerMille::from_percent(50));

    pub const fn from_percent(percent: u8) -> Self {
        Self::from_proportion(PerMille::from_percent(percent))
    }

    pub const fn from_per_mille(value: u16) -> Self {
        Self::from_proportion(PerMille::from_per_mille(value))
    }

    pub const fn from_proportion(proportion: PerMille) -> Self {
        Self(proportion.clamp_between(PerMille::ZERO, Self::MAX.0))
    }

    pub const fn proportion(self) -> PerMille {
        self.0
    }

    pub const fn is_none(self) -> bool {
        self.0.is_zero()
    }

    pub const fn clamp_between(self, low: Self, high: Self) -> Self {
        Self(self.0.clamp_between(low.0, high.0))
    }

    /// Parity shards this rate calls for, given a block of `data_shards`.
    ///
    /// Rounds up, so a non-zero rate always produces at least one shard: a rate that silently
    /// became no protection would be worse than being told the block is too small.
    pub fn parity_shards_for(self, data_shards: u16) -> u16 {
        if self.0.is_zero() || data_shards == 0 {
            return 0;
        }
        let shards = self.0.apply_ceil(u32::from(data_shards));
        u16::try_from(shards).unwrap_or(u16::MAX).max(1)
    }
}
