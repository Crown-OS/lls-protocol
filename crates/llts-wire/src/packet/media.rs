use crate::packet::{BodyPrefix, PacketHeader, PacketKind};
use crate::{SequenceNumber, WireError};

/// One fragment of an encoded frame, borrowed straight out of the receive buffer.
#[derive(Debug, Clone, Copy)]
pub struct MediaPacketView<'a> {
    header: PacketHeader,
    prefix: BodyPrefix,
    payload: &'a [u8],
}

impl<'a> MediaPacketView<'a> {
    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Media)?;
        Self::from_parts(header, body)
    }

    pub(crate) fn from_parts(header: PacketHeader, body: &'a [u8]) -> Result<Self, WireError> {
        let (prefix, payload) = BodyPrefix::split_from(&header, body)?;
        Ok(Self {
            header,
            prefix,
            payload,
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub const fn prefix(&self) -> &BodyPrefix {
        &self.prefix
    }

    /// Where this fragment sits in its stream's numbering, which for a retransmission is the
    /// sequence it replaces rather than the one in its header.
    pub fn media_sequence(&self) -> SequenceNumber {
        self.prefix.media_sequence(&self.header)
    }

    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }
}
