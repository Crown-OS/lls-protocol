use core::time::Duration;

/// Tracks the shortest round trip seen recently.
///
/// The minimum is the sample least polluted by queueing, which is what the repair policy wants
/// when deciding whether a retransmission can still arrive in time. It decays so that a link
/// which genuinely slowed down is not judged against a stale best case.
#[derive(Debug, Default)]
pub struct RoundTripFilter {
    best: Option<Duration>,
    samples: u32,
}

impl RoundTripFilter {
    /// Forgetting the best sample this often keeps the estimate honest across a Wi-Fi rate change
    /// without letting one lucky packet define the link for the rest of the session.
    const FORGET_AFTER: u32 = 256;

    pub const fn new() -> Self {
        Self {
            best: None,
            samples: 0,
        }
    }

    pub const fn best(&self) -> Duration {
        match self.best {
            Some(best) => best,
            None => Duration::ZERO,
        }
    }

    pub const fn has_sample(&self) -> bool {
        self.best.is_some()
    }

    pub fn observe(&mut self, sample: Duration) {
        self.samples = self.samples.saturating_add(1);
        if self.samples >= Self::FORGET_AFTER {
            self.samples = 0;
            self.best = Some(sample);
            return;
        }
        self.best = Some(match self.best {
            Some(best) if best <= sample => best,
            _ => sample,
        });
    }
}
