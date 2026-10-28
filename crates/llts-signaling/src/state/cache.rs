use std::collections::HashMap;

use crate::SignalingError;
use crate::device::DeviceId;
use crate::message::{Snapshot, Update};
use crate::state::document::Document;
use crate::state::payload::decode_payload;
use crate::state::topic_map::TopicMap;
use crate::state::{Clipboard, Topic, TopicPayload, Version};

/// Whether a received update was taken into the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Applied {
    Accepted,
    /// The cache already holds this write or a later one from the same incarnation.
    Stale,
}

/// The latest copy of every peer's documents, as their owners last published them.
#[derive(Debug, Default)]
pub struct StateCache {
    owners: HashMap<DeviceId, TopicMap<Document>>,
}

impl StateCache {
    pub fn apply_update(&mut self, owner: DeviceId, update: &Update<'_>) -> Applied {
        let slot = self.owners.entry(owner).or_default().slot(update.topic);
        if slot
            .as_ref()
            .is_some_and(|cached| !update.version.supersedes(cached.version))
        {
            return Applied::Stale;
        }
        Document::store(slot, update.version, update.payload);
        Applied::Accepted
    }

    /// Replaces every topic the snapshot covers, clearing the ones it reports empty.
    ///
    /// A snapshot opens a session, so it is authoritative regardless of what was cached before.
    pub fn apply_snapshot(&mut self, owner: DeviceId, snapshot: &Snapshot<'_>) {
        let documents = self.owners.entry(owner).or_default();
        for topic in snapshot.topics.iter() {
            let slot = documents.slot(topic);
            match snapshot.updates.iter().find(|update| update.topic == topic) {
                Some(update) => Document::store(slot, update.version, update.payload),
                None => *slot = None,
            }
        }
    }

    /// Drops everything cached for a device, for when it is unpaired.
    pub fn forget(&mut self, owner: &DeviceId) -> bool {
        self.owners.remove(owner).is_some()
    }

    pub fn owners(&self) -> impl Iterator<Item = &DeviceId> {
        self.owners.keys()
    }

    pub fn version(&self, owner: &DeviceId, topic: Topic) -> Option<Version> {
        self.document(owner, topic).map(|document| document.version)
    }

    pub fn update(&self, owner: &DeviceId, topic: Topic) -> Option<Update<'_>> {
        self.document(owner, topic)
            .map(|document| document.as_update(topic))
    }

    /// Decodes one owner's document, borrowing its strings and bytes from the cache.
    ///
    /// # Errors
    ///
    /// Returns [`SignalingError::MalformedPayload`] if the owner published something this build
    /// cannot decode.
    pub fn get<'c, P: TopicPayload<'c>>(
        &'c self,
        owner: &DeviceId,
    ) -> Result<Option<P>, SignalingError> {
        self.document(owner, P::TOPIC)
            .map(|document| decode_payload(&document.payload))
            .transpose()
    }

    /// The clipboard copied most recently on any device, and which device copied it.
    pub fn newest_clipboard(&self) -> Option<(DeviceId, Clipboard<'_>)> {
        self.owners
            .iter()
            .filter_map(|(owner, documents)| {
                let document = documents.get(Topic::Clipboard)?;
                let clipboard = decode_payload::<Clipboard<'_>>(&document.payload).ok()?;
                Some((*owner, clipboard))
            })
            .max_by_key(|(owner, clipboard)| clipboard.order(*owner))
    }

    fn document(&self, owner: &DeviceId, topic: Topic) -> Option<&Document> {
        self.owners.get(owner)?.get(topic)
    }
}
