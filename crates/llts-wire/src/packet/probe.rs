use crate::packet::{PacketHeader, PacketKind, SequenceSpace};
use crate::util::{ByteReader, ByteWriter};
use crate::{MonotonicMicros, StreamId, WireError};

/// The sender's full 64-bit counter for one `(stream, space)`, as of the probe that carries it.
///
/// Headers carry only sixteen bits of sequence, so a receiver that missed more than half a
/// counter space — a ten-second blackout at 9 000 packets per second is well past that — can no
/// longer infer the high bits. A probe is rare enough to still decrypt after such a gap, and the
/// records it carries put every other counter back in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterRecord {
    pub stream: StreamId,
    pub space: SequenceSpace,
    pub counter: u64,
}

impl CounterRecord {
    pub const ENCODED_LEN: usize = 13;
}

/// Round-trip, path-MTU and path-validation measurement.
///
/// Padded to the size under test: a probe that does not come back tells the sender the path
/// cannot carry that datagram size. With [`crate::packet::PacketFlags::PATH_CHALLENGE`] set it
/// asks the peer to prove it is reachable at the address the probe was sent to.
///
/// ```text
/// 0  token                  4
/// 4  record_count           1
/// 5  counter records        13 each
///    padding                ..
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathProbe {
    pub token: u32,
}

impl PathProbe {
    /// A probe with no counter records; the packet cipher adds them as it seals.
    pub const BODY_LEN: usize = 5;

    /// Offset within the body at which the record count starts.
    pub const RECORDS_OFFSET: usize = 4;

    pub fn parse(datagram: &[u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Probe)?;
        let mut reader = ByteReader::new(body);
        Ok(Self {
            token: reader.u32_le()?,
        })
    }

    pub fn write_body(&self, destination: &mut [u8]) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u32_le(self.token)?;
        writer.u8(0)?;
        Ok(writer.written_len())
    }

    /// The counter records in a probe body. Records naming a space this build does not know are
    /// skipped rather than failing the whole probe.
    pub fn counter_records(
        body: &[u8],
    ) -> Result<impl Iterator<Item = CounterRecord> + '_, WireError> {
        let mut reader = ByteReader::new(body);
        reader.take(Self::RECORDS_OFFSET)?;
        let count = usize::from(reader.u8()?);
        let records = reader.take(count.saturating_mul(CounterRecord::ENCODED_LEN))?;
        let mut reader = ByteReader::new(records);
        Ok(core::iter::from_fn(move || {
            loop {
                let stream = StreamId::new(reader.u32_le().ok()?);
                let space = SequenceSpace::from_byte(reader.u8().ok()?);
                let counter = reader.u64_le().ok()?;
                if let Some(space) = space {
                    return Some(CounterRecord {
                        stream,
                        space,
                        counter,
                    });
                }
            }
        }))
    }

    /// Writes as many records as fit into `body`, after the token, returning the body length
    /// the records reach.
    pub fn write_counter_records(
        body: &mut [u8],
        records: impl Iterator<Item = CounterRecord>,
    ) -> Result<usize, WireError> {
        let capacity = body.len();
        let region = body
            .get_mut(Self::RECORDS_OFFSET..)
            .ok_or(WireError::TooShort {
                required: Self::BODY_LEN,
                available: capacity,
            })?;
        let (count_slot, record_area) = region.split_first_mut().ok_or(WireError::TooShort {
            required: Self::BODY_LEN,
            available: capacity,
        })?;

        let mut writer = ByteWriter::new(record_area);
        let mut written: u8 = 0;
        for record in records {
            if written == u8::MAX
                || writer.written_len() + CounterRecord::ENCODED_LEN > writer.capacity()
            {
                break;
            }
            writer.u32_le(record.stream.get())?;
            writer.u8(record.space.as_byte())?;
            writer.u64_le(record.counter)?;
            written += 1;
        }
        *count_slot = written;
        Ok(Self::BODY_LEN + writer.written_len())
    }
}

/// Reply to a [`PathProbe`], carrying enough information for a min-filtered clock-offset estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathProbeAck {
    pub token: u32,
    /// Receiver-local time the probe arrived.
    pub probe_arrival: MonotonicMicros,
    /// Receiver-local time this reply left, so the sender can subtract peer processing delay.
    pub ack_departure: MonotonicMicros,
}

impl PathProbeAck {
    pub const BODY_LEN: usize = 12;

    pub fn parse(datagram: &[u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::ProbeAck)?;
        let mut reader = ByteReader::new(body);
        Ok(Self {
            token: reader.u32_le()?,
            probe_arrival: MonotonicMicros::new(reader.u32_le()?),
            ack_departure: MonotonicMicros::new(reader.u32_le()?),
        })
    }

    pub fn write_body(&self, destination: &mut [u8]) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u32_le(self.token)?;
        writer.u32_le(self.probe_arrival.get())?;
        writer.u32_le(self.ack_departure.get())?;
        Ok(writer.written_len())
    }
}
