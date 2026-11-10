//! Control messages carried on the llts reliable Control and Data channels.
//!
//! Each channel message is one envelope: a little-endian `u16` [`MessageType`] followed by the
//! postcard body. A message's layout never changes once it ships. Changing one means registering
//! a new type (`StartMirrorV2` beside `StartMirrorV1`) and keeping the old one until every peer
//! has moved on. A type this build does not know decodes to [`Signal::Unknown`] instead of an
//! error, so a newer peer's messages are skipped rather than tearing the session down.

#[macro_use]
mod tolerant;

pub mod limits;

mod blob;
mod blob_key;
mod capture;
mod codec;
mod command;
mod contacts;
mod counters;
mod drag;
mod envelope;
mod feature;
mod files;
mod find;
mod headset;
mod links;
mod messages;
mod options;
mod phone;
mod registry;
mod request;
mod route;
mod session;
mod state;
mod stream;
mod stream_ref;
mod validate;
mod wifi;

pub use blob::{
    BlobAck, BlobCancel, BlobCancelReason, BlobChunk, BlobEnd, BlobId, BlobPurpose, BlobRole,
    BlobStart, FetchBlob,
};
pub use blob_key::BlobKey;
pub use capture::{Capture, CaptureKind};
pub use codec::{AudioCodec, CodecList, MAX_CODECS, StreamCodec, VideoCodec, negotiate_codec};
pub use command::{Call, FfspTransferId, FileOffer, Lock, Media, Reply, SetHotspot, SetVolume};
pub use contacts::{Contact, ContactPage, ContactPhone, ListContacts, PhoneKind};
pub use counters::{
    CallCapabilities, CallCounters, ContactCapabilities, ContactCounters, MessageCapabilities,
    MessageCounters,
};
pub use drag::{DragCancel, DragDrop, DragEnter, DragId, DragItem, DragItemKind};
pub use envelope::{ENVELOPE_HEADER_LEN, Envelope, Message};
pub use feature::{
    StartAudio, StartCamera, StartCast, StartMic, StartMirror, StartMonitor, Stop, UnicursorEnter,
    UnicursorLeave,
};
pub use files::{FileListening, FilePush};
pub use find::{Locate, Location, Ring, RingAction};
pub use headset::{ReleaseHeadset, TakeHeadset};
pub use links::{LiveActivityAction, OpenUri};
pub use messages::{
    Attachment, AttachmentSpec, Conversation, ConversationPage, DeliveryStatus, ListConversations,
    ListMessages, MarkRead, MessageChange, MessageEvent, MessageFolder, MessageItem, MessagePage,
    Participant, SendMessage,
};
pub use options::{CameraLens, Direction, Edge, EdgePosition, VideoLimits};
pub use phone::{
    CallKind, CallLogEntry, CallLogPage, ListCallLog, ListVoicemail, Voicemail, VoicemailPage,
};
pub use registry::{FIRST_APPLICATION_TYPE, MessageType, Signal};
pub use request::{
    PageRequest, RequestCancel, RequestDone, RequestError, RequestFailed, RequestId,
};
pub use route::{AppRoute, Gate, Lane};
pub use session::{Capabilities, Goodbye, GoodbyeReason, KeyframeRequest, Rekey};
pub use state::{Snapshot, Subscribe, Unsubscribe, Update};
pub use stream::{AcceptStream, AudioParams, CloseStream, OpenStream, StreamParams, VideoParams};
pub use stream_ref::{MuxClass, StreamRef};
pub use validate::Validate;
pub use wifi::{WifiCredentials, WifiRequest, WifiSecurity};
