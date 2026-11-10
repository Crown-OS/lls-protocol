#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_signaling::SignalingError;
use llts_signaling::device::DeviceId;
use llts_signaling::message::Update;
use llts_signaling::state::{
    Applied, AudioRoutes, Battery, Chronometer, Clipboard, ClipboardContent, Focus, FocusMode,
    Headset, Hotspot, HybridTimestamp, Incarnation, LiveActivities, LiveActivity, LiveProgress,
    MAX_HEADSETS, MAX_INLINE_CLIPBOARD_BYTES, MAX_LIVE_ACTIVITIES, MAX_LIVE_FIELD_BYTES,
    MediaSession, NotificationAction, Published, StateCache, StateOutbox, StatePublisher, Topic,
    TopicSet, Version, Volume,
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

fn timer<'a>(title: &'a str) -> LiveActivity<'a> {
    LiveActivity {
        id: "0|com.clock|7|null|10101",
        package: "com.clock",
        app: "Clock",
        title,
        text: "Pasta",
        short_text: Some("4:59"),
        category: Some("stopwatch"),
        progress: Some(LiveProgress {
            value: 1,
            max: 300,
            indeterminate: false,
        }),
        chronometer: Some(Chronometer {
            base_unix_ms: 1_700_000_300_000,
            counts_down: true,
        }),
        promoted: true,
        color_argb: Some(0xFF20_8040),
        actions: vec![NotificationAction {
            id: "pause",
            label: "Pause",
            accepts_text: false,
        }],
    }
}

#[test]
fn the_continuity_documents_round_trip_through_a_cache() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(21));
    let focus = Focus {
        mode: FocusMode::Named("Work"),
        until_unix_ms: Some(1_700_003_600_000),
        stamp: HybridTimestamp {
            wall_ms: 1_700_000_000_000,
            counter: 3,
        },
    };
    let live = LiveActivities {
        activities: vec![timer("Timer")],
    };
    let routes = AudioRoutes {
        headsets: vec![Headset {
            address: "AA:BB:CC:DD:EE:FF",
            name: "Buds",
            connected: true,
        }],
    };
    let _ = publisher.publish(&focus).unwrap();
    let _ = publisher.publish(&live).unwrap();
    let _ = publisher.publish(&routes).unwrap();

    let mut cache = StateCache::default();
    cache.apply_snapshot(OWNER, &publisher.snapshot(TopicSet::ALL));
    assert_eq!(cache.get::<Focus<'_>>(&OWNER).unwrap(), Some(focus));
    assert_eq!(cache.get::<LiveActivities<'_>>(&OWNER).unwrap(), Some(live));
    assert_eq!(cache.get::<AudioRoutes<'_>>(&OWNER).unwrap(), Some(routes));
}

#[test]
fn live_activities_breaking_a_cap_are_refused_at_publication() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(22));
    let crowded = LiveActivities {
        activities: vec![timer("Timer"); MAX_LIVE_ACTIVITIES + 1],
    };
    let bytes = postcard::to_allocvec(&crowded).unwrap();
    assert!(matches!(
        publisher.publish_encoded(Topic::LiveActivities, &bytes),
        Err(SignalingError::TooManyEntries { .. })
    ));

    let long = "t".repeat(MAX_LIVE_FIELD_BYTES + 1);
    let wordy = LiveActivities {
        activities: vec![timer(&long)],
    };
    let bytes = postcard::to_allocvec(&wordy).unwrap();
    assert!(matches!(
        publisher.publish_encoded(Topic::LiveActivities, &bytes),
        Err(SignalingError::FieldTooLong { .. })
    ));

    let oversized = vec![0; 24 * 1024 + 1];
    assert!(matches!(
        publisher.publish_encoded(Topic::LiveActivities, &oversized),
        Err(SignalingError::DocumentTooLarge { .. })
    ));

    let headset = Headset {
        address: "AA:BB:CC:DD:EE:FF",
        name: "Buds",
        connected: false,
    };
    let routes = AudioRoutes {
        headsets: vec![headset; MAX_HEADSETS + 1],
    };
    let bytes = postcard::to_allocvec(&routes).unwrap();
    assert!(
        publisher
            .publish_encoded(Topic::AudioRoutes, &bytes)
            .is_err()
    );

    assert!(matches!(
        publisher.publish(&crowded),
        Err(SignalingError::TooManyEntries { .. })
    ));
    assert!(publisher.publish(&routes).is_err());
    assert!(publisher.topics().is_empty());
}

#[test]
fn a_focus_mode_from_the_future_is_refused_rather_than_misread() {
    let mut publisher = StatePublisher::new(Incarnation::from_raw(23));
    let off = postcard::to_allocvec(&Focus {
        mode: FocusMode::Off,
        until_unix_ms: None,
        stamp: HybridTimestamp::default(),
    })
    .unwrap();
    assert_eq!(off.first(), Some(&0));
    let mut unknown = off.clone();
    if let Some(mode) = unknown.first_mut() {
        *mode = 3;
    }
    assert!(publisher.publish_encoded(Topic::Focus, &unknown).is_err());
    assert!(publisher.publish_encoded(Topic::Focus, &off).is_ok());
}
