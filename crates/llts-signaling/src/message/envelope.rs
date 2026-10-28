use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::MessageType;

/// Bytes of the `msg_type` prefix in front of every body.
pub const ENVELOPE_HEADER_LEN: usize = 2;

/// A body with a fixed place in the [`MessageType`] registry.
pub trait Message<'a>: Serialize + Deserialize<'a> {
    const TYPE: MessageType;

    /// Appends the envelope to `buffer`, which lets a caller reuse one allocation per channel.
    ///
    /// # Errors
    ///
    /// Returns [`SignalingError::Encode`] if the body cannot be encoded.
    fn encode(&self, mut buffer: Vec<u8>) -> Result<Vec<u8>, SignalingError> {
        buffer.extend_from_slice(&Self::TYPE.as_u16().to_le_bytes());
        postcard::to_extend(self, buffer).map_err(SignalingError::Encode)
    }
}

/// One received channel message, split but not yet decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope<'a> {
    pub msg_type: u16,
    pub body: &'a [u8],
}

impl<'a> Envelope<'a> {
    /// # Errors
    ///
    /// Returns [`SignalingError::TruncatedEnvelope`] if there is no room for the type prefix.
    pub const fn parse(message: &'a [u8]) -> Result<Self, SignalingError> {
        match message.split_first_chunk::<ENVELOPE_HEADER_LEN>() {
            Some((header, body)) => Ok(Self {
                msg_type: u16::from_le_bytes(*header),
                body,
            }),
            None => Err(SignalingError::TruncatedEnvelope { len: message.len() }),
        }
    }

    pub const fn message_type(&self) -> Option<MessageType> {
        MessageType::from_u16(self.msg_type)
    }

    pub(crate) fn decode_body<M: Message<'a>>(&self) -> Result<M, SignalingError> {
        postcard::from_bytes(self.body).map_err(|source| SignalingError::MalformedBody {
            msg_type: M::TYPE,
            source,
        })
    }
}
