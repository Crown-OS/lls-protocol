use serde::{Deserialize, Serialize};

/// The llts multiplexing classes, with the discriminant the wire `StreamId` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum MuxClass {
    Control = 0x00,
    Data = 0x01,
    Video = 0x10,
    Audio = 0x11,
    Input = 0x20,
}

impl MuxClass {
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
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
}

/// Names one stream within a session: the `class ‖ index` half of a wire `StreamId`, without
/// the session tag the transport adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamRef {
    pub class: MuxClass,
    pub index: u8,
}
