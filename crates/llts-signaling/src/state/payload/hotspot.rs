use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

/// The device's mobile hotspot, so a laptop can offer to join it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotspot<'a> {
    pub enabled: bool,
    #[serde(borrow)]
    pub ssid: Option<&'a str>,
}

impl<'a> TopicPayload<'a> for Hotspot<'a> {
    const TOPIC: Topic = Topic::Hotspot;
}
