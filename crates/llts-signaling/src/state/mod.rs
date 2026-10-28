//! Single-writer, versioned state that every device publishes about itself.
//!
//! Each device is authoritative for its own documents, one per [`Topic`]. Peers subscribe and
//! keep a [`StateCache`]; nothing is merged, because no two devices ever write the same document.
//! A [`Version`] pairs a random per-process [`Incarnation`] with a sequence number, so a restarted
//! owner replaces what peers cached instead of being ignored for having a lower sequence.
//! Clipboards are the one cross-device question ("what did the user copy last?"), answered by
//! ordering each owner's clipboard on a [`HybridTimestamp`] with the device id as tiebreaker.

mod cache;
mod document;
mod outbox;
mod payload;
mod publisher;
mod topic;
mod topic_map;
mod version;

pub use crate::util::hlc::{HybridClock, HybridTimestamp};
pub use cache::{Applied, StateCache};
pub use outbox::StateOutbox;
pub use payload::{
    Battery, CallInfo, CallState, CallStatus, Clipboard, ClipboardContent, ClipboardOrder,
    DeviceInfo, Hotspot, MAX_INLINE_CLIPBOARD_BYTES, MediaSession, Notification,
    NotificationAction, Notifications, TopicPayload, Volume,
};
pub use publisher::{Published, StatePublisher};
pub use topic::{Topic, TopicSet};
pub use version::{Incarnation, Version};
