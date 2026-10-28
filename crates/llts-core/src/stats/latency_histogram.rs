use core::time::Duration;

const BUCKET_WIDTH: Duration = Duration::from_micros(100);
const BUCKET_COUNT: usize = 512;
const DECAY_AFTER: u32 = 4_096;

/// Fixed-resolution latency histogram: 100 µs buckets out to 51.2 ms, plus an overflow bucket.
///
/// Chosen over a streaming quantile estimator because it needs no floating point and no
/// allocation, and because the percentiles it has to answer — is the transport inside its 12 ms
/// budget — sit comfortably inside that range.
#[derive(Debug)]
pub struct LatencyHistogram {
    buckets: [u16; BUCKET_COUNT],
    overflow: u16,
    samples: u32,
    worst: Duration,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyHistogram {
    const BUCKET_WIDTH_MICROS: u64 = 100;
    pub const CEILING: Duration =
        Duration::from_micros(Self::BUCKET_WIDTH_MICROS * BUCKET_COUNT as u64);

    pub const fn new() -> Self {
        Self {
            buckets: [0; BUCKET_COUNT],
            overflow: 0,
            samples: 0,
            worst: Duration::ZERO,
        }
    }

    pub const fn samples(&self) -> u32 {
        self.samples
    }

    pub const fn worst(&self) -> Duration {
        self.worst
    }

    pub fn record(&mut self, latency: Duration) {
        let bucket = (latency.as_micros() / BUCKET_WIDTH.as_micros()) as usize;
        match self.buckets.get_mut(bucket) {
            Some(count) => *count = count.saturating_add(1),
            None => self.overflow = self.overflow.saturating_add(1),
        }
        self.samples = self.samples.saturating_add(1);
        self.worst = self.worst.max(latency);

        if self.samples >= DECAY_AFTER {
            for count in &mut self.buckets {
                *count /= 2;
            }
            self.overflow /= 2;
            self.samples /= 2;
        }
    }

    /// The latency at or below which `percentile` of samples fell.
    pub fn quantile(&self, percentile: u32) -> Duration {
        if self.samples == 0 {
            return Duration::ZERO;
        }
        let threshold = self.samples.saturating_mul(percentile.min(100)) / 100;
        let mut cumulative = 0u32;
        for (index, count) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(u32::from(*count));
            if cumulative >= threshold {
                return BUCKET_WIDTH.saturating_mul(u32::try_from(index).unwrap_or(0) + 1);
            }
        }
        Self::CEILING
    }
}
