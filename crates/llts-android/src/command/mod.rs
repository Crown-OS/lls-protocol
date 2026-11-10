//! What Kotlin asks the runtime to do, parsed and checked on the calling thread so a malformed
//! argument throws there instead of surfacing later as an event.

mod feature_request;
mod parse;
mod peer_command;

use core::time::Duration;
use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::SyncSender;

use llts_node::{FeatureRequest, MediaRole, NodeError};
use llts_signaling::device::{DeviceId, Feature};
use llts_signaling::discovery::Beacon;
use llts_signaling::state::Topic;

pub(crate) use feature_request::FeatureRequestJson;
pub(crate) use parse::{
    parse_beacon, parse_feature, parse_feature_request, parse_ip_addresses, parse_peer_command,
    parse_socket_addresses, parse_topic,
};
pub(crate) use peer_command::PeerCommand;

use crate::media::Surface;
use crate::state_json::LocalDocument;

/// One request for the runtime thread, owning everything it carries.
#[derive(Debug)]
pub(crate) enum Command {
    Stop,
    Node(NodeCommand),
    PublishState {
        topic: Topic,
        document: LocalDocument,
    },
    /// An encoded application message for `peer`; the node's verdict goes back on `outcome`,
    /// so the caller learns at once whether it was sent.
    SendApplication {
        peer: DeviceId,
        message: Vec<u8>,
        outcome: SyncSender<Result<(), NodeError>>,
    },
    /// One moment of input, already framed as an Input-stream batch.
    SendInput {
        peer: DeviceId,
        batch: Vec<u8>,
    },
    AttachSurface {
        peer: DeviceId,
        index: u8,
        role: MediaRole,
        surface: Surface,
    },
    DetachSurface {
        peer: DeviceId,
        index: u8,
    },
}

/// A request the node itself answers.
#[derive(Debug)]
pub(crate) enum NodeCommand {
    BeginPairing {
        duration: Option<Duration>,
    },
    CancelPairing,
    PairWithQr {
        uri: String,
    },
    PairNearby {
        address: SocketAddr,
    },
    ConfirmPairing {
        peer: DeviceId,
        accept: bool,
    },
    Forget {
        peer: DeviceId,
    },
    SetFeature {
        peer: DeviceId,
        feature: Feature,
        enabled: bool,
    },
    StartFeature {
        peer: DeviceId,
        request: FeatureRequest,
    },
    StopFeature {
        peer: DeviceId,
        feature: Feature,
    },
    Send {
        peer: DeviceId,
        command: PeerCommand,
    },
    Presence {
        beacon: Beacon,
        addresses: Vec<SocketAddr>,
        seen: bool,
    },
    NetworkChanged {
        addresses: Vec<IpAddr>,
    },
}
