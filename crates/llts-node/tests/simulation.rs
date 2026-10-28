//! Whole-node scenarios on the in-memory mesh in `support`: pairing both ways, autoconnect and
//! backoff, reconnecting after restarts, state sync, feature negotiation and forgetting.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

mod support;

use std::time::Duration;

use llts_core::session::Role;
use llts_node::{
    AppCommand, AppEvent, FeatureFailure, FeatureRequest, FeatureState, MediaRequest, MediaRole,
    NodeInput, OutgoingCommand, RemoteCommand,
};
use llts_signaling::device::{DeviceClass, Feature};
use llts_signaling::discovery::Beacon;
use llts_signaling::message::{
    Call, CameraLens, Direction, Media, MessageType, StartCamera, StartMirror, StreamParams,
    VideoCodec, VideoLimits,
};
use llts_signaling::state::{
    Battery, Clipboard, ClipboardContent, DeviceInfo, Hotspot, Incarnation, Topic,
};
use support::{CODE, Sim, capabilities};

const LAPTOP: usize = 0;
const PHONE: usize = 1;
const TABLET: usize = 2;

const LIMITS: VideoLimits = VideoLimits {
    max_width: 1920,
    max_height: 1080,
    max_fps: 60,
};

fn laptop_and_phone() -> Sim {
    Sim::new(&[
        (
            "laptop",
            capabilities(&[VideoCodec::Hevc, VideoCodec::H264], DeviceClass::Computer),
        ),
        (
            "phone",
            capabilities(&[VideoCodec::Hevc, VideoCodec::H264], DeviceClass::Phone),
        ),
    ])
}

fn paired() -> Sim {
    let mut sim = laptop_and_phone();
    sim.pair_by_qr(LAPTOP, PHONE);
    sim
}

const fn battery(percent: u8) -> Battery {
    Battery {
        percent,
        charging: false,
        time_to_empty_min: None,
    }
}

fn remote_battery(sim: &Sim, viewer: usize, owner: usize) -> Option<Battery> {
    sim.nodes[viewer]
        .node
        .remote_state::<Battery>(&sim.id(owner))
        .unwrap()
}

fn publish_battery(sim: &mut Sim, owner: usize, percent: u8) {
    let now = sim.now();
    let _ = sim.nodes[owner]
        .node
        .publish(now, &battery(percent))
        .unwrap();
}

fn assert_mutually_connected(sim: &Sim, left: usize, right: usize) {
    assert!(sim.nodes[left].is_connected_to(sim.id(right)));
    assert!(sim.nodes[right].is_connected_to(sim.id(left)));
}

#[test]
fn qr_pairing_trusts_both_ways_syncs_state_and_exports_the_ffsp_link() {
    let mut sim = laptop_and_phone();
    publish_battery(&mut sim, PHONE, 77);
    sim.pair_by_qr(LAPTOP, PHONE);
    sim.advance(Duration::from_millis(50));

    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));
    assert_mutually_connected(&sim, LAPTOP, PHONE);
    for (viewer, peer) in [(LAPTOP, phone), (PHONE, laptop)] {
        let events = &sim.nodes[viewer].events;
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PairingResult { peer: Some(id), accepted: true } if *id == peer
        )));
        let export = sim.nodes[viewer].exports.first().expect("an ffsp export");
        assert_eq!(export.peer, peer);
        assert!(sim.nodes[viewer].stored_features.contains_key(&peer));
    }
    assert_eq!(
        sim.nodes[LAPTOP].exports[0].link_secret, sim.nodes[PHONE].exports[0].link_secret,
        "both ends hand ffsp the same link secret"
    );
    assert_eq!(sim.nodes[LAPTOP].exports[0].name, "phone");

    assert_eq!(remote_battery(&sim, LAPTOP, PHONE), Some(battery(77)));
    let info = sim.nodes[PHONE]
        .node
        .remote_state::<DeviceInfo<'_>>(&laptop)
        .unwrap()
        .unwrap();
    assert_eq!(info.name, "laptop");

    assert!(
        sim.nodes[LAPTOP]
            .advertised
            .iter()
            .all(|beacon| matches!(beacon, Beacon::Linked(_))),
        "the pairing window closed once a device paired through it"
    );
    assert_eq!(sim.nodes[LAPTOP].advertised.len(), 1);
}

