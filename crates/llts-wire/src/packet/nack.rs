use crate::packet::{PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{SequenceNumber, WireError};

/// A run of consecutive missing sequence numbers.
///
/// Losses cluster — a Wi-Fi aggregate drops as a unit — so run-length encoding keeps a NACK for a
/// whole lost burst inside a single small datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingRange {
    pub first: SequenceNumber,
    /// Sequence numbers after `first` that are also missing, so the run covers `extra + 1`.
    pub extra: u8,
}

impl MissingRange {
    pub const ENCODED_LEN: usize = 3;

    pub const fn single(sequence: SequenceNumber) -> Self {
        Self {
            first: sequence,
            extra: 0,
        }
    }

    /// Always at least one: a range names `first` plus `extra` successors.
    pub const fn sequence_count(&self) -> u16 {
        self.extra as u16 + 1
    }

    pub fn sequences(&self) -> impl Iterator<Item = SequenceNumber> + use<> {
        let first = self.first;
        (0..self.sequence_count()).map(move |offset| first.advanced_by(offset))
    }
}

/// Sequence numbers the receiver wants retransmitted before its playout deadline.
#[derive(Debug, Clone, Copy)]
pub struct NackRequest<'a> {
    header: PacketHeader,
    ranges: &'a [u8],
}

impl<'a> NackRequest<'a> {
    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Nack)?;
        if !body.len().is_multiple_of(MissingRange::ENCODED_LEN) {
            return Err(WireError::MalformedField { field: "ranges" });
        }
        Ok(Self {
            header,
            ranges: body,
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub const fn range_count(&self) -> usize {
        self.ranges.len() / MissingRange::ENCODED_LEN
    }

    pub fn ranges(&self) -> impl Iterator<Item = MissingRange> + 'a {
        let mut reader = ByteReader::new(self.ranges);
        core::iter::from_fn(move || {
            if reader.remaining_len() < MissingRange::ENCODED_LEN {
                return None;
            }
            let first = SequenceNumber::new(reader.u16_le().ok()?);
            let extra = reader.u8().ok()?;
            Some(MissingRange { first, extra })
        })
    }

    pub fn write_range(destination: &mut [u8], range: MissingRange) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(range.first.get())?;
        writer.u8(range.extra)?;
        Ok(writer.written_len())
    }
}
