use llts_node::NodeError;
use llts_signaling::SignalingError;
use llts_signaling::discovery::DiscoveryError;
use llts_transport::TransportError;

use crate::media::MediaError;

/// Everything a JNI call can fail with; the bridge turns each into a Java exception.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BridgeError {
    #[error("{what} is not valid JSON for it")]
    Json {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("{what} is not a known name: {name}")]
    UnknownName { what: &'static str, name: String },

    #[error("a device id is 64 hex digits")]
    DeviceId,

    #[error("{0} is not a stream index")]
    StreamIndex(i32),

    #[error("the socket address is malformed")]
    Address(#[from] std::net::AddrParseError),

    #[error("the discovery beacon is malformed")]
    Beacon(#[from] DiscoveryError),

    #[error("the state document does not match its topic")]
    State(#[from] SignalingError),

    #[error("the value could not be encoded")]
    Encoding(#[from] postcard::Error),

    #[error("the llts runtime is not running")]
    NotRunning,

    #[error("the Java call failed")]
    Jni(#[from] jni::errors::Error),

    #[error("the files directory could not be used")]
    Io(#[from] std::io::Error),

    #[error("the transport refused the request")]
    Transport(#[from] TransportError),

    #[error("the node refused the request")]
    Node(#[from] NodeError),

    #[error("the media pipeline failed")]
    Media(#[from] MediaError),
}

impl BridgeError {
    pub(crate) fn json(what: &'static str) -> impl FnOnce(serde_json::Error) -> Self {
        move |source| Self::Json { what, source }
    }

    /// Whether the caller passed something malformed, as opposed to the runtime failing.
    pub(crate) const fn is_caller_mistake(&self) -> bool {
        matches!(
            self,
            Self::Json { .. }
                | Self::UnknownName { .. }
                | Self::DeviceId
                | Self::Address(_)
                | Self::StreamIndex(_)
                | Self::Beacon(_)
                | Self::State(_)
        )
    }
}

/// An error followed by every cause behind it, for a log line or an exception message.
pub(crate) fn describe(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    message
}
