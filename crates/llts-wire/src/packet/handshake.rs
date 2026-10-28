use crate::packet::{PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{SessionTag, WireError};

/// Which Noise pattern a handshake message belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HandshakePattern {
    /// First contact nearby, confirmed by comparing a six-digit code.
    Xx = 0,
    /// First contact through a scanned QR code, authenticated by the one-time key it carries.
    XxPsk3 = 1,
    /// Reconnecting to a device already paired, in one round trip.
    Ik = 2,
}

impl HandshakePattern {
    const fn from_byte(value: u8) -> Result<Self, WireError> {
        match value {
            0 => Ok(Self::Xx),
            1 => Ok(Self::XxPsk3),
            2 => Ok(Self::Ik),
            _ => Err(WireError::MalformedField { field: "pattern" }),
        }
    }

    pub const fn message_count(self) -> u8 {
        match self {
            Self::Xx | Self::XxPsk3 => 3,
            Self::Ik => 2,
        }
    }
}

/// One Noise handshake message.
///
/// The header's stream tag names the receiver's session once it has one; the first message
/// carries [`SessionTag::UNASSIGNED`] because the responder has not chosen yet. `sender_tag` is
/// the tag the sender wants the peer to use from now on, sent in the clear only so the reply can
/// be routed — the authenticated copy travels inside the Hello payload.
///
/// ```text
/// 0  pattern                1
/// 1  message_index          1
/// 2  sender_tag             2
/// 4  noise message          ..
/// ```
#[derive(Debug, Clone, Copy)]
pub struct HandshakeView<'a> {
    header: PacketHeader,
    pattern: HandshakePattern,
    message_index: u8,
    sender_tag: SessionTag,
    message: &'a [u8],
}

impl<'a> HandshakeView<'a> {
    pub const PREFIX_LEN: usize = 4;

    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Handshake)?;
        let mut reader = ByteReader::new(body);
        let pattern = HandshakePattern::from_byte(reader.u8()?)?;
        let message_index = reader.u8()?;
        if message_index >= pattern.message_count() {
            return Err(WireError::MalformedField {
                field: "message_index",
            });
        }
        let sender_tag = SessionTag::new(reader.u16_le()?);
        Ok(Self {
            header,
            pattern,
            message_index,
            sender_tag,
            message: reader.into_remaining(),
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub const fn pattern(&self) -> HandshakePattern {
        self.pattern
    }

    /// Zero for the initiator's first message.
    pub const fn message_index(&self) -> u8 {
        self.message_index
    }

    pub const fn sender_tag(&self) -> SessionTag {
        self.sender_tag
    }

    pub const fn message(&self) -> &'a [u8] {
        self.message
    }

    pub fn write_prefix(
        destination: &mut [u8],
        pattern: HandshakePattern,
        message_index: u8,
        sender_tag: SessionTag,
    ) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u8(pattern as u8)?;
        writer.u8(message_index)?;
        writer.u16_le(sender_tag.get())?;
        Ok(writer.written_len())
    }
}
