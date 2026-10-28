use std::net::SocketAddr;

use llts_core::clock::Instant;

/// Where the link to one paired peer stands.
///
/// A peer that is seen while [`Idle`](Self::Idle) is dialled at once, which is the whole
/// autoconnect policy: anywhere the peer is seen, on any plane, is worth a dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkState {
    /// No session and nothing scheduled; the next sighting dials.
    Idle,
    /// A reconnect handshake to `address` is in flight until `until`.
    Dialing {
        address: SocketAddr,
        until: Instant,
    },
    Connected,
    /// The last attempt failed; the next one waits until `until`.
    Backoff {
        until: Instant,
    },
}

impl LinkState {
    pub(crate) const fn deadline(self) -> Option<Instant> {
        match self {
            Self::Dialing { until, .. } | Self::Backoff { until } => Some(until),
            Self::Idle | Self::Connected => None,
        }
    }
}
