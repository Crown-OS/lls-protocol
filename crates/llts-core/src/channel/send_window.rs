use alloc::collections::VecDeque;
use core::time::Duration;

use llts_wire::{FrameId, SequenceNumber};

use super::{Delivery, MAX_MESSAGE_LEN};
use crate::CoreError;
use crate::buffer::{BufferPool, PooledBuffer};
use crate::clock::Instant;
use crate::util::chunking::chunks_including_empty;

/// One fragment held until the peer acknowledges it.
#[derive(Debug)]
pub(crate) struct PendingFragment {
    pub(crate) channel_sequence: SequenceNumber,
    pub(crate) message: FrameId,
    pub(crate) index: u16,
    pub(crate) count: u16,
    pub(crate) byte_offset: u32,
    pub(crate) delivery: Delivery,
    pub(crate) payload: PooledBuffer,
    sent_at: Option<Instant>,
    attempts: u8,
}

impl PendingFragment {
    pub(crate) const fn is_last(&self) -> bool {
        self.index + 1 == self.count
    }

    fn retransmit_at(&self, timeout: Duration) -> Option<Instant> {
        let backoff = 1u32 << self.attempts.saturating_sub(1).min(5);
        self.sent_at
            .map(|sent| sent.saturating_add(timeout.saturating_mul(backoff)))
    }
}

/// The sending half of a reliable channel: fragments in channel-sequence order, each held until
/// a cumulative acknowledgement covers it and resent when its timer runs out.
///
/// Fragments are sent in order, so the ones on the wire are always a prefix of the queue and
/// the rest wait for the window to open.
#[derive(Debug)]
pub(crate) struct SendWindow {
    pending: VecDeque<PendingFragment>,
    sent_count: usize,
    next_channel_sequence: SequenceNumber,
    next_message: FrameId,
    pool: BufferPool,
    fragment_len: usize,
    queued_bytes: usize,
}

impl SendWindow {
    /// Fragments on the wire at once. The receiver's window is sized to match.
    pub(crate) const IN_FLIGHT_LIMIT: usize = 256;

    /// Bytes a channel will hold before refusing more, so a stalled peer cannot grow it forever.
    pub(crate) const QUEUE_LIMIT_BYTES: usize = 1 << 20;

    /// A message may span this many fragments, one bit each in the receiver's bookkeeping.
    pub(crate) const MAX_FRAGMENTS: usize = 64;

    pub(crate) fn new(pool: BufferPool, fragment_len: usize) -> Self {
        Self {
            pending: VecDeque::with_capacity(64),
            sent_count: 0,
            next_channel_sequence: SequenceNumber::ZERO,
            next_message: FrameId::ZERO,
            pool,
            fragment_len: fragment_len.max(1),
            queued_bytes: 0,
        }
    }

    pub(crate) const fn next_channel_sequence(&self) -> SequenceNumber {
        self.next_channel_sequence
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }

    pub(crate) fn queue(&mut self, message: &[u8], delivery: Delivery) -> Result<(), CoreError> {
        let limit = MAX_MESSAGE_LEN.min(self.fragment_len * Self::MAX_FRAGMENTS);
        if message.len() > limit {
            return Err(CoreError::MessageTooLarge {
                len: message.len(),
                limit,
            });
        }
        if self.queued_bytes.saturating_add(message.len()) > Self::QUEUE_LIMIT_BYTES {
            return Err(CoreError::SendQueueFull {
                limit: Self::QUEUE_LIMIT_BYTES,
            });
        }
        let count =
            u16::try_from(message.len().div_ceil(self.fragment_len).max(1)).unwrap_or(u16::MAX);
        let message_id = self.next_message;
        self.next_message = self.next_message.next();

        for (index, chunk) in chunks_including_empty(message, self.fragment_len).enumerate() {
            let mut payload = self.pool.acquire();
            payload.extend_from_slice(chunk)?;
            let index = u16::try_from(index).unwrap_or(u16::MAX);
            self.pending.push_back(PendingFragment {
                channel_sequence: self.next_channel_sequence,
                message: message_id,
                index,
                count,
                byte_offset: u32::try_from(usize::from(index) * self.fragment_len)
                    .unwrap_or(u32::MAX),
                delivery,
                payload,
                sent_at: None,
                attempts: 0,
            });
            self.next_channel_sequence = self.next_channel_sequence.next();
        }
        self.queued_bytes = self.queued_bytes.saturating_add(message.len());
        Ok(())
    }

    /// Drops every sent fragment the peer's cumulative acknowledgement covers.
    ///
    /// An acknowledgement naming a sequence never sent is ignored rather than trusted, so a
    /// confused peer cannot make this side forget data it still owes.
    pub(crate) fn acknowledge(&mut self, cumulative: SequenceNumber) {
        let Some(last_sent) = self
            .sent_count
            .checked_sub(1)
            .and_then(|index| self.pending.get(index))
            .map(|fragment| fragment.channel_sequence)
        else {
            return;
        };
        if cumulative.distance_to(last_sent) < 0 {
            return;
        }
        while self.sent_count > 0 {
            let Some(front) = self.pending.front() else {
                break;
            };
            if front.channel_sequence.distance_to(cumulative) < 0 {
                break;
            }
            if let Some(acked) = self.pending.pop_front() {
                self.queued_bytes = self.queued_bytes.saturating_sub(acked.payload.len());
            }
            self.sent_count -= 1;
        }
    }

    /// The fragment to put on the wire now: the most overdue retransmission, else the next
    /// fragment the window has room for.
    pub(crate) fn next_to_send(
        &mut self,
        now: Instant,
        timeout: Duration,
    ) -> Option<&PendingFragment> {
        let overdue = self
            .pending
            .iter()
            .take(self.sent_count)
            .enumerate()
            .filter_map(|(position, fragment)| {
                fragment
                    .retransmit_at(timeout)
                    .filter(|due| !due.is_after(now))
                    .map(|due| (position, due))
            })
            .min_by_key(|(_, due)| *due)
            .map(|(position, _)| position);

        let position = match overdue {
            Some(position) => position,
            None if self.sent_count < self.pending.len().min(Self::IN_FLIGHT_LIMIT) => {
                self.sent_count += 1;
                self.sent_count - 1
            }
            None => return None,
        };
        let fragment = self.pending.get_mut(position)?;
        fragment.sent_at = Some(now);
        fragment.attempts = fragment.attempts.saturating_add(1);
        Some(fragment)
    }

    /// When the window next has something to send.
    pub(crate) fn next_deadline(&self, now: Instant, timeout: Duration) -> Option<Instant> {
        if self.sent_count < self.pending.len().min(Self::IN_FLIGHT_LIMIT) {
            return Some(now);
        }
        self.pending
            .iter()
            .take(self.sent_count)
            .filter_map(|fragment| fragment.retransmit_at(timeout))
            .min()
    }
}
