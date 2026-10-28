use crate::packet::{BodyPrefix, PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{SequenceNumber, WireError};

/// Where a Reed-Solomon block sits inside its frame, and how its shards are laid out.
///
/// A block covers **one slice**, not a whole frame. Slice-level pipelining fragments each slice
/// independently, so only a slice has the property Reed-Solomon needs: every shard the same size
/// except the last. Making the block a slice also gets the parity onto the wire a slice earlier
/// than waiting for the frame would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairBlockGeometry {
    /// Fragment index within the frame that the block's first data shard occupies.
    pub first_data_index: u16,
    pub data_shard_count: u16,
    /// Size every shard was padded to for the encoding.
    pub shard_bytes: u16,
    /// Byte offset of the block's first shard within the frame.
    pub block_byte_offset: u32,
    /// Bytes the block protects before shard padding, so the decoder can trim.
    pub block_byte_len: u32,
}

impl RepairBlockGeometry {
    pub const ENCODED_LEN: usize = 14;

    fn read_from(reader: &mut ByteReader<'_>) -> Result<Self, WireError> {
        let geometry = Self {
            first_data_index: reader.u16_le()?,
            data_shard_count: reader.u16_le()?,
            shard_bytes: reader.u16_le()?,
            block_byte_offset: reader.u32_le()?,
            block_byte_len: reader.u32_le()?,
        };
        if geometry.data_shard_count == 0 {
            return Err(WireError::MalformedField {
                field: "data_shard_count",
            });
        }
        if !geometry.shard_bytes.is_multiple_of(2) || geometry.shard_bytes == 0 {
            return Err(WireError::MalformedField {
                field: "shard_bytes",
            });
        }
        Ok(geometry)
    }

    pub fn write_into(&self, destination: &mut [u8]) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(self.first_data_index)?;
        writer.u16_le(self.data_shard_count)?;
        writer.u16_le(self.shard_bytes)?;
        writer.u32_le(self.block_byte_offset)?;
        writer.u32_le(self.block_byte_len)?;
        Ok(writer.written_len())
    }

    /// Real, unpadded length of shard `index`.
    pub const fn shard_len(&self, index: u16) -> u32 {
        let consumed = (index as u32).saturating_mul(self.shard_bytes as u32);
        let remaining = self.block_byte_len.saturating_sub(consumed);
        if remaining > self.shard_bytes as u32 {
            self.shard_bytes as u32
        } else {
            remaining
        }
    }

    /// Byte offset of shard `index` within the frame.
    pub const fn shard_offset(&self, index: u16) -> u32 {
        self.block_byte_offset
            .saturating_add((index as u32).saturating_mul(self.shard_bytes as u32))
    }
}

/// A parity shard for one frame's repair block.
#[derive(Debug, Clone, Copy)]
pub struct RepairPacketView<'a> {
    header: PacketHeader,
    prefix: BodyPrefix,
    geometry: RepairBlockGeometry,
    shard: &'a [u8],
}

impl<'a> RepairPacketView<'a> {
    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Repair)?;
        let (prefix, body) = BodyPrefix::split_from(&header, body)?;
        let mut reader = ByteReader::new(body);
        let geometry = RepairBlockGeometry::read_from(&mut reader)?;
        if header.packet_index < geometry.data_shard_count {
            return Err(WireError::MalformedField {
                field: "packet_index",
            });
        }
        Ok(Self {
            header,
            prefix,
            geometry,
            shard: reader.into_remaining(),
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub const fn prefix(&self) -> &BodyPrefix {
        &self.prefix
    }

    /// Where this shard sits in its stream's numbering, which for a retransmission is the
    /// sequence it replaces rather than the one in its header.
    pub fn media_sequence(&self) -> SequenceNumber {
        self.prefix.media_sequence(&self.header)
    }

    pub const fn geometry(&self) -> RepairBlockGeometry {
        self.geometry
    }

    /// Index of this shard within the parity range, counting from zero.
    pub const fn parity_index(&self) -> u16 {
        self.header.packet_index - self.geometry.data_shard_count
    }

    pub const fn shard(&self) -> &'a [u8] {
        self.shard
    }
}
