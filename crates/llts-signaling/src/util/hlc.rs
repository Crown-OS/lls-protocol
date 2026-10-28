use serde::{Deserialize, Serialize};

/// A hybrid logical clock reading: wall-clock milliseconds, plus a counter that orders events the
/// wall clock cannot tell apart.
///
/// Readings compare by wall time first, so they stay close to real time, and by counter second,
/// so causality survives a peer whose clock runs behind.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct HybridTimestamp {
    pub wall_ms: u64,
    pub counter: u32,
}

/// Issues [`HybridTimestamp`]s that never go backwards, even when the local clock does.
#[derive(Debug, Clone, Copy, Default)]
pub struct HybridClock {
    latest: HybridTimestamp,
}

impl HybridClock {
    pub const fn latest(&self) -> HybridTimestamp {
        self.latest
    }

    /// Stamps a local event, such as the user copying something.
    pub const fn tick(&mut self, physical_ms: u64) -> HybridTimestamp {
        self.latest = if physical_ms > self.latest.wall_ms {
            HybridTimestamp {
                wall_ms: physical_ms,
                counter: 0,
            }
        } else {
            self.latest.successor()
        };
        self.latest
    }

    /// Folds in a reading received from a peer, so the next local event orders after it.
    pub fn observe(&mut self, remote: HybridTimestamp, physical_ms: u64) -> HybridTimestamp {
        let newest_seen = self.latest.max(remote);
        self.latest = if physical_ms > newest_seen.wall_ms {
            HybridTimestamp {
                wall_ms: physical_ms,
                counter: 0,
            }
        } else {
            newest_seen.successor()
        };
        self.latest
    }
}

impl HybridTimestamp {
    const fn successor(self) -> Self {
        match self.counter.checked_add(1) {
            Some(counter) => Self {
                wall_ms: self.wall_ms,
                counter,
            },
            None => Self {
                wall_ms: self.wall_ms.saturating_add(1),
                counter: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_follow_the_wall_clock_when_it_advances() {
        let mut clock = HybridClock::default();
        assert_eq!(
            clock.tick(100),
            HybridTimestamp {
                wall_ms: 100,
                counter: 0
            }
        );
        assert_eq!(
            clock.tick(250),
            HybridTimestamp {
                wall_ms: 250,
                counter: 0
            }
        );
    }

    #[test]
    fn ticks_stay_monotonic_when_the_wall_clock_stalls_or_steps_back() {
        let mut clock = HybridClock::default();
        let first = clock.tick(1_000);
        let stalled = clock.tick(1_000);
        let stepped_back = clock.tick(400);
        assert!(first < stalled);
        assert!(stalled < stepped_back);
        assert_eq!(stepped_back.wall_ms, 1_000);
    }

    #[test]
    fn observing_a_peer_ahead_orders_the_next_local_event_after_it() {
        let mut behind = HybridClock::default();
        behind.tick(10);
        let remote = HybridTimestamp {
            wall_ms: 5_000,
            counter: 7,
        };
        let merged = behind.observe(remote, 20);
        assert!(merged > remote);
        assert!(behind.tick(30) > remote);
    }

    #[test]
    fn observing_a_stale_peer_does_not_rewind_the_clock() {
        let mut clock = HybridClock::default();
        let local = clock.tick(9_000);
        let merged = clock.observe(
            HybridTimestamp {
                wall_ms: 1,
                counter: 0,
            },
            9_000,
        );
        assert!(merged > local);
    }

    #[test]
    fn a_counter_overflow_carries_into_the_wall_time() {
        let mut clock = HybridClock {
            latest: HybridTimestamp {
                wall_ms: 7,
                counter: u32::MAX,
            },
        };
        assert_eq!(
            clock.tick(0),
            HybridTimestamp {
                wall_ms: 8,
                counter: 0
            }
        );
    }
}
