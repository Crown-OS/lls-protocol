use llts_wire::packet::{HandshakePattern, HandshakeView, PacketFlags, PacketHeader, PacketKind};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, SessionTag,
    StreamClass, StreamId,
};

use crate::TransportError;

/// Frames one Noise message as a handshake datagram addressed to `receiver_tag`.
pub(crate) fn handshake_datagram(
    receiver_tag: SessionTag,
    pattern: HandshakePattern,
    message_index: u8,
    sender_tag: SessionTag,
    noise: &[u8],
) -> Result<Vec<u8>, TransportError> {
    let header = PacketHeader {
        version: ProtocolVersion::CURRENT,
        kind: PacketKind::Handshake,
        flags: PacketFlags::empty(),
        stream: StreamId::compose(receiver_tag, StreamClass::Control, 0),
        sequence: SequenceNumber::new(u16::from(message_index)),
        frame: FrameId::ZERO,
        timestamp: MediaTimestamp::ZERO,
        packet_index: 0,
        packet_count: 1,
        departure: MonotonicMicros::ZERO,
        payload_offset: 0,
    };
    let mut datagram =
        vec![0u8; PacketHeader::ENCODED_LEN + HandshakeView::PREFIX_LEN + noise.len()];
    let header_len = header.write_into(&mut datagram)?;
    let (_, body) = datagram.split_at_mut(header_len);
    let prefix_len = HandshakeView::write_prefix(body, pattern, message_index, sender_tag)?;
    if let Some(slot) = body.get_mut(prefix_len..) {
        slot.copy_from_slice(noise);
    }
    Ok(datagram)
}
