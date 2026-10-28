#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_signaling::SignalingError;
use llts_signaling::device::DeviceId;
use llts_signaling::message::Update;
use llts_signaling::state::{
    Applied, Battery, Clipboard, ClipboardContent, Hotspot, HybridTimestamp, Incarnation,
    MAX_INLINE_CLIPBOARD_BYTES, MediaSession, Published, StateCache, StateOutbox, StatePublisher,
    Topic, TopicSet, Version, Volume,
};

const OWNER: DeviceId = DeviceId([7; 32]);

const fn battery(percent: u8) -> Battery {
    Battery {
        percent,
        charging: false,
        time_to_empty_min: Some(240),
    }
}

fn published(publisher: &mut StatePublisher, value: Battery) -> Version {
    match publisher.publish(&value).unwrap() {
        Published::Changed(version) => version,
        Published::Unchanged => unreachable!("every call publishes a new value"),
    }
}

#[test]
fn a_cache_accepts_newer_writes_and_rejects_replays_of_older_ones() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(1));
    let mut cache = StateCache::default();

    published(&mut publisher, battery(90));
    let old = publisher.update(Topic::Battery).unwrap();
    let old_payload = old.payload.to_vec();
    let old_version = old.version;
    assert_eq!(cache.apply_update(OWNER, &old), Applied::Accepted);

    published(&mut publisher, battery(80));
    let new = publisher.update(Topic::Battery).unwrap();
    assert_eq!(cache.apply_update(OWNER, &new), Applied::Accepted);
    assert_eq!(cache.apply_update(OWNER, &new), Applied::Stale);

    let replay = Update {
        topic: Topic::Battery,
        version: old_version,
        payload: &old_payload,
    };
    assert_eq!(cache.apply_update(OWNER, &replay), Applied::Stale);
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), Some(battery(80)));
}

#[test]
fn a_restarted_owner_replaces_the_cache_despite_a_lower_sequence() {
    let mut before = StatePublisher::new(Incarnation::from_raw(1));
    for percent in 50..60 {
        published(&mut before, battery(percent));
    }
    let mut cache = StateCache::default();
    let _ = cache.apply_update(OWNER, &before.update(Topic::Battery).unwrap());

    let mut after = StatePublisher::new(Incarnation::from_raw(2));
    let version = published(&mut after, battery(12));
    assert_eq!(version.seq, 1);
    assert!(version.supersedes(cache.version(&OWNER, Topic::Battery).unwrap()));
    assert_eq!(
        cache.apply_update(OWNER, &after.update(Topic::Battery).unwrap()),
        Applied::Accepted
    );
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), Some(battery(12)));
}

#[test]
fn publishing_an_identical_value_does_not_bump_the_version() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(3));
    let first = published(&mut publisher, battery(40));
    assert_eq!(
        publisher.publish(&battery(40)).unwrap(),
        Published::Unchanged
    );
    assert_eq!(publisher.update(Topic::Battery).unwrap().version, first);
    assert!(published(&mut publisher, battery(39)).seq > first.seq);
}

#[test]
fn sequence_numbers_are_shared_across_topics() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(4));
    let battery_version = published(&mut publisher, battery(10));
    let Published::Changed(volume_version) = publisher
        .publish(&Volume {
            percent: 20,
            muted: false,
        })
        .unwrap()
    else {
        unreachable!("first volume publish");
    };
    assert!(volume_version.seq > battery_version.seq);
    assert_eq!(
        publisher.topics(),
        [Topic::Battery, Topic::Volume].into_iter().collect()
    );
}

#[test]
fn the_outbox_coalesces_many_changes_into_one_update_per_topic() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(5));
    let mut outbox = StateOutbox::new();
    let snapshot = outbox.subscribe(Topic::Battery.into(), &publisher);
    assert!(snapshot.updates.is_empty());

    for percent in [70, 69, 68] {
        published(&mut publisher, battery(percent));
        outbox.mark_changed(Topic::Battery);
    }
    let _ = publisher
        .publish(&Volume {
            percent: 5,
            muted: true,
        })
        .unwrap();
    outbox.mark_changed(Topic::Volume);

    let updates: Vec<Update<'_>> = outbox.drain(&publisher).collect();
    assert_eq!(updates.len(), 1, "volume was never subscribed");
    let mut cache = StateCache::default();
    let _ = cache.apply_update(OWNER, updates.first().unwrap());
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), Some(battery(68)));
    assert!(!outbox.has_pending());
    assert_eq!(outbox.drain(&publisher).count(), 0);
}

