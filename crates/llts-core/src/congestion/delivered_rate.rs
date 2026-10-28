use core::time::Duration;

use crate::units::Bitrate;

/// Bytes the receiver actually acknowledged, over a sliding window.
///
/// The rate controller backs off against this rather than against its own target, because after a
/// Wi-Fi rate change the target no longer describes the path.
#[derive(Debug)]
pub struct DeliveredRate {
    bytes: u64,
    window: Duration,
    elapsed: Duration,
}

impl DeliveredRate {
    const DEFAULT_WINDOW: Duration = Duration::from_millis(200);

    pub const fn new() -> Self {
        Self {
            bytes: 0,
            window: Self::DEFAULT_WINDOW,
            elapsed: Duration::ZERO,
        }
    }

    pub fn record(&mut self, byte_len: u32, since_previous: Duration) {
        self.bytes = self.bytes.saturating_add(u64::from(byte_len));
        self.elapsed = self.elapsed.saturating_add(since_previous);

        if self.elapsed > self.window {
            self.bytes /= 2;
            self.elapsed /= 2;
        }
    }

    pub fn rate(&self, fallback: Bitrate) -> Bitrate {
        let Ok(micros) = u64::try_from(self.elapsed.as_micros()) else {
            return fallback;
        };
        if micros == 0 || self.bytes == 0 {
            return fallback;
        }
        let bits_per_second = self
            .bytes
            .saturating_mul(8)
            .saturating_mul(1_000_000)
            .checked_div(micros)
            .unwrap_or(0);
        u32::try_from(bits_per_second)
            .ok()
            .and_then(core::num::NonZeroU32::new)
            .map_or(fallback, Bitrate::from_bits_per_second)
    }
}

impl Default for DeliveredRate {
    fn default() -> Self {
        Self::new()
    }
}
