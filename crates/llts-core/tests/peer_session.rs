#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

mod support;

use std::collections::HashSet;
use std::time::Duration;

use llts_core::CoreError;
use llts_core::channel::Delivery;
use llts_core::repair::{RepairMode, RepairRate};
use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, MediaBudget, PathId, Role};
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_core::units::{Bitrate, FrameRate};
use llts_wire::packet::{PacketHeader, PacketKind};
use llts_wire::{MediaTimestamp, StreamClass};
use support::{LinkModel, Pair, ramp, session};

const SCREEN: StreamKey = StreamKey::new(StreamClass::Video, 0);
const MICROPHONE: StreamKey = StreamKey::new(StreamClass::Audio, 0);
const POINTER: StreamKey = StreamKey::new(StreamClass::Input, 0);
const DATA: StreamKey = StreamKey::new(StreamClass::Data, 0);

/// Frame 0 is a keyframe, as every encoder starts with one.
fn send_screen_frame(pair: &mut Pair, tick: u32) {
    let payload = ramp(20_000, tick.to_le_bytes()[0]);
    let slice = EncodedSlice::new(&payload, MediaTimestamp::new(1_500 * tick)).ending_frame();
    let slice = if tick == 0 {
        slice.as_keyframe()
    } else {
        slice
    };
    pair.left_event(&Event::SliceEncoded {
        stream: SCREEN,
        slice,
    });
}

fn open_screen_pair() -> Pair {
    let mut pair = Pair::new(LinkModel::CLEAN);
    let mut config = MediaStreamConfig::screen_share(Role::Sender);
    config.repair = RepairMode::Off;
    pair.left.open_media(SCREEN, config).unwrap();
    config.role = Role::Receiver;
    pair.right.open_media(SCREEN, config).unwrap();
    pair
}

/// The first datagram `session` wants to put on the wire that has the given kind.
fn first_transmitted(session: &mut llts_core::session::PeerSession, kind: PacketKind) -> Vec<u8> {
    while let Some(action) = session.poll_action() {
        if let Action::Transmit { datagram, .. } = action
            && datagram.header().kind == kind
        {
            return datagram.bytes().to_vec();
        }
    }
    panic!("no {kind:?} datagram was transmitted");
}

#[test]
fn a_datagram_for_a_stream_that_is_not_open_is_refused() {
    let mut sender = session(0x1111, 0x2222);
    let mut receiver = session(0x2222, 0x1111);
    sender
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    let payload = ramp(500, 0);
    let now = llts_core::clock::Instant::SESSION_START;
    sender
        .handle(
            now,
            &Event::SliceEncoded {
                stream: SCREEN,
                slice: EncodedSlice::new(&payload, MediaTimestamp::ZERO).ending_frame(),
            },
        )
        .unwrap();
    let media = first_transmitted(&mut sender, PacketKind::Media);

    let refused = receiver.handle(
        now,
        &Event::DatagramReceived {
            path: PathId::default(),
            bytes: &media,
            arrived_at: now.wire_micros(),
        },
    );
    assert!(matches!(refused, Err(CoreError::UnknownStream { .. })));
}

#[test]
fn a_datagram_carrying_another_sessions_tag_is_refused() {
    let mut sender = session(0x1111, 0x2222);
    let mut stranger = session(0x3333, 0x1111);
    let now = llts_core::clock::Instant::SESSION_START;
    sender.poll(now).unwrap();
    let probe = first_transmitted(&mut sender, PacketKind::Probe);
    let refused = stranger.handle(
        now,
        &Event::DatagramReceived {
            path: PathId::default(),
            bytes: &probe,
            arrived_at: now.wire_micros(),
        },
    );
    assert!(matches!(refused, Err(CoreError::UnknownStream { .. })));
}

#[test]
fn audio_is_delivered_frame_by_frame_and_never_retransmitted() {
    let mut pair = Pair::new(LinkModel {
        loss_per_mille: 100,
        ..LinkModel::CLEAN
    });
    pair.left
        .open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Sender))
        .unwrap();
    pair.right
        .open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Receiver))
        .unwrap();

    let frames = 300u32;
    for tick in 0..frames {
        let opus = ramp(120, tick.to_le_bytes()[0]);
        pair.left_event(&Event::SliceEncoded {
            stream: MICROPHONE,
            slice: EncodedSlice::new(&opus, MediaTimestamp::new(480 * tick)).ending_frame(),
        });
        pair.run_for(FrameRate::OPUS_10_MS.period());
    }

    let heard = pair
        .right_seen
        .presented
        .iter()
        .filter(|(stream, _)| *stream == MICROPHONE)
        .count();
    assert!(
        heard > frames as usize * 8 / 10 && heard < frames as usize,
        "{heard} of {frames} audio frames played out"
    );
    let stats = pair.right.stats(MICROPHONE).unwrap();
    assert_eq!(
        stats.retransmissions_requested, 0,
        "audio must never be NACKed"
    );
    assert_eq!(
        stats.frames_superseded, 0,
        "audio frames must not supersede each other"
    );
}

