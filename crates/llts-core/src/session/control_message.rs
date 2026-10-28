use llts_wire::StreamClass;

use crate::stream::StreamKey;

/// A message the session sends itself over the control stream.
///
/// Every control-stream message starts with a little-endian `u16` type. Types below
/// [`SessionControl::RESERVED_BELOW`] belong to the transport and never reach the application;
/// everything above is the signalling layer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionControl {
    /// The receiver of `stream` lost its decoder reference and needs an intra frame.
    KeyframeRequest(StreamKey),
}

impl SessionControl {
    pub const RESERVED_BELOW: u16 = 0x0100;
    pub const MAX_ENCODED_LEN: usize = 4;

    const KEYFRAME_REQUEST: u16 = 0x0001;

    pub const fn encode(&self) -> ([u8; Self::MAX_ENCODED_LEN], usize) {
        match self {
            Self::KeyframeRequest(stream) => {
                let [low, high] = Self::KEYFRAME_REQUEST.to_le_bytes();
                ([low, high, stream.class.as_byte(), stream.index], 4)
            }
        }
    }

    /// The message type of a control-stream message, if it is long enough to have one.
    pub fn message_type(message: &[u8]) -> Option<u16> {
        match message {
            [low, high, ..] => Some(u16::from_le_bytes([*low, *high])),
            _ => None,
        }
    }

    pub fn is_reserved(message: &[u8]) -> bool {
        Self::message_type(message).is_some_and(|kind| kind < Self::RESERVED_BELOW)
    }

    /// Decodes a reserved message; `None` for one this build does not know or a malformed body.
    pub fn decode(message: &[u8]) -> Option<Self> {
        match (Self::message_type(message)?, message) {
            (Self::KEYFRAME_REQUEST, [_, _, class, index]) => Some(Self::KeyframeRequest(
                StreamKey::new(StreamClass::from_byte(*class)?, *index),
            )),
            _ => None,
        }
    }
}
