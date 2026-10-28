use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    /// The platform's estimate, absent while charging or when it has none.
    pub time_to_empty_min: Option<u16>,
}

impl TopicPayload<'_> for Battery {
    const TOPIC: Topic = Topic::Battery;
}
