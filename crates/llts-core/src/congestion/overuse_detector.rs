use core::time::Duration;

use crate::clock::Instant;

/// What the delay trend says the path is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkUsage {
    /// A queue is forming; send less.
    Overusing,
    #[default]
    Normal,
    /// A queue is draining; hold until it has.
    Underusing,
}

/// Turns a continuous delay trend into a three-way verdict against a threshold that adapts.
///
/// The threshold moves because a fixed one is wrong twice over: too tight and ordinary Wi-Fi
/// aggregation reads as congestion, too loose and a real queue goes unnoticed. It tracks the
/// magnitude it observes — rising quickly so a jittery link stops crying wolf, falling slowly so
/// a link that has calmed down regains its sensitivity without flapping.
#[derive(Debug)]
pub struct OveruseDetector {
    threshold_millis: f64,
    last_update: Option<Instant>,
    overusing_since: Option<Instant>,
    usage: NetworkUsage,
}

impl Default for OveruseDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl OveruseDetector {
    const INITIAL_THRESHOLD_MILLIS: f64 = 1.5;
    const MIN_THRESHOLD_MILLIS: f64 = 0.4;
    const MAX_THRESHOLD_MILLIS: f64 = 12.0;
    const RISE_GAIN: f64 = 0.05;
    const FALL_GAIN: f64 = 0.003;

    /// A trend must stay above the threshold this long before it counts, so one late aggregate
    /// does not halve the bitrate.
    const CONFIRMATION: Duration = Duration::from_millis(6);

    pub const fn new() -> Self {
        Self {
            threshold_millis: Self::INITIAL_THRESHOLD_MILLIS,
            last_update: None,
            overusing_since: None,
            usage: NetworkUsage::Normal,
        }
    }

    pub const fn usage(&self) -> NetworkUsage {
        self.usage
    }

    pub const fn threshold_millis(&self) -> f64 {
        self.threshold_millis
    }

    pub fn observe(&mut self, trend_millis: f64, now: Instant) -> NetworkUsage {
        let magnitude = if trend_millis < 0.0 {
            -trend_millis
        } else {
            trend_millis
        };

        self.usage = if trend_millis > self.threshold_millis {
            let since = *self.overusing_since.get_or_insert(now);
            if now.saturating_elapsed_since(since) >= Self::CONFIRMATION {
                NetworkUsage::Overusing
            } else {
                NetworkUsage::Normal
            }
        } else {
            self.overusing_since = None;
            if trend_millis < -self.threshold_millis {
                NetworkUsage::Underusing
            } else {
                NetworkUsage::Normal
            }
        };

        self.adapt_threshold(magnitude, now);
        self.usage
    }

    fn adapt_threshold(&mut self, magnitude: f64, now: Instant) {
        let elapsed = self
            .last_update
            .map_or(Duration::ZERO, |last| now.saturating_elapsed_since(last));
        self.last_update = Some(now);

        let millis = micros_as_f64(elapsed) / 1_000.0;
        if millis <= 0.0 {
            return;
        }
        let gain = if magnitude > self.threshold_millis {
            Self::RISE_GAIN
        } else {
            Self::FALL_GAIN
        };
        self.threshold_millis += gain * (magnitude - self.threshold_millis) * millis;

        self.threshold_millis = self
            .threshold_millis
            .clamp(Self::MIN_THRESHOLD_MILLIS, Self::MAX_THRESHOLD_MILLIS);
    }
}

fn micros_as_f64(span: Duration) -> f64 {
    u32::try_from(span.as_micros()).map_or(f64::from(u32::MAX), f64::from)
}
