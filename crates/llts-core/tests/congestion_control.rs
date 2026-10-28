#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use core::time::Duration;

use llts_core::clock::Instant;
use llts_core::congestion::{AckedPacket, CongestionController, NetworkUsage};
use llts_core::repair::RepairRate;
use llts_core::units::{Bitrate, FrameRate};
use llts_wire::MonotonicMicros;

const PACKET_BYTES: u32 = 1_170;
const FLOOR: Bitrate = Bitrate::LOW.scaled_by(1, 12);

const fn fresh_controller() -> CongestionController {
    CongestionController::new(Bitrate::STANDARD, FLOOR, Bitrate::HIGH)
}

/// Feeds `count` packets whose arrival spacing exceeds their departure spacing by `stretch_micros`,
/// which is exactly what a growing queue looks like.
fn feed(
    controller: &mut CongestionController,
    count: u32,
    departure_step: u32,
    stretch_micros: u32,
) {
    let mut departure = 1_000u32;
    let mut arrival = 5_000u32;
    for index in 0..count {
        controller.observe(
            AckedPacket {
                departure: MonotonicMicros::new(departure),
                arrival: MonotonicMicros::new(arrival),
                byte_len: PACKET_BYTES,
            },
            Instant::from_micros(u64::from(arrival)),
        );
        departure = departure.wrapping_add(departure_step);
        arrival = arrival.wrapping_add(departure_step + stretch_micros);
        let _ = index;
    }
}

#[test]
fn a_clean_path_reads_as_normal_and_the_rate_climbs() {
    let mut controller = fresh_controller();
    let before = controller.target_bitrate();

    feed(&mut controller, 200, 4_000, 0);

    assert_eq!(controller.usage(), NetworkUsage::Normal);
    assert!(
        controller.target_bitrate() > before,
        "a clean path never reclaimed any headroom"
    );
}

/// The threshold starts deliberately loose and tightens as the link proves itself quiet, so a
/// queue smaller than the initial threshold is invisible until the detector has calibrated.
#[test]
fn an_uncalibrated_detector_ignores_a_small_queue() {
    let mut controller = fresh_controller();
    feed(&mut controller, 150, 4_000, 900);

    assert_eq!(controller.usage(), NetworkUsage::Normal);
}

#[test]
fn a_growing_queue_is_detected_and_the_rate_backs_off() {
    let mut controller = fresh_controller();
    feed(&mut controller, 60, 4_000, 0);
    let steady = controller.target_bitrate();

    feed(&mut controller, 120, 4_000, 900);

    assert_eq!(controller.usage(), NetworkUsage::Overusing);
    assert!(
        controller.target_bitrate() < steady,
        "the rate held at {steady:?} while the path was queueing"
    );
    assert!(controller.queue_estimate_millis() > 0.0);
}

#[test]
fn the_rate_never_leaves_its_bounds() {
    let mut controller = CongestionController::new(Bitrate::STANDARD, FLOOR, Bitrate::HIGH);

    feed(&mut controller, 400, 4_000, 4_000);
    assert!(controller.target_bitrate() >= FLOOR, "fell below the floor");

    let mut climbing = fresh_controller();
    feed(&mut climbing, 2_000, 4_000, 0);
    assert!(
        climbing.target_bitrate() <= Bitrate::HIGH,
        "rose above the ceiling"
    );
}

#[test]
fn pacing_runs_above_the_media_rate() {
    let controller = fresh_controller();
    let media = controller.target_bitrate().bits_per_second();
    let pacing = controller.pacing_rate().bits_per_second();

    assert!(
        pacing > media,
        "pacing at the media rate would put every frame's tail a full period late"
    );
    assert_eq!(
        pacing,
        media / CongestionController::PACING_DENOMINATOR * CongestionController::PACING_NUMERATOR
    );
}

