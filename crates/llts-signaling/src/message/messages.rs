use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::{
    MAX_ATTACHMENTS_PER_ITEM, MAX_BLOB_BYTES, MAX_CURSOR_BYTES, MAX_DRAG_NAME_BYTES,
    MAX_INLINE_TEXT_BYTES, MAX_MIME_BYTES, MAX_PARTICIPANTS, MAX_SEND_TEXT_BYTES,
    MAX_SNIPPET_BYTES,
};
use crate::message::{BlobId, PageRequest, RequestId, Validate};
use crate::util::bounds;

/// Someone in a conversation. `name` and `contact_key` are only filled in for a peer that is
/// also allowed the phone's contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Participant<'a> {
    pub address: &'a str,
    #[serde(borrow)]
    pub name: Option<&'a str>,
    #[serde(borrow)]
    pub contact_key: Option<&'a str>,
}

/// One conversation of the phone, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Conversation<'a> {
    pub thread_id: u64,
    #[serde(borrow)]
    pub participants: Vec<Participant<'a>>,
    /// The real number of participants, which `participants` may fall short of.
    pub participant_count: u16,
    pub snippet: &'a str,
    pub last_unix_ms: u64,
    pub unread: u32,
    pub has_attachments: bool,
}

impl Conversation<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::count("participants", self.participants.len(), MAX_PARTICIPANTS)?;
        bounds::text("snippet", self.snippet, MAX_SNIPPET_BYTES)
    }
}

tolerant_value! {
    /// Which box a message sits in, the platform's `TYPE_*` number; an unknown one is shown as
    /// received or sent by its `sender`.
    MessageFolder(u8) {
        INBOX = 1,
        SENT = 2,
        DRAFT = 3,
        OUTBOX = 4,
        FAILED = 5,
        QUEUED = 6,
    }
}

tolerant_value! {
    /// How far a sent message got; an unknown value reads as [`Self::NONE`].
    DeliveryStatus(u8) {
        NONE = 0,
        PENDING = 1,
        SENT = 2,
        DELIVERED = 3,
        FAILED = 4,
    }
}

/// A part of a message that is not its text, fetched on demand by `key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Attachment<'a> {
    pub key: &'a str,
    pub mime: &'a str,
    #[serde(borrow)]
    pub name: Option<&'a str>,
    pub len: u64,
    #[serde(borrow)]
    pub thumbnail_key: Option<&'a str>,
    /// `0` when unknown.
    pub width: u16,
    /// `0` when unknown.
    pub height: u16,
}

impl Attachment<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("attachment key", self.key, MAX_CURSOR_BYTES)?;
        bounds::text("attachment mime", self.mime, MAX_MIME_BYTES)?;
        bounds::optional_text("attachment name", self.name, MAX_DRAG_NAME_BYTES)?;
        bounds::optional_text("thumbnail key", self.thumbnail_key, MAX_CURSOR_BYTES)
    }
}

/// One attachment of a message being sent, which follows the request as blob `blob`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttachmentSpec<'a> {
    pub blob: BlobId,
    pub len: u64,
    pub mime: &'a str,
}

/// One SMS or MMS.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageItem<'a> {
    pub thread_id: u64,
    /// `sms/<id>`, `mms/<id>`, or `pending/<request>` for one not yet in the provider.
    pub message_id: &'a str,
    pub folder: MessageFolder,
    /// `None` when this phone sent it.
    #[serde(borrow)]
    pub sender: Option<&'a str>,
    pub unix_ms: u64,
    #[serde(borrow)]
    pub text: Option<&'a str>,
    /// The text was cut to the inline limit; the rest is fetched as a blob.
    pub text_truncated: bool,
    pub read: bool,
    pub status: DeliveryStatus,
    #[serde(borrow)]
    pub attachments: Vec<Attachment<'a>>,
    /// Attachments left out to keep to the per-item cap.
    pub attachments_omitted: u16,
    pub sim_slot: Option<u8>,
}

impl MessageItem<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("message id", self.message_id, MAX_CURSOR_BYTES)?;
        bounds::optional_text("message text", self.text, MAX_INLINE_TEXT_BYTES)?;
        bounds::count(
            "attachments",
            self.attachments.len(),
            MAX_ATTACHMENTS_PER_ITEM,
        )?;
        self.attachments.iter().try_for_each(Attachment::validate)
    }
}

/// A change to the phone's messages. Frozen: a new kind of change needs a new message type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageChange<'a> {
    #[serde(borrow)]
    Added(MessageItem<'a>),
    #[serde(borrow)]
    Updated(MessageItem<'a>),
    Deleted {
        thread_id: u64,
        message_id: &'a str,
    },
}

/// Asks for a page of conversations, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListConversations<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub page: PageRequest<'a>,
}

impl Validate for ListConversations<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationPage<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub items: Vec<Conversation<'a>>,
    /// The cursor of the next page; `None` at the end.
    #[serde(borrow)]
    pub next: Option<&'a str>,
}

impl Validate for ConversationPage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.next, MAX_CURSOR_BYTES)?;
        self.items.iter().try_for_each(Conversation::validate)
    }
}

/// Asks for a page of one conversation's messages, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListMessages<'a> {
    pub request: RequestId,
    pub thread_id: u64,
    #[serde(borrow)]
    pub page: PageRequest<'a>,
}

impl Validate for ListMessages<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessagePage<'a> {
    pub request: RequestId,
    pub thread_id: u64,
    #[serde(borrow)]
    pub items: Vec<MessageItem<'a>>,
    #[serde(borrow)]
    pub next: Option<&'a str>,
}

impl Validate for MessagePage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.next, MAX_CURSOR_BYTES)?;
        self.items.iter().try_for_each(MessageItem::validate)
    }
}

/// Sends a message from the phone, into `thread_id` or to `recipients`; its attachments
/// follow as blobs on the same lane.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SendMessage<'a> {
    pub request: RequestId,
    pub thread_id: Option<u64>,
    #[serde(borrow)]
    pub recipients: Vec<&'a str>,
    pub text: &'a str,
    #[serde(borrow)]
    pub attachments: Vec<AttachmentSpec<'a>>,
    pub sim_slot: Option<u8>,
}

impl Validate for SendMessage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        if self.thread_id.is_none() && self.recipients.is_empty() {
            return Err(SignalingError::OutOfRange {
                field: "recipients",
            });
        }
        bounds::count("recipients", self.recipients.len(), MAX_PARTICIPANTS)?;
        bounds::text("message text", self.text, MAX_SEND_TEXT_BYTES)?;
        bounds::count(
            "attachments",
            self.attachments.len(),
            MAX_ATTACHMENTS_PER_ITEM,
        )?;
        self.attachments.iter().try_for_each(|attachment| {
            if attachment.len > MAX_BLOB_BYTES {
                return Err(SignalingError::OutOfRange {
                    field: "attachment length",
                });
            }
            bounds::text("attachment mime", attachment.mime, MAX_MIME_BYTES)
        })
    }
}

/// Marks every message of `thread_id` read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarkRead {
    pub request: RequestId,
    pub thread_id: u64,
}

impl Validate for MarkRead {}

/// A message arrived, changed or went away on the phone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageEvent<'a> {
    #[serde(borrow)]
    pub change: MessageChange<'a>,
}

impl Validate for MessageEvent<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        match &self.change {
            MessageChange::Added(item) | MessageChange::Updated(item) => item.validate(),
            MessageChange::Deleted { message_id, .. } => {
                bounds::text("message id", message_id, MAX_CURSOR_BYTES)
            }
        }
    }
}