#[test]
fn input_batches_survive_loss_through_redundancy_exactly_once() {
    let mut pair = Pair::new(LinkModel {
        loss_per_mille: 200,
        ..LinkModel::CLEAN
    });
    pair.left.open_input(POINTER).unwrap();
    pair.right.open_input(POINTER).unwrap();

    let mut sent = Vec::new();
    for batch in 0..300u32 {
        let events = batch.to_le_bytes().repeat(3);
        pair.left_event(&Event::InputQueued {
            stream: POINTER,
            events: &events,
        });
        sent.push(events);
        pair.run_for(Duration::from_millis(4));
    }
    pair.run_for(Duration::from_millis(50));

    let delivered: Vec<_> = pair
        .right_seen
        .delivered
        .iter()
        .filter(|(stream, _)| *stream == POINTER)
        .map(|(_, bytes)| bytes.clone())
        .collect();
    let unique: HashSet<_> = delivered.iter().collect();
    assert_eq!(unique.len(), delivered.len(), "a batch was delivered twice");
    assert!(
        delivered.len() >= sent.len() - 2,
        "{} of {} batches arrived through 20 % loss",
        delivered.len(),
        sent.len()
    );
    assert!(delivered.iter().all(|batch| sent.contains(batch)));
}

#[test]
fn a_key_state_snapshot_repeats_every_half_second() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    pair.left.open_input(POINTER).unwrap();
    pair.right.open_input(POINTER).unwrap();
    pair.left_event(&Event::KeyStateUpdated {
        stream: POINTER,
        state: b"shift+a",
    });
    pair.run_for(Duration::from_millis(2_100));

    let snapshots = &pair.right_seen.key_states;
    assert_eq!(
        snapshots.len(),
        5,
        "expected one at once and one every 500 ms"
    );
    assert!(
        snapshots
            .iter()
            .all(|(stream, state)| *stream == POINTER && state == b"shift+a")
    );
}

#[test]
fn a_silent_peer_is_declared_dead_after_five_seconds() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    pair.run_for(Duration::from_secs(2));
    assert_eq!(pair.left_seen.peer_dead, 0, "a live peer was declared dead");

    pair.blackout = true;
    pair.run_for(Duration::from_millis(4_900));
    assert_eq!(
        pair.left_seen.peer_dead, 0,
        "declared dead before five seconds of silence"
    );
    pair.run_for(Duration::from_millis(400));
    assert_eq!(pair.left_seen.peer_dead, 1);
    assert!(pair.left.is_dead());

    pair.blackout = false;
    pair.run_for(Duration::from_secs(2));
    assert!(!pair.left.is_dead(), "a returning peer was not noticed");
}

#[test]
fn a_peer_seen_on_a_new_path_is_challenged_before_traffic_moves() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    pair.run_for(Duration::from_millis(500));

    pair.left_path_at_right = PathId(7);
    pair.run_for(Duration::from_millis(1_500));

    assert_eq!(pair.right_seen.path_changes, vec![PathId(7)]);
    assert_eq!(pair.right.active_path(), PathId(7));
    let after_switch = pair.right_seen.paths_used.iter().rev().take(3);
    assert!(after_switch.into_iter().all(|path| *path == PathId(7)));
}

#[test]
fn an_abandoned_frame_asks_the_sender_for_a_keyframe() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    let mut config = MediaStreamConfig::screen_share(Role::Sender);
    config.repair = RepairMode::Off;
    pair.left.open_media(SCREEN, config).unwrap();
    config.role = Role::Receiver;
    pair.right.open_media(SCREEN, config).unwrap();

    for tick in 0..4 {
        send_screen_frame(&mut pair, tick);
        pair.run_for(FrameRate::STANDARD.period());
    }
    assert!(pair.left_seen.keyframe_requests.is_empty());

    send_screen_frame(&mut pair, 4);
    pair.run_for(Duration::from_millis(1));
    pair.blackout = true;
    pair.run_for(Duration::from_millis(30));
    pair.blackout = false;
    for tick in 5..8 {
        send_screen_frame(&mut pair, tick);
        pair.run_for(FrameRate::STANDARD.period());
    }

    assert_eq!(
        pair.left_seen.keyframe_requests.first(),
        Some(&SCREEN),
        "the sender was never asked for a keyframe"
    );
}

