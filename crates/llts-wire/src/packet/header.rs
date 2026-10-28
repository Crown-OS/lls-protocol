use crate::packet::{PacketFlags, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, StreamId, WireError,
};

/// Two-byte prefix, present in the clear so a dissector can recognise the protocol and so a
/// stray datagram on a shared port is rejected before any crypto work.
const MAGIC: [u8; 2] = *b"LL";

const VERSION_SHIFT: u32 = 4;
const KIND_MASK: u8 = 0x0F;

/// The fixed 28-byte prefix every LLTS datagram carries.
///
/// Authenticated as additional data rather than encrypted, because the transport needs it before
/// it can decrypt anything: [`PacketHeader::stream`] and the [`PacketFlags::KEY_PHASE`] bit pick
/// the key, and the kind, flags and [`PacketHeader::sequence`] rebuild the nonce.
///
/// The `stream_id` field is `session_tag:u16 ‖ class:u8 ‖ index:u8`, see [`StreamId`].
/// ```text
/// 0  magic b"LL"            2
/// 2  version:4 | kind:4     1
/// 3  flags                  1
/// 4  stream_id              4
/// 8  sequence               2
/// 10 frame_id               2
/// 12 media_timestamp        4
/// 16 packet_index           2
/// 18 packet_count           2
/// 20 departure_micros       4
/// 24 payload_offset         4
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub version: ProtocolVersion,
    pub kind: PacketKind,
    pub flags: PacketFlags,
    pub stream: StreamId,
    pub sequence: SequenceNumber,
    pub frame: FrameId,
    pub timestamp: MediaTimestamp,
    /// Position of this fragment within its frame, always below [`PacketHeader::packet_count`].
    pub packet_index: u16,
    /// Fragments in this frame.
    ///
    /// Authoritative only on the packet carrying [`PacketFlags::FRAME_END`]. Slice-level
    /// pipelining puts the first fragment on the wire while the encoder is still working, so
    /// until the frame closes this is a lower bound — `packet_index + 1` — and the receiver
    /// treats it as such.
    pub packet_count: u16,
    pub departure: MonotonicMicros,
    /// Byte position of this payload within its frame.
    ///
    /// Carried explicitly rather than derived from `packet_index`, because slice-level
    /// pipelining fragments each slice separately: a slice whose length is not a whole number of
    /// payloads ends in a short fragment, so index times payload size stops matching reality
    /// from the next slice onward.
    pub payload_offset: u32,
}

impl PacketHeader {
    pub const ENCODED_LEN: usize = 28;

    /// Where the flags byte sits, so the packet cipher can set the key phase just before sealing.
    pub const FLAGS_OFFSET: usize = 3;

    /// Parses a header and returns it alongside the bytes that follow it.
    pub fn read_from(datagram: &[u8]) -> Result<(Self, &[u8]), WireError> {
        let mut reader = ByteReader::new(datagram);

        if reader.take_array::<2>()? != MAGIC {
            return Err(WireError::BadMagic);
        }

        let version_and_kind = reader.u8()?;
        let version = ProtocolVersion::new(version_and_kind >> VERSION_SHIFT)
            .ok_or(WireError::MalformedField { field: "version" })?
            .check_supported()?;
        let kind = PacketKind::from_nibble(version_and_kind & KIND_MASK)?;
        let flags = PacketFlags::from_bits_truncate(reader.u8()?);

        let stream = StreamId::new(reader.u32_le()?);
        let sequence = SequenceNumber::new(reader.u16_le()?);
        let frame = FrameId::new(reader.u16_le()?);
        let timestamp = MediaTimestamp::new(reader.u32_le()?);
        let packet_index = reader.u16_le()?;
        let packet_count = reader.u16_le()?;
        let departure = MonotonicMicros::new(reader.u32_le()?);
        let payload_offset = reader.u32_le()?;

        if packet_count == 0 {
            return Err(WireError::MalformedField {
                field: "packet_count",
            });
        }
        if packet_index >= packet_count {
            return Err(WireError::MalformedField {
                field: "packet_index",
            });
        }

        let header = Self {
            version,
            kind,
            flags,
            stream,
            sequence,
            frame,
            timestamp,
            packet_index,
            packet_count,
            departure,
            payload_offset,
        };
        Ok((header, reader.into_remaining()))
    }

    /// Writes the header at the start of `destination`, returning how many bytes it used.
    pub fn write_into(&self, destination: &mut [u8]) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.put(&MAGIC)?;
        writer.u8((self.version.get() << VERSION_SHIFT) | self.kind.as_nibble())?;
        writer.u8(self.flags.bits())?;
        writer.u32_le(self.stream.get())?;
        writer.u16_le(self.sequence.get())?;
        writer.u16_le(self.frame.get())?;
        writer.u32_le(self.timestamp.ticks())?;
        writer.u16_le(self.packet_index)?;
        writer.u16_le(self.packet_count)?;
        writer.u32_le(self.departure.get())?;
        writer.u32_le(self.payload_offset)?;
        Ok(writer.written_len())
    }

    pub const fn is_frame_end(&self) -> bool {
        self.flags.contains(PacketFlags::FRAME_END)
    }

    pub const fn is_keyframe(&self) -> bool {
        self.flags.contains(PacketFlags::KEYFRAME)
    }

    pub const fn is_discardable(&self) -> bool {
        self.flags.contains(PacketFlags::DISCARDABLE)
    }
}
