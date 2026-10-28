//! The eight datagram shapes, each parsed into a borrowed view.

mod body_prefix;
mod control;
mod feedback;
mod flags;
mod handshake;
mod header;
mod input;
mod kind;
mod media;
mod nack;
mod probe;
mod repair;
mod space;

pub use body_prefix::BodyPrefix;
pub use control::ControlEnvelope;
pub use feedback::{EcnMark, FeedbackEntry, FeedbackReport};
pub use flags::PacketFlags;
pub use handshake::{HandshakePattern, HandshakeView};
pub use header::PacketHeader;
pub use input::{InputBatch, InputBatches};
pub use kind::PacketKind;
pub use media::MediaPacketView;
pub use nack::{MissingRange, NackRequest};
pub use probe::{CounterRecord, PathProbe, PathProbeAck};
pub use repair::{RepairBlockGeometry, RepairPacketView};
pub use space::SequenceSpace;

use crate::WireError;

/// A parsed datagram, dispatched on its [`PacketKind`].
///
/// Parsing the header twice is cheap enough that the per-kind `parse` functions stay available for
/// callers that already know what they are holding.
#[derive(Debug, Clone, Copy)]
pub enum PacketView<'a> {
    Media(MediaPacketView<'a>),
    Repair(RepairPacketView<'a>),
    Feedback(FeedbackReport<'a>),
    Nack(NackRequest<'a>),
    Control(ControlEnvelope<'a>),
    Probe(PathProbe),
    ProbeAck(PathProbeAck),
    Handshake(HandshakeView<'a>),
}

impl<'a> PacketView<'a> {
    pub fn parse(datagram: &'a [u8]) -> Result<Self, WireError> {
        let (header, body) = PacketHeader::read_from(datagram)?;
        match header.kind {
            PacketKind::Media => MediaPacketView::from_parts(header, body).map(Self::Media),
            PacketKind::Repair => RepairPacketView::parse(datagram).map(Self::Repair),
            PacketKind::Feedback => FeedbackReport::parse(datagram).map(Self::Feedback),
            PacketKind::Nack => NackRequest::parse(datagram).map(Self::Nack),
            PacketKind::Control => ControlEnvelope::parse(datagram).map(Self::Control),
            PacketKind::Probe => PathProbe::parse(datagram).map(Self::Probe),
            PacketKind::ProbeAck => PathProbeAck::parse(datagram).map(Self::ProbeAck),
            PacketKind::Handshake => HandshakeView::parse(datagram).map(Self::Handshake),
        }
    }
}
