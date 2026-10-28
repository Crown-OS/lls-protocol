#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use core::time::Duration;

use llts_core::buffer::{BufferPool, Datagram};
use llts_core::clock::Instant;
use llts_core::send::{DatagramSizing, EncodedSlice, FramePacketizer, LanePacer, PacerLane};
use llts_core::units::Bitrate;
use llts_wire::{MediaTimestamp, MonotonicMicros, StreamId};

const MTU: usize = 1200;

fn datagrams(count: usize) -> Vec<Datagram> {
    let pool = BufferPool::new(MTU, 512);
    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut packetizer = FramePacketizer::new(StreamId::new(1), sizing, pool);
    let payload = vec![7u8; sizing.max_payload_len() * count];
    let mut out = Vec::new();
    packetizer
        .packetize(
            EncodedSlice::new(&payload, MediaTimestamp::ZERO).ending_frame(),
            MonotonicMicros::ZERO,
            &mut out,
        )
        .unwrap();
    out
}

#[test]
fn control_input_and_audio_overtake_a_queued_video_burst() {
    let mut pacer = LanePacer::new(Bitrate::STANDARD, 1 << 22);
    let mut pool = datagrams(23).into_iter();
    for _ in 0..20 {
        pacer.enqueue(PacerLane::Video, pool.next().unwrap());
    }
    pacer.enqueue(PacerLane::Audio, pool.next().unwrap());
    pacer.enqueue(PacerLane::Input, pool.next().unwrap());
    pacer.enqueue(PacerLane::Control, pool.next().unwrap());

    let mut now = Instant::SESSION_START;
    let mut order = Vec::new();
    while order.len() < 4 {
        if let Some((lane, _)) = pacer.poll_departure(now) {
            order.push(lane);
        } else {
            now = pacer.next_departure(now).unwrap();
        }
    }
    assert_eq!(
        order,
        vec![
            PacerLane::Control,
            PacerLane::Input,
            PacerLane::Audio,
            PacerLane::Video
        ]
    );
}

#[test]
fn data_is_held_to_its_share_under_video_load() {
    let mut pacer = LanePacer::new(Bitrate::STANDARD, 1 << 24);
    for datagram in datagrams(400) {
        pacer.enqueue(PacerLane::Video, datagram);
    }
    for datagram in datagrams(400) {
        pacer.enqueue(PacerLane::Data, datagram);
    }

    let mut now = Instant::SESSION_START;
    let (mut data, mut total) = (0u32, 0u32);
    let end = now.saturating_add(Duration::from_millis(200));
    while now < end {
        match pacer.poll_departure(now) {
            Some((lane, _)) => {
                total += 1;
                data += u32::from(lane == PacerLane::Data);
            }
            None => now = pacer.next_departure(now).unwrap(),
        }
    }
    let share = data * 100 / total;
    assert!(
        (12..=18).contains(&share),
        "data took {share} % of {total} departures"
    );
}
