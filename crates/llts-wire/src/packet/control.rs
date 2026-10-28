use crate::packet::{BodyPrefix, PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{SequenceNumber, WireError};

/// Framing for the reliable channels that share the media socket.
///
/// Sunshine spends an entire ENet dependency on this; a cumulative acknowledgement plus a
/// retransmit timer covers everything the control plane needs. A message longer than one
/// datagram is split across consecutive channel sequences: the header's `frame` names the
/// message, `packet_index`/`packet_count` place the fragment, `payload_offset` is its byte
/// offset, and `FRAME_END` marks the last one. An envelope flagged `DISCARDABLE` is a bare
/// acknowledgement that consumes no channel sequence.
#[derive(Debug, Clone, Copy)]
pub struct ControlEnvelope<'a> {
    header: PacketHeader,
    prefix: BodyPrefix,
    channel_sequence: SequenceNumber,
    cumulative_ack: SequenceNumber,
    body: &'a [u8],
}

impl<'a> ControlEnvelope<'a> {
    pub const PREFIX_LEN: usize = 4;

    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Control)?;
        let (prefix, body) = BodyPrefix::split_from(&header, body)?;
        let mut reader = ByteReader::new(body);
        let channel_sequence = SequenceNumber::new(reader.u16_le()?);
        let cumulative_ack = SequenceNumber::new(reader.u16_le()?);
        Ok(Self {
            header,
            prefix,
            channel_sequence,
            cumulative_ack,
            body: reader.into_remaining(),
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub const fn prefix(&self) -> &BodyPrefix {
        &self.prefix
    }

    pub const fn channel_sequence(&self) -> SequenceNumber {
        self.channel_sequence
    }

    /// Highest control sequence the peer has received contiguously.
    pub const fn cumulative_ack(&self) -> SequenceNumber {
        self.cumulative_ack
    }

    pub const fn body(&self) -> &'a [u8] {
        self.body
    }

    pub fn write_prefix(
        destination: &mut [u8],
        channel_sequence: SequenceNumber,
        cumulative_ack: SequenceNumber,
    ) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(channel_sequence.get())?;
        writer.u16_le(cumulative_ack.get())?;
        Ok(writer.written_len())
    }
}
