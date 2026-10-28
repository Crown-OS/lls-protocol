use core::time::Duration;

use crate::clock::Instant;

/// The six instants a frame passes through, carried per frame so a latency figure can be
/// decomposed instead of merely observed.
///
/// A budget you cannot attribute is a budget you cannot optimise, which is why this exists before
/// any of the code it measures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageTimeline {
    pub captured_at: Option<Instant>,
    pub encoded_at: Option<Instant>,
    pub transmitted_at: Option<Instant>,
    pub received_at: Option<Instant>,
    pub assembled_at: Option<Instant>,
    pub presented_at: Option<Instant>,
}

impl StageTimeline {
    pub const fn captured(at: Instant) -> Self {
        Self {
            captured_at: Some(at),
            encoded_at: None,
            transmitted_at: None,
            received_at: None,
            assembled_at: None,
            presented_at: None,
        }
    }

    pub fn encode(&self) -> Option<Duration> {
        span(self.captured_at, self.encoded_at)
    }

    pub fn packetize(&self) -> Option<Duration> {
        span(self.encoded_at, self.transmitted_at)
    }

    pub fn network(&self) -> Option<Duration> {
        span(self.transmitted_at, self.received_at)
    }

    pub fn assemble(&self) -> Option<Duration> {
        span(self.received_at, self.assembled_at)
    }

    pub fn playout(&self) -> Option<Duration> {
        span(self.assembled_at, self.presented_at)
    }

    /// The figure the CI gate reads: everything the library is responsible for.
    pub fn protocol(&self) -> Option<Duration> {
        span(self.encoded_at, self.presented_at)
    }

    pub fn glass_to_glass(&self) -> Option<Duration> {
        span(self.captured_at, self.presented_at)
    }
}

fn span(from: Option<Instant>, to: Option<Instant>) -> Option<Duration> {
    Some(to?.saturating_elapsed_since(from?))
}
