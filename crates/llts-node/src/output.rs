use core::time::Duration;
use std::net::SocketAddr;

use llts_core::channel::Delivery;
use llts_core::session::Role;
use llts_signaling::device::{DeviceClass, DeviceId, Feature};
use llts_signaling::discovery::{Beacon, PairingFlags};
use llts_signaling::message::{
    Call, FileOffer, Media, SetHotspot, SetVolume, Signal, StreamParams, StreamRef, UnicursorEnter,
    UnicursorLeave,
};
use llts_signaling::state::Topic;
use llts_transport::pairing::QrOffer;

use crate::NodeError;
use crate::feature::{FeatureRequest, FeatureState, MediaRole};
use crate::link_secret::LinkSecret;

/// Everything a node asks its runtime to do or know.
#[derive(Debug)]
pub enum NodeOutput {
    Driver(DriverCommand),
    App(AppEvent),
    Media(MediaRequest),
    Persist(PersistRequest),
    /// Make `peer` trusted by ffsp too; the bridge derives ffsp's secret from `link_secret`.
    ExportFfspLink(FfspLinkExport),
    /// The complete set of beacons to advertise from now on, over BLE and mDNS alike.
    Advertise(Vec<Beacon>),
}

/// A call to make on the llts driver; [`crate::bridge::execute`] makes it.
#[derive(Debug)]
pub enum DriverCommand {
    /// Reconnect to a trusted peer (Noise IK).
    Dial {
        peer: DeviceId,
        address: SocketAddr,
    },
    /// Open a pairing window and produce a QR offer for it, answered by
    /// [`LinkEvent::QrOfferReady`](crate::LinkEvent::QrOfferReady).
    OfferQrPairing {
        addresses: Vec<SocketAddr>,
        duration: Duration,
    },
    ClosePairing,
    PairNearby {
        address: SocketAddr,
    },
    PairWithOffer {
        offer: Box<QrOffer>,
    },
    ConfirmPairing {
        peer: DeviceId,
    },
    RejectPairing {
        peer: DeviceId,
    },
    /// Queue one signalling envelope on a reliable stream.
    Send {
        peer: DeviceId,
        stream: StreamRef,
        message: Vec<u8>,
        delivery: Delivery,
    },
    OpenReliable {
        peer: DeviceId,
        stream: StreamRef,
    },
    OpenMedia {
        peer: DeviceId,
        stream: StreamRef,
        params: StreamParams,
        role: Role,
    },
    CloseStream {
        peer: DeviceId,
        stream: StreamRef,
    },
    /// Remove `peer` from the trust store and drop its session.
    Forget {
        peer: DeviceId,
    },
    Rebind {
        local: SocketAddr,
    },
}

/// Something the user interface or the platform should hear about.
#[derive(Debug)]
pub enum AppEvent {
    /// The device list changed; [`Node::devices`](crate::Node::devices) has the new one.
    DevicesChanged,
    DeviceConnected {
        peer: DeviceId,
        connected: bool,
    },
    /// Show `uri` as a QR code until `expires_unix_ms`. It holds a one-time secret: never log it.
    PairingOffer {
        uri: String,
        expires_unix_ms: u64,
    },
    /// A nearby device wants to pair; show `code` and answer with
    /// [`AppCommand::ConfirmPairing`](crate::AppCommand::ConfirmPairing).
    PairingRequest {
        peer: DeviceId,
        name: String,
        code: u32,
    },
    /// A pairing finished either way; `peer` is `None` for a nearby attempt that failed before
    /// the peer proved who it is.
    PairingResult {
        peer: Option<DeviceId>,
        accepted: bool,
    },
    PairingWindowClosed,
    /// A device nearby has its pairing window open.
    PairableDeviceSeen {
        class: DeviceClass,
        flags: PairingFlags,
        address: Option<SocketAddr>,
    },
    /// A peer published a new document; read it with
    /// [`Node::remote_state`](crate::Node::remote_state).
    RemoteStateChanged {
        peer: DeviceId,
        topic: Topic,
    },
    FeatureStateChanged {
        peer: DeviceId,
        feature: Feature,
        state: FeatureState,
    },
    FeatureFailed {
        peer: DeviceId,
        feature: Feature,
        reason: FeatureFailure,
    },
    /// A peer asked this device to do something.
    RemoteCommand {
        peer: DeviceId,
        command: RemoteCommand,
    },
    /// A command or input could not be carried out.
    Error(NodeError),
}

