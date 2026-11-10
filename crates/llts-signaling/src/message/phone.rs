use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::{MAX_CURSOR_BYTES, MAX_INLINE_TEXT_BYTES};
use crate::message::{PageRequest, RequestId, Validate};
use crate::util::bounds;

tolerant_value! {
    /// What kind of call a log entry records, the platform's `TYPE` number; an unknown one is
    /// shown as "Other".
    CallKind(u8) {
        INCOMING = 1,
        OUTGOING = 2,
        MISSED = 3,
        VOICEMAIL = 4,
        REJECTED = 5,
        BLOCKED = 6,
        ANSWERED_EXTERNALLY = 7,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallLogEntry<'a> {
    pub entry_id: &'a str,
    pub number: &'a str,
    /// The name the phone cached when the call happened.
    #[serde(borrow)]
    pub name: Option<&'a str>,
    /// Only for a peer that is also allowed the phone's contacts.
    #[serde(borrow)]
    pub contact_key: Option<&'a str>,
    pub kind: CallKind,
    pub start_unix_ms: u64,
    pub duration_s: u32,
    pub is_new: bool,
    pub sim_slot: Option<u8>,
}

impl CallLogEntry<'_> {
    const fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("call log entry id", self.entry_id, MAX_CURSOR_BYTES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Voicemail<'a> {
    pub voicemail_id: &'a str,
    pub number: &'a str,
    #[serde(borrow)]
    pub name: Option<&'a str>,
    pub received_unix_ms: u64,
    pub duration_s: u32,
    pub heard: bool,
    #[serde(borrow)]
    pub transcript: Option<&'a str>,
    pub transcript_truncated: bool,
    /// Where to fetch the recording; `None` when the phone cannot read it.
    #[serde(borrow)]
    pub audio_key: Option<&'a str>,
}

impl Voicemail<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("voicemail id", self.voicemail_id, MAX_CURSOR_BYTES)?;
        bounds::optional_text("transcript", self.transcript, MAX_INLINE_TEXT_BYTES)?;
        bounds::optional_text("voicemail audio key", self.audio_key, MAX_CURSOR_BYTES)
    }
}

/// Asks for a page of the call log, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListCallLog<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub page: PageRequest<'a>,
}

impl Validate for ListCallLog<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallLogPage<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub items: Vec<CallLogEntry<'a>>,
    #[serde(borrow)]
    pub next: Option<&'a str>,
}

impl Validate for CallLogPage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.next, MAX_CURSOR_BYTES)?;
        self.items.iter().try_for_each(CallLogEntry::validate)
    }
}

/// Asks for a page of voicemails, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListVoicemail<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub page: PageRequest<'a>,
}

impl Validate for ListVoicemail<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VoicemailPage<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub items: Vec<Voicemail<'a>>,
    #[serde(borrow)]
    pub next: Option<&'a str>,
}

impl Validate for VoicemailPage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.next, MAX_CURSOR_BYTES)?;
        self.items.iter().try_for_each(Voicemail::validate)
    }
}
