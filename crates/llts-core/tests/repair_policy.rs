#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use core::time::Duration;

use llts_core::link::{LinkKind, LinkQuality};
use llts_core::repair::{RepairMode, RepairPolicy, RepairRate};
use llts_core::units::LossRate;

const DATA_SHARDS: u16 = 32;

const fn wifi(loss_per_mille: u16, round_trip_micros: u64) -> LinkQuality {
    LinkQuality {
        kind: LinkKind::Wifi,
        loss_rate: LossRate::from_per_mille(loss_per_mille),
        round_trip: Duration::from_micros(round_trip_micros),
    }
}

#[test]
fn a_quiet_lan_pays_nothing_for_parity() {
    let plan = RepairPolicy::default().plan(DATA_SHARDS, &wifi(1, 3_000));

    assert!(
        !plan.sends_parity(),
        "spent parity on a link that is not losing"
    );
    assert!(
        plan.requests_retransmission(),
        "a 3 ms round trip leaves ample room to ask again"
    );
}

#[test]
fn moderate_loss_buys_parity_and_keeps_asking() {
    let plan = RepairPolicy::default().plan(DATA_SHARDS, &wifi(10, 3_000));

    assert!(plan.sends_parity());
    assert!(plan.requests_retransmission());
    assert!(
        plan.rate <= RepairPolicy::ASSISTED_CEILING,
        "over-provisioned parity while retransmission was still carrying residual loss"
    );
}

#[test]
fn parity_covers_the_variance_not_just_the_mean() {
    // 32 shards at 2 % loss expects 0.64 losses, so provisioning for the mean would send a single
    // parity shard and fail whenever two go missing. Three standard deviations of a binomial adds
    // the margin that makes that rare.
    let plan = RepairPolicy::default().plan(DATA_SHARDS, &wifi(20, 3_000));

    assert!(
        plan.parity_shards >= 4,
        "only {} shards for a block that regularly loses two",
        plan.parity_shards
    );
    assert!(plan.rate <= RepairPolicy::ASSISTED_CEILING);
    assert!(plan.requests_retransmission());
}

#[test]
fn the_provisioning_margin_shrinks_as_a_block_grows() {
    // The margin is proportional to sqrt(k), so it costs proportionally less on a larger block.
    let small = RepairPolicy::provisioned_rate(LossRate::from_per_mille(20), 16);
    let large = RepairPolicy::provisioned_rate(LossRate::from_per_mille(20), 256);

    assert!(
        large < small,
        "a 256-shard block should need a smaller fraction than a 16-shard one, got {large:?} against {small:?}"
    );
}

#[test]
fn a_clean_link_provisions_nothing() {
    assert_eq!(
        RepairPolicy::provisioned_rate(LossRate::NONE, 32),
        RepairRate::NONE
    );
}

#[test]
fn heavy_loss_abandons_retransmission_for_parity() {
    let plan = RepairPolicy::default().plan(DATA_SHARDS, &wifi(80, 3_000));

    assert!(
        !plan.requests_retransmission(),
        "kept asking on a link where one round is unlikely to be enough"
    );
    assert!(plan.rate >= RepairPolicy::HEAVY_LOSS_FLOOR);
}

#[test]
fn a_long_round_trip_abandons_retransmission_even_when_loss_is_low() {
    let plan = RepairPolicy::default().plan(DATA_SHARDS, &wifi(1, 12_000));

    assert!(
        !plan.requests_retransmission(),
        "a 12 ms round trip cannot answer inside a 60 Hz frame period"
    );
    assert!(
        plan.sends_parity(),
        "dropped both forms of protection at once"
    );
}

#[test]
fn usb_never_sends_parity() {
    let policy = RepairPolicy::default();
    for loss in [0, 10, 80, 200] {
        let plan = policy.plan(
            DATA_SHARDS,
            &LinkQuality {
                kind: LinkKind::Usb,
                loss_rate: LossRate::from_per_mille(loss),
                round_trip: Duration::from_micros(800),
            },
        );
        assert!(
            !plan.sends_parity(),
            "spent parity on USB at {loss} per mille, where CRC and retry already handle it"
        );
        assert!(plan.requests_retransmission());
    }
}

#[test]
fn the_off_mode_is_honoured_however_bad_the_link_gets() {
    let policy = RepairPolicy::new(RepairMode::Off);
    let plan = policy.plan(DATA_SHARDS, &wifi(200, 20_000));

    assert!(!plan.sends_parity());
    assert!(!plan.requests_retransmission());
}

#[test]
fn a_fixed_rate_ignores_the_link() {
    let policy = RepairPolicy::new(RepairMode::Fixed(RepairRate::SUNSHINE_DEFAULT));

    for quality in [wifi(0, 1_000), wifi(90, 30_000)] {
        let plan = policy.plan(DATA_SHARDS, &quality);
        assert_eq!(plan.rate, RepairRate::SUNSHINE_DEFAULT);
        assert_eq!(plan.parity_shards, 7, "20 % of 32 shards, rounded up");
    }
}

#[test]
fn adaptive_bounds_are_respected_at_both_ends() {
    let policy = RepairPolicy::new(RepairMode::Adaptive {
        min: RepairRate::from_percent(5),
        max: RepairRate::from_percent(15),
    });

    let floor = policy.plan(DATA_SHARDS, &wifi(0, 2_000));
    assert_eq!(
        floor.rate,
        RepairRate::from_percent(5),
        "fell below the floor"
    );

    let ceiling = policy.plan(DATA_SHARDS, &wifi(300, 2_000));
    assert_eq!(
        ceiling.rate,
        RepairRate::from_percent(15),
        "rose above the ceiling"
    );
}

#[test]
fn a_non_zero_rate_always_yields_at_least_one_shard() {
    let policy = RepairPolicy::new(RepairMode::Fixed(RepairRate::from_per_mille(5)));
    let plan = policy.plan(4, &wifi(0, 1_000));

    assert!(
        plan.parity_shards >= 1,
        "a rate that rounds to nothing is worse than being told the block is too small"
    );
}

#[test]
fn parity_never_exceeds_half_the_block() {
    let policy = RepairPolicy::new(RepairMode::Fixed(RepairRate::from_percent(90)));
    let plan = policy.plan(DATA_SHARDS, &wifi(0, 1_000));

    assert_eq!(plan.rate, RepairRate::MAX);
    assert_eq!(plan.parity_shards, DATA_SHARDS / 2);
}

#[test]
fn the_retransmission_window_scales_with_the_round_trip() {
    let policy = RepairPolicy::default();
    let window = policy
        .plan(DATA_SHARDS, &wifi(1, 4_000))
        .retransmission
        .unwrap();

    assert_eq!(window.abandon_before_deadline, Duration::from_micros(4_000));
    assert!(
        window.gap_settle < Duration::from_millis(1),
        "waited too long to distinguish loss from reordering on a single hop"
    );
}

#[test]
fn an_empty_block_asks_for_no_parity() {
    let policy = RepairPolicy::new(RepairMode::Fixed(RepairRate::from_percent(50)));
    assert_eq!(policy.plan(0, &wifi(0, 1_000)).parity_shards, 0);
}