#[test]
fn nearby_pairing_shows_one_code_on_both_screens_and_survives_a_late_confirmation() {
    let mut sim = laptop_and_phone();
    sim.app(LAPTOP, AppCommand::BeginPairing { duration: None });
    sim.app(PHONE, AppCommand::BeginPairing { duration: None });
    sim.show(LAPTOP, PHONE);
    sim.show(PHONE, LAPTOP);

    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));
    for (viewer, peer) in [(LAPTOP, phone), (PHONE, laptop)] {
        assert!(sim.nodes[viewer].events.iter().any(|event| matches!(
            event,
            AppEvent::PairingRequest { peer: id, code: CODE, .. } if *id == peer
        )));
    }
    let dialled = sim.nodes[LAPTOP].dials.len() + sim.nodes[PHONE].dials.len();
    assert_eq!(dialled, 0, "nearby pairing is not a reconnect dial");

    sim.app(
        PHONE,
        AppCommand::ConfirmPairing {
            peer: laptop,
            accept: true,
        },
    );
    sim.advance(Duration::from_secs(5));
    sim.app(
        LAPTOP,
        AppCommand::ConfirmPairing {
            peer: phone,
            accept: true,
        },
    );
    sim.advance(Duration::from_millis(50));

    assert_mutually_connected(&sim, LAPTOP, PHONE);
    let laptop_info = sim.nodes[PHONE]
        .node
        .remote_state::<DeviceInfo<'_>>(&laptop)
        .unwrap();
    let phone_info = sim.nodes[LAPTOP]
        .node
        .remote_state::<DeviceInfo<'_>>(&phone)
        .unwrap();
    assert_eq!(laptop_info.map(|info| info.name), Some("laptop"));
    assert_eq!(
        phone_info.map(|info| info.name),
        Some("phone"),
        "the side that confirmed first still receives state from the late one"
    );
}

#[test]
fn an_unanswered_pairing_code_is_rejected_when_it_times_out() {
    let mut sim = laptop_and_phone();
    sim.app(LAPTOP, AppCommand::BeginPairing { duration: None });
    let laptop_address = sim.nodes[LAPTOP].address;
    sim.app(
        PHONE,
        AppCommand::PairNearby {
            address: laptop_address,
        },
    );
    assert!(
        sim.nodes[PHONE]
            .events
            .iter()
            .any(|event| matches!(event, AppEvent::PairingRequest { .. }))
    );
    sim.advance(sim.config.code_confirmation + Duration::from_secs(1));
    let laptop = sim.id(LAPTOP);
    assert!(sim.nodes[PHONE].events.iter().any(|event| matches!(
        event,
        AppEvent::PairingResult { peer: Some(id), accepted: false } if *id == laptop
    )));
    assert!(sim.nodes[PHONE].node.devices().next().is_none());
}

#[test]
fn a_restarted_device_reconnects_from_its_trust_store_when_it_sees_the_peer() {
    let mut sim = paired();
    publish_battery(&mut sim, PHONE, 64);
    sim.advance(Duration::from_millis(50));

    sim.restart(LAPTOP);
    assert!(!sim.nodes[LAPTOP].is_connected_to(sim.id(PHONE)));
    assert_eq!(remote_battery(&sim, LAPTOP, PHONE), None);
    assert!(
        sim.nodes[LAPTOP]
            .node
            .devices()
            .any(|device| device.id == sim.id(PHONE)),
        "the paired device survives the restart"
    );

    sim.show(PHONE, LAPTOP);
    sim.advance(Duration::from_millis(50));
    assert_mutually_connected(&sim, LAPTOP, PHONE);
    assert_eq!(remote_battery(&sim, LAPTOP, PHONE), Some(battery(64)));
}

