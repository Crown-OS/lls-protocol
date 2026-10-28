/// What a stream carries, which fixes how it is delivered and where it queues in the pacer.
///
/// The high nibble groups classes by delivery: `0x0_` reliable, `0x1_` media, `0x2_` input. A
/// receiver that sees a class it does not know drops the stream rather than guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StreamClass {
    /// Reliable, ordered session control.
    Control = 0x00,
    /// Reliable application data, ordered or not, up to 64 KiB per message.
    Data = 0x01,
    /// Encoded video, protected by parity and retransmission.
    Video = 0x10,
    /// Opus frames, protected by the codec's own in-band redundancy.
    Audio = 0x11,
    /// Pointer and key events, sent unreliably with redundancy.
    Input = 0x20,
}

impl StreamClass {
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Control),
            0x01 => Some(Self::Data),
            0x10 => Some(Self::Video),
            0x11 => Some(Self::Audio),
            0x20 => Some(Self::Input),
            _ => None,
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Data => "data",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Input => "input",
        }
    }
}