#[test]
fn a_snapshot_clears_topics_the_owner_no_longer_has() {
    let mut cache = StateCache::default();
    let mut old = StatePublisher::new(Incarnation::from_raw(6));
    published(&mut old, battery(30));
    let _ = old
        .publish(&Hotspot {
            enabled: true,
            ssid: Some("crown"),
        })
        .unwrap();
    cache.apply_snapshot(OWNER, &old.snapshot(TopicSet::ALL));
    assert!(cache.get::<Hotspot<'_>>(&OWNER).unwrap().is_some());

    let mut new = StatePublisher::new(Incarnation::from_raw(7));
    published(&mut new, battery(31));
    cache.apply_snapshot(OWNER, &new.snapshot(TopicSet::ALL));
    assert_eq!(cache.get::<Hotspot<'_>>(&OWNER).unwrap(), None);
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), Some(battery(31)));
}

#[test]
fn a_snapshot_leaves_topics_it_does_not_cover_alone() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(8));
    published(&mut publisher, battery(55));
    let mut cache = StateCache::default();
    cache.apply_snapshot(OWNER, &publisher.snapshot(TopicSet::ALL));

    let empty = StatePublisher::new(Incarnation::from_raw(9));
    cache.apply_snapshot(OWNER, &empty.snapshot(Topic::Volume.into()));
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), Some(battery(55)));
}

#[test]
fn documents_decode_as_their_own_topic_only() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(10));
    let session = Some(MediaSession {
        app: "Spotify",
        title: "Song",
        artist: "Artist",
        position_ms: 1_000,
        duration_ms: 180_000,
        playing: true,
    });
    let _ = publisher.publish(&session).unwrap();
    let mut cache = StateCache::default();
    cache.apply_snapshot(OWNER, &publisher.snapshot(TopicSet::ALL));

    assert_eq!(
        cache.get::<Option<MediaSession<'_>>>(&OWNER).unwrap(),
        Some(session)
    );
    assert_eq!(cache.get::<Battery>(&OWNER).unwrap(), None);
    assert_eq!(
        publisher.get::<Option<MediaSession<'_>>>().unwrap(),
        Some(session)
    );
}

#[test]
fn a_clipboard_over_the_inline_limit_needs_an_ffsp_offer() {
    let small = vec![0; MAX_INLINE_CLIPBOARD_BYTES];
    assert!(ClipboardContent::inline(&small).is_ok());
    let large = vec![0; MAX_INLINE_CLIPBOARD_BYTES + 1];
    assert_eq!(
        ClipboardContent::inline(&large),
        Err(SignalingError::ClipboardTooLarge {
            len: MAX_INLINE_CLIPBOARD_BYTES + 1,
            limit: MAX_INLINE_CLIPBOARD_BYTES,
        })
    );
}

#[test]
fn the_newest_clipboard_wins_and_identical_stamps_fall_to_the_device_id() {
    let stamp = HybridTimestamp {
        wall_ms: 1_000,
        counter: 0,
    };
    let mut cache = StateCache::default();
    for (seed, text, stamp) in [
        (1, "older", stamp),
        (
            2,
            "tie low",
            HybridTimestamp {
                counter: 1,
                ..stamp
            },
        ),
        (
            3,
            "tie high",
            HybridTimestamp {
                counter: 1,
                ..stamp
            },
        ),
    ] {
        let mut publisher = StatePublisher::new(Incarnation::from_raw(u64::from(seed)));
        let _ = publisher
            .publish(&Clipboard {
                mime: "text/plain",
                content: ClipboardContent::inline(text.as_bytes()).unwrap(),
                stamp,
            })
            .unwrap();
        cache.apply_snapshot(DeviceId([seed; 32]), &publisher.snapshot(TopicSet::ALL));
    }

    let (origin, clipboard) = cache.newest_clipboard().unwrap();
    assert_eq!(origin, DeviceId([3; 32]));
    assert_eq!(clipboard.content, ClipboardContent::Inline(b"tie high"));
}

#[test]
fn forgetting_an_owner_drops_its_documents() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(11));
    published(&mut publisher, battery(1));
    let mut cache = StateCache::default();
    cache.apply_snapshot(OWNER, &publisher.snapshot(TopicSet::ALL));
    assert!(cache.forget(&OWNER));
    assert_eq!(cache.version(&OWNER, Topic::Battery), None);
    assert!(!cache.forget(&OWNER));
}

#[test]
fn an_encoded_payload_publishes_like_a_typed_one_and_garbage_is_refused() {
    let mut typed = StatePublisher::new(Incarnation::from_raw(5));
    let mut encoded = StatePublisher::new(Incarnation::from_raw(5));
    let version = published(&mut typed, battery(61));
    let bytes = typed.update(Topic::Battery).unwrap().payload.to_vec();

    assert_eq!(
        encoded.publish_encoded(Topic::Battery, &bytes).unwrap(),
        Published::Changed(version)
    );
    assert_eq!(encoded.get::<Battery>().unwrap(), Some(battery(61)));
    assert_eq!(
        encoded.publish_encoded(Topic::Battery, &bytes).unwrap(),
        Published::Unchanged
    );
    assert!(matches!(
        encoded.publish_encoded(Topic::Hotspot, &[0xFF, 0xFF]),
        Err(SignalingError::MalformedPayload {
            topic: Topic::Hotspot,
            ..
        })
    ));
}