#[test]
fn three_devices_autoconnect_into_a_full_mesh_and_converge() {
    let mut sim = Sim::new(&[
        (
            "laptop",
            capabilities(&[VideoCodec::Hevc], DeviceClass::Computer),
        ),
        (
            "phone",
            capabilities(&[VideoCodec::Hevc], DeviceClass::Phone),
        ),
        (
            "tablet",
            capabilities(&[VideoCodec::Hevc], DeviceClass::Tablet),
        ),
    ]);
    sim.pair_by_qr(LAPTOP, PHONE);
    sim.pair_by_qr(LAPTOP, TABLET);
    sim.pair_by_qr(PHONE, TABLET);
    for index in [LAPTOP, PHONE, TABLET] {
        sim.restart(index);
    }
    publish_battery(&mut sim, PHONE, 50);
    publish_battery(&mut sim, TABLET, 20);

    sim.show_all();
    sim.advance(Duration::from_millis(50));
    for (left, right) in [(LAPTOP, PHONE), (LAPTOP, TABLET), (PHONE, TABLET)] {
        assert_mutually_connected(&sim, left, right);
    }
    let dials: usize = sim.nodes.iter().map(|node| node.dials.len()).sum();
    assert!(dials <= 6, "one dial per direction at most, got {dials}");

    publish_battery(&mut sim, PHONE, 49);
    sim.advance(Duration::from_millis(50));
    for viewer in [LAPTOP, TABLET] {
        assert_eq!(remote_battery(&sim, viewer, PHONE), Some(battery(49)));
    }
    assert_eq!(remote_battery(&sim, LAPTOP, TABLET), Some(battery(20)));
    assert_eq!(remote_battery(&sim, PHONE, TABLET), Some(battery(20)));
}

#[test]
fn an_unreachable_peer_is_redialled_with_growing_jittered_backoff() {
    let mut sim = paired();
    sim.show(PHONE, LAPTOP);
    sim.stop(PHONE);
    let phone = sim.id(PHONE);
    let dials_before = sim.nodes[LAPTOP].dials.len();

    sim.advance(Duration::from_secs(20));
    let times: Vec<Duration> = sim.nodes[LAPTOP].dials[dials_before..]
        .iter()
        .map(|(at, peer)| {
            assert_eq!(*peer, phone);
            *at
        })
        .collect();
    assert!(times.len() >= 4, "only {} redials", times.len());
    let policy = sim.config.backoff;
    for (attempt, pair) in times.windows(2).enumerate() {
        let gap = pair[1] - pair[0];
        let base = policy.initial * (1 << (attempt + 1));
        let base = base.min(policy.max);
        let low = base.mul_f64(0.79);
        let high = base.mul_f64(1.21);
        assert!(
            gap >= low && gap <= high,
            "gap {attempt} was {gap:?}, expected {low:?}..{high:?}"
        );
    }

    let before = sim.nodes[LAPTOP].dials.len();
    for _ in 0..5 {
        sim.show(PHONE, LAPTOP);
    }
    assert_eq!(
        sim.nodes[LAPTOP].dials.len(),
        before,
        "sightings during a backoff do not add dials"
    );

    sim.restart(PHONE);
    sim.advance(Duration::from_secs(40));
    assert_mutually_connected(&sim, LAPTOP, PHONE);
}

#[test]
fn a_lost_peer_is_not_redialled_until_it_is_seen_again() {
    let mut sim = paired();
    sim.show(PHONE, LAPTOP);
    let beacons = sim.nodes[PHONE].advertised.clone();
    sim.stop(PHONE);
    for beacon in beacons {
        sim.input(
            LAPTOP,
            NodeInput::Presence(llts_node::Presence::Lost { beacon }),
        );
    }
    sim.advance(Duration::from_secs(30));
    let dials = sim.nodes[LAPTOP].dials.len();
    sim.advance(Duration::from_secs(120));
    assert_eq!(sim.nodes[LAPTOP].dials.len(), dials);
}

