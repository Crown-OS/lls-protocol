use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

/// The device's main output volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Volume {
    pub percent: u8,
    pub muted: bool,
}

impl TopicPayload<'_> for Volume {
    const TOPIC: Topic = Topic::Volume;
}
