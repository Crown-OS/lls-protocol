use llts_core::CoreError;
use llts_wire::packet::SequenceSpace;
use llts_wire::{StreamId, WireError};

/// Every way a link or its crypto can refuse to carry a datagram.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    #[error("socket operation failed")]
    Io(#[from] std::io::Error),

    #[error("protocol engine rejected the datagram")]
    Core(#[from] CoreError),

    #[error("datagram is malformed")]
    Wire(#[from] WireError),

    #[error("a {len}-byte datagram exceeds the {limit}-byte path limit")]
    DatagramTooLarge { len: usize, limit: usize },

    #[error("noise handshake failed while {step}")]
    Noise {
        step: &'static str,
        #[source]
        source: snow::Error,
    },

    #[error("handshake rejected: {reason}")]
    HandshakeRejected { reason: &'static str },

    #[error("datagram failed authentication and was discarded")]
    Unauthenticated,

    #[error("datagram repeats a counter already accepted, or one too old to judge")]
    Replayed,

    #[error("{kind} packets are not sealed by the packet cipher")]
    NotSealable { kind: &'static str },

    #[error(
        "sealing {space:?} sequence on {stream:?} would not advance its counter and would reuse a nonce"
    )]
    SequenceRegressed {
        stream: StreamId,
        space: SequenceSpace,
    },

    #[error("more than {limit} streams in one direction")]
    TooManyStreams { limit: usize },

    #[error("the kernel refused a datagram to {to}")]
    SendFailed {
        to: std::net::SocketAddr,
        #[source]
        source: std::io::Error,
    },

    #[error("no link is available to carry the session")]
    LinkUnavailable,

    #[error("the operating system's random source failed: {0}")]
    Randomness(getrandom::Error),

    #[error("could not encode or decode a stored or handshake record")]
    Encoding(#[from] postcard::Error),

    #[error("pairing code `{0}` is not a valid llts pairing offer")]
    InvalidPairingOffer(&'static str),

    #[error("the pairing offer expired")]
    PairingOfferExpired,

    #[error("no pairing window is open")]
    PairingClosed,

    #[error("too many handshakes from this source; try again shortly")]
    RateLimited,

    #[error("peer is not paired with this device")]
    UnknownPeer,

    #[error("trust store at {path} could not be {action}")]
    TrustStore {
        path: std::path::PathBuf,
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
}
