use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

/// What a device is playing. The topic's document is `Option<MediaSession>`, `None` meaning
/// nothing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaSession<'a> {
    pub app: &'a str,
    pub title: &'a str,
    pub artist: &'a str,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
}

impl<'a> TopicPayload<'a> for Option<MediaSession<'a>> {
    const TOPIC: Topic = Topic::MediaSession;
}
