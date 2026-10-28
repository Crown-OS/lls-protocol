use core::time::Duration;

use super::{
    AckedPacket, DelayTrendEstimator, DeliveredRate, InterArrivalFilter, NetworkUsage,
    OveruseDetector, ProbePadding, RateController,
};
use crate::clock::Instant;
use crate::repair::RepairRate;
use crate::units::{Bitrate, FrameRate};

/// The whole delay-based sending rate loop.
///
/// Composed from the four pieces it needs so each can be tested on its own: group arrivals,
/// estimate the trend, judge it against an adaptive threshold, then move the rate.
#[derive(Debug)]
pub struct CongestionController {
    arrivals: InterArrivalFilter,
    trend: DelayTrendEstimator,
    detector: OveruseDetector,
    rate: RateController,
    delivered: DeliveredRate,
    padding: ProbePadding,
    last_ack: Option<Instant>,
}

impl CongestionController {
    /// Pacing runs above the media rate so a frame's burst clears in a fraction of its period
    /// instead of smearing across the whole of it. Pacing at the media rate would put the tail of
    /// every frame a full period late, which is most of the latency budget.
    pub const PACING_NUMERATOR: u32 = 5;
    pub const PACING_DENOMINATOR: u32 = 2;

    pub const fn new(initial: Bitrate, floor: Bitrate, ceiling: Bitrate) -> Self {
        Self {
            arrivals: InterArrivalFilter::new(),
            trend: DelayTrendEstimator::new(),
            detector: OveruseDetector::new(),
            rate: RateController::new(initial, floor, ceiling),
            delivered: DeliveredRate::new(),
            padding: ProbePadding::new(),
            last_ack: None,
        }
    }

    pub const fn target_bitrate(&self) -> Bitrate {
        self.rate.target()
    }

    /// The rate the pacer should run at, above the media rate by design.
    pub const fn pacing_rate(&self) -> Bitrate {
        self.rate
            .target()
            .scaled_by(Self::PACING_NUMERATOR, Self::PACING_DENOMINATOR)
    }

    pub const fn usage(&self) -> NetworkUsage {
        self.detector.usage()
    }

    pub const fn queue_estimate_millis(&self) -> f64 {
        self.trend.estimate_millis()
    }

    pub const fn set_bounds(&mut self, floor: Bitrate, ceiling: Bitrate) {
        self.rate.set_bounds(floor, ceiling);
    }

    pub const fn set_padding(&mut self, padding: ProbePadding) {
        self.padding = padding;
    }

    /// Folds in one acknowledged packet and returns the target bitrate that results.
    pub fn observe(&mut self, packet: AckedPacket, now: Instant) -> Bitrate {
        let since_previous = self
            .last_ack
            .map_or(Duration::ZERO, |last| now.saturating_elapsed_since(last));
        self.last_ack = Some(now);
        self.delivered.record(packet.byte_len, since_previous);

        if let Some(delta) = self.arrivals.accept(packet) {
            self.trend.observe(delta);
            let usage = self.detector.observe(self.trend.estimate_millis(), now);
            let delivered = self.delivered.rate(self.rate.target());
            self.rate.observe(usage, delivered, now);
        }
        self.rate.target()
    }

    /// Bytes the next frame may occupy at the current target, after reserving for parity and
    /// controller overshoot.
    pub fn frame_byte_budget(&self, rate: FrameRate, repair: RepairRate) -> u32 {
        crate::session::frame_byte_budget(self.rate.target(), rate, repair)
    }

    /// Filler bytes worth adding when the encoder underran its budget.
    pub fn padding_deficit(&self, produced_bytes: u32, rate: FrameRate) -> u32 {
        self.padding
            .deficit(produced_bytes, self.pacing_rate(), rate.period())
    }
}
