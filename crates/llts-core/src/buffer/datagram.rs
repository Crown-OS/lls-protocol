use llts_wire::packet::{BodyPrefix, PacketHeader};
use llts_wire::{SequenceNumber, WireError};

use super::{BufferPool, PooledBuffer};
use crate::CoreError;

/// One wire-ready datagram: its parsed header alongside the bytes that encode it.
///
/// Keeping the header decoded means the pacer, the retransmit cache and the repair encoder can
/// all route a datagram without re-parsing it.
#[derive(Debug)]
pub struct Datagram {
    header: PacketHeader,
    buffer: PooledBuffer,
}

impl Datagram {
    /// Writes `header`, the body prefix its kind carries, and each of `sections` in turn into a
    /// buffer drawn from `pool`.
    ///
    /// The transport sequence is left at zero: it is stamped when the pacer releases the
    /// datagram, because only then is its place in the path's send order known.
    pub(crate) fn assemble(
        header: &PacketHeader,
        sections: &[&[u8]],
        pool: &BufferPool,
    ) -> Result<Self, CoreError> {
        Self::assemble_with_prefix(header, BodyPrefix::fresh(), sections, pool)
    }

    /// Like [`Datagram::assemble`], for a retransmission that names the sequence it replaces.
    pub(crate) fn assemble_retransmission(
        header: &PacketHeader,
        original: SequenceNumber,
        payload: &[u8],
        pool: &BufferPool,
    ) -> Result<Self, CoreError> {
        Self::assemble_with_prefix(
            header,
            BodyPrefix::retransmitting(original),
            &[payload],
            pool,
        )
    }

    fn assemble_with_prefix(
        header: &PacketHeader,
        prefix: BodyPrefix,
        sections: &[&[u8]],
        pool: &BufferPool,
    ) -> Result<Self, CoreError> {
        let mut buffer = pool.acquire();
        let mut filled = header.write_into(buffer.spare_capacity_mut())?;
        if BodyPrefix::is_carried_by(header.kind) {
            let spare = buffer.spare_capacity_mut();
            let capacity = spare.len();
            let destination = spare.get_mut(filled..).ok_or(CoreError::BufferOverflow {
                requested: filled,
                capacity,
            })?;
            filled += prefix.write_into(header, destination)?;
        }
        buffer.set_filled(filled)?;
        for section in sections {
            buffer.extend_from_slice(section)?;
        }
        Ok(Self {
            header: *header,
            buffer,
        })
    }

    /// Copies this datagram into a fresh pooled buffer.
    ///
    /// The retransmit cache needs its own copy so the paced original can be handed to the link
    /// and dropped. At roughly a kilobyte per packet the copy costs tens of nanoseconds, which is
    /// a better trade than routing ownership back from the transport.
    pub(crate) fn duplicate(&self, pool: &BufferPool) -> Result<Self, CoreError> {
        let mut buffer = pool.acquire();
        buffer.extend_from_slice(self.bytes())?;
        Ok(Self {
            header: self.header,
            buffer,
        })
    }

    /// Adopts a buffer a caller has already filled with `header` and its body.
    pub(crate) const fn from_buffer(header: PacketHeader, buffer: PooledBuffer) -> Self {
        Self { header, buffer }
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    pub fn bytes(&self) -> &[u8] {
        self.buffer.filled()
    }

    /// The encoded media or repair bytes, without the header or the body prefix.
    pub fn payload(&self) -> &[u8] {
        let start = PacketHeader::ENCODED_LEN
            + BodyPrefix::encoded_len(self.header.kind, self.header.flags);
        self.bytes().get(start..).unwrap_or_default()
    }

    /// Where this datagram sits in its stream's media numbering: the sequence a retransmission
    /// replaces, or the header's own.
    pub fn media_sequence(&self) -> SequenceNumber {
        let body = self
            .bytes()
            .get(PacketHeader::ENCODED_LEN..)
            .unwrap_or_default();
        BodyPrefix::split_from(&self.header, body).map_or(self.header.sequence, |(prefix, _)| {
            prefix.media_sequence(&self.header)
        })
    }

    /// Lets `write` encode up to `limit` body bytes straight into the buffer after whatever it
    /// already holds.
    pub(crate) fn append_with(
        &mut self,
        limit: usize,
        write: impl FnOnce(&mut [u8]) -> Result<usize, WireError>,
    ) -> Result<(), CoreError> {
        let filled = self.buffer.len();
        let end = filled.saturating_add(limit);
        let spare = self.buffer.spare_capacity_mut();
        let capacity = spare.len();
        let region = spare
            .get_mut(filled..end.min(capacity))
            .ok_or(CoreError::BufferOverflow {
                requested: end,
                capacity,
            })?;
        let written = write(region)?;
        self.buffer.set_filled(filled + written)
    }

    /// The whole buffer and how much of it is filled, so a transport can seal in place: the
    /// room past the filled length is where the authentication tag goes.
    pub fn sealable_mut(&mut self) -> (&mut [u8], usize) {
        let filled = self.buffer.len();
        (self.buffer.spare_capacity_mut(), filled)
    }

    /// Marks the buffer as holding `len` sealed bytes, tag included, so [`Datagram::bytes`]
    /// returns what goes on the wire.
    pub fn set_sealed_len(&mut self, len: usize) -> Result<(), CoreError> {
        self.buffer.set_filled(len)
    }

    /// Numbers this datagram in the path's send order, once the pacer has released it.
    pub fn stamp_transport_sequence(
        &mut self,
        transport_sequence: SequenceNumber,
    ) -> Result<(), CoreError> {
        if !BodyPrefix::is_carried_by(self.header.kind) {
            return Ok(());
        }
        let filled = self.buffer.len();
        let bytes = self
            .buffer
            .spare_capacity_mut()
            .get_mut(..filled)
            .unwrap_or_default();
        BodyPrefix::stamp_transport_sequence(bytes, transport_sequence)?;
        Ok(())
    }

    pub const fn byte_len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the buffer to its pool and yields the header, for a caller that wants to record
    /// what was sent without holding the memory.
    pub fn into_header(self) -> PacketHeader {
        self.header
    }

    /// Replaces the header in place, which is only sound because the encoding is fixed-length.
    ///
    /// The packetizer needs this to stamp a frame's true fragment count onto the closing packet:
    /// with slice-level pipelining the total is not known until the encoder says the frame is
    /// done, by which point the earlier fragments are already built.
    pub(crate) fn rewrite_header(&mut self, header: &PacketHeader) -> Result<(), CoreError> {
        header.write_into(self.buffer.spare_capacity_mut())?;
        self.header = *header;
        Ok(())
    }
}
