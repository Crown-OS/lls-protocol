use alloc::vec::Vec;
use core::time::Duration;

use llts_wire::SequenceNumber;
use llts_wire::packet::MissingRange;

use crate::clock::Instant;
use crate::limits::MAX_PENDING_RETRANSMISSIONS;
use crate::repair::RetransmissionWindow;
use crate::util::sequence_runs::append_coalescing;

#[derive(Debug)]
struct PendingRetransmission {
    sequence: SequenceNumber,
    missed_at: Instant,
    last_requested_at: Option<Instant>,
    attempts: u8,
}

impl PendingRetransmission {
    fn is_due(&self, now: Instant, gap_settle: Duration, retry_interval: Duration) -> bool {
        if now.saturating_elapsed_since(self.missed_at) < gap_settle {
            return false;
        }
        match self.last_requested_at {
            None => true,
            Some(last) => now.saturating_elapsed_since(last) >= retry_interval,
        }
    }
}

/// Decides which lost sequences are worth asking for, and stops asking once they cannot arrive
/// in time.
///
/// Three rules do the work. A gap must persist for [`RetransmissionWindow::gap_settle`] before it
/// counts as loss rather than reordering. A repeat request waits a whole round trip, because
/// asking sooner cannot possibly have new information. And any request that would land after the
/// playout deadline is dropped, since a frame nobody will wait for is not worth the bandwidth.
#[derive(Debug, Default)]
pub struct NackScheduler {
    pending: Vec<PendingRetransmission>,
    requested: u64,
    abandoned: u64,
}

impl NackScheduler {
    /// Beyond this the link is losing the retransmissions too, and parity is the better answer.
    pub const MAX_ATTEMPTS: u8 = 3;

    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(MAX_PENDING_RETRANSMISSIONS),
            requested: 0,
            abandoned: 0,
        }
    }

    pub const fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub const fn requested(&self) -> u64 {
        self.requested
    }

    pub const fn abandoned(&self) -> u64 {
        self.abandoned
    }

    /// Registers newly observed gaps, ignoring any already being chased.
    pub fn note_gaps(&mut self, gaps: impl Iterator<Item = SequenceNumber>, now: Instant) {
        for sequence in gaps {
            if self.pending.len() >= MAX_PENDING_RETRANSMISSIONS {
                return;
            }
            if self.is_pending(sequence) {
                continue;
            }
            self.pending.push(PendingRetransmission {
                sequence,
                missed_at: now,
                last_requested_at: None,
                attempts: 0,
            });
        }
    }

    /// Marks a sequence as arrived, whether from a retransmission or a reordered original.
    pub fn resolve(&mut self, sequence: SequenceNumber) {
        self.pending.retain(|request| request.sequence != sequence);
    }

    /// Fills `ranges` with the requests due now, coalescing consecutive sequences.
    pub fn collect_due(
        &mut self,
        now: Instant,
        playout_deadline: Instant,
        window: &RetransmissionWindow,
        ranges: &mut Vec<MissingRange>,
    ) {
        let give_up_at = playout_deadline.saturating_sub(window.abandon_before_deadline);
        let before = self.pending.len();
        self.pending
            .retain(|request| !now.is_after(give_up_at) && request.attempts < Self::MAX_ATTEMPTS);
        self.abandoned = self
            .abandoned
            .saturating_add((before - self.pending.len()) as u64);

        let retry_interval = window.abandon_before_deadline;
        for request in &mut self.pending {
            if !request.is_due(now, window.gap_settle, retry_interval) {
                continue;
            }
            request.attempts = request.attempts.saturating_add(1);
            request.last_requested_at = Some(now);
            append_coalescing(ranges, request.sequence);
        }
        self.requested = self.requested.saturating_add(
            ranges
                .iter()
                .map(|range| u64::from(range.sequence_count()))
                .sum(),
        );
    }

    /// When the next request becomes due, so a gap at the tail of a burst is chased on a timer
    /// rather than waiting for the next frame's first packet to notice it.
    pub fn next_due(&self, window: &RetransmissionWindow) -> Option<Instant> {
        self.pending
            .iter()
            .filter(|request| request.attempts < Self::MAX_ATTEMPTS)
            .map(|request| match request.last_requested_at {
                None => request.missed_at.saturating_add(window.gap_settle),
                Some(last) => last.saturating_add(window.abandon_before_deadline),
            })
            .min()
    }

    fn is_pending(&self, sequence: SequenceNumber) -> bool {
        self.pending
            .iter()
            .any(|request| request.sequence == sequence)
    }
}
