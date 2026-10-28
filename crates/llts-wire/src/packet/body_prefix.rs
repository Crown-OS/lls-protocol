use crate::packet::{PacketFlags, PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{SequenceNumber, WireError};

/// The first body bytes of every paced packet, encrypted along with the rest of the body.
///
/// `transport_sequence` numbers every paced packet on a path, across all streams, so one
/// congestion controller can be fed from one arrival report no matter which stream a packet
/// belonged to. A retransmission also carries the sequence it replaces: its header sequence is a
/// fresh one from the retransmission space, because reusing the original would reuse a nonce
/// under different additional data.
///
/// ```text
/// 0  transport_sequence     2
/// 2  original_sequence      2   only when RETRANSMISSION is set
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyPrefix {
    pub transport_sequence: SequenceNumber,
    pub original_sequence: Option<SequenceNumber>,
}

impl BodyPrefix {
    pub const TRANSPORT_SEQUENCE_LEN: usize = 2;
    pub const MAX_ENCODED_LEN: usize = 4;

    /// Where `transport_sequence` sits in the datagram, so the pacer can stamp it at departure.
    pub const TRANSPORT_SEQUENCE_OFFSET: usize = PacketHeader::ENCODED_LEN;

    pub const fn fresh() -> Self {
        Self {
            transport_sequence: SequenceNumber::ZERO,
            original_sequence: None,
        }
    }

    pub const fn retransmitting(original: SequenceNumber) -> Self {
        Self {
            transport_sequence: SequenceNumber::ZERO,
            original_sequence: Some(original),
        }
    }

    /// Whether packets of this kind are paced, and so carry a prefix at all.
    pub const fn is_carried_by(kind: PacketKind) -> bool {
        matches!(
            kind,
            PacketKind::Media | PacketKind::Repair | PacketKind::Control
        )
    }

    /// Bytes the prefix occupies in a packet with this header, zero when it has none.
    pub const fn encoded_len(kind: PacketKind, flags: PacketFlags) -> usize {
        if !Self::is_carried_by(kind) {
            0
        } else if flags.contains(PacketFlags::RETRANSMISSION) {
            Self::MAX_ENCODED_LEN
        } else {
            Self::TRANSPORT_SEQUENCE_LEN
        }
    }

    /// Splits the prefix off a body whose header says it carries one.
    pub fn split_from<'a>(
        header: &PacketHeader,
        body: &'a [u8],
    ) -> Result<(Self, &'a [u8]), WireError> {
        let mut reader = ByteReader::new(body);
        let transport_sequence = SequenceNumber::new(reader.u16_le()?);
        let original_sequence = if header.flags.contains(PacketFlags::RETRANSMISSION) {
            Some(SequenceNumber::new(reader.u16_le()?))
        } else {
            None
        };
        Ok((
            Self {
                transport_sequence,
                original_sequence,
            },
            reader.into_remaining(),
        ))
    }

    /// Writes the prefix for a packet with `header`, which decides whether the original sequence
    /// is included.
    pub fn write_into(
        &self,
        header: &PacketHeader,
        destination: &mut [u8],
    ) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(self.transport_sequence.get())?;
        if header.flags.contains(PacketFlags::RETRANSMISSION) {
            let original = self.original_sequence.ok_or(WireError::MalformedField {
                field: "original_sequence",
            })?;
            writer.u16_le(original.get())?;
        }
        Ok(writer.written_len())
    }

    /// Overwrites the transport sequence of an already encoded datagram.
    pub fn stamp_transport_sequence(
        datagram: &mut [u8],
        transport_sequence: SequenceNumber,
    ) -> Result<(), WireError> {
        let available = datagram.len();
        let slot = datagram
            .get_mut(
                Self::TRANSPORT_SEQUENCE_OFFSET
                    ..Self::TRANSPORT_SEQUENCE_OFFSET + Self::TRANSPORT_SEQUENCE_LEN,
            )
            .ok_or(WireError::TooShort {
                required: Self::TRANSPORT_SEQUENCE_OFFSET + Self::TRANSPORT_SEQUENCE_LEN,
                available,
            })?;
        slot.copy_from_slice(&transport_sequence.get().to_le_bytes());
        Ok(())
    }

    /// The sequence this packet occupies in its stream's media numbering: the original for a
    /// retransmission, the header's own otherwise.
    pub fn media_sequence(&self, header: &PacketHeader) -> SequenceNumber {
        self.original_sequence.unwrap_or(header.sequence)
    }
}
