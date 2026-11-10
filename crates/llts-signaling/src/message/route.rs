//! Which lane each application message travels on and which feature lets it through.
//!
//! The node is a gate and a router for these messages, not an interpreter: it checks the
//! session, the lane and the feature, then hands the envelope to the platform untouched.
//!
//! # Enum tolerance rule
//!
//! An unknown variant inside a body fails [`Signal::decode`](crate::message::Signal::decode)
//! and drops the whole message, while an unknown message *type* is merely skipped. So inside an
//! application body, every value whose set may grow — platform integers, error and reason
//! codes, kinds, capability bits — is a `#[serde(transparent)]` integer with named constants,
//! which a receiver maps to a documented fallback. The few real enums in these bodies
//! (`MessageChange`, `BlobPurpose`, `BlobRole`, `RingAction`, `FocusMode`) are frozen. Adding a
//! variant to a frozen enum, or a meaning to a tolerant value that older receivers would
//! misread, takes a new message type.

use crate::device::Feature;
use crate::message::{MessageType, MuxClass, StreamRef};

/// The reliable stream an application message travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Requests and commands, small and latency-sensitive.
    Control,
    /// Pages, pushes and answers, which may be large.
    Bulk,
    /// Blobs and everything that must stay ordered with them.
    Blob,
}

impl Lane {
    pub const fn stream(self) -> StreamRef {
        let (class, index) = match self {
            Self::Control => (MuxClass::Control, 0),
            Self::Bulk => (MuxClass::Data, 1),
            Self::Blob => (MuxClass::Data, 2),
        };
        StreamRef { class, index }
    }
}

/// What must be shared with a peer for a message to cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gate {
    /// Nothing beyond the session: answers, cancellations and blob plumbing, whose platform
    /// tables bind them to the feature of their request.
    Open,
    Feature(Feature),
    /// The feature named in the body, as `FetchBlob` carries it.
    FromBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppRoute {
    pub lane: Lane,
    pub gate: Gate,
}

impl AppRoute {
    const fn control(feature: Feature) -> Self {
        Self {
            lane: Lane::Control,
            gate: Gate::Feature(feature),
        }
    }

    const fn bulk(feature: Feature) -> Self {
        Self {
            lane: Lane::Bulk,
            gate: Gate::Feature(feature),
        }
    }

    const fn open(lane: Lane) -> Self {
        Self {
            lane,
            gate: Gate::Open,
        }
    }
}

impl MessageType {
    /// `Some` for application messages the node routes without interpreting; `None` for
    /// everything the node handles itself: session, streams, state, the original commands and
    /// the feature requests it negotiates.
    pub const fn app_route(self) -> Option<AppRoute> {
        use Feature as F;
        Some(match self {
            Self::OpenUriV1 => AppRoute::control(F::Links),
            Self::LiveActivityActionV1 => AppRoute::control(F::LiveActivities),
            Self::RingV1 => AppRoute::control(F::FindMy),
            Self::WifiCredentialsV1 => AppRoute::control(F::WifiSharing),
            Self::TakeHeadsetV1 | Self::ReleaseHeadsetV1 => AppRoute::control(F::AudioHandoff),
            Self::FileListeningV1 | Self::FilePushV1 => AppRoute::control(F::Files),
            Self::DragEnterV1 | Self::DragDropV1 | Self::DragCancelV1 => {
                AppRoute::control(F::DragDrop)
            }
            Self::RequestFailedV1 | Self::RequestDoneV1 => AppRoute::open(Lane::Bulk),
            Self::ListConversationsV1 | Self::ListMessagesV1 | Self::MarkReadV1 => {
                AppRoute::control(F::Messages)
            }
            Self::ConversationPageV1
            | Self::MessagePageV1
            | Self::MessageEventV1
            | Self::MessageCountersV1 => AppRoute::bulk(F::Messages),
            Self::SendMessageV1 => AppRoute {
                lane: Lane::Blob,
                gate: Gate::Feature(F::Messages),
            },
            Self::ListCallLogV1 | Self::ListVoicemailV1 => AppRoute::control(F::CallHistory),
            Self::CallLogPageV1 | Self::VoicemailPageV1 | Self::CallCountersV1 => {
                AppRoute::bulk(F::CallHistory)
            }
            Self::ListContactsV1 => AppRoute::control(F::Contacts),
            Self::ContactPageV1 | Self::ContactCountersV1 => AppRoute::bulk(F::Contacts),
            Self::FetchBlobV1 => AppRoute {
                lane: Lane::Control,
                gate: Gate::FromBody,
            },
            Self::CaptureV1 => AppRoute::control(F::Capture),
            Self::LocateV1 => AppRoute::control(F::FindMy),
            Self::LocationV1 => AppRoute::bulk(F::FindMy),
            Self::WifiRequestV1 => AppRoute::control(F::WifiSharing),
            Self::RequestCancelV1 => AppRoute::open(Lane::Control),
            Self::BlobStartV1
            | Self::BlobChunkV1
            | Self::BlobEndV1
            | Self::BlobAckV1
            | Self::BlobCancelV1 => AppRoute::open(Lane::Blob),
            Self::CapabilitiesV1
            | Self::GoodbyeV1
            | Self::RekeyV1
            | Self::KeyframeRequestV1
            | Self::OpenStreamV1
            | Self::AcceptStreamV1
            | Self::CloseStreamV1
            | Self::SubscribeV1
            | Self::UnsubscribeV1
            | Self::UpdateV1
            | Self::SnapshotV1
            | Self::CallV1
            | Self::MediaV1
            | Self::ReplyV1
            | Self::SetVolumeV1
            | Self::LockV1
            | Self::SetHotspotV1
            | Self::FileOfferV1
            | Self::StartMirrorV1
            | Self::StartCameraV1
            | Self::StartMicV1
            | Self::StartMonitorV1
            | Self::UnicursorEnterV1
            | Self::UnicursorLeaveV1
            | Self::StopV1
            | Self::StartCastV1
            | Self::StartAudioV1 => return None,
        })
    }
}
