use crate::message::{Snapshot, Update};
use crate::state::{StatePublisher, Topic, TopicSet};

/// What one peer session still owes its subscriber.
///
/// Only a set of topics is queued, never the values: however often a topic changes between
/// flushes, the subscriber receives one update carrying the latest document.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateOutbox {
    subscribed: TopicSet,
    pending: TopicSet,
}

impl StateOutbox {
    pub const fn new() -> Self {
        Self {
            subscribed: TopicSet::EMPTY,
            pending: TopicSet::EMPTY,
        }
    }

    pub const fn subscribed(&self) -> TopicSet {
        self.subscribed
    }

    /// Records a subscription and returns the snapshot to answer it with, which supersedes
    /// anything pending for those topics.
    pub fn subscribe<'p>(
        &mut self,
        topics: TopicSet,
        publisher: &'p StatePublisher,
    ) -> Snapshot<'p> {
        self.subscribed = self.subscribed.union(topics);
        self.pending = self.pending.difference(topics);
        publisher.snapshot(topics)
    }

    pub const fn unsubscribe(&mut self, topics: TopicSet) {
        self.subscribed = self.subscribed.difference(topics);
        self.pending = self.pending.difference(topics);
    }

    pub const fn mark_changed(&mut self, topic: Topic) {
        if self.subscribed.contains(topic) {
            self.pending = self.pending.with(topic);
        }
    }

    pub const fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Takes every pending topic, yielding the publisher's current document for each.
    pub fn drain<'p>(
        &mut self,
        publisher: &'p StatePublisher,
    ) -> impl Iterator<Item = Update<'p>> + use<'p> {
        let pending = core::mem::take(&mut self.pending);
        pending
            .iter()
            .filter_map(move |topic| publisher.update(topic))
    }
}
