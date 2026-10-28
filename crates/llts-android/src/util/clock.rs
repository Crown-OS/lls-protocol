use std::time::{Instant as WallClock, SystemTime, UNIX_EPOCH};

use llts_core::clock::Instant;
use llts_node::Now;

/// Samples the two clocks a node reads, relative to when the runtime started.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeClock {
    origin: WallClock,
}

impl NodeClock {
    pub(crate) fn start() -> Self {
        Self {
            origin: WallClock::now(),
        }
    }

    pub(crate) fn now(self) -> Now {
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        let micros = u64::try_from(self.origin.elapsed().as_micros()).unwrap_or(u64::MAX);
        Now::new(Instant::from_micros(micros), unix_ms)
    }
}
