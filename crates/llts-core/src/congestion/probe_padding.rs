use core::time::Duration;

use crate::units::Bitrate;

/// Works out how much filler to send when the encoder produced less than the pacing rate.
///
/// A 15 Mbit/s stream on a 600 Mbit/s link never queues, so a delay-based estimator sees a flat
/// line and learns nothing — it cannot tell a healthy link from one it has not probed. Padding to
/// the pacing rate keeps the gradient observable. Vidaptive measured this as worth 1.5× the
/// bitrate and 40 % VMAF on a constrained path; here it mostly buys a controller that knows what
/// headroom it has.
///
/// Filler is marked discardable, so it is never delivered to a decoder and never retransmitted.
#[derive(Debug, Clone, Copy)]
pub struct ProbePadding {
    enabled: bool,
    ceiling_bytes: u32,
}

impl ProbePadding {
    /// Padding beyond this per period is spending real bandwidth to learn something the next
    /// keyframe would have revealed anyway.
    pub const DEFAULT_CEILING_BYTES: u32 = 16 * 1024;

    pub const fn new() -> Self {
        Self {
            enabled: true,
            ceiling_bytes: Self::DEFAULT_CEILING_BYTES,
        }
    }

    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            ceiling_bytes: 0,
        }
    }

    pub const fn set_ceiling_bytes(&mut self, ceiling: u32) {
        self.ceiling_bytes = ceiling;
    }

    /// Filler bytes to add so that `produced_bytes` reaches `rate` over `period`.
    pub fn deficit(&self, produced_bytes: u32, rate: Bitrate, period: Duration) -> u32 {
        if !self.enabled {
            return 0;
        }
        let expected = u32::try_from(rate.bytes_in(period)).unwrap_or(u32::MAX);
        expected
            .saturating_sub(produced_bytes)
            .min(self.ceiling_bytes)
    }
}

impl Default for ProbePadding {
    fn default() -> Self {
        Self::new()
    }
}
