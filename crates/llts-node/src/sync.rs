//! Which documents each feature shares, and packing them into channel messages.

use llts_core::channel::MAX_MESSAGE_LEN;
use llts_signaling::device::{Feature, FeatureSet};
use llts_signaling::message::{Signal, Snapshot, Update};
use llts_signaling::state::{Topic, TopicSet};

use crate::NodeError;

/// Documents every paired device shares, whatever features it allows.
const ALWAYS_SHARED: TopicSet = TopicSet::EMPTY
    .with(Topic::DeviceInfo)
    .with(Topic::MediaSession)
    .with(Topic::Volume);

const FEATURE_TOPICS: [(Feature, Topic); 5] = [
    (Feature::Battery, Topic::Battery),
    (Feature::Calls, Topic::CallState),
    (Feature::Notifications, Topic::Notifications),
    (Feature::Clipboard, Topic::Clipboard),
    (Feature::Hotspot, Topic::Hotspot),
];

/// Bytes an update adds around its payload: topic, version and length prefixes, generously.
const UPDATE_OVERHEAD: usize = 32;
const SNAPSHOT_OVERHEAD: usize = 16;

pub(crate) fn topics_for(features: FeatureSet) -> TopicSet {
    FEATURE_TOPICS
        .into_iter()
        .filter(|(feature, _)| features.contains(*feature))
        .fold(ALWAYS_SHARED, |topics, (_, topic)| topics.with(topic))
}

/// One envelope, refused if the reliable channel could not carry it.
pub(crate) fn encode(signal: &Signal<'_>) -> Result<Vec<u8>, NodeError> {
    let message = signal.encode(Vec::new())?;
    if message.len() > MAX_MESSAGE_LEN {
        return Err(NodeError::MessageTooLarge {
            len: message.len(),
            limit: MAX_MESSAGE_LEN,
        });
    }
    Ok(message)
}

/// A snapshot as channel messages: the snapshot itself with every update that fits, then the
/// rest as plain updates. The snapshot names all its topics, so the ones sent after it are
/// cleared first and restored a moment later rather than kept stale.
pub(crate) fn encode_snapshot(snapshot: Snapshot<'_>) -> Result<Vec<Vec<u8>>, NodeError> {
    let mut budget = MAX_MESSAGE_LEN - SNAPSHOT_OVERHEAD;
    let (fitting, overflow): (Vec<Update<'_>>, Vec<Update<'_>>) =
        snapshot.updates.into_iter().partition(|update| {
            let cost = update.payload.len() + UPDATE_OVERHEAD;
            let fits = cost <= budget;
            if fits {
                budget -= cost;
            }
            fits
        });
    let head = Signal::SnapshotV1(Snapshot {
        topics: snapshot.topics,
        updates: fitting,
    });
    core::iter::once(encode(&head))
        .chain(
            overflow
                .into_iter()
                .map(|update| encode(&Signal::UpdateV1(update))),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use llts_signaling::state::{Incarnation, Version};

    use super::*;

    #[test]
    fn a_snapshot_too_large_for_one_message_spills_into_updates() -> Result<(), NodeError> {
        let big = vec![7; 40_000];
        let version = Version {
            incarnation: Incarnation::from_raw(1),
            seq: 1,
        };
        let update = |topic| Update {
            topic,
            version,
            payload: &big,
        };
        let messages = encode_snapshot(Snapshot {
            topics: TopicSet::ALL,
            updates: vec![update(Topic::Clipboard), update(Topic::Notifications)],
        })?;
        assert_eq!(messages.len(), 2);
        assert!(
            messages
                .iter()
                .all(|message| message.len() <= MAX_MESSAGE_LEN)
        );
        Ok(())
    }

    #[test]
    fn features_add_their_topics_to_the_ones_always_shared() {
        let topics = topics_for(FeatureSet::EMPTY.with(Feature::Battery));
        assert!(topics.contains(Topic::Battery));
        assert!(topics.contains(Topic::DeviceInfo));
        assert!(!topics.contains(Topic::Clipboard));
    }
}
