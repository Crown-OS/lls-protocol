use crate::packet::{PacketHeader, PacketKind};
use crate::util::{ByteReader, ByteWriter};
use crate::{MonotonicMicros, SequenceNumber, WireError};

/// Explicit congestion notification as observed by the receiver's IP stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EcnMark {
    #[default]
    NotCapable = 0,
    Capable = 1,
    CongestionExperienced = 2,
}

impl EcnMark {
    const fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::Capable,
            2 => Self::CongestionExperienced,
            _ => Self::NotCapable,
        }
    }
}

/// Arrival record for a single packet, named by its transport sequence.
///
/// Transport-wide rather than per stream, as in TWCC: every paced packet on a path carries a
/// transport sequence in its body prefix, so one report feeds one congestion controller with
/// audio, video and control arrivals alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackEntry {
    pub transport_sequence: SequenceNumber,
    /// Receiver-local arrival time. Comparable to a departure timestamp only after the session's
    /// clock-offset estimate has been applied.
    pub arrival: MonotonicMicros,
    pub ecn: EcnMark,
}

impl FeedbackEntry {
    pub const ENCODED_LEN: usize = 7;
}

/// A batch of arrival records, sent when a frame completes, when the batch fills, or a few
/// milliseconds after its first entry — whichever comes first.
///
/// This replaces RFC 8888, whose arrival-time offsets quantise to 1/1024 s — about 0.98 ms, or
/// roughly half a LAN round trip, which destroys the delay-gradient signal this feeds.
#[derive(Debug, Clone, Copy)]
pub struct FeedbackReport<'a> {
    header: PacketHeader,
    loss_per_mille: u16,
    entries: &'a [u8],
}

impl<'a> FeedbackReport<'a> {
    /// Bytes of loss estimate that precede the entries.
    pub const PREFIX_LEN: usize = 2;

    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        header.kind.expect(PacketKind::Feedback)?;

        let mut reader = ByteReader::new(body);
        let loss_per_mille = reader.u16_le()?;
        let entries = reader.into_remaining();
        if !entries.len().is_multiple_of(FeedbackEntry::ENCODED_LEN) {
            return Err(WireError::MalformedField { field: "entries" });
        }
        Ok(Self {
            header,
            loss_per_mille,
            entries,
        })
    }

    pub const fn header(&self) -> &PacketHeader {
        &self.header
    }

    /// The receiver's own loss estimate, in thousandths.
    ///
    /// Sent explicitly because the sender cannot derive it: from the sender's side a gap that a
    /// retransmission later filled is indistinguishable from one that never happened, so a link
    /// held together by retransmission would look clean.
    pub const fn loss_per_mille(&self) -> u16 {
        self.loss_per_mille
    }

    pub fn write_prefix(destination: &mut [u8], loss_per_mille: u16) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(loss_per_mille)?;
        Ok(writer.written_len())
    }

    pub const fn entry_count(&self) -> usize {
        self.entries.len() / FeedbackEntry::ENCODED_LEN
    }

    pub fn entries(&self) -> impl Iterator<Item = FeedbackEntry> + 'a {
        let mut reader = ByteReader::new(self.entries);
        core::iter::from_fn(move || {
            if reader.remaining_len() < FeedbackEntry::ENCODED_LEN {
                return None;
            }
            let transport_sequence = SequenceNumber::new(reader.u16_le().ok()?);
            let arrival = MonotonicMicros::new(reader.u32_le().ok()?);
            let ecn = EcnMark::from_byte(reader.u8().ok()?);
            Some(FeedbackEntry {
                transport_sequence,
                arrival,
                ecn,
            })
        })
    }

    pub fn write_entry(destination: &mut [u8], entry: FeedbackEntry) -> Result<usize, WireError> {
        let mut writer = ByteWriter::new(destination);
        writer.u16_le(entry.transport_sequence.get())?;
        writer.u32_le(entry.arrival.get())?;
        writer.u8(entry.ecn as u8)?;
        Ok(writer.written_len())
    }
}
