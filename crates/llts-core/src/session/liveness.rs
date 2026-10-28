use core::time::Duration;

use crate::clock::Instant;

/// Keeps a quiet session provably alive and notices when the peer has gone.
///
/// Probes go out every second when idle and four times a second while media flows, because a
/// media session is the one where a vanished peer costs the most to notice late. Five seconds
/// without a single authenticated packet means the peer is gone.
#[derive(Debug, Default)]
pub struct Liveness {
    last_inbound: Option<Instant>,
    last_probe: Option<Instant>,
    media_active_until: Option<Instant>,
    declared_dead: bool,
}

impl Liveness {
    pub const IDLE_PROBE_INTERVAL: Duration = Duration::from_secs(1);
    pub const ACTIVE_PROBE_INTERVAL: Duration = Duration::from_millis(250);
    pub const DEAD_AFTER: Duration = Duration::from_secs(5);

    /// Media counts as flowing for this long after the last frame either way.
    const MEDIA_ACTIVITY: Duration = Duration::from_secs(1);

    pub const fn new() -> Self {
        Self {
            last_inbound: None,
            last_probe: None,
            media_active_until: None,
            declared_dead: false,
        }
    }

    pub const fn note_inbound(&mut self, now: Instant) {
        self.last_inbound = Some(now);
        self.declared_dead = false;
    }

    pub const fn note_media(&mut self, now: Instant) {
        self.media_active_until = Some(now.saturating_add(Self::MEDIA_ACTIVITY));
    }

    pub const fn is_dead(&self) -> bool {
        self.declared_dead
    }

    /// Whether a probe should go out now; recording that one has.
    pub fn take_probe_due(&mut self, now: Instant) -> bool {
        let due = self.next_probe(now).is_none_or(|due| !due.is_after(now));
        if due {
            self.last_probe = Some(now);
        }
        due
    }

    /// True exactly once, when the peer has been silent for [`Liveness::DEAD_AFTER`].
    pub fn take_newly_dead(&mut self, now: Instant) -> bool {
        let silent_since = *self.last_inbound.get_or_insert(now);
        if self.declared_dead || now.saturating_elapsed_since(silent_since) < Self::DEAD_AFTER {
            return false;
        }
        self.declared_dead = true;
        true
    }

    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let death = self
            .last_inbound
            .filter(|_| !self.declared_dead)
            .map(|last| last.saturating_add(Self::DEAD_AFTER));
        match (self.next_probe(now), death) {
            (Some(probe), Some(death)) => Some(probe.min(death)),
            (probe, death) => probe.or(death),
        }
    }

    fn next_probe(&self, now: Instant) -> Option<Instant> {
        let interval = if self
            .media_active_until
            .is_some_and(|until| until.is_after(now))
        {
            Self::ACTIVE_PROBE_INTERVAL
        } else {
            Self::IDLE_PROBE_INTERVAL
        };
        self.last_probe.map(|last| last.saturating_add(interval))
    }
}
