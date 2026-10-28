use crate::message::Update;
use crate::state::{Topic, Version};

/// A document as a publisher or cache holds it: the postcard payload, kept encoded so it can be
/// forwarded without re-encoding and decoded on demand without copying its strings.
#[derive(Debug, Clone)]
pub(crate) struct Document {
    pub(crate) version: Version,
    pub(crate) payload: Vec<u8>,
}

impl Document {
    pub(crate) fn as_update(&self, topic: Topic) -> Update<'_> {
        Update {
            topic,
            version: self.version,
            payload: &self.payload,
        }
    }

    pub(crate) fn overwrite(&mut self, version: Version, payload: &[u8]) {
        self.version = version;
        self.payload.clear();
        self.payload.extend_from_slice(payload);
    }

    pub(crate) fn store(slot: &mut Option<Self>, version: Version, payload: &[u8]) {
        match slot {
            Some(document) => document.overwrite(version, payload),
            None => {
                *slot = Some(Self {
                    version,
                    payload: payload.to_vec(),
                });
            }
        }
    }
}
