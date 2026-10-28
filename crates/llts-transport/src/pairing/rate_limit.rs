use core::time::Duration;
use std::net::IpAddr;

use llts_core::clock::Instant;

use crate::util::BoundedMap;

#[derive(Debug, Clone, Copy)]
struct TokenBucket {
    tokens: u32,
    refilled_at: Instant,
}

impl TokenBucket {
    fn take(&mut self, now: Instant, capacity: u32, refill_every: Duration) -> bool {
        let elapsed = now.saturating_elapsed_since(self.refilled_at);
        let refills = u32::try_from(elapsed.as_micros() / refill_every.as_micros().max(1))
            .unwrap_or(u32::MAX);
        if refills > 0 {
            self.tokens = self.tokens.saturating_add(refills).min(capacity);
            self.refilled_at = now;
        }
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

/// Caps how often first handshake messages are processed, overall and per source address.
///
/// A first message costs the responder a Diffie-Hellman or two before it knows whether the
/// sender is anyone it trusts. Both buckets have to allow it; the per-source table is bounded,
/// and a full table falls back on the global bucket alone.
#[derive(Debug)]
pub struct HandshakeRateLimiter {
    global: TokenBucket,
    per_source: BoundedMap<IpAddr, TokenBucket, { HandshakeRateLimiter::TRACKED_SOURCES }>,
}

impl Default for HandshakeRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl HandshakeRateLimiter {
    const TRACKED_SOURCES: usize = 64;
    const GLOBAL_BURST: u32 = 32;
    const GLOBAL_REFILL: Duration = Duration::from_millis(50);
    const SOURCE_BURST: u32 = 4;
    const SOURCE_REFILL: Duration = Duration::from_millis(250);

    pub fn new() -> Self {
        Self {
            global: TokenBucket {
                tokens: Self::GLOBAL_BURST,
                refilled_at: Instant::SESSION_START,
            },
            per_source: BoundedMap::new(),
        }
    }

    pub fn allow(&mut self, source: IpAddr, now: Instant) -> bool {
        let fresh = TokenBucket {
            tokens: Self::SOURCE_BURST,
            refilled_at: now,
        };
        let source_allows = self
            .per_source
            .get_or_insert_with(source, || fresh)
            .is_none_or(|bucket| bucket.take(now, Self::SOURCE_BURST, Self::SOURCE_REFILL));
        source_allows
            && self
                .global
                .take(now, Self::GLOBAL_BURST, Self::GLOBAL_REFILL)
    }
}
