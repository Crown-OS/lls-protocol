use core::time::Duration;

use super::{RepairMode, RepairPlan, RepairRate, RetransmissionWindow};
use crate::link::LinkQuality;
use crate::units::{LossRate, PerMille};

/// Chooses between parity and retransmission for each repair block.
///
/// On a LAN retransmission wins, and by a wide margin. Parity costs its full rate on every frame
/// forever; a NACK costs one round trip only on the frames that actually lost something, and a
/// 2–4 ms Wi-Fi round trip fits inside a 16.7 ms frame period with room for a second attempt.
/// Moonlight and Sunshine send parity unconditionally because they target wide-area links at
/// 30–80 ms, where a retransmission arrives after the frame is already on screen. That reasoning
/// does not transfer, so this policy keeps parity off until the link earns it.
#[derive(Debug, Clone, Copy)]
pub struct RepairPolicy {
    mode: RepairMode,
}

impl RepairPolicy {
    /// Below this, loss is rare enough that per-frame parity costs more than it recovers.
    pub const PARITY_LOSS_FLOOR: LossRate = LossRate::from_per_mille(2);

    /// Above this, loss is bursty enough that a single retransmission round is unlikely to be
    /// enough, so the bandwidth is better spent on parity.
    pub const RETRANSMISSION_LOSS_CEILING: LossRate = LossRate::from_percent(3);

    /// A round trip longer than this leaves no room to ask and be answered inside a frame period
    /// at 60 Hz, once encode and present are paid for.
    pub const RETRANSMISSION_ROUND_TRIP_CEILING: Duration = Duration::from_millis(8);

    /// Parity to send once loss is high enough that retransmission has been abandoned.
    pub const HEAVY_LOSS_FLOOR: RepairRate = RepairRate::from_percent(25);

    /// Parity ceiling while retransmission is still carrying the residual losses.
    pub const ASSISTED_CEILING: RepairRate = RepairRate::from_percent(30);

    /// Squared confidence factor for the provisioning margin: three standard deviations.
    ///
    /// Squared because the margin is `3 * sqrt(expected)`, and `sqrt(9 * expected)` computes that
    /// in integer arithmetic without leaving `no_std`.
    const CONFIDENCE_SQUARED: u32 = 9;

    /// A gap this old is loss, not reordering. Reordering on a single Wi-Fi hop resolves within
    /// one aggregate.
    const GAP_SETTLE: Duration = Duration::from_micros(500);

    /// Floor on how often the same sequence may be asked for again.
    ///
    /// The interval is normally the round trip, but that reads as zero until the first feedback
    /// lands — and a zero interval means re-asking for the same packet on every arrival, which
    /// buries the link in NACKs exactly when it is least able to carry them.
    const MIN_RETRY_INTERVAL: Duration = Duration::from_millis(1);

    pub const fn new(mode: RepairMode) -> Self {
        Self { mode }
    }

    pub const fn mode(&self) -> RepairMode {
        self.mode
    }

    pub const fn set_mode(&mut self, mode: RepairMode) {
        self.mode = mode;
    }

    /// Decides the protection for a block of `data_shards` fragments on the current link.
    pub fn plan(&self, data_shards: u16, quality: &LinkQuality) -> RepairPlan {
        let rate = self.rate_for(data_shards, quality);
        RepairPlan {
            rate,
            parity_shards: rate.parity_shards_for(data_shards),
            retransmission: self.window_for(quality),
        }
    }

    /// The parity a loss rate calls for on a block of `data_shards`.
    ///
    /// Not simply `loss × shards`. That provisions for the *mean* number of losses, which fails
    /// every time a block loses more than average — about half of them. A block of twenty-three
    /// fragments at 2 % loss expects half a loss and gets one parity shard, yet two or more
    /// fragments go missing often enough to lose several frames a minute. Adding three standard
    /// deviations of a binomial, `3 * sqrt(k·p)`, costs a few percent of bandwidth and turns that
    /// into a rounding error.
    pub fn provisioned_rate(loss: LossRate, data_shards: u16) -> RepairRate {
        if data_shards == 0 || loss.is_none() {
            return RepairRate::NONE;
        }
        let expected = loss.proportion().apply_ceil(u32::from(data_shards));
        let margin = expected.saturating_mul(Self::CONFIDENCE_SQUARED).isqrt();
        RepairRate::from_proportion(PerMille::from_ratio(
            u64::from(expected.saturating_add(margin)),
            u64::from(data_shards),
        ))
    }

    fn rate_for(self, data_shards: u16, quality: &LinkQuality) -> RepairRate {
        match self.mode {
            RepairMode::Off => RepairRate::NONE,
            RepairMode::Fixed(rate) => rate,
            RepairMode::Adaptive { min, max } => self.adaptive_rate(data_shards, quality, min, max),
        }
    }

    fn adaptive_rate(
        self,
        data_shards: u16,
        quality: &LinkQuality,
        min: RepairRate,
        max: RepairRate,
    ) -> RepairRate {
        if !quality.kind.loses_packets() {
            return min;
        }
        let provisioned = Self::provisioned_rate(quality.loss_rate, data_shards);

        let target = if !self.retransmission_is_viable(quality) {
            provisioned.clamp_between(Self::HEAVY_LOSS_FLOOR, RepairRate::MAX)
        } else if quality.loss_rate < Self::PARITY_LOSS_FLOOR {
            RepairRate::NONE
        } else {
            provisioned.clamp_between(RepairRate::NONE, Self::ASSISTED_CEILING)
        };

        target.clamp_between(min, max)
    }

    fn window_for(self, quality: &LinkQuality) -> Option<RetransmissionWindow> {
        if !self.retransmission_is_viable(quality) {
            return None;
        }
        Some(RetransmissionWindow {
            gap_settle: Self::GAP_SETTLE,
            abandon_before_deadline: if quality.round_trip > Self::MIN_RETRY_INTERVAL {
                quality.round_trip
            } else {
                Self::MIN_RETRY_INTERVAL
            },
        })
    }

    fn retransmission_is_viable(self, quality: &LinkQuality) -> bool {
        self.round_trip_affords_a_retry(quality) && !self.parity_is_the_better_spend(quality)
    }

    fn round_trip_affords_a_retry(self, quality: &LinkQuality) -> bool {
        quality.round_trip < Self::RETRANSMISSION_ROUND_TRIP_CEILING
    }

    /// The loss ceiling exists because past it the same bandwidth protects more frames as parity
    /// than as retries. That trade only exists on a link where parity is worth sending at all,
    /// which is why USB never reaches it.
    fn parity_is_the_better_spend(self, quality: &LinkQuality) -> bool {
        quality.kind.loses_packets() && quality.loss_rate > Self::RETRANSMISSION_LOSS_CEILING
    }
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self::new(RepairMode::default())
    }
}