#[test]
fn state_changes_coalesce_and_an_owner_restart_replaces_what_peers_cached() {
    let mut sim = paired();
    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));
    let now = sim.now();
    let _ = sim.nodes[PHONE]
        .node
        .publish(
            now,
            &Hotspot {
                enabled: true,
                ssid: Some("phone-ap"),
            },
        )
        .unwrap();
    sim.advance(Duration::from_millis(50));
    let updates_before = sim.nodes[PHONE].sent_count(laptop, MessageType::UpdateV1);
    for percent in [90, 89, 88] {
        publish_battery(&mut sim, PHONE, percent);
    }
    sim.advance(Duration::from_millis(50));
    assert_eq!(
        sim.nodes[PHONE].sent_count(laptop, MessageType::UpdateV1) - updates_before,
        1,
        "three battery changes inside the flush delay leave as one update"
    );
    assert_eq!(remote_battery(&sim, LAPTOP, PHONE), Some(battery(88)));

    sim.restart(PHONE);
    publish_battery(&mut sim, PHONE, 15);
    sim.show(LAPTOP, PHONE);
    sim.advance(Duration::from_millis(50));
    assert_mutually_connected(&sim, LAPTOP, PHONE);
    assert_eq!(remote_battery(&sim, LAPTOP, PHONE), Some(battery(15)));
    assert_eq!(
        sim.nodes[LAPTOP]
            .node
            .remote_state::<Hotspot<'_>>(&phone)
            .unwrap(),
        None,
        "the restarted owner no longer has a hotspot document"
    );
    let version = sim.nodes[LAPTOP]
        .node
        .remote_update(&phone, Topic::Battery)
        .unwrap()
        .version;
    assert_eq!(
        version.incarnation,
        sim.nodes[PHONE].node.local_state().incarnation()
    );
    assert_ne!(version.incarnation, Incarnation::from_raw(0));
}

#[test]
fn the_clipboard_copied_last_wins_on_every_device() {
    let mut sim = paired();
    for (index, text) in [(LAPTOP, "from laptop"), (PHONE, "from phone")] {
        let now = sim.now();
        let stamp = sim.nodes[index].node.clipboard_stamp(now);
        let clipboard = Clipboard {
            mime: "text/plain",
            content: ClipboardContent::inline(text.as_bytes()).unwrap(),
            stamp,
        };
        let _ = sim.nodes[index].node.publish(now, &clipboard).unwrap();
        sim.advance(Duration::from_millis(50));
    }
    for index in [LAPTOP, PHONE] {
        let (origin, clipboard) = sim.nodes[index].node.newest_clipboard().unwrap();
        assert_eq!(origin, sim.id(PHONE));
        assert_eq!(clipboard.content, ClipboardContent::Inline(b"from phone"));
    }
}

