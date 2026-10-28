use crate::message::MessageType;
use crate::state::Topic;

/// Every way encoding, decoding or applying a signalling message can fail.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignalingError {
    #[error("a {len}-byte message is shorter than the 2-byte envelope header")]
    TruncatedEnvelope { len: usize },

    #[error("message type {0:#06x} is not registered in this build")]
    UnknownMessageType(u16),

    #[error("malformed {msg_type:?} body")]
    MalformedBody {
        msg_type: MessageType,
        source: postcard::Error,
    },

    #[error("message body could not be encoded")]
    Encode(#[source] postcard::Error),

    #[error("{topic:?} payload is malformed")]
    MalformedPayload {
        topic: Topic,
        source: postcard::Error,
    },

    #[error("a {len}-byte clipboard exceeds the {limit}-byte inline limit and needs an ffsp offer")]
    ClipboardTooLarge { len: usize, limit: usize },

    #[error("malformed input message")]
    MalformedInput(#[source] postcard::Error),

    #[error("a {len}-byte input message exceeds the framing limit")]
    InputMessageTooLong { len: usize },

    #[error("the operating system has no entropy to draw from")]
    Entropy(#[from] getrandom::Error),
}
