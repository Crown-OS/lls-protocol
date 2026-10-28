//! Media features and the negotiation that turns a request into running streams.
//!
//! The side that encodes always proposes the stream: it picks the codec from its own preference
//! order among what the peer can decode, allocates the index, and sends
//! [`OpenStream`](llts_signaling::message::OpenStream). The decoder confirms with
//! [`AcceptStream`](llts_signaling::message::AcceptStream), and only then does the encoder start,
//! so no media reaches a stream the receiver has not opened.

mod active;
mod pending;
mod request;

pub(crate) use active::{ActiveFeature, ActiveFeatures, Phase};
pub(crate) use pending::{PendingStart, PendingStarts};
pub use request::{FeatureRequest, FeatureState, MediaRole};
