use alloc::vec::Vec;
use core::time::Duration;

use llts_wire::packet::{
    EcnMark, FeedbackEntry, FeedbackReport, PacketFlags, PacketHeader, PacketKind,
};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, StreamId,
};

use crate::CoreError;
use crate::buffer::{BufferPool, Datagram};
use crate::clock::Instant;
use crate::receive::GapTracker;
use crate::send::DatagramSizing;

/// Collects the arrival of every paced packet on a path and reports them by transport sequence.
///
/// Deliberately not RFC 8888: its arrival-time offsets quantise to 1/1024 s, about 0.98 ms, which
/// is half a LAN round trip. Reporting raw microseconds costs three bytes more per entry and
/// keeps the delay signal the estimator actually needs.
///
/// The loss figure comes from gaps in the transport sequence, so it counts every paced packet
/// the path dropped — including retransmissions and control — which is the loss the repair
/// policy has to price.
#[derive(Debug)]
pub struct TransportFeedback {
    entries: Vec<FeedbackEntry>,
    capacity: usize,
    oldest_at: Option<Instant>,
    gaps: GapTracker,
    sequence: SequenceNumber,
}

impl TransportFeedback {
    /// A report goes out this soon after its first entry even if no frame has completed, so
    /// audio-only and control-only sessions still feed the controller.
    pub const MAX_DELAY: Duration = Duration::from_millis(5);

    pub fn new(sizing: DatagramSizing) -> Self {
        let capacity =
            (sizing.max_payload_len() - FeedbackReport::PREFIX_LEN) / FeedbackEntry::ENCODED_LEN;
        Self {
            entries: Vec::with_capacity(capacity),
            capacity: capacity.max(1),
            oldest_at: None,
            gaps: GapTracker::new(),
            sequence: SequenceNumber::ZERO,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records an arrival, reporting whether the report is now full enough to send.
    pub fn record(
        &mut self,
        transport_sequence: SequenceNumber,
        arrival: MonotonicMicros,
        now: Instant,
    ) -> bool {
        self.gaps.record(transport_sequence, false);
        if self.entries.len() < self.capacity {
            self.entries.push(FeedbackEntry {
                transport_sequence,
                arrival,
                ecn: EcnMark::NotCapable,
            });
        }
        self.oldest_at.get_or_insert(now);
        self.entries.len() >= self.capacity
    }

    pub fn is_due(&self, now: Instant) -> bool {
        self.next_deadline().is_some_and(|due| !due.is_after(now))
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.oldest_at
            .map(|oldest| oldest.saturating_add(Self::MAX_DELAY))
    }

    /// Packages and clears the accumulated entries, sealed later in `stream`'s feedback space.
    pub fn build(
        &mut self,
        stream: StreamId,
        now: Instant,
        pool: &BufferPool,
    ) -> Result<Option<Datagram>, CoreError> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        self.oldest_at = None;
        let header = PacketHeader {
            version: ProtocolVersion::CURRENT,
            kind: PacketKind::Feedback,
            flags: PacketFlags::empty(),
            stream,
            sequence: self.sequence,
            frame: FrameId::ZERO,
            timestamp: MediaTimestamp::ZERO,
            packet_index: 0,
            packet_count: 1,
            departure: now.wire_micros(),
            payload_offset: 0,
        };
        self.sequence = self.sequence.next();

        let mut buffer = pool.acquire();
        let header_len = header.write_into(buffer.spare_capacity_mut())?;
        buffer.set_filled(header_len)?;

        let mut prefix = [0u8; FeedbackReport::PREFIX_LEN];
        FeedbackReport::write_prefix(&mut prefix, self.gaps.loss_rate().proportion().get())?;
        buffer.extend_from_slice(&prefix)?;

        let mut scratch = [0u8; FeedbackEntry::ENCODED_LEN];
        for entry in self.entries.drain(..) {
            FeedbackReport::write_entry(&mut scratch, entry)?;
            buffer.extend_from_slice(&scratch)?;
        }
        Ok(Some(Datagram::from_buffer(header, buffer)))
    }
}
