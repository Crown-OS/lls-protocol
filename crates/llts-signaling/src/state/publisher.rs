use core::mem;

use crate::SignalingError;
use crate::message::{Snapshot, Update};
use crate::state::document::Document;
use crate::state::payload::{decode_payload, validate_payload};
use crate::state::topic_map::TopicMap;
use crate::state::{Incarnation, Topic, TopicPayload, TopicSet, Version};

/// What [`StatePublisher::publish`] did with a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Published {
    /// The document changed and now carries this version; subscribers need an update.
    Changed(Version),
    /// The value encodes to exactly what was already published, so nothing needs to go out.
    Unchanged,
}

/// The documents one device is authoritative for.
///
/// Every write draws the next sequence number of this incarnation, so versions are unique across
/// topics and a peer can never confuse two writes.
#[derive(Debug)]
pub struct StatePublisher {
    incarnation: Incarnation,
    seq: u64,
    documents: TopicMap<Document>,
    scratch: Vec<u8>,
}

impl StatePublisher {
    pub fn new(incarnation: Incarnation) -> Self {
        Self {
            incarnation,
            seq: 0,
            documents: TopicMap::default(),
            scratch: Vec::new(),
        }
    }

    pub const fn incarnation(&self) -> Incarnation {
        self.incarnation
    }

    /// Replaces this device's document for `P::TOPIC`.
    ///
    /// # Errors
    ///
    /// Returns [`SignalingError::Encode`] if the value cannot be encoded.
    pub fn publish<'a, P: TopicPayload<'a>>(
        &mut self,
        value: &P,
    ) -> Result<Published, SignalingError> {
        self.scratch.clear();
        let encoded = postcard::to_extend(value, mem::take(&mut self.scratch))
            .map_err(SignalingError::Encode)?;

        let slot = self.documents.slot(P::TOPIC);
        if slot
            .as_ref()
            .is_some_and(|document| document.payload == encoded)
        {
            self.scratch = encoded;
            return Ok(Published::Unchanged);
        }

        self.seq += 1;
        let version = Version {
            incarnation: self.incarnation,
            seq: self.seq,
        };
        match slot {
            Some(document) => {
                document.version = version;
                self.scratch = mem::replace(&mut document.payload, encoded);
            }
            None => {
                *slot = Some(Document {
                    version,
                    payload: encoded,
                });
            }
        }
        Ok(Published::Changed(version))
    }

    /// Replaces this device's document for `topic` with a payload encoded elsewhere, such as by
    /// a platform binding that builds the value on its own side of an FFI boundary.
    ///
    /// # Errors
    ///
    /// Returns [`SignalingError::MalformedPayload`] if `payload` is not a valid document for
    /// `topic`, so a peer is never sent something it cannot decode.
    pub fn publish_encoded(
        &mut self,
        topic: Topic,
        payload: &[u8],
    ) -> Result<Published, SignalingError> {
        validate_payload(topic, payload)?;
        let slot = self.documents.slot(topic);
        if slot
            .as_ref()
            .is_some_and(|document| document.payload == payload)
        {
            return Ok(Published::Unchanged);
        }
        self.seq += 1;
        let version = Version {
            incarnation: self.incarnation,
            seq: self.seq,
        };
        Document::store(slot, version, payload);
        Ok(Published::Changed(version))
    }

    /// Decodes this device's own document for `P::TOPIC`.
    ///
    /// # Errors
    ///
    /// Returns [`SignalingError::MalformedPayload`] only if `P` changed shape since publishing.
    pub fn get<'p, P: TopicPayload<'p>>(&'p self) -> Result<Option<P>, SignalingError> {
        self.documents
            .get(P::TOPIC)
            .map(|document| decode_payload(&document.payload))
            .transpose()
    }

    pub fn update(&self, topic: Topic) -> Option<Update<'_>> {
        self.documents
            .get(topic)
            .map(|document| document.as_update(topic))
    }

    /// Everything a new subscriber to `topics` needs, including which of them are empty.
    pub fn snapshot(&self, topics: TopicSet) -> Snapshot<'_> {
        Snapshot {
            topics,
            updates: topics
                .iter()
                .filter_map(|topic| self.update(topic))
                .collect(),
        }
    }

    pub fn topics(&self) -> TopicSet {
        self.documents.iter().map(|(topic, _)| topic).collect()
    }
}
