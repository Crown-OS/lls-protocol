use crate::packet::{PacketFlags, PacketKind};

/// Which counter a sealed packet's sequence number is drawn from.
///
/// The space is the first byte of the AEAD nonce, so two packets of one stream can share a
/// sequence number only if they belong to different spaces — which is what lets feedback,
/// retransmissions and media each count from zero without ever colliding under one key. The
/// receiver reads the space off the clear header, so it needs no state to rebuild the nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SequenceSpace {
    /// Original media fragments and the parity shards that protect them.
    Media = 0,
    /// Fragments resent in answer to a NACK. The original sequence travels encrypted in the body.
    Retransmission = 1,
    /// Arrival reports and NACKs.
    Feedback = 2,
    /// Probes, path challenges and their acknowledgements.
    Probe = 3,
    /// Reliable-channel envelopes.
    Control = 4,
}

impl SequenceSpace {
    pub const COUNT: usize = 5;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Media,
        Self::Retransmission,
        Self::Feedback,
        Self::Probe,
        Self::Control,
    ];

    /// The space a packet of this shape is numbered in, or `None` for a handshake, which the
    /// packet cipher never seals.
    pub const fn of(kind: PacketKind, flags: PacketFlags) -> Option<Self> {
        match kind {
            PacketKind::Media | PacketKind::Repair => {
                if flags.contains(PacketFlags::RETRANSMISSION) {
                    Some(Self::Retransmission)
                } else {
                    Some(Self::Media)
                }
            }
            PacketKind::Feedback | PacketKind::Nack => Some(Self::Feedback),
            PacketKind::Probe | PacketKind::ProbeAck => Some(Self::Probe),
            PacketKind::Control => Some(Self::Control),
            PacketKind::Handshake => None,
        }
    }

    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Media),
            1 => Some(Self::Retransmission),
            2 => Some(Self::Feedback),
            3 => Some(Self::Probe),
            4 => Some(Self::Control),
            _ => None,
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    /// Position in [`SequenceSpace::ALL`], for per-space state kept in a fixed array.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::Retransmission => "retransmission",
            Self::Feedback => "feedback",
            Self::Probe => "probe",
            Self::Control => "control",
        }
    }
}
