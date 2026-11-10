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

    #[error("{field} is {len} bytes, above the {limit}-byte limit")]
    FieldTooLong {
        field: &'static str,
        len: usize,
        limit: usize,
    },

    #[error("{field} has {count} entries, above the limit of {limit}")]
    TooManyEntries {
        field: &'static str,
        count: usize,
        limit: usize,
    },

    #[error("{field} is out of range")]
    OutOfRange { field: &'static str },

    #[error("a {len}-byte {topic:?} document exceeds the {limit}-byte limit")]
    DocumentTooLarge {
        topic: Topic,
        len: usize,
        limit: usize,
    },

    #[error("the blob key is malformed")]
    MalformedBlobKey,

    #[error("the URI's scheme is not one a peer may open")]
    UriSchemeRefused,
}
