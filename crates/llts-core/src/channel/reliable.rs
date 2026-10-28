use core::time::Duration;

use llts_wire::packet::{ControlEnvelope, PacketFlags, PacketHeader, PacketKind};
use llts_wire::{FrameId, MediaTimestamp, ProtocolVersion, SequenceNumber, SessionTag, StreamId};

use super::Delivery;
use super::reassembly::{FragmentOutcome, Reassembly};
use super::send_window::{PendingFragment, SendWindow};
use crate::CoreError;
use crate::buffer::{BufferPool, Datagram, PooledBuffer};
use crate::clock::Instant;
use crate::send::DatagramSizing;
use crate::stream::StreamKey;

/// A reliable message stream inside the session: the control plane, or bulk application data.
///
/// Every datagram — first send, retransmission or bare acknowledgement — takes a fresh header
/// sequence from the control space, so the packet cipher never sees one twice. Reliability
/// lives one layer up, in the channel sequence the envelope carries.
#[derive(Debug)]
pub struct ReliableChannel {
    key: StreamKey,
    outbound: StreamId,
    window: SendWindow,
    reassembly: Reassembly,
    pool: BufferPool,
    packet_sequence: SequenceNumber,
    ack_due_at: Option<Instant>,
}

impl ReliableChannel {
    /// A lone fragment waits this long for outgoing data to carry its acknowledgement.
    const DELAYED_ACK: Duration = Duration::from_millis(2);

    const MIN_RETRANSMIT_TIMEOUT: Duration = Duration::from_millis(10);
    const MAX_RETRANSMIT_TIMEOUT: Duration = Duration::from_secs(1);

    /// `datagrams` supplies one-datagram buffers for fragments and small messages; `messages`
    /// supplies buffers large enough to rebuild a whole [`super::MAX_MESSAGE_LEN`] message.
    pub fn new(
        key: StreamKey,
        peer_tag: SessionTag,
        sizing: DatagramSizing,
        datagrams: BufferPool,
        messages: BufferPool,
    ) -> Self {
        let fragment_len = sizing.max_payload_len() - ControlEnvelope::PREFIX_LEN;
        Self {
            key,
            outbound: key.with_tag(peer_tag),
            window: SendWindow::new(datagrams.clone(), fragment_len),
            reassembly: Reassembly::new(datagrams.clone(), messages),
            pool: datagrams,
            packet_sequence: SequenceNumber::ZERO,
            ack_due_at: None,
        }
    }

    pub const fn key(&self) -> StreamKey {
        self.key
    }

    /// Whether every queued message has been acknowledged.
    pub fn is_idle(&self) -> bool {
        self.window.is_idle()
    }

    pub fn queue(&mut self, message: &[u8], delivery: Delivery) -> Result<(), CoreError> {
        self.window.queue(message, delivery)
    }

    /// Folds in one arriving envelope: its acknowledgement, and its fragment unless it is a
    /// bare acknowledgement.
    pub fn on_envelope(
        &mut self,
        envelope: &ControlEnvelope<'_>,
        now: Instant,
    ) -> Result<(), CoreError> {
        self.window.acknowledge(envelope.cumulative_ack());
        if envelope.header().is_discardable() {
            return Ok(());
        }
        let outcome = self.reassembly.accept(envelope)?;
        let immediate = outcome == FragmentOutcome::Duplicate
            || (outcome == FragmentOutcome::Accepted && self.reassembly.has_gap());
        if immediate {
            self.ack_due_at = Some(now);
        } else if outcome == FragmentOutcome::Accepted && self.ack_due_at.is_none() {
            self.ack_due_at = Some(now.saturating_add(Self::DELAYED_ACK));
        }
        Ok(())
    }

    pub fn poll_delivery(&mut self) -> Option<PooledBuffer> {
        self.reassembly.poll_delivery()
    }

