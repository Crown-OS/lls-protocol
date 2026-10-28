use core::time::Duration;

use llts_signaling::device::{DeviceClass, DeviceId, Feature, FeatureSet};
use llts_signaling::message::Capabilities;
use llts_transport::pairing::TrustedPeer;

use crate::util::device::{class_from_transport, device_id};

/// How this device presents itself to its peers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDevice {
    pub name: String,
    pub model: String,
    /// Codecs this device can encode and decode, the features it implements, and its class.
    pub capabilities: Capabilities,
}

/// How quickly a peer that cannot be reached is tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub initial: Duration,
    pub max: Duration,
    /// Each delay is moved by up to this share either way, so devices that lost the same network
    /// do not all redial in the same instant.
    pub jitter_percent: u8,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(500),
            max: Duration::from_secs(60),
            jitter_percent: 20,
        }
    }
}

/// Policy knobs; [`NodeConfig::default`] is what CrownConnect ships with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeConfig {
    /// Features a newly paired device is allowed until the user changes them.
    pub default_features: FeatureSet,
    /// How long a pairing window, and the QR offer shown in it, stays open.
    pub pairing_window: Duration,
    /// How long a nearby pairing code waits for the user before it is rejected.
    pub code_confirmation: Duration,
    /// How long a dial may go unanswered before the node gives up on it.
    pub dial_timeout: Duration,
    pub backoff: BackoffPolicy,
    /// How long state changes gather before one update per topic goes to each subscriber.
    pub state_flush_delay: Duration,
    /// How long a feature started before the peer announced its capabilities waits for them.
    pub capabilities_wait: Duration,
    /// The starting bitrate proposed for video streams; congestion control takes over from it.
    pub video_bitrate_kbps: u32,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            default_features: Feature::ALL.into_iter().collect(),
            pairing_window: Duration::from_secs(120),
            code_confirmation: Duration::from_secs(60),
            dial_timeout: Duration::from_secs(4),
            backoff: BackoffPolicy::default(),
            state_flush_delay: Duration::from_millis(10),
            capabilities_wait: Duration::from_secs(5),
            video_bitrate_kbps: 20_000,
        }
    }
}

/// A paired device as the runtime persisted it: the trust store entry plus the features the
/// user allows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeer {
    pub id: DeviceId,
    pub name: String,
    pub class: DeviceClass,
    pub enabled: FeatureSet,
}

impl KnownPeer {
    pub fn from_trusted(peer: &TrustedPeer, enabled: FeatureSet) -> Self {
        Self {
            id: device_id(peer.static_key),
            name: peer.name.clone(),
            class: class_from_transport(peer.class),
            enabled,
        }
    }
}
