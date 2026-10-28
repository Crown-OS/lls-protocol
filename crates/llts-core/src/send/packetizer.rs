use alloc::vec::Vec;

use llts_wire::packet::{PacketFlags, PacketHeader, PacketKind};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, StreamId,
};

use super::{DatagramSizing, EncodedSlice};
use crate::CoreError;
use crate::buffer::{BufferPool, Datagram};
use crate::limits::MAX_FRAGMENTS_PER_FRAME;
use crate::util::chunking::chunks_including_empty;

/// What one call to [`FramePacketizer::packetize`] put on the wire.
///
/// The repair encoder and the retransmit cache both need this: the first tells them which
/// sequence numbers now belong to the open frame, the second what to hold for a NACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmittedRange {
    pub frame: FrameId,
    pub first_sequence: SequenceNumber,
    /// Index within the frame of the first fragment this call produced.
    pub first_fragment_index: u16,
    pub fragment_count: u16,
    /// Byte offset within the frame at which this slice starts.
    pub byte_offset: u32,
    pub byte_len: u32,
    pub frame_closed: bool,
}

/// Splits encoded slices into datagrams, assigning sequence and frame numbering.
///
/// Frame numbering advances when a slice reports [`EncodedSlice::ends_frame`], so a frame the
/// encoder emits in four slices still occupies one frame id and therefore one repair block.
#[derive(Debug)]
pub struct FramePacketizer {
    stream: StreamId,
    sizing: DatagramSizing,
    pool: BufferPool,
    next_sequence: SequenceNumber,
    current_frame: FrameId,
    fragments_in_frame: u16,
    frame_byte_len: u32,
    open_frame_timestamp: MediaTimestamp,
}

impl FramePacketizer {
    pub const fn new(stream: StreamId, sizing: DatagramSizing, pool: BufferPool) -> Self {
        Self {
            stream,
            sizing,
            pool,
            next_sequence: SequenceNumber::ZERO,
            current_frame: FrameId::ZERO,
            fragments_in_frame: 0,
            frame_byte_len: 0,
            open_frame_timestamp: MediaTimestamp::ZERO,
        }
    }

    pub const fn sizing(&self) -> DatagramSizing {
        self.sizing
    }

    /// Adopts a newly probed path MTU. Takes effect from the next slice, leaving fragments
    /// already emitted for the open frame untouched.
    pub const fn resize(&mut self, sizing: DatagramSizing) {
        self.sizing = sizing;
    }

    pub const fn stream(&self) -> StreamId {
        self.stream
    }

    /// Timestamp of the frame currently being emitted, so a parity packet carries the same
    /// capture instant as the shards it protects.
    pub const fn open_frame_timestamp(&self) -> MediaTimestamp {
        self.open_frame_timestamp
    }

    pub const fn current_frame(&self) -> FrameId {
        self.current_frame
    }

    /// Reserves the next sequence number, for a repair shard that shares the media stream's
    /// numbering so the receiver's gap tracker sees one contiguous run.
    pub const fn take_sequence(&mut self) -> SequenceNumber {
        let taken = self.next_sequence;
        self.next_sequence = self.next_sequence.next();
        taken
    }

    /// Bytes emitted for the open frame so far, which the repair block records so a decoder can
    /// trim shard padding.
    pub const fn open_frame_byte_len(&self) -> u32 {
        self.frame_byte_len
    }

    /// Appends this slice's datagrams to `sink` and reports what was produced.
    pub fn packetize(
        &mut self,
        slice: EncodedSlice<'_>,
        departure: MonotonicMicros,
        sink: &mut Vec<Datagram>,
    ) -> Result<EmittedRange, CoreError> {
        let payload_len = self.sizing.max_payload_len();
        let fragment_count = self.sizing.fragment_count(slice.byte_len());
        let total_fragments = usize::from(self.fragments_in_frame) + fragment_count;

        if total_fragments > MAX_FRAGMENTS_PER_FRAME {
            return Err(CoreError::FrameTooFragmented {
                frame_id: self.current_frame.get(),
                required: total_fragments,
                limit: MAX_FRAGMENTS_PER_FRAME,
            });
        }

        self.open_frame_timestamp = slice.timestamp();
        let first_sequence = self.next_sequence;
        let first_fragment_index = self.fragments_in_frame;
        let byte_offset = self.frame_byte_len;
        let mut emitted: u16 = 0;

        for (position, chunk) in chunks_including_empty(slice.bytes(), payload_len).enumerate() {
            let is_last_of_slice = position + 1 == fragment_count;
            let closes_frame = is_last_of_slice && slice.ends_frame();

            let mut flags = PacketFlags::empty();
            if slice.is_keyframe() {
                flags |= PacketFlags::KEYFRAME;
            }
            if is_last_of_slice {
                flags |= PacketFlags::SLICE_END;
            }
            if closes_frame {
                flags |= PacketFlags::FRAME_END;
            }

            let packet_index = self.fragments_in_frame.saturating_add(emitted);
            let payload_offset = self.frame_byte_len.saturating_add(
                u32::try_from(position.saturating_mul(payload_len)).unwrap_or(u32::MAX),
            );
            let header = PacketHeader {
                version: ProtocolVersion::CURRENT,
                kind: PacketKind::Media,
                flags,
                stream: self.stream,
                sequence: self.next_sequence,
                frame: self.current_frame,
                timestamp: slice.timestamp(),
                packet_index,
                // Only meaningful once FRAME_END is set; until then it is the lower bound the
                // receiver treats it as.
                packet_count: packet_index.saturating_add(1),
                departure,
                payload_offset,
            };

            sink.push(Datagram::assemble(&header, &[chunk], &self.pool)?);
            self.next_sequence = self.next_sequence.next();
            emitted = emitted.saturating_add(1);
        }

        self.fragments_in_frame = self.fragments_in_frame.saturating_add(emitted);
        self.frame_byte_len = self
            .frame_byte_len
            .saturating_add(u32::try_from(slice.byte_len()).unwrap_or(u32::MAX));

        let range = EmittedRange {
            frame: self.current_frame,
            first_sequence,
            first_fragment_index,
            fragment_count: emitted,
            byte_offset,
            byte_len: u32::try_from(slice.byte_len()).unwrap_or(u32::MAX),
            frame_closed: slice.ends_frame(),
        };

        if slice.ends_frame() {
            self.stamp_frame_total(sink)?;
            self.current_frame = self.current_frame.next();
            self.fragments_in_frame = 0;
            self.frame_byte_len = 0;
        }

        Ok(range)
    }

    /// Rewrites `packet_count` on the closing fragment so it carries the frame's true total.
    fn stamp_frame_total(&self, sink: &mut [Datagram]) -> Result<(), CoreError> {
        let Some(last) = sink.last_mut() else {
            return Ok(());
        };
        let mut header = *last.header();
        header.packet_count = self.fragments_in_frame;
        last.rewrite_header(&header)
    }
}
