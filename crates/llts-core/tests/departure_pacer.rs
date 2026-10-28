#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use core::time::Duration;

use llts_core::buffer::{BufferPool, Datagram};
use llts_core::clock::Instant;
use llts_core::send::{DatagramSizing, DeparturePacer, EncodedSlice, FramePacketizer};
use llts_core::units::{Bitrate, FrameRate};
use llts_wire::{MediaTimestamp, MonotonicMicros, StreamId};

const MTU: usize = 1200;
const SCREEN: StreamId = StreamId::new(1);
const AT_ONE_MS: MonotonicMicros = MonotonicMicros::new(1_000);

fn frame_datagrams(byte_len: usize) -> (Vec<Datagram>, BufferPool) {
    let pool = BufferPool::new(MTU, 512);
    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut packetizer = FramePacketizer::new(SCREEN, sizing, pool.clone());
    let payload = vec![0xA5u8; byte_len];
    let mut sent = Vec::new();
    packetizer
        .packetize(
            EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
            AT_ONE_MS,
            &mut sent,
        )
        .unwrap();
    (sent, pool)
}

/// Drains the pacer from `start`, stepping the clock one microsecond at a time, and returns the
/// release instant of each datagram.
fn drain(pacer: &mut DeparturePacer, start: Instant, limit: Duration) -> Vec<Instant> {
    let mut released = Vec::new();
    let mut now = start;
    let deadline = start.saturating_add(limit);
    while !now.is_after(deadline) {
        while let Some(_datagram) = pacer.poll_departure(now) {
            released.push(now);
        }
        now = Instant::from_micros(now.as_micros() + 25);
    }
    released
}

#[test]
fn an_idle_pacer_releases_a_burst_immediately() {
    let (datagrams, _pool) = frame_datagrams(40_000);
    let mut pacer = DeparturePacer::new(Bitrate::STANDARD, 1 << 20);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    let now = Instant::from_micros(10_000_000);
    let mut immediate = 0;
    while pacer.poll_departure(now).is_some() {
        immediate += 1;
    }

    let allowed = Bitrate::STANDARD.bytes_in(DeparturePacer::DEFAULT_BURST_TOLERANCE);
    assert!(immediate > 0, "an idle pacer released nothing");
    assert!(
        immediate <= allowed / 1_000 + 2,
        "released {immediate} datagrams, more than the burst tolerance allows"
    );
}

#[test]
fn a_frame_drains_within_its_period_at_the_paced_rate() {
    // 20 Mbit/s of media paced at 2.5x drains a 41 kB frame in well under one 60 Hz period.
    let media = Bitrate::from_kilobits_per_second(20_000).unwrap();
    let pacing = media.scaled_by(5, 2);
    let (datagrams, _pool) = frame_datagrams(41_000);
    let count = datagrams.len();

    let mut pacer = DeparturePacer::new(pacing, 1 << 20);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    let start = Instant::from_micros(1_000_000);
    let released = drain(&mut pacer, start, FrameRate::STANDARD.period());

    assert_eq!(
        released.len(),
        count,
        "the frame did not drain in one period"
    );
    let span = released
        .last()
        .unwrap()
        .saturating_elapsed_since(*released.first().unwrap());
    assert!(
        span < FrameRate::STANDARD.period(),
        "the frame took {span:?}, longer than a 60 Hz period"
    );
}

#[test]
fn the_release_rate_tracks_the_configured_rate() {
    let rate = Bitrate::from_kilobits_per_second(8_000).unwrap();
    let (datagrams, _pool) = frame_datagrams(60_000);
    let total_bytes: usize = datagrams.iter().map(Datagram::byte_len).sum();

    let mut pacer = DeparturePacer::new(rate, 1 << 20);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    let start = Instant::from_micros(0);
    let released = drain(&mut pacer, start, Duration::from_millis(200));
    let span = released
        .last()
        .unwrap()
        .saturating_elapsed_since(*released.first().unwrap());

    let ideal = rate.transmission_time(u32::try_from(total_bytes).unwrap());
    let ratio = span.as_micros() as f64 / ideal.as_micros() as f64;
    assert!(
        (0.85..=1.15).contains(&ratio),
        "drained in {span:?} against an ideal {ideal:?} (ratio {ratio:.2})"
    );
}

