use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::device::DeviceId;
use crate::message::FfspTransferId;
use crate::state::{HybridTimestamp, Topic, TopicPayload};

/// The largest clipboard carried inline on the Data channel; anything larger is offered over
/// ffsp instead.
///
/// Four KiB below the 64 KiB reliable-message limit, so the envelope, version and MIME type fit
/// around a full clipboard in one message.
pub const MAX_INLINE_CLIPBOARD_BYTES: usize = 60 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClipboardContent<'a> {
    Inline(#[serde(serialize_with = "crate::util::serde_bytes::serialize")] &'a [u8]),
    /// Too large to inline: the peer fetches it with this ffsp transfer.
    Offer {
        ffsp_transfer_id: FfspTransferId,
        len: u64,
    },
}

impl<'a> ClipboardContent<'a> {
    /// # Errors
    ///
    /// Returns [`SignalingError::ClipboardTooLarge`] above [`MAX_INLINE_CLIPBOARD_BYTES`].
    pub const fn inline(bytes: &'a [u8]) -> Result<Self, SignalingError> {
        if bytes.len() > MAX_INLINE_CLIPBOARD_BYTES {
            return Err(SignalingError::ClipboardTooLarge {
                len: bytes.len(),
                limit: MAX_INLINE_CLIPBOARD_BYTES,
            });
        }
        Ok(Self::Inline(bytes))
    }
}

/// What the device last copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Clipboard<'a> {
    pub mime: &'a str,
    #[serde(borrow)]
    pub content: ClipboardContent<'a>,
    /// When it was copied, on the copying device's hybrid logical clock.
    pub stamp: HybridTimestamp,
}

/// Orders clipboards across devices: the latest copy wins, and two copies stamped identically
/// are settled by device id so every device picks the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClipboardOrder {
    pub stamp: HybridTimestamp,
    pub origin: DeviceId,
}

impl Clipboard<'_> {
    pub const fn order(&self, origin: DeviceId) -> ClipboardOrder {
        ClipboardOrder {
            stamp: self.stamp,
            origin,
        }
    }
}

impl<'a> TopicPayload<'a> for Clipboard<'a> {
    const TOPIC: Topic = Topic::Clipboard;
}
