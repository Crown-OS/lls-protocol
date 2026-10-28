use super::DelayDelta;

/// Tracks the underlying trend in one-way delay, separating a real queue from measurement noise.
///
/// A scalar Kalman filter, the same shape Google Congestion Control uses, but with a process
/// noise an order of magnitude larger. GCC is tuned to ramp over about a second because it runs
/// across the internet; on a two-millisecond LAN that is far too slow to catch a queue before it
/// has already cost a frame.
#[derive(Debug)]
pub struct DelayTrendEstimator {
    estimate: f64,
    error_variance: f64,
    noise_variance: f64,
}

impl Default for DelayTrendEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl DelayTrendEstimator {
    const PROCESS_NOISE: f64 = 1e-2;
    const NOISE_SMOOTHING: f64 = 0.95;
    const MIN_NOISE_VARIANCE: f64 = 1.0;

    pub const fn new() -> Self {
        Self {
            estimate: 0.0,
            error_variance: 0.1,
            noise_variance: Self::MIN_NOISE_VARIANCE,
        }
    }

    /// Milliseconds of queue the path is currently holding, positive when it is growing.
    pub const fn estimate_millis(&self) -> f64 {
        self.estimate
    }

    pub fn observe(&mut self, delta: DelayDelta) {
        let measurement = f64::from(delta.micros) / 1_000.0;
        let residual = measurement - self.estimate;

        let predicted_variance = self.error_variance + Self::PROCESS_NOISE;
        let gain = predicted_variance / (predicted_variance + self.noise_variance);

        self.estimate += gain * residual;
        self.error_variance = (1.0 - gain) * predicted_variance;

        let smoothed = Self::NOISE_SMOOTHING * self.noise_variance
            + (1.0 - Self::NOISE_SMOOTHING) * residual * residual;
        self.noise_variance = if smoothed < Self::MIN_NOISE_VARIANCE {
            Self::MIN_NOISE_VARIANCE
        } else {
            smoothed
        };
    }

    pub const fn reset(&mut self) {
        *self = Self::new();
    }
}
