#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::receive::{Arrival, GapTracker, SequenceWindow};
use llts_core::units::LossRate;
use llts_wire::SequenceNumber;

const fn sequence(value: u16) -> SequenceNumber {
    SequenceNumber::new(value)
}

fn gaps(tracker: &GapTracker, depth: u16) -> Vec<u16> {
    tracker
        .gaps_within(depth)
        .map(SequenceNumber::get)
        .collect()
}

#[test]
fn a_session_starting_mid_stream_reports_no_history_as_lost() {
    let mut tracker = GapTracker::new();
    assert_eq!(tracker.record(sequence(40_000), false), Arrival::Advanced);

    assert!(
        gaps(&tracker, 64).is_empty(),
        "everything before the first packet was counted as loss"
    );
    assert_eq!(tracker.loss_rate(), LossRate::NONE);
}

#[test]
fn an_unbroken_stream_reports_no_loss() {
    let mut tracker = GapTracker::new();
    for value in 100..400 {
        assert_eq!(tracker.record(sequence(value), false), Arrival::Advanced);
    }

    assert!(gaps(&tracker, 128).is_empty());
    assert_eq!(tracker.loss_rate(), LossRate::NONE);
}

#[test]
fn a_hole_is_reported_until_it_is_filled() {
    let mut tracker = GapTracker::new();
    for value in [10, 11, 13, 14] {
        tracker.record(sequence(value), false);
    }
    assert_eq!(gaps(&tracker, 16), vec![12]);

    assert_eq!(tracker.record(sequence(12), false), Arrival::Backfilled);
    assert!(gaps(&tracker, 16).is_empty());
}

#[test]
fn a_hole_filled_by_a_retransmission_still_counts_against_the_link() {
    let mut tracker = GapTracker::new();
    for value in 0..50 {
        tracker.record(sequence(value), false);
    }
    for value in 51..60 {
        tracker.record(sequence(value), false);
    }
    let with_hole = tracker.loss_rate();

    tracker.record(sequence(50), true);

    assert_eq!(
        tracker.loss_rate(),
        with_hole,
        "a packet the sender had to resend was forgiven, so the link looks cleaner than it is"
    );
}

#[test]
fn filling_a_hole_takes_the_loss_back() {
    let mut tracker = GapTracker::new();
    for value in 0..50 {
        tracker.record(sequence(value), false);
    }
    for value in 51..60 {
        tracker.record(sequence(value), false);
    }
    let with_hole = tracker.loss_rate();
    assert!(with_hole > LossRate::NONE);

    tracker.record(sequence(50), false);
    assert!(
        tracker.loss_rate() < with_hole,
        "a repaired packet still counted against the link"
    );
}

#[test]
fn reordering_inside_the_window_is_not_a_duplicate() {
    let mut tracker = GapTracker::new();
    tracker.record(sequence(1), false);
    tracker.record(sequence(4), false);

    assert_eq!(tracker.record(sequence(2), false), Arrival::Backfilled);
    assert_eq!(tracker.record(sequence(3), false), Arrival::Backfilled);
    assert_eq!(tracker.record(sequence(2), false), Arrival::Duplicate);
}

#[test]
fn a_repeated_head_packet_is_a_duplicate() {
    let mut tracker = GapTracker::new();
    tracker.record(sequence(7), false);
    assert_eq!(tracker.record(sequence(7), false), Arrival::Duplicate);
}

#[test]
fn gaps_are_tracked_across_the_wraparound() {
    let mut tracker = GapTracker::new();
    for value in [65_533, 65_534, 1, 2] {
        tracker.record(sequence(value), false);
    }

    assert_eq!(gaps(&tracker, 8), vec![65_535, 0]);
    assert_eq!(tracker.record(sequence(65_535), false), Arrival::Backfilled);
    assert_eq!(tracker.record(sequence(0), false), Arrival::Backfilled);
    assert!(gaps(&tracker, 8).is_empty());
}

#[test]
fn a_packet_older_than_the_window_is_forgotten_not_duplicated() {
    let mut tracker = GapTracker::new();
    tracker.record(sequence(0), false);
    for value in 1..=SequenceWindow::CAPACITY + 10 {
        tracker.record(sequence(value), false);
    }

    assert_eq!(tracker.record(sequence(0), false), Arrival::Forgotten);
}

#[test]
fn the_loss_rate_follows_the_measured_ratio() {
    let mut tracker = GapTracker::new();
    // Drop every tenth packet.
    for value in 0..500u16 {
        if value % 10 != 0 {
            tracker.record(sequence(value), false);
        }
    }

    let observed = tracker.loss_rate();
    assert!(
        observed > LossRate::from_per_mille(70) && observed < LossRate::from_per_mille(130),
        "expected roughly 10 % loss, measured {observed:?}"
    );
}

#[test]
fn the_loss_rate_recovers_once_the_link_settles() {
    let mut tracker = GapTracker::new();
    for value in 0..200u16 {
        if value % 4 != 0 {
            tracker.record(sequence(value), false);
        }
    }
    let while_bad = tracker.loss_rate();

    for value in 200..3_000u16 {
        tracker.record(sequence(value), false);
    }

    assert!(
        tracker.loss_rate() < while_bad,
        "the estimate stayed at {while_bad:?} long after the link recovered"
    );
    assert!(tracker.loss_rate() < LossRate::from_per_mille(5));
}

#[test]
fn a_large_jump_forward_does_not_report_the_whole_window_as_gaps() {
    let mut tracker = GapTracker::new();
    tracker.record(sequence(0), false);
    tracker.record(sequence(5_000), false);

    assert!(
        gaps(&tracker, 32).len() <= 31,
        "the gap scan ran past the depth it was given"
    );
    assert_eq!(tracker.highest_seen(), Some(sequence(5_000)));
}

#[test]
fn contains_answers_only_within_the_window() {
    let mut tracker = GapTracker::new();
    tracker.record(sequence(100), false);
    tracker.record(sequence(102), false);

    assert!(tracker.contains(sequence(100)));
    assert!(!tracker.contains(sequence(101)));
    assert!(tracker.contains(sequence(102)));
    assert!(
        !tracker.contains(sequence(103)),
        "a future sequence cannot have arrived"
    );
}
