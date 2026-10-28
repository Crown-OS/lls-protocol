use crate::util::{ByteReader, ByteWriter};
use crate::{SequenceNumber, WireError};

/// One batch of input events, opaque to the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputBatch<'a> {
    pub sequence: SequenceNumber,
    pub events: &'a [u8],
}

impl InputBatch<'_> {
    pub const HEADER_LEN: usize = 4;

    pub const fn encoded_len(&self) -> usize {
        Self::HEADER_LEN + self.events.len()
    }
}

/// The payload of an input-stream packet: the newest few event batches, newest first.
///
/// Input is sent unreliably but redundantly. Resending the last few batches in every packet
/// costs a few dozen bytes and means one lost datagram loses nothing, without the round trip a
/// retransmission would add to a keypress. A key-state snapshot, flagged `KEYFRAME`, carries
/// the whole pressed-key set instead, so a stuck key cannot outlive a burst of loss.
///
/// ```text
/// 0  batch_count            1
///    per batch: sequence 2, events_len 2, events
/// ```
#[derive(Debug, Clone, Copy)]
pub struct InputBatches<'a> {
    count: u8,
    batches: &'a [u8],
}

impl<'a> InputBatches<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireError> {
        let mut reader = ByteReader::new(payload);
        let count = reader.u8()?;
        Ok(Self {
            count,
            batches: reader.into_remaining(),
        })
    }

    pub const fn count(&self) -> u8 {
        self.count
    }

    /// Batches newest first, stopping at the first one that does not fit the payload.
    pub fn iter(&self) -> impl Iterator<Item = InputBatch<'a>> + 'a {
        let mut reader = ByteReader::new(self.batches);
        let mut remaining = self.count;
        core::iter::from_fn(move || {
            remaining = remaining.checked_sub(1)?;
            let sequence = SequenceNumber::new(reader.u16_le().ok()?);
            let len = usize::from(reader.u16_le().ok()?);
            let events = reader.take(len).ok()?;
            Some(InputBatch { sequence, events })
        })
    }

    /// Writes `batches` behind a count, stopping before the first that would not fit.
    pub fn write<'b>(
        destination: &mut [u8],
        batches: impl Iterator<Item = InputBatch<'b>>,
    ) -> Result<usize, WireError> {
        let (count_slot, area) = destination.split_first_mut().ok_or(WireError::TooShort {
            required: 1,
            available: 0,
        })?;
        let mut writer = ByteWriter::new(area);
        let mut count: u8 = 0;
        for batch in batches {
            let Ok(len) = u16::try_from(batch.events.len()) else {
                break;
            };
            if count == u8::MAX || writer.written_len() + batch.encoded_len() > writer.capacity() {
                break;
            }
            writer.u16_le(batch.sequence.get())?;
            writer.u16_le(len)?;
            writer.put(batch.events)?;
            count += 1;
        }
        *count_slot = count;
        Ok(1 + writer.written_len())
    }
}
