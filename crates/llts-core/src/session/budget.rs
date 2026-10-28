use crate::repair::RepairRate;
use crate::send::LanePacer;
use crate::units::{Bitrate, FrameRate};

/// Splits the congestion controller's estimate between the streams sharing a path.
///
/// Audio takes its fixed rate first, since an Opus encoder does not follow a target; an open
/// data channel reserves its pacer share; video divides what is left. Parity then comes out of
/// each video stream's slice, so the encoder is told the rate its *source* bytes may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaBudget {
    pub estimate: Bitrate,
    pub reserved_bits_per_second: u64,
    pub reserves_data_share: bool,
    pub video_streams: u32,
}

impl MediaBudget {
    /// Headroom left for the rate controller's own overshoot when sizing a frame.
    const FRAME_BUDGET_NUMERATOR: u64 = 9;
    const FRAME_BUDGET_DENOMINATOR: u64 = 10;

    /// Bandwidth one video stream may use, parity included.
    pub fn video_share(&self) -> Bitrate {
        let total = u64::from(self.estimate.bits_per_second());
        let data = if self.reserves_data_share {
            total * u64::from(LanePacer::DATA_SHARE_PERCENT) / 100
        } else {
            0
        };
        let remaining = total
            .saturating_sub(self.reserved_bits_per_second)
            .saturating_sub(data)
            / u64::from(self.video_streams.max(1));
        let clamped = u32::try_from(remaining.max(1)).unwrap_or(u32::MAX);
        core::num::NonZeroU32::new(clamped).map_or(self.estimate, Bitrate::from_bits_per_second)
    }

    /// The rate the encoder should target once parity has taken its share.
    pub fn encoder_target(&self, repair: RepairRate) -> Bitrate {
        let share = self.video_share();
        let parity = repair.proportion().get();
        share.scaled_by(1_000_u32.saturating_sub(u32::from(parity)).max(1), 1_000)
    }
}

/// Bytes the next frame may occupy at `rate`, after reserving for parity and controller
/// overshoot.
///
/// Handing the encoder a hard per-frame cap, rather than a bitrate it averages over a GOP, is
/// what lets the rate loop close inside one frame — the same choice Salsify and Parsec both
/// arrived at independently.
pub fn frame_byte_budget(rate: Bitrate, frame_rate: FrameRate, repair: RepairRate) -> u32 {
    let per_frame = rate.bytes_in(frame_rate.period());
    let after_parity = per_frame.saturating_sub(u64::from(
        repair
            .proportion()
            .apply_ceil(u32::try_from(per_frame).unwrap_or(u32::MAX)),
    ));
    let budgeted = after_parity.saturating_mul(MediaBudget::FRAME_BUDGET_NUMERATOR)
        / MediaBudget::FRAME_BUDGET_DENOMINATOR;
    u32::try_from(budgeted).unwrap_or(u32::MAX)
}