#[test]
fn mirroring_falls_back_to_h264_when_the_viewer_cannot_decode_hevc() {
    let mut sim = Sim::new(&[
        (
            "laptop",
            capabilities(&[VideoCodec::Hevc, VideoCodec::H264], DeviceClass::Computer),
        ),
        (
            "tablet",
            capabilities(&[VideoCodec::H264], DeviceClass::Tablet),
        ),
    ]);
    sim.pair_by_qr(LAPTOP, PHONE);
    let tablet = sim.id(PHONE);
    let laptop = sim.id(LAPTOP);

    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: tablet,
            request: FeatureRequest::Mirror(StartMirror {
                direction: Direction::ToPeer,
                limits: LIMITS,
                remote_input: true,
            }),
        },
    );

    let Some(MediaRequest::Start {
        role: MediaRole::Encoder,
        stream,
        params: StreamParams::Video(video),
        ..
    }) = sim.nodes[LAPTOP].media.last().copied()
    else {
        panic!(
            "the laptop never started its encoder: {:?}",
            sim.nodes[LAPTOP].media
        );
    };
    assert_eq!(video.codec, VideoCodec::H264);
    assert_eq!(
        (video.width, video.height, video.fps_mhz),
        (1920, 1080, 60_000)
    );
    assert!(matches!(
        sim.nodes[PHONE].media.last(),
        Some(MediaRequest::Start {
            role: MediaRole::Decoder,
            request: FeatureRequest::Mirror(StartMirror {
                direction: Direction::FromPeer,
                ..
            }),
            ..
        })
    ));
    assert!(matches!(
        sim.nodes[LAPTOP].sessions[&tablet].streams.get(&stream),
        Some(Some((_, Role::Sender)))
    ));
    assert!(matches!(
        sim.nodes[PHONE].sessions[&laptop].streams.get(&stream),
        Some(Some((_, Role::Receiver)))
    ));
    for (index, peer) in [(LAPTOP, tablet), (PHONE, laptop)] {
        assert_eq!(
            sim.nodes[index].node.feature_state(&peer, Feature::Mirror),
            Some(FeatureState::Active)
        );
        assert_eq!(
            sim.nodes[index].node.stream_purpose(&peer, stream),
            Some((
                Feature::Mirror,
                if index == LAPTOP {
                    MediaRole::Encoder
                } else {
                    MediaRole::Decoder
                }
            ))
        );
    }

    sim.app(
        PHONE,
        AppCommand::StopFeature {
            peer: laptop,
            feature: Feature::Mirror,
        },
    );
    for (index, peer) in [(LAPTOP, tablet), (PHONE, laptop)] {
        assert!(matches!(
            sim.nodes[index].media.last(),
            Some(MediaRequest::Stop {
                feature: Feature::Mirror,
                ..
            })
        ));
        assert!(
            !sim.nodes[index].sessions[&peer]
                .streams
                .contains_key(&stream)
        );
        assert_eq!(
            sim.nodes[index].node.feature_state(&peer, Feature::Mirror),
            Some(FeatureState::Enabled)
        );
    }
}

#[test]
fn the_camera_streams_hevc_from_the_peer_when_both_decode_it() {
    let mut sim = paired();
    let phone = sim.id(PHONE);
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Camera(StartCamera {
                lens: CameraLens::Back,
                limits: LIMITS,
            }),
        },
    );
    let Some(MediaRequest::Start {
        role: MediaRole::Encoder,
        request: FeatureRequest::Camera(camera),
        params: StreamParams::Video(video),
        ..
    }) = sim.nodes[PHONE].media.last().copied()
    else {
        panic!("the phone never started its camera encoder");
    };
    assert_eq!(camera.lens, CameraLens::Back);
    assert_eq!(video.codec, VideoCodec::Hevc);
    assert!(matches!(
        sim.nodes[LAPTOP].media.last(),
        Some(MediaRequest::Start {
            role: MediaRole::Decoder,
            ..
        })
    ));

    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Mic,
        },
    );
    assert!(matches!(
        sim.nodes[PHONE].media.last(),
        Some(MediaRequest::Start {
            request: FeatureRequest::Mic,
            role: MediaRole::Encoder,
            params: StreamParams::Audio(_),
            ..
        })
    ));

    sim.stop(PHONE);
    let stops = sim.nodes[LAPTOP]
        .media
        .iter()
        .filter(|request| matches!(request, MediaRequest::Stop { .. }))
        .count();
    assert_eq!(stops, 2, "a lost session stops every pipeline it fed");
}

