use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::state::{Topic, TopicPayload};
use crate::util::bounds;

/// The most headsets one [`AudioRoutes`] document lists.
pub const MAX_HEADSETS: usize = 16;

/// A bonded Bluetooth audio sink (A2DP or HFP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Headset<'a> {
    /// Uppercase `AA:BB:CC:DD:EE:FF`.
    pub address: &'a str,
    pub name: &'a str,
    pub connected: bool,
}

/// The audio sinks a device could hand over, and whether each is connected to it now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioRoutes<'a> {
    #[serde(borrow)]
    pub headsets: Vec<Headset<'a>>,
}

impl AudioRoutes<'_> {
    /// # Errors
    ///
    /// Returns [`SignalingError::TooManyEntries`] above [`MAX_HEADSETS`].
    pub const fn validate(&self) -> Result<(), SignalingError> {
        bounds::count("headsets", self.headsets.len(), MAX_HEADSETS)
    }
}

impl<'a> TopicPayload<'a> for AudioRoutes<'a> {
    const TOPIC: Topic = Topic::AudioRoutes;
}
