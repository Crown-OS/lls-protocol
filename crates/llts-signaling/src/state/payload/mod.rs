//! The value each [`Topic`] carries.

mod battery;
mod call_state;
mod clipboard;
mod device_info;
mod hotspot;
mod media_session;
mod notification;
mod volume;

pub use battery::Battery;
pub use call_state::{CallInfo, CallState, CallStatus};
pub use clipboard::{Clipboard, ClipboardContent, ClipboardOrder, MAX_INLINE_CLIPBOARD_BYTES};
pub use device_info::DeviceInfo;
pub use hotspot::Hotspot;
pub use media_session::MediaSession;
pub use notification::{Notification, NotificationAction, Notifications};
pub use volume::Volume;

use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::state::Topic;

/// A value that is the document for exactly one [`Topic`].
///
/// Tying the type to its topic means a document can never be published under, or decoded as,
/// the wrong one.
pub trait TopicPayload<'a>: Serialize + Deserialize<'a> {
    const TOPIC: Topic;
}

pub(crate) fn decode_payload<'a, P: TopicPayload<'a>>(
    bytes: &'a [u8],
) -> Result<P, SignalingError> {
    postcard::from_bytes(bytes).map_err(|source| SignalingError::MalformedPayload {
        topic: P::TOPIC,
        source,
    })
}

/// Checks that `bytes` decode as the document `topic` carries, for payloads encoded elsewhere.
pub(crate) fn validate_payload(topic: Topic, bytes: &[u8]) -> Result<(), SignalingError> {
    match topic {
        Topic::DeviceInfo => decode_payload::<DeviceInfo<'_>>(bytes).map(drop),
        Topic::Battery => decode_payload::<Battery>(bytes).map(drop),
        Topic::MediaSession => decode_payload::<Option<MediaSession<'_>>>(bytes).map(drop),
        Topic::CallState => decode_payload::<CallState<'_>>(bytes).map(drop),
        Topic::Notifications => decode_payload::<Notifications<'_>>(bytes).map(drop),
        Topic::Clipboard => decode_payload::<Clipboard<'_>>(bytes).map(drop),
        Topic::Volume => decode_payload::<Volume>(bytes).map(drop),
        Topic::Hotspot => decode_payload::<Hotspot<'_>>(bytes).map(drop),
    }
}