#[test]
fn features_the_peer_disabled_or_cannot_encode_are_refused() {
    let mut sim = Sim::new(&[
        (
            "laptop",
            capabilities(&[VideoCodec::Hevc], DeviceClass::Computer),
        ),
        (
            "phone",
            capabilities(&[VideoCodec::Av1], DeviceClass::Phone),
        ),
    ]);
    sim.pair_by_qr(LAPTOP, PHONE);
    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));

    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Mirror(StartMirror {
                direction: Direction::ToPeer,
                limits: LIMITS,
                remote_input: false,
            }),
        },
    );
    assert!(sim.nodes[LAPTOP].events.iter().any(|event| matches!(
        event,
        AppEvent::FeatureFailed {
            feature: Feature::Mirror,
            reason: FeatureFailure::NoCommonCodec,
            ..
        }
    )));

    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Camera(StartCamera {
                lens: CameraLens::Front,
                limits: LIMITS,
            }),
        },
    );
    assert!(sim.nodes[LAPTOP].events.iter().any(|event| matches!(
        event,
        AppEvent::FeatureFailed {
            feature: Feature::Camera,
            reason: FeatureFailure::Refused,
            ..
        }
    )));

    sim.app(
        PHONE,
        AppCommand::SetFeature {
            peer: laptop,
            feature: Feature::Mic,
            enabled: false,
        },
    );
    assert!(!sim.nodes[PHONE].stored_features[&laptop].contains(Feature::Mic));
    let available = sim.nodes[LAPTOP].node.device(&phone).unwrap().available;
    assert!(!available.contains(Feature::Mic));
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Mic,
        },
    );
    assert!(sim.nodes[LAPTOP].events.iter().any(|event| matches!(
        event,
        AppEvent::FeatureFailed {
            feature: Feature::Mic,
            reason: FeatureFailure::Unavailable,
            ..
        }
    )));
    assert!(sim.nodes[PHONE].media.is_empty());
}

#[test]
fn commands_reach_the_peer_only_for_features_both_allow() {
    let mut sim = paired();
    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));
    sim.app(
        LAPTOP,
        AppCommand::Send {
            peer: phone,
            command: OutgoingCommand::Media(Media::Pause),
        },
    );
    assert!(sim.nodes[PHONE].events.iter().any(|event| matches!(
        event,
        AppEvent::RemoteCommand { peer, command: RemoteCommand::Media(Media::Pause) } if *peer == laptop
    )));

    sim.app(
        PHONE,
        AppCommand::SetFeature {
            peer: laptop,
            feature: Feature::Calls,
            enabled: false,
        },
    );
    sim.app(
        LAPTOP,
        AppCommand::Send {
            peer: phone,
            command: OutgoingCommand::Call(Call::Dial { number: "123" }),
        },
    );
    assert!(sim.nodes[LAPTOP].events.iter().any(|event| matches!(
        event,
        AppEvent::Error(llts_node::NodeError::FeatureUnavailable {
            feature: Feature::Calls,
            ..
        })
    )));
    assert!(!sim.nodes[PHONE].events.iter().any(|event| matches!(
        event,
        AppEvent::RemoteCommand {
            command: RemoteCommand::Call(_),
            ..
        }
    )));
}

#[test]
fn forgetting_a_device_unpairs_it_on_both_ends() {
    let mut sim = paired();
    let (laptop, phone) = (sim.id(LAPTOP), sim.id(PHONE));
    sim.app(LAPTOP, AppCommand::Forget { peer: phone });

    for (index, peer) in [(LAPTOP, phone), (PHONE, laptop)] {
        assert!(sim.nodes[index].node.devices().next().is_none());
        assert!(!sim.nodes[index].trusted.contains_key(&peer));
        assert!(!sim.nodes[index].stored_features.contains_key(&peer));
        assert!(sim.nodes[index].advertised.is_empty());
        assert_eq!(
            sim.nodes[index]
                .node
                .remote_update(&peer, Topic::DeviceInfo),
            None
        );
    }
    sim.show(PHONE, LAPTOP);
    sim.advance(Duration::from_secs(5));
    assert!(sim.nodes[LAPTOP].dials.is_empty());
}

const fn mirror_to_peer() -> FeatureRequest {
    FeatureRequest::Mirror(StartMirror {
        direction: Direction::ToPeer,
        limits: LIMITS,
        remote_input: false,
    })
}

fn failures(sim: &Sim, index: usize, feature: Feature) -> Vec<FeatureFailure> {
    sim.nodes[index]
        .events
        .iter()
        .filter_map(|event| match event {
            AppEvent::FeatureFailed {
                feature: failed,
                reason,
                ..
            } if *failed == feature => Some(*reason),
            _ => None,
        })
        .collect()
}

