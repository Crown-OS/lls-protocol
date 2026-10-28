#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "test arithmetic over a simulated clock"
)]

mod support;

use llts_core::clock::Instant;
use llts_core::session::ClockSync;
use llts_wire::MonotonicMicros;
use support::Lcg;

/// The peer's clock runs this fast relative to ours, in parts per million.
const SKEW_PPM: f64 = 100.0;
const PEER_START_MICROS: f64 = 5_000_000.0;

fn peer_clock(local_micros: u64) -> f64 {
    PEER_START_MICROS + local_micros as f64 * (1.0 + SKEW_PPM / 1_000_000.0)
}

/// Runs probe exchanges every `interval` micros for `duration` micros over a path whose two
/// legs each take 400 µs plus up to `jitter` of independent queueing.
fn converge(interval: u64, duration: u64, jitter: u32) -> (ClockSync, Instant) {
    let mut sync = ClockSync::new();
    let mut random = Lcg::new(42);
    let mut local = 1_000_000u64;
    let end = local + duration;
    while local < end {
        let outbound = 400 + u64::from(random.below(jitter));
        let inbound = 400 + u64::from(random.below(jitter));
        let turnaround = 30;
        let peer_received = peer_clock(local + outbound);
        let peer_sent = peer_clock(local + outbound + turnaround);
        let received = local + outbound + turnaround + inbound;
        sync.observe(
            Instant::from_micros(local),
            MonotonicMicros::new(peer_received as u64 as u32),
            MonotonicMicros::new(peer_sent as u64 as u32),
            Instant::from_micros(received),
        );
        local += interval;
    }
    (sync, Instant::from_micros(local))
}

/// Offset error at the end of a run, in microseconds, and the skew estimate's error in ppb.
fn errors(sync: &ClockSync, now: Instant) -> (i64, i64) {
    let peer_now = peer_clock(now.as_micros());
    let estimated_local = sync
        .to_local(MonotonicMicros::new(peer_now as u64 as u32), now)
        .as_micros();
    let offset_error = estimated_local as i64 - now.as_micros() as i64;
    let skew_error = sync.skew_ppb() + (SKEW_PPM * 1_000.0) as i64;
    (offset_error, skew_error)
}

#[test]
fn the_offset_tracks_a_hundred_ppm_skew_within_half_a_millisecond() {
    // Up to 3 ms of independent queueing on each leg: far worse than a healthy Wi-Fi hop.
    let (sync, now) = converge(250_000, 60_000_000, 3_000);
    let (offset_error, skew_error) = errors(&sync, now);
    assert!(
        offset_error.abs() <= 500,
        "estimate is {offset_error} µs off after a minute at {SKEW_PPM} ppm"
    );
    assert!(
        skew_error.abs() <= 30_000,
        "skew estimate {} ppb is {skew_error} ppb off",
        sync.skew_ppb()
    );
}

#[test]
fn a_quiet_link_pins_the_skew_closely() {
    let (sync, now) = converge(250_000, 60_000_000, 300);
    let (offset_error, skew_error) = errors(&sync, now);
    assert!(offset_error.abs() <= 100, "offset {offset_error} µs off");
    assert!(skew_error.abs() <= 5_000, "skew {skew_error} ppb off");
}

#[test]
fn the_estimate_survives_the_peer_clock_wrapping() {
    let mut sync = ClockSync::new();
    let wraps_at = u64::from(u32::MAX) - 2_000_000;
    let mut local = 10_000u64;
    for _ in 0..40 {
        let peer = (local + wraps_at) as u32;
        sync.observe(
            Instant::from_micros(local),
            MonotonicMicros::new(peer.wrapping_add(300)),
            MonotonicMicros::new(peer.wrapping_add(320)),
            Instant::from_micros(local + 620),
        );
        local += 250_000;
    }
    let peer_now = (local + wraps_at) as u32;
    let estimated = sync
        .to_local(MonotonicMicros::new(peer_now), Instant::from_micros(local))
        .as_micros();
    assert!((estimated as i64 - local as i64).abs() <= 200);
}
