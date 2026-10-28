use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::time::Duration;

use llts_wire::packet::{
    InputBatch, InputBatches, MediaPacketView, PacketFlags, PacketHeader, PacketKind,
};
use llts_wire::{FrameId, MediaTimestamp, ProtocolVersion, SequenceNumber, SessionTag, StreamId};

use super::InputDelivery;
use crate::CoreError;
use crate::buffer::{BufferPool, Datagram, PooledBuffer};
use crate::clock::Instant;
use crate::receive::{Arrival, SequenceWindow};
use crate::send::DatagramSizing;
use crate::stream::StreamKey;

#[derive(Debug)]
struct RecentBatch {
    sequence: SequenceNumber,
    events: PooledBuffer,
}

/// Pointer and key events: unreliable, redundant, deduplicated.
///
/// Every packet repeats the last [`InputChannel::REDUNDANCY`] batches, so a lost datagram is
/// covered by the next one without a retransmission round trip, and the receiver delivers each
/// batch exactly once by its batch sequence. A key-state snapshot goes out every
/// [`InputChannel::SNAPSHOT_INTERVAL`] while one is set, so a key-up lost in a burst of loss
/// cannot leave a key held down for longer than that.
#[derive(Debug)]
pub struct InputChannel {
    key: StreamKey,
    outbound: StreamId,
    sizing: DatagramSizing,
    pool: BufferPool,
    packet_sequence: SequenceNumber,
    next_batch: SequenceNumber,
    recent: VecDeque<RecentBatch>,
    key_state: Option<PooledBuffer>,
    snapshot_generation: FrameId,
    next_snapshot_at: Option<Instant>,
    outgoing: Vec<Datagram>,
    seen_batches: SequenceWindow,
    latest_snapshot: Option<FrameId>,
    delivered: VecDeque<InputDelivery>,
}

impl InputChannel {
    pub const REDUNDANCY: usize = 4;
    pub const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(500);

    pub fn new(
        key: StreamKey,
        peer_tag: SessionTag,
        sizing: DatagramSizing,
        pool: BufferPool,
    ) -> Self {
        Self {
            key,
            outbound: key.with_tag(peer_tag),
            sizing,
            pool,
            packet_sequence: SequenceNumber::ZERO,
            next_batch: SequenceNumber::ZERO,
            recent: VecDeque::with_capacity(Self::REDUNDANCY),
            key_state: None,
            snapshot_generation: FrameId::ZERO,
            next_snapshot_at: None,
            outgoing: Vec::with_capacity(4),
            seen_batches: SequenceWindow::new(),
            latest_snapshot: None,
            delivered: VecDeque::with_capacity(8),
        }
    }

    pub const fn key(&self) -> StreamKey {
        self.key
    }

    /// Sends one batch of events, repeating the few before it.
    pub fn queue_events(&mut self, events: &[u8], now: Instant) -> Result<(), CoreError> {
        let mut buffer = self.pool.acquire();
        buffer.extend_from_slice(events)?;
        if self.recent.len() == Self::REDUNDANCY {
            self.recent.pop_back();
        }
        self.recent.push_front(RecentBatch {
            sequence: self.next_batch,
            events: buffer,
        });
        self.next_batch = self.next_batch.next();

        let header = self.header(PacketFlags::FRAME_END, self.frame_of_newest(), now);
        let mut datagram = Datagram::assemble(&header, &[], &self.pool)?;
        let recent = &self.recent;
        datagram.append_with(self.sizing.max_payload_len(), |body| {
            InputBatches::write(
                body,
                recent.iter().map(|batch| InputBatch {
                    sequence: batch.sequence,
                    events: batch.events.filled(),
                }),
            )
        })?;
        self.outgoing.push(datagram);
        Ok(())
    }

    /// Replaces the key state and sends it at once, then again every snapshot interval.
    pub fn update_key_state(&mut self, state: &[u8], now: Instant) -> Result<(), CoreError> {
        let mut buffer = self.pool.acquire();
        buffer.extend_from_slice(state)?;
        self.key_state = Some(buffer);
        self.next_snapshot_at = Some(now);
        self.poll(now)
    }

    /// Sends the key-state snapshot if it is due.
    pub fn poll(&mut self, now: Instant) -> Result<(), CoreError> {
        let Some(due) = self.next_snapshot_at else {
            return Ok(());
        };
        if due.is_after(now) {
            return Ok(());
        }
        self.next_snapshot_at = Some(now.saturating_add(Self::SNAPSHOT_INTERVAL));
        self.snapshot_generation = self.snapshot_generation.next();
        let header = self.header(
            PacketFlags::FRAME_END | PacketFlags::KEYFRAME,
            self.snapshot_generation,
            now,
        );
        let state = self
            .key_state
            .as_ref()
            .map(PooledBuffer::filled)
            .unwrap_or_default();
        let datagram = Datagram::assemble(&header, &[state], &self.pool)?;
        self.outgoing.push(datagram);
        Ok(())
    }

    pub const fn next_timeout(&self) -> Option<Instant> {
        self.next_snapshot_at
    }

    pub fn drain_outgoing(&mut self) -> impl Iterator<Item = Datagram> + '_ {
        self.outgoing.drain(..)
    }

    pub fn poll_delivery(&mut self) -> Option<InputDelivery> {
        self.delivered.pop_front()
    }

    pub fn on_media(&mut self, media: &MediaPacketView<'_>) -> Result<(), CoreError> {
        if media.header().is_keyframe() {
            return self.on_snapshot(media);
        }
        let batches = InputBatches::parse(media.payload())?;
        let mut newest_first: [Option<InputBatch<'_>>; Self::REDUNDANCY] = [None; Self::REDUNDANCY];
        for (slot, batch) in newest_first.iter_mut().zip(batches.iter()) {
            *slot = Some(batch);
        }
        for batch in newest_first.into_iter().rev().flatten() {
            if matches!(
                self.seen_batches.insert(batch.sequence),
                Arrival::Advanced | Arrival::Backfilled
            ) {
                let mut buffer = self.pool.acquire();
                buffer.extend_from_slice(batch.events)?;
                self.delivered.push_back(InputDelivery::Events(buffer));
            }
        }
        Ok(())
    }

    fn on_snapshot(&mut self, media: &MediaPacketView<'_>) -> Result<(), CoreError> {
        let generation = media.header().frame;
        if self
            .latest_snapshot
            .is_some_and(|latest| !latest.precedes(generation))
        {
            return Ok(());
        }
        self.latest_snapshot = Some(generation);
        let mut buffer = self.pool.acquire();
        buffer.extend_from_slice(media.payload())?;
        self.delivered.push_back(InputDelivery::KeyState(buffer));
        Ok(())
    }

    const fn frame_of_newest(&self) -> FrameId {
        FrameId::new(self.next_batch.get().wrapping_sub(1))
    }

    const fn header(&mut self, flags: PacketFlags, frame: FrameId, now: Instant) -> PacketHeader {
        let sequence = self.packet_sequence;
        self.packet_sequence = sequence.next();
        PacketHeader {
            version: ProtocolVersion::CURRENT,
            kind: PacketKind::Media,
            flags,
            stream: self.outbound,
            sequence,
            frame,
            timestamp: MediaTimestamp::ZERO,
            packet_index: 0,
            packet_count: 1,
            departure: now.wire_micros(),
            payload_offset: 0,
        }
    }
}