/// Why a feature did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureFailure {
    /// Disabled for this peer, or one end does not implement it.
    Unavailable,
    /// No codec both ends support.
    NoCommonCodec,
    /// The peer refused or stopped it before it ran.
    Refused,
    AlreadyActive,
    NotConnected,
}

/// A local media pipeline to start or stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRequest {
    /// Run an encoder (`MediaRole::Encoder`) or decoder into `stream` with `params`. `request`
    /// says what it is for: for the encoder of a camera, which lens; for the decoder of a
    /// monitor, the mode to create a virtual output in.
    Start {
        peer: DeviceId,
        request: FeatureRequest,
        role: MediaRole,
        stream: StreamRef,
        params: StreamParams,
    },
    Stop {
        peer: DeviceId,
        feature: Feature,
        stream: StreamRef,
    },
}

/// State the node cannot keep across restarts by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistRequest {
    /// Store which features the user allows `peer`, to hand back as [`crate::KnownPeer`].
    PeerFeatures {
        peer: DeviceId,
        enabled: llts_signaling::device::FeatureSet,
    },
    ForgetPeer {
        peer: DeviceId,
    },
}

/// Everything the ffsp bridge needs to write one link record.
#[derive(Debug, Clone)]
pub struct FfspLinkExport {
    pub peer: DeviceId,
    pub name: String,
    pub class: DeviceClass,
    pub link_secret: LinkSecret,
}

/// A peer's request, owning its strings so it outlives the received buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    Call(CallCommand),
    Media(Media),
    Reply {
        conversation_id: String,
        text: String,
    },
    SetVolume(SetVolume),
    Lock,
    SetHotspot(SetHotspot),
    FileOffer(FileOffer),
    UnicursorEnter(UnicursorEnter),
    UnicursorLeave(UnicursorLeave),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallCommand {
    Pickup { call_id: String },
    Decline { call_id: String },
    Hangup { call_id: String },
    Dial { number: String },
}

impl RemoteCommand {
    /// The owned form of a command signal; `None` for signals that are not commands.
    pub(crate) fn from_signal(signal: &Signal<'_>) -> Option<Self> {
        Some(match *signal {
            Signal::CallV1(call) => Self::Call(CallCommand::from(call)),
            Signal::MediaV1(media) => Self::Media(media),
            Signal::ReplyV1(reply) => Self::Reply {
                conversation_id: reply.conversation_id.to_owned(),
                text: reply.text.to_owned(),
            },
            Signal::SetVolumeV1(volume) => Self::SetVolume(volume),
            Signal::LockV1(_) => Self::Lock,
            Signal::SetHotspotV1(hotspot) => Self::SetHotspot(hotspot),
            Signal::FileOfferV1(offer) => Self::FileOffer(offer),
            Signal::UnicursorEnterV1(enter) => Self::UnicursorEnter(enter),
            Signal::UnicursorLeaveV1(leave) => Self::UnicursorLeave(leave),
            _ => return None,
        })
    }
}

/// The feature both ends must allow before a command may cross; `None` for commands every
/// paired device accepts.
pub(crate) const fn command_feature(signal: &Signal<'_>) -> Option<Feature> {
    match signal {
        Signal::CallV1(_) => Some(Feature::Calls),
        Signal::ReplyV1(_) => Some(Feature::Notifications),
        Signal::SetHotspotV1(_) => Some(Feature::Hotspot),
        Signal::FileOfferV1(_) => Some(Feature::Files),
        Signal::UnicursorEnterV1(_) | Signal::UnicursorLeaveV1(_) => Some(Feature::Unicursor),
        _ => None,
    }
}

impl From<Call<'_>> for CallCommand {
    fn from(call: Call<'_>) -> Self {
        match call {
            Call::Pickup { call_id } => Self::Pickup {
                call_id: call_id.to_owned(),
            },
            Call::Decline { call_id } => Self::Decline {
                call_id: call_id.to_owned(),
            },
            Call::Hangup { call_id } => Self::Hangup {
                call_id: call_id.to_owned(),
            },
            Call::Dial { number } => Self::Dial {
                number: number.to_owned(),
            },
        }
    }
}
