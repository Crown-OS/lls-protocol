use alloc::collections::VecDeque;
use alloc::vec::Vec;

use llts_wire::packet::{ControlEnvelope, PacketFlags};
use llts_wire::{FrameId, SequenceNumber};

use super::received_set::ReceivedSet;
use super::send_window::SendWindow;
use super::{Delivery, MAX_MESSAGE_LEN};
use crate::CoreError;
use crate::buffer::{BufferPool, PooledBuffer};

/// What became of one arriving fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FragmentOutcome {
    /// New, stored, and now acknowledged.
    Accepted,
    /// Already held or already delivered; the peer missed an acknowledgement.
    Duplicate,
    /// Outside the window or with no room to hold it. Not acknowledged, so it will be resent.
    Refused,
}

#[derive(Debug)]
struct PartialMessage {
    message: FrameId,
    first_sequence: SequenceNumber,
    count: u16,
    delivery: Delivery,
    arrived: u64,
    arrived_count: u16,
    buffer: PooledBuffer,
}

#[derive(Debug)]
struct WaitingMessage {
    first_sequence: SequenceNumber,
    last_sequence: SequenceNumber,
    buffer: PooledBuffer,
}

/// The receiving half of a reliable channel: fragments back into messages, each delivered
/// exactly once.
///
/// Every fragment's channel sequence goes into a [`ReceivedSet`], which is what makes
/// duplicates harmless and what the cumulative acknowledgement reads. An unordered message is
/// released the moment its last fragment lands; an ordered one waits until the cumulative point
/// has passed it, which is exactly when everything sent before it has arrived.
#[derive(Debug)]
pub(crate) struct Reassembly {
    received: ReceivedSet,
    partials: Vec<PartialMessage>,
    waiting: Vec<WaitingMessage>,
    delivered: VecDeque<PooledBuffer>,
    small_pool: BufferPool,
    message_pool: BufferPool,
}

impl Reassembly {
    /// Multi-fragment messages being rebuilt at once.
    const MAX_PARTIALS: usize = 8;

    pub(crate) fn new(small_pool: BufferPool, message_pool: BufferPool) -> Self {
        Self {
            received: ReceivedSet::new(),
            partials: Vec::with_capacity(Self::MAX_PARTIALS),
            waiting: Vec::with_capacity(Self::MAX_PARTIALS),
            delivered: VecDeque::with_capacity(8),
            small_pool,
            message_pool,
        }
    }

    pub(crate) const fn cumulative(&self) -> SequenceNumber {
        self.received.cumulative()
    }

    /// Whether a gap sits ahead of the cumulative point, which is worth acknowledging at once.
    pub(crate) fn has_gap(&self) -> bool {
        self.received.has_gap()
    }

    pub(crate) fn poll_delivery(&mut self) -> Option<PooledBuffer> {
        self.delivered.pop_front()
    }

    pub(crate) fn accept(
        &mut self,
        envelope: &ControlEnvelope<'_>,
    ) -> Result<FragmentOutcome, CoreError> {
        let sequence = envelope.channel_sequence();
        if self.received.contains(sequence) {
            return Ok(FragmentOutcome::Duplicate);
        }
        if !self.received.is_in_window(sequence) {
            return Ok(FragmentOutcome::Refused);
        }
        let header = envelope.header();
        let delivery = if header.flags.contains(PacketFlags::UNORDERED) {
            Delivery::Unordered
        } else {
            Delivery::Ordered
        };
        let offset = usize::try_from(header.payload_offset).unwrap_or(usize::MAX);
        if usize::from(header.packet_count) > SendWindow::MAX_FRAGMENTS
            || offset.saturating_add(envelope.body().len()) > MAX_MESSAGE_LEN
        {
            return Err(CoreError::Wire(llts_wire::WireError::MalformedField {
                field: "payload_offset",
            }));
        }

        let first_sequence = SequenceNumber::new(sequence.get().wrapping_sub(header.packet_index));
        let Some(position) =
            self.partial_for(header.frame, first_sequence, header.packet_count, delivery)
        else {
            return Ok(FragmentOutcome::Refused);
        };
        let Some(partial) = self.partials.get_mut(position) else {
            return Ok(FragmentOutcome::Refused);
        };
        partial.buffer.write_at(offset, envelope.body())?;
        let bit = 1u64 << header.packet_index.min(63);
        if partial.arrived & bit == 0 {
            partial.arrived |= bit;
            partial.arrived_count += 1;
        }
        self.received.insert(sequence);

        if partial.arrived_count >= partial.count {
            let complete = self.partials.swap_remove(position);
            self.complete(complete);
        }
        self.release_ordered();
        Ok(FragmentOutcome::Accepted)
    }

    fn partial_for(
        &mut self,
        message: FrameId,
        first_sequence: SequenceNumber,
        count: u16,
        delivery: Delivery,
    ) -> Option<usize> {
        if let Some(position) = self.partials.iter().position(|partial| {
            partial.message == message && partial.first_sequence == first_sequence
        }) {
            return Some(position);
        }
        if self.partials.len() >= Self::MAX_PARTIALS {
            return None;
        }
        let buffer = if count <= 1 {
            self.small_pool.acquire()
        } else {
            self.message_pool.acquire()
        };
        self.partials.push(PartialMessage {
            message,
            first_sequence,
            count: count.max(1),
            delivery,
            arrived: 0,
            arrived_count: 0,
            buffer,
        });
        Some(self.partials.len() - 1)
    }

    fn complete(&mut self, message: PartialMessage) {
        match message.delivery {
            Delivery::Unordered => self.delivered.push_back(message.buffer),
            Delivery::Ordered => self.waiting.push(WaitingMessage {
                first_sequence: message.first_sequence,
                last_sequence: message
                    .first_sequence
                    .advanced_by(message.count.saturating_sub(1)),
                buffer: message.buffer,
            }),
        }
    }

    /// Releases, oldest first, every ordered message the cumulative point has passed.
    fn release_ordered(&mut self) {
        let cumulative = self.received.cumulative();
        loop {
            let ready = self
                .waiting
                .iter()
                .enumerate()
                .filter(|(_, waiting)| waiting.last_sequence.distance_to(cumulative) >= 0)
                .min_by_key(|(_, waiting)| cumulative.distance_to(waiting.first_sequence))
                .map(|(position, _)| position);
            let Some(position) = ready else {
                return;
            };
            let message = self.waiting.swap_remove(position);
            self.delivered.push_back(message.buffer);
        }
    }
}
