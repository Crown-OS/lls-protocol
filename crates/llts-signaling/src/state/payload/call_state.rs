use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

/// Where a call stands, matching the `crownconnect` IPC `CallStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallStatus {
    Ringing,
    Dialing,
    Active,
    Held,
    Ended,
}

/// One call on a device's phone line, matching the `crownconnect` IPC `CallInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallInfo<'a> {
    pub call_id: &'a str,
    pub number: &'a str,
    #[serde(borrow)]
    pub contact_name: Option<&'a str>,
    pub status: CallStatus,
    pub answered_unix_ms: Option<u64>,
}

/// Every call the device currently has, usually none or one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallState<'a> {
    #[serde(borrow)]
    pub calls: Vec<CallInfo<'a>>,
}

impl<'a> TopicPayload<'a> for CallState<'a> {
    const TOPIC: Topic = Topic::CallState;
}
