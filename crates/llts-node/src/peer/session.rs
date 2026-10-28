use llts_signaling::device::FeatureSet;
use llts_signaling::message::Capabilities;
use llts_signaling::state::{StateOutbox, TopicSet};

use crate::feature::{ActiveFeatures, PendingStarts};

/// What one live session with a peer knows beyond the link itself.
#[derive(Debug, Default)]
pub(crate) struct Session {
    /// The peer's announcement, once it arrived.
    pub(crate) capabilities: Option<Capabilities>,
    /// Topics this device asked the peer to publish to it.
    pub(crate) subscribed: TopicSet,
    /// What this device still owes the peer's subscription.
    pub(crate) outbox: StateOutbox,
    pub(crate) features: ActiveFeatures,
    /// Starts asked for before [`capabilities`](Self::capabilities) arrived.
    pub(crate) pending_starts: PendingStarts,
    /// The session began with a pairing whose confirmation the peer may not have finished, so
    /// the first announcements this device sent may have been dropped there.
    pub(crate) just_paired: bool,
}

impl Session {
    pub(crate) fn peer_features(&self) -> FeatureSet {
        self.capabilities
            .as_ref()
            .map_or(FeatureSet::EMPTY, |capabilities| capabilities.features)
    }
}
