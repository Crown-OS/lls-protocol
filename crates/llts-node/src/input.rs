use core::time::Duration;
use std::net::SocketAddr;

use llts_signaling::device::{DeviceClass, DeviceId, Feature};
use llts_signaling::discovery::Beacon;
use llts_signaling::message::{
    Call, FileOffer, Lock, Media, Reply, SetHotspot, SetVolume, Signal, StreamRef, UnicursorEnter,
    UnicursorLeave,
};
use llts_signaling::state::Topic;
use llts_transport::pairing::QrOffer;

use crate::feature::FeatureRequest;

/// Everything a node reacts to.
#[derive(Debug, Clone, Copy)]
pub enum NodeInput<'a> {
    Presence(Presence<'a>),
    Link(LinkEvent<'a>),
    /// A local publisher's new document for `topic`, already postcard-encoded. Typed callers can
    /// use [`Node::publish`](crate::Node::publish) instead.
    LocalState {
        topic: Topic,
        payload: &'a [u8],
    },
    App(AppCommand<'a>),
    /// The local network changed: these are the addresses peers can now reach this device at,
    /// and `rebind` is the socket address to move the driver to, if it must move.
    NetworkChanged {
        addresses: &'a [SocketAddr],
        rebind: Option<SocketAddr>,
    },
    /// [`Node::next_timeout`](crate::Node::next_timeout) has passed.
    TimerExpired,
}

/// A discovery plane saw a beacon appear or disappear.
#[derive(Debug, Clone, Copy)]
pub enum Presence<'a> {
    /// `addresses` are where the sender can be dialled; empty for a plane that carries none,
    /// such as BLE.
    Seen {
        beacon: Beacon,
        addresses: &'a [SocketAddr],
    },
    Lost {
        beacon: Beacon,
    },
}

/// What the transport reported, with borrowed payloads; see [`crate::bridge::link_event`].
#[derive(Debug, Clone, Copy)]
pub enum LinkEvent<'a> {
    /// A session with a trusted peer is up.
    Connected {
        peer: DeviceId,
        name: &'a str,
        class: DeviceClass,
    },
    /// The session with `peer` stopped carrying traffic.
    Disconnected { peer: DeviceId },
    /// A complete message arrived on a reliable stream.
    Message {
        peer: DeviceId,
        stream: StreamRef,
        bytes: &'a [u8],
    },
    /// Nearby pairing needs the user to compare `code` on both screens.
    PairingCode {
        peer: DeviceId,
        name: &'a str,
        code: u32,
    },
    /// `peer` is now in the transport's trust store.
    Paired { peer: DeviceId },
    /// A handshake this device started to `address` got no usable answer.
    HandshakeFailed { address: SocketAddr },
    /// The driver opened a QR pairing window and produced this offer to show.
    QrOfferReady { offer: &'a QrOffer },
}

/// What the user, or another app on this device, asked for.
#[derive(Debug, Clone, Copy)]
pub enum AppCommand<'a> {
    /// Opens a pairing window: a QR offer to show, and nearby pairing alongside it.
    /// `None` keeps the configured duration.
    BeginPairing {
        duration: Option<Duration>,
    },
    CancelPairing,
    /// Pairs with the device showing this QR offer.
    PairWithQr {
        uri: &'a str,
    },
    /// Starts nearby pairing with a device advertising a pairing beacon at `address`.
    PairNearby {
        address: SocketAddr,
    },
    /// The user compared the code of a pairing request and accepted or rejected it.
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
        command: OutgoingCommand<'a>,
    },
}

/// A command for a peer to carry out on its side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutgoingCommand<'a> {
    Call(Call<'a>),
    Media(Media),
    Reply(Reply<'a>),
    SetVolume(SetVolume),
    Lock,
    SetHotspot(SetHotspot),
    FileOffer(FileOffer),
    UnicursorEnter(UnicursorEnter),
    UnicursorLeave(UnicursorLeave),
}

impl<'a> OutgoingCommand<'a> {
    pub(crate) const fn as_signal(self) -> Signal<'a> {
        match self {
            Self::Call(call) => Signal::CallV1(call),
            Self::Media(media) => Signal::MediaV1(media),
            Self::Reply(reply) => Signal::ReplyV1(reply),
            Self::SetVolume(volume) => Signal::SetVolumeV1(volume),
            Self::Lock => Signal::LockV1(Lock),
            Self::SetHotspot(hotspot) => Signal::SetHotspotV1(hotspot),
            Self::FileOffer(offer) => Signal::FileOfferV1(offer),
            Self::UnicursorEnter(enter) => Signal::UnicursorEnterV1(enter),
            Self::UnicursorLeave(leave) => Signal::UnicursorLeaveV1(leave),
        }
    }
}
