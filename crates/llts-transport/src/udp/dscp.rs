use llts_wire::StreamClass;
use llts_wire::packet::PacketKind;

use super::socket_options::{DSCP_BULK, DSCP_CONTROL, DSCP_VIDEO};

/// The TOS byte a datagram should carry, from what it is and which stream it belongs to.
///
/// Video goes to the Wi-Fi video access category; bulk data to best effort, so a file transfer
/// never competes with a frame; everything small and urgent — audio, input, control, feedback,
/// probes and handshakes — to voice.
pub const fn tos_for(kind: PacketKind, class: Option<StreamClass>) -> u8 {
    let dscp = match (kind, class) {
        (PacketKind::Media | PacketKind::Repair, Some(StreamClass::Video)) => DSCP_VIDEO,
        (_, Some(StreamClass::Data)) => DSCP_BULK,
        _ => DSCP_CONTROL,
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the DSCP constants are TOS bytes by definition"
    )]
    let tos = dscp as u8;
    tos
}