#[test]
fn drain_time_reports_the_self_inflicted_delay() {
    let rate = Bitrate::from_kilobits_per_second(10_000).unwrap();
    let (datagrams, _pool) = frame_datagrams(50_000);
    let mut pacer = DeparturePacer::new(rate, 1 << 20);

    assert_eq!(pacer.drain_time(), Duration::ZERO);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    let queued = u32::try_from(pacer.queued_bytes()).unwrap();
    assert_eq!(pacer.drain_time(), rate.transmission_time(queued));
    assert!(pacer.drain_time() > Duration::from_millis(35));
}

#[test]
fn an_overflowing_queue_drops_the_stalest_datagram() {
    let (datagrams, _pool) = frame_datagrams(40_000);
    let count = datagrams.len();
    let capacity = MTU * 4;

    let mut pacer = DeparturePacer::new(Bitrate::LOW, capacity);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    assert!(pacer.queued_bytes() <= capacity);
    assert!(
        pacer.queued_datagrams() < count,
        "nothing was evicted despite the queue being over budget"
    );
    assert_eq!(
        usize::try_from(pacer.dropped_on_overflow()).unwrap(),
        count - pacer.queued_datagrams(),
        "every evicted datagram should be counted"
    );
}

#[test]
fn the_survivors_of_an_overflow_are_the_freshest() {
    let pool = BufferPool::new(MTU, 32);
    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut packetizer = FramePacketizer::new(SCREEN, sizing, pool);
    let mut pacer = DeparturePacer::new(Bitrate::LOW, MTU * 2);

    let payload = vec![0u8; 600];
    let mut sequences = Vec::new();
    for tick in 0..8u32 {
        let mut sent = Vec::new();
        packetizer
            .packetize(
                EncodedSlice::new(&payload, MediaTimestamp::new(tick)).ending_frame(),
                AT_ONE_MS,
                &mut sent,
            )
            .unwrap();
        for datagram in sent {
            sequences.push(datagram.header().sequence);
            pacer.enqueue(datagram);
        }
    }

    let now = Instant::from_micros(1_000_000);
    let mut released = Vec::new();
    while let Some(datagram) = pacer.poll_departure(now) {
        released.push(datagram.header().sequence);
    }

    let newest: Vec<_> = sequences[sequences.len() - released.len()..].to_vec();
    assert_eq!(
        released, newest,
        "the pacer kept stale datagrams over fresh"
    );
}

#[test]
fn an_empty_pacer_asks_for_no_timer() {
    let pacer = DeparturePacer::new(Bitrate::STANDARD, 1 << 20);
    assert_eq!(pacer.next_departure(Instant::from_micros(500)), None);
}

#[test]
fn a_backlogged_pacer_names_when_to_wake() {
    let (datagrams, _pool) = frame_datagrams(40_000);
    let mut pacer = DeparturePacer::new(Bitrate::LOW, 1 << 20);
    for datagram in datagrams {
        pacer.enqueue(datagram);
    }

    let start = Instant::from_micros(5_000_000);
    while pacer.poll_departure(start).is_some() {}

    let wake = pacer.next_departure(start).unwrap();
    assert!(
        wake.is_after(start),
        "the pacer asked to be woken in the past despite being rate limited"
    );
    assert!(pacer.poll_departure(wake).is_some());
}

#[test]
fn raising_the_rate_drains_the_backlog_faster() {
    let slow = Bitrate::from_kilobits_per_second(4_000).unwrap();
    let fast = Bitrate::from_kilobits_per_second(40_000).unwrap();

    let mut spans = Vec::new();
    for rate in [slow, fast] {
        let (datagrams, _pool) = frame_datagrams(40_000);
        let mut pacer = DeparturePacer::new(rate, 1 << 20);
        for datagram in datagrams {
            pacer.enqueue(datagram);
        }
        let released = drain(
            &mut pacer,
            Instant::from_micros(0),
            Duration::from_millis(500),
        );
        spans.push(
            released
                .last()
                .unwrap()
                .saturating_elapsed_since(*released.first().unwrap()),
        );
    }

    assert!(
        spans[1] * 4 < spans[0],
        "a ten times faster rate drained in {:?} against {:?}",
        spans[1],
        spans[0]
    );
}
