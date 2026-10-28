use core::time::Duration;

use crate::config::BackoffPolicy;
use crate::util::jitter::Jitter;

/// Consecutive failures to reach one peer, which set how long the next retry waits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Backoff {
    failures: u32,
}

/// Doubling stops here; by then the policy's maximum has long been reached.
const MAX_DOUBLINGS: u32 = 16;

impl Backoff {
    /// Records a failure and returns how long to wait before trying again: the policy's initial
    /// delay doubled per earlier failure, capped, then jittered.
    pub(crate) fn next_delay(&mut self, policy: &BackoffPolicy, jitter: &mut Jitter) -> Duration {
        let doubled = policy
            .initial
            .saturating_mul(1 << self.failures.min(MAX_DOUBLINGS));
        self.failures = self.failures.saturating_add(1);
        jitter.spread(doubled.min(policy.max), policy.jitter_percent)
    }

    pub(crate) const fn reset(&mut self) {
        self.failures = 0;
    }
}
