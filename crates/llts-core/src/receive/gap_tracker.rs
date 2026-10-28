use llts_wire::SequenceNumber;

use super::{Arrival, SequenceWindow};
use crate::units::LossRate;

/// Watches the arriving sequence stream: what is missing, and how much is being lost.
///
/// The loss figure decays geometrically rather than averaging over all time, so the repair policy
/// reacts to the link as it is now instead of as it was when the session started.
#[derive(Debug, Default)]
pub struct GapTracker {
    window: SequenceWindow,
    expected: u32,
    lost: u32,
}

impl GapTracker {
    /// Halving both counters at this point gives a smoothing horizon of roughly a quarter second
    /// at 1080p60, which is short enough to follow a link degrading and long enough that a single
    /// dropped aggregate does not swing the repair rate.
    const DECAY_THRESHOLD: u32 = 512;

    pub const fn new() -> Self {
        Self {
            window: SequenceWindow::new(),
            expected: 0,
            lost: 0,
        }
    }

    /// Records an arrival.
    ///
    /// `was_retransmission` keeps the estimate honest about the *link* rather than about what
    /// survived repair: a gap the sender had to fill on request still cost the link a packet, and
    /// forgetting that would let a link kept alive entirely by retransmission look pristine — so
    /// the repair policy would never provision the parity that would stop the retransmitting.
    pub fn record(&mut self, sequence: SequenceNumber, was_retransmission: bool) -> Arrival {
        let advance = self.advance_for(sequence);
        let arrival = self.window.insert(sequence);

        match arrival {
            Arrival::Advanced => {
                self.expected = self.expected.saturating_add(advance);
                self.lost = self.lost.saturating_add(advance.saturating_sub(1));
            }
            Arrival::Backfilled if !was_retransmission => {
                self.lost = self.lost.saturating_sub(1);
            }
            Arrival::Backfilled | Arrival::Duplicate | Arrival::Forgotten => {}
        }

        if self.expected >= Self::DECAY_THRESHOLD {
            self.expected /= 2;
            self.lost /= 2;
        }
        arrival
    }

    pub fn loss_rate(&self) -> LossRate {
        LossRate::observed(u64::from(self.lost), u64::from(self.expected))
    }

    pub const fn highest_seen(&self) -> Option<SequenceNumber> {
        self.window.highest()
    }

    pub fn contains(&self, sequence: SequenceNumber) -> bool {
        self.window.contains(sequence)
    }

    pub fn gaps_within(&self, depth: u16) -> impl Iterator<Item = SequenceNumber> + '_ {
        self.window.gaps_within(depth)
    }

    fn advance_for(&self, sequence: SequenceNumber) -> u32 {
        match self.window.highest() {
            None => 1,
            Some(highest) => u32::try_from(highest.distance_to(sequence)).unwrap_or(0),
        }
    }
}
