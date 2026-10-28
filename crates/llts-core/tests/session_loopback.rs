#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

mod support;

use core::time::Duration;

use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, Role};
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_core::units::{FrameRate, LossRate};
use llts_wire::{MediaTimestamp, StreamClass};
use support::{LinkModel, Pair, ramp};

const SCREEN: StreamKey = StreamKey::new(StreamClass::Video, 0);

fn screen_pair(model: LinkModel) -> Pair {
    let mut pair = Pair::new(model);
    pair.left
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    pair.right
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Receiver))
        .unwrap();
    pair
}

fn send_frame(pair: &mut Pair, payload: &[u8], tick: u32) {
    let slice = EncodedSlice::new(payload, MediaTimestamp::new(1_500 * tick))
        .as_keyframe()
        .ending_frame();
    pair.left_event(&Event::SliceEncoded {
        stream: SCREEN,
        slice,
    });
    pair.run_for(FrameRate::STANDARD.period());
}

const fn drop_every(every: u64) -> LinkModel {
    LinkModel {
        drop_every: every,
        ..LinkModel::CLEAN
    }
}

fn presented(pair: &Pair) -> Vec<Vec<u8>> {
    pair.right_seen
        .presented
        .iter()
        .filter(|(stream, _)| *stream == SCREEN)
        .map(|(_, bytes)| bytes.clone())
        .collect()
}

#[test]
fn a_frame_crosses_two_sessions_intact() {
    let mut pair = screen_pair(LinkModel::CLEAN);
    let payload = ramp(40_000, 0);
    send_frame(&mut pair, &payload, 1);

    assert_eq!(presented(&pair), vec![payload]);
}

#[test]
fn a_stream_of_frames_all_arrive_in_order() {
    let mut pair = screen_pair(LinkModel::CLEAN);
    let mut expected = Vec::new();

    for tick in 0..30u32 {
        let payload = ramp(20_000, tick.to_le_bytes()[0]);
        expected.push(payload.clone());
        send_frame(&mut pair, &payload, tick);
    }

    assert_eq!(presented(&pair), expected);
}

#[test]
fn a_lossy_link_still_delivers_every_frame() {
    let mut pair = screen_pair(drop_every(23));
    let mut expected = Vec::new();

    for tick in 0..40u32 {
        let payload = ramp(20_000, tick.to_le_bytes()[0]);
        expected.push(payload.clone());
        send_frame(&mut pair, &payload, tick);
    }

    assert!(pair.dropped() > 20, "the loss model never fired");
    let presented = presented(&pair);
    assert_eq!(
        presented.len(),
        expected.len(),
        "{} of {} frames survived {} drops",
        presented.len(),
        expected.len(),
        pair.dropped()
    );
    assert_eq!(presented, expected);
}

#[test]
fn a_heavily_lossy_link_degrades_rather_than_collapses() {
    // Roughly 6 % packet loss. The first frames go out unprotected because the sender has not
    // measured any loss yet, so losing one during the ramp-in is the expected shape: the point is
    // that the policy then adapts and the rest survive.
    let mut pair = screen_pair(drop_every(17));
    let frames = 25u32;
    for tick in 0..frames {
        let payload = ramp(20_000, tick.to_le_bytes()[0]);
        send_frame(&mut pair, &payload, tick);
    }

    let stats = pair.right.stats(SCREEN).unwrap();
    assert!(
        stats.frames_presented >= u64::from(frames) - 1,
        "only {} of {frames} frames survived {} drops",
        stats.frames_presented,
        pair.dropped()
    );
    assert!(
        stats.fragments_repaired > 0 || stats.retransmissions_requested > 0,
        "frames survived loss without any repair being recorded"
    );
    assert!(
        pair.left.link_quality().loss_rate > LossRate::NONE,
        "the sender never noticed the link was losing packets"
    );
}

#[test]
fn the_sender_learns_a_bitrate_from_feedback() {
    let mut pair = screen_pair(LinkModel::CLEAN);
    let before = pair.left.target_bitrate();

    for tick in 0..60u32 {
        let payload = ramp(30_000, tick.to_le_bytes()[0]);
        send_frame(&mut pair, &payload, tick);
    }

    assert!(
        pair.left.target_bitrate() != before,
        "the rate controller never moved despite a full feedback loop"
    );
    assert!(
        pair.left_seen
            .bitrates
            .iter()
            .any(|(stream, _)| *stream == SCREEN),
        "the encoder was never told a target"
    );
}

#[test]
fn protocol_latency_stays_inside_the_budget_on_a_clean_link() {
    let mut pair = screen_pair(LinkModel::CLEAN);
    for tick in 0..60u32 {
        let payload = ramp(20_000, tick.to_le_bytes()[0]);
        send_frame(&mut pair, &payload, tick);
    }

    let stats = pair.right.stats(SCREEN).unwrap();
    assert!(
        stats.protocol_latency_p99 <= Duration::from_millis(12),
        "p99 assembly-to-present was {:?}, over the 12 ms budget",
        stats.protocol_latency_p99
    );
}

#[test]
fn the_session_always_names_when_to_wake_while_work_remains() {
    let mut pair = screen_pair(LinkModel::CLEAN);
    let payload = ramp(60_000, 0);
    let slice = EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame();
    pair.left_event(&Event::SliceEncoded {
        stream: SCREEN,
        slice,
    });

    let mut armed = false;
    while let Some(action) = pair.left.poll_action() {
        if let Action::ArmTimer(deadline) = action {
            armed = true;
            assert!(!deadline.is_after(pair.now.saturating_add(Duration::from_secs(1))));
        }
    }
    assert!(armed, "the session queued work without asking to be woken");
}
