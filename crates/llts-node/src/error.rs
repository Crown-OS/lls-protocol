use llts_signaling::SignalingError;
use llts_signaling::device::{DeviceId, Feature};
use llts_signaling::discovery::DiscoveryError;
use llts_transport::TransportError;

/// Every way a node can refuse a command or fail to act on an input.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NodeError {
    #[error("signalling message could not be encoded or decoded")]
    Signaling(#[from] SignalingError),

    #[error("the transport refused the request")]
    Transport(#[from] TransportError),

    #[error("discovery beacon could not be built")]
    Discovery(#[from] DiscoveryError),

    #[error("{0} is not paired with this device")]
    UnknownPeer(DeviceId),

    #[error("{0} has no session right now")]
    NotConnected(DeviceId),

    #[error("{feature:?} is not available with {peer}")]
    FeatureUnavailable { peer: DeviceId, feature: Feature },

    #[error("a {len}-byte message exceeds the {limit}-byte channel limit")]
    MessageTooLarge { len: usize, limit: usize },

    #[error("this device already trusts the maximum of {limit} peers")]
    TooManyPeers { limit: usize },

    #[error("the operating system has no entropy to draw from")]
    Entropy(#[from] getrandom::Error),
}
