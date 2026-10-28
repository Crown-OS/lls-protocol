use core::time::Duration;

const BUCKET_WIDTH: Duration = Duration::from_micros(250);
const BUCKET_COUNT: usize = 128;
const DECAY_AFTER: u32 = 512;
const TARGET_PERCENTILE: u32 = 99;

/// Chooses how long to hold a completed frame before presenting it.
///
/// Targets a high percentile of observed arrival deviation rather than the mean, because the
/// tail is what causes a visible stall — the same shape as WebRTC's NetEQ, calibrated for a LAN
/// where the histogram usually settles within a millisecond or two of zero.
///
/// Clamped to one frame period: a jitter buffer deeper than that has stopped hiding jitter and
/// started adding latency.
#[derive(Debug)]
pub struct AdaptivePlayoutDelay {
    buckets: [u16; BUCKET_COUNT],
    samples: u32,
    ceiling: Duration,
    target: Duration,
}

impl AdaptivePlayoutDelay {
    pub const fn new(ceiling: Duration) -> Self {
        Self {
            buckets: [0; BUCKET_COUNT],
            samples: 0,
            ceiling,
            target: Duration::ZERO,
        }
    }

    pub const fn target(&self) -> Duration {
        self.target
    }

    pub const fn ceiling(&self) -> Duration {
        self.ceiling
    }

    /// Narrows or widens the clamp when the frame rate changes.
    pub fn set_ceiling(&mut self, ceiling: Duration) {
        self.ceiling = ceiling;
        self.target = self.target.min(ceiling);
    }

    /// Records how far one frame's arrival strayed from the cadence it should have kept.
    pub fn observe(&mut self, deviation: Duration) {
        let bucket = (deviation.as_micros() / BUCKET_WIDTH.as_micros()) as usize;
        let bucket = bucket.min(BUCKET_COUNT - 1);
        if let Some(count) = self.buckets.get_mut(bucket) {
            *count = count.saturating_add(1);
        }
        self.samples = self.samples.saturating_add(1);

        if self.samples >= DECAY_AFTER {
            for count in &mut self.buckets {
                *count /= 2;
            }
            self.samples /= 2;
        }
        self.target = self.percentile().min(self.ceiling);
    }

    fn percentile(&self) -> Duration {
        let threshold = u32::from(u16::try_from(self.samples).unwrap_or(u16::MAX))
            .saturating_mul(TARGET_PERCENTILE)
            / 100;
        let mut cumulative = 0u32;
        for (index, count) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(u32::from(*count));
            if cumulative >= threshold {
                return BUCKET_WIDTH.saturating_mul(u32::try_from(index).unwrap_or(0) + 1);
            }
        }
        Duration::ZERO
    }
}
