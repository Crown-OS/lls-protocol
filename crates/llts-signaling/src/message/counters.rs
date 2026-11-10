use serde::{Deserialize, Serialize};

use crate::message::Validate;

tolerant_bits! {
    MessageCapabilities { SEND_SMS = 0, SEND_MMS = 1, MARK_READ = 2 }
}

tolerant_bits! {
    CallCapabilities { VOICEMAIL_AUDIO = 0, VOICEMAIL_TRANSCRIPTS = 1 }
}

tolerant_bits! {
    ContactCapabilities { PHOTOS = 0 }
}

/// The phone's messages at a glance, pushed whenever they change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageCounters {
    /// Changes whenever any conversation does, so a receiver knows its pages are stale.
    pub revision: u64,
    pub unread: u32,
    pub capabilities: MessageCapabilities,
    /// The carrier's MMS size limit; `0` when unknown.
    pub mms_max_bytes: u32,
}

impl Validate for MessageCounters {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallCounters {
    pub call_log_revision: u64,
    pub voicemail_revision: u64,
    pub missed: u32,
    pub unheard_voicemails: u32,
    pub capabilities: CallCapabilities,
}

impl Validate for CallCounters {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContactCounters {
    pub revision: u64,
    pub count: u32,
    pub capabilities: ContactCapabilities,
}

impl Validate for ContactCounters {}