#[test]
fn the_frame_budget_reserves_room_for_parity() {
    let controller = fresh_controller();
    let unprotected = controller.frame_byte_budget(FrameRate::STANDARD, RepairRate::NONE);
    let protected = controller.frame_byte_budget(FrameRate::STANDARD, RepairRate::from_percent(25));

    assert!(protected < unprotected, "parity was not paid for");
    assert!(
        unprotected > 0 && protected > 0,
        "the encoder was handed a budget of nothing"
    );
}

#[test]
fn the_frame_budget_tracks_the_frame_rate() {
    let controller = fresh_controller();
    let at_sixty = controller.frame_byte_budget(FrameRate::STANDARD, RepairRate::NONE);
    let at_one_twenty = controller.frame_byte_budget(FrameRate::SUPER_SMOOTH, RepairRate::NONE);

    assert!(
        at_one_twenty < at_sixty,
        "twice the frames should mean half the bytes each"
    );
}

#[test]
fn padding_fills_the_gap_an_underrunning_encoder_leaves() {
    let controller = fresh_controller();
    let produced = 1_000;
    let deficit = controller.padding_deficit(produced, FrameRate::STANDARD);

    assert!(
        deficit > 0,
        "an underrunning encoder left the delay estimator nothing to read"
    );

    let saturated = controller.padding_deficit(u32::MAX, FrameRate::STANDARD);
    assert_eq!(
        saturated, 0,
        "padded a frame that already filled its budget"
    );
}

#[test]
fn a_settled_controller_does_not_thrash_within_one_frame() {
    let mut controller = fresh_controller();
    feed(&mut controller, 40, 4_000, 0);
    let first = controller.target_bitrate();

    // Every observation lands inside the same frame period, so nothing should move.
    for _ in 0..20 {
        controller.observe(
            AckedPacket {
                departure: MonotonicMicros::new(1_000),
                arrival: MonotonicMicros::new(5_000),
                byte_len: PACKET_BYTES,
            },
            Instant::from_micros(1),
        );
    }
    assert_eq!(controller.target_bitrate(), first);
}

#[test]
fn a_recovered_path_climbs_back() {
    let mut controller = fresh_controller();
    // A clean stretch first, so the adaptive threshold has calibrated to the link before the
    // queue appears. A detector judged on its very first samples has nothing to compare against.
    feed(&mut controller, 60, 4_000, 0);
    feed(&mut controller, 150, 4_000, 900);
    let depressed = controller.target_bitrate();
    assert_eq!(controller.usage(), NetworkUsage::Overusing);

    feed(&mut controller, 600, 4_000, 0);

    assert_eq!(controller.usage(), NetworkUsage::Normal);
    assert!(
        controller.target_bitrate() > depressed,
        "the rate stayed at {depressed:?} after the path recovered"
    );
}

#[test]
fn a_single_late_packet_is_not_mistaken_for_congestion() {
    let mut controller = fresh_controller();
    feed(&mut controller, 100, 4_000, 0);

    controller.observe(
        AckedPacket {
            departure: MonotonicMicros::new(500_000),
            arrival: MonotonicMicros::new(520_000),
            byte_len: PACKET_BYTES,
        },
        Instant::from_micros(520_000),
    );

    assert_ne!(
        controller.usage(),
        NetworkUsage::Overusing,
        "one late aggregate should not count as a queue"
    );
}

#[test]
fn the_playout_target_never_exceeds_a_frame_period() {
    use llts_core::receive::AdaptivePlayoutDelay;

    let mut delay = AdaptivePlayoutDelay::new(FrameRate::STANDARD.period());
    for _ in 0..500 {
        delay.observe(Duration::from_millis(80));
    }

    assert_eq!(delay.target(), FrameRate::STANDARD.period());
}

#[test]
fn a_steady_arrival_cadence_needs_almost_no_playout_delay() {
    use llts_core::receive::AdaptivePlayoutDelay;

    let mut delay = AdaptivePlayoutDelay::new(FrameRate::STANDARD.period());
    for _ in 0..500 {
        delay.observe(Duration::from_micros(120));
    }

    assert!(
        delay.target() <= Duration::from_micros(500),
        "a clean link was given {:?} of buffering",
        delay.target()
    );
}