/// Pairs laptop and phone with every Capabilities message held back, so both sessions are up
/// but neither side knows what the other supports.
fn paired_before_capabilities(sim: &mut Sim) {
    sim.hold(MessageType::CapabilitiesV1);
    sim.pair_by_qr(LAPTOP, PHONE);
    assert_mutually_connected(sim, LAPTOP, PHONE);
}

#[test]
fn a_feature_started_before_the_peer_announced_itself_starts_once_it_does() {
    let mut sim = laptop_and_phone();
    paired_before_capabilities(&mut sim);
    let phone = sim.id(PHONE);

    for _ in 0..3 {
        sim.app(
            LAPTOP,
            AppCommand::StartFeature {
                peer: phone,
                request: mirror_to_peer(),
            },
        );
    }
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());
    assert!(sim.nodes[LAPTOP].media.is_empty());
    assert_eq!(
        sim.nodes[LAPTOP].sent_count(phone, MessageType::StartMirrorV1),
        0
    );

    sim.release();
    assert_eq!(
        sim.nodes[LAPTOP].sent_count(phone, MessageType::StartMirrorV1),
        1,
        "repeated starts wait as one"
    );
    assert!(matches!(
        sim.nodes[LAPTOP].media.as_slice(),
        [MediaRequest::Start {
            role: MediaRole::Encoder,
            ..
        }]
    ));
    assert_eq!(
        sim.nodes[LAPTOP]
            .node
            .feature_state(&phone, Feature::Mirror),
        Some(FeatureState::Active)
    );
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());
}

#[test]
fn a_waiting_start_is_decided_against_the_capabilities_that_arrive() {
    let mut sim = Sim::new(&[
        (
            "laptop",
            capabilities(&[VideoCodec::Hevc], DeviceClass::Computer),
        ),
        (
            "phone",
            capabilities(&[VideoCodec::Av1], DeviceClass::Phone),
        ),
    ]);
    paired_before_capabilities(&mut sim);
    let phone = sim.id(PHONE);
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: mirror_to_peer(),
        },
    );
    sim.app(
        LAPTOP,
        AppCommand::SetFeature {
            peer: phone,
            feature: Feature::Mic,
            enabled: false,
        },
    );
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Mic,
        },
    );
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());

    sim.release();
    assert_eq!(
        failures(&sim, LAPTOP, Feature::Mirror),
        [FeatureFailure::NoCommonCodec]
    );
    assert_eq!(
        failures(&sim, LAPTOP, Feature::Mic),
        [FeatureFailure::Unavailable]
    );
    assert!(sim.nodes[PHONE].media.is_empty());
}

#[test]
fn a_waiting_start_gives_up_when_the_capabilities_never_come() {
    let mut sim = laptop_and_phone();
    paired_before_capabilities(&mut sim);
    let phone = sim.id(PHONE);
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: FeatureRequest::Mic,
        },
    );

    sim.advance(sim.config.capabilities_wait - Duration::from_millis(1));
    assert!(failures(&sim, LAPTOP, Feature::Mic).is_empty());
    sim.advance(Duration::from_millis(2));
    assert_eq!(
        failures(&sim, LAPTOP, Feature::Mic),
        [FeatureFailure::Unavailable]
    );

    sim.release();
    assert!(sim.nodes[LAPTOP].media.is_empty());
    assert_eq!(
        sim.nodes[LAPTOP].sent_count(phone, MessageType::StartMicV1),
        0
    );
    assert_eq!(failures(&sim, LAPTOP, Feature::Mic).len(), 1);
}

#[test]
fn a_waiting_start_is_dropped_when_the_session_ends_or_the_user_stops_it() {
    let mut sim = laptop_and_phone();
    paired_before_capabilities(&mut sim);
    let phone = sim.id(PHONE);
    for request in [FeatureRequest::Mic, mirror_to_peer()] {
        sim.app(
            LAPTOP,
            AppCommand::StartFeature {
                peer: phone,
                request,
            },
        );
    }
    sim.app(
        LAPTOP,
        AppCommand::StopFeature {
            peer: phone,
            feature: Feature::Mirror,
        },
    );
    sim.stop(PHONE);
    assert_eq!(
        failures(&sim, LAPTOP, Feature::Mic),
        [FeatureFailure::NotConnected]
    );
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());

    sim.release();
    sim.advance(sim.config.capabilities_wait * 2);
    assert!(sim.nodes[LAPTOP].media.is_empty());
    assert_eq!(failures(&sim, LAPTOP, Feature::Mic).len(), 1);
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());
}

