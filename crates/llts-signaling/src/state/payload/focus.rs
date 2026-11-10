use serde::{Deserialize, Serialize};

use crate::state::{HybridTimestamp, Topic, TopicPayload};

/// Which focus a device is in.
///
/// Frozen: a new kind of mode needs a new topic payload version, because an older peer would
/// fail to decode the whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FocusMode<'a> {
    Off,
    DoNotDisturb,
    Named(&'a str),
}

/// The device's focus, ordered across devices like clipboards: the newest stamp wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Focus<'a> {
    #[serde(borrow)]
    pub mode: FocusMode<'a>,
    pub until_unix_ms: Option<u64>,
    pub stamp: HybridTimestamp,
}

impl<'a> TopicPayload<'a> for Focus<'a> {
    const TOPIC: Topic = Topic::Focus;
}