    /// The next datagram this channel wants on the wire, if any is due.
    pub fn poll_transmit(
        &mut self,
        now: Instant,
        round_trip: Duration,
    ) -> Result<Option<Datagram>, CoreError> {
        let timeout = Self::retransmit_timeout(round_trip);
        let cumulative = self.reassembly.cumulative();
        let sequence = self.packet_sequence;
        let datagram = match self.window.next_to_send(now, timeout) {
            Some(fragment) => Some(build_fragment(
                self.outbound,
                sequence,
                cumulative,
                fragment,
                now,
                &self.pool,
            )?),
            None if self.ack_due_at.is_some_and(|due| !due.is_after(now)) => Some(build_ack(
                self.outbound,
                sequence,
                self.window.next_channel_sequence(),
                cumulative,
                now,
                &self.pool,
            )?),
            None => None,
        };
        if datagram.is_some() {
            self.packet_sequence = sequence.next();
            self.ack_due_at = None;
        }
        Ok(datagram)
    }

    pub fn next_timeout(&self, now: Instant, round_trip: Duration) -> Option<Instant> {
        let window = self
            .window
            .next_deadline(now, Self::retransmit_timeout(round_trip));
        match (window, self.ack_due_at) {
            (Some(window), Some(ack)) => Some(window.min(ack)),
            (window, ack) => window.or(ack),
        }
    }

    /// Three round trips, clamped: generous enough that queueing jitter on Wi-Fi does not fire
    /// a spurious resend, short enough that one lost fragment costs milliseconds, not seconds.
    fn retransmit_timeout(round_trip: Duration) -> Duration {
        round_trip
            .saturating_mul(3)
            .saturating_add(Duration::from_millis(2))
            .clamp(Self::MIN_RETRANSMIT_TIMEOUT, Self::MAX_RETRANSMIT_TIMEOUT)
    }
}

const fn envelope_header(
    stream: StreamId,
    sequence: SequenceNumber,
    flags: PacketFlags,
    now: Instant,
) -> PacketHeader {
    PacketHeader {
        version: ProtocolVersion::CURRENT,
        kind: PacketKind::Control,
        flags,
        stream,
        sequence,
        frame: FrameId::ZERO,
        timestamp: MediaTimestamp::ZERO,
        packet_index: 0,
        packet_count: 1,
        departure: now.wire_micros(),
        payload_offset: 0,
    }
}

fn build_fragment(
    stream: StreamId,
    sequence: SequenceNumber,
    cumulative: SequenceNumber,
    fragment: &PendingFragment,
    now: Instant,
    pool: &BufferPool,
) -> Result<Datagram, CoreError> {
    let mut flags = PacketFlags::empty();
    if fragment.is_last() {
        flags |= PacketFlags::FRAME_END;
    }
    if fragment.delivery == Delivery::Unordered {
        flags |= PacketFlags::UNORDERED;
    }
    let mut header = envelope_header(stream, sequence, flags, now);
    header.frame = fragment.message;
    header.packet_index = fragment.index;
    header.packet_count = fragment.count;
    header.payload_offset = fragment.byte_offset;

    let mut prefix = [0u8; ControlEnvelope::PREFIX_LEN];
    ControlEnvelope::write_prefix(&mut prefix, fragment.channel_sequence, cumulative)?;
    Datagram::assemble(&header, &[&prefix, fragment.payload.filled()], pool)
}

fn build_ack(
    stream: StreamId,
    sequence: SequenceNumber,
    next_channel_sequence: SequenceNumber,
    cumulative: SequenceNumber,
    now: Instant,
    pool: &BufferPool,
) -> Result<Datagram, CoreError> {
    let header = envelope_header(stream, sequence, PacketFlags::DISCARDABLE, now);
    let mut prefix = [0u8; ControlEnvelope::PREFIX_LEN];
    ControlEnvelope::write_prefix(&mut prefix, next_channel_sequence, cumulative)?;
    Datagram::assemble(&header, &[&prefix], pool)
}
