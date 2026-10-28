use core::time::Duration;

/// SplitMix64: a tiny generator for spreading retry delays.
///
/// Only timing depends on it, never anything an attacker could exploit, so a fast
/// non-cryptographic generator seeded once from the operating system is enough.
#[derive(Debug, Clone)]
pub(crate) struct Jitter {
    state: u64,
}

impl Jitter {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    const fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// `base` moved uniformly within `±percent` of itself.
    pub(crate) fn spread(&mut self, base: Duration, percent: u8) -> Duration {
        let base_us = u64::try_from(base.as_micros()).unwrap_or(u64::MAX);
        let reach = base_us / 100 * u64::from(percent.min(100));
        let offset = self.next() % (reach * 2 + 1);
        Duration::from_micros((base_us - reach).saturating_add(offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_stays_within_the_requested_band() {
        let mut jitter = Jitter::new(7);
        let base = Duration::from_millis(1_000);
        for _ in 0..1_000 {
            let delay = jitter.spread(base, 20);
            assert!(delay >= Duration::from_millis(800) && delay <= Duration::from_millis(1_200));
        }
        assert_eq!(jitter.spread(base, 0), base);
    }
}