#[test]
fn a_receiver_that_joins_after_the_keyframe_asks_for_one() {
    let mut pair = open_screen_pair();
    for tick in 1..4 {
        send_screen_frame(&mut pair, tick);
        pair.run_for(FrameRate::STANDARD.period());
    }
    assert_eq!(pair.left_seen.keyframe_requests.first(), Some(&SCREEN));
}

#[test]
fn the_application_can_ask_for_a_keyframe_after_dropping_frames() {
    let mut pair = open_screen_pair();
    for tick in 0..3 {
        send_screen_frame(&mut pair, tick);
        pair.run_for(FrameRate::STANDARD.period());
    }
    assert!(pair.left_seen.keyframe_requests.is_empty());

    pair.right_event(&Event::KeyframeNeeded { stream: SCREEN });
    send_screen_frame(&mut pair, 3);
    pair.run_for(FrameRate::STANDARD.period());
    assert_eq!(pair.left_seen.keyframe_requests.first(), Some(&SCREEN));
}

#[test]
fn video_gets_the_estimate_minus_audio_data_and_parity() {
    let mut sender = session(0x1111, 0x2222);
    sender
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    let alone = last_video_target(&mut sender);
    assert_eq!(alone, sender.target_bitrate());

    sender
        .open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Sender))
        .unwrap();
    sender.open_reliable(DATA).unwrap();
    let shared = last_video_target(&mut sender);

    let estimate = u64::from(sender.target_bitrate().bits_per_second());
    let expected = estimate - 64_000 - estimate * 15 / 100;
    assert_eq!(u64::from(shared.bits_per_second()), expected);

    let budget = MediaBudget {
        estimate: sender.target_bitrate(),
        reserved_bits_per_second: 64_000,
        reserves_data_share: true,
        video_streams: 1,
    };
    let protected = budget.encoder_target(RepairRate::from_percent(20));
    assert_eq!(
        u64::from(protected.bits_per_second()),
        expected * 8 / 10,
        "parity must come out of the encoder's share"
    );
}

fn last_video_target(session: &mut llts_core::session::PeerSession) -> Bitrate {
    let mut target = None;
    while let Some(action) = session.poll_action() {
        if let Action::SetTargetBitrate { stream, bitrate } = action
            && stream == SCREEN
        {
            target = Some(bitrate);
        }
    }
    target.unwrap()
}

#[test]
fn control_and_audio_interleave_with_video_without_disturbing_it() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    pair.left
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    pair.right
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Receiver))
        .unwrap();
    pair.left
        .open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Sender))
        .unwrap();
    pair.right
        .open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Receiver))
        .unwrap();

    let mut audio_tick = 0u32;
    for tick in 0..60u32 {
        send_screen_frame(&mut pair, tick);
        let message = [0x00, 0x40, tick.to_le_bytes()[0]];
        pair.left_event(&Event::ControlQueued {
            stream: StreamKey::CONTROL,
            message: &message,
            delivery: Delivery::Ordered,
        });
        for _ in 0..2 {
            let opus = ramp(120, audio_tick.to_le_bytes()[0]);
            pair.left_event(&Event::SliceEncoded {
                stream: MICROPHONE,
                slice: EncodedSlice::new(&opus, MediaTimestamp::new(480 * audio_tick))
                    .ending_frame(),
            });
            audio_tick += 1;
            pair.run_for(FrameRate::STANDARD.period() / 2);
        }
    }
    pair.run_for(Duration::from_millis(100));

    let count = |stream| {
        pair.right_seen
            .presented
            .iter()
            .filter(|(key, _)| *key == stream)
            .count()
    };
    assert_eq!(count(SCREEN), 60);
    assert_eq!(count(MICROPHONE), 120);
    assert_eq!(
        pair.right_seen
            .delivered
            .iter()
            .filter(|(key, _)| *key == StreamKey::CONTROL)
            .count(),
        60
    );
    let stats = pair.right.stats(SCREEN).unwrap();
    assert!(stats.protocol_latency_p99 <= Duration::from_millis(12));
}

#[test]
fn the_header_of_every_paced_packet_names_the_peers_tag() {
    let mut pair = Pair::new(LinkModel::CLEAN);
    pair.left
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    send_screen_frame(&mut pair, 0);
    let media = first_transmitted(&mut pair.left, PacketKind::Media);
    let (header, _) = PacketHeader::read_from(&media).unwrap();
    assert_eq!(
        header.stream,
        SCREEN.with_tag(llts_wire::SessionTag::new(0x2222))
    );
}