fn v4_mapped(address: std::net::SocketAddr) -> std::net::SocketAddr {
    let std::net::SocketAddr::V4(ipv4) = address else {
        panic!("the simulation uses IPv4 addresses");
    };
    std::net::SocketAddr::from((ipv4.ip().to_ipv6_mapped(), ipv4.port()))
}

#[test]
fn a_failed_dial_to_a_v4_mapped_address_backs_off_at_once() {
    let mut sim = paired();
    let beacons = sim.nodes[PHONE].advertised.clone();
    let mapped = v4_mapped(sim.nodes[PHONE].address);
    sim.stop(PHONE);
    sim.advance(Duration::from_secs(120));
    let before = sim.nodes[LAPTOP].dials.len();

    for beacon in beacons {
        sim.input(
            LAPTOP,
            NodeInput::Presence(llts_node::Presence::Seen {
                beacon,
                addresses: &[mapped],
            }),
        );
    }
    sim.settle();
    sim.advance(sim.config.dial_timeout - Duration::from_millis(1));
    let dials = &sim.nodes[LAPTOP].dials[before..];
    assert!(
        dials.len() >= 2,
        "the refused dial waited for its timeout instead of backing off: {dials:?}"
    );

    sim.restart(PHONE);
    sim.advance(Duration::from_secs(60));
    assert_mutually_connected(&sim, LAPTOP, PHONE);
}

#[test]
fn a_nearby_pairing_to_a_v4_mapped_address_fails_as_soon_as_the_handshake_does() {
    let mut sim = laptop_and_phone();
    let mapped = v4_mapped(sim.nodes[LAPTOP].address);
    sim.app(PHONE, AppCommand::PairNearby { address: mapped });
    assert!(sim.nodes[PHONE].events.iter().any(|event| matches!(
        event,
        AppEvent::PairingResult {
            peer: None,
            accepted: false
        }
    )));
}

#[test]
fn a_waiting_start_survives_its_session_being_replaced() {
    let mut sim = laptop_and_phone();
    paired_before_capabilities(&mut sim);
    let phone = sim.id(PHONE);
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: mirror_to_peer(),
        },
    );
    sim.input(
        LAPTOP,
        NodeInput::Link(llts_node::LinkEvent::Connected {
            peer: phone,
            name: "phone",
            class: DeviceClass::Phone,
        }),
    );
    sim.settle();
    assert!(failures(&sim, LAPTOP, Feature::Mirror).is_empty());

    sim.release();
    assert_eq!(
        sim.nodes[LAPTOP]
            .node
            .feature_state(&phone, Feature::Mirror),
        Some(FeatureState::Active)
    );
}

#[test]
fn a_peer_seen_between_pairing_and_its_session_is_not_dialled_again() {
    let mut sim = laptop_and_phone();
    sim.static_peers = true;
    sim.pair_by_qr(LAPTOP, PHONE);
    assert_mutually_connected(&sim, LAPTOP, PHONE);
    for index in [LAPTOP, PHONE] {
        assert!(
            sim.nodes[index].dials.is_empty(),
            "a dial would replace the pairing session: {:?}",
            sim.nodes[index].dials
        );
    }

    let phone = sim.id(PHONE);
    sim.app(
        LAPTOP,
        AppCommand::StartFeature {
            peer: phone,
            request: mirror_to_peer(),
        },
    );
    assert_eq!(
        sim.nodes[LAPTOP]
            .node
            .feature_state(&phone, Feature::Mirror),
        Some(FeatureState::Active)
    );
}
