use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicSet, Version};

/// Asks the receiving device to publish `topics` to the sender, starting with a [`Snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Subscribe {
    pub topics: TopicSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Unsubscribe {
    pub topics: TopicSet,
}

/// The sender's own document for one topic.
///
/// The payload stays postcard-encoded inside the message, so a cache can store it as received
/// and decode it only when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Update<'a> {
    pub topic: Topic,
    pub version: Version,
    #[serde(serialize_with = "crate::util::serde_bytes::serialize")]
    pub payload: &'a [u8],
}

/// Every subscribed document at once, answering a [`Subscribe`].
///
/// Topics in `topics` with no update are ones the sender has nothing for, which clears any copy
/// the receiver kept from an earlier session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Snapshot<'a> {
    pub topics: TopicSet,
    #[serde(borrow)]
    pub updates: Vec<Update<'a>>,
}
