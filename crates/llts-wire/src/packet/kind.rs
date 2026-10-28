use crate::WireError;

/// Discriminates the eight datagram shapes the protocol defines.
///
/// Encoded in the low nibble of the second header byte, so at most sixteen kinds can ever exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PacketKind {
    /// A fragment of an encoded video or audio frame.
    Media = 0,
    /// A Reed-Solomon parity shard for one frame's repair block.
    Repair = 1,
    /// Per-packet arrival times, feeding the sender's bandwidth estimator.
    Feedback = 2,
    /// Ranges of sequence numbers the receiver wants retransmitted.
    Nack = 3,
    /// Reliable, ordered application control traffic.
    Control = 4,
    /// Round-trip and path-MTU measurement.
    Probe = 5,
    /// Reply to a [`PacketKind::Probe`].
    ProbeAck = 6,
    /// One Noise message, sent in the clear because it is what establishes the keys.
    Handshake = 7,
}

impl PacketKind {
    pub(crate) const fn from_nibble(nibble: u8) -> Result<Self, WireError> {
        match nibble {
            0 => Ok(Self::Media),
            1 => Ok(Self::Repair),
            2 => Ok(Self::Feedback),
            3 => Ok(Self::Nack),
            4 => Ok(Self::Control),
            5 => Ok(Self::Probe),
            6 => Ok(Self::ProbeAck),
            7 => Ok(Self::Handshake),
            other => Err(WireError::UnknownPacketKind(other)),
        }
    }

    pub(crate) const fn as_nibble(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::Repair => "repair",
            Self::Feedback => "feedback",
            Self::Nack => "nack",
            Self::Control => "control",
            Self::Probe => "probe",
            Self::ProbeAck => "probe-ack",
            Self::Handshake => "handshake",
        }
    }

    pub(crate) const fn expect(self, wanted: Self) -> Result<(), WireError> {
        if self as u8 == wanted as u8 {
            Ok(())
        } else {
            Err(WireError::UnexpectedPacketKind {
                expected: wanted.name(),
                found: self.name(),
            })
        }
    }
}
