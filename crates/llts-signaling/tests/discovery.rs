#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_signaling::device::{DeviceClass, DeviceId};
use llts_signaling::discovery::{
    ADVERTISEMENT_LEN, Advertisement, BEACON_VERSION, Beacon, DiscoveryError, DiscoverySecret,
    EPOCH_SECONDS, EphemeralId, Epoch, PAIRING_BEACON_LEN, PairingBeacon, PairingFlags,
    PairingNonce, PeerResolver, SERVICE_TYPE, SERVICE_UUID_16, TxtRecord,
};

const NOW: Epoch = Epoch::new(1_000_000);

struct Link {
    device: DeviceId,
    secret: DiscoverySecret,
}

fn link(seed: u8) -> Link {
    Link {
        device: DeviceId([seed; 32]),
        secret: DiscoverySecret::derive(&[seed ^ 0xA5; 32]),
    }
}

fn resolver(links: &[Link], tolerance: u8) -> PeerResolver {
    PeerResolver::build(
        links.iter().map(|link| (link.device, &link.secret)),
        NOW,
        tolerance,
    )
}

fn advertisement(link: &Link, epoch: Epoch) -> Advertisement {
    Advertisement::new(
        DeviceClass::Phone,
        EphemeralId::derive(&link.secret, epoch),
        epoch,
    )
}

fn pairing_beacon() -> PairingBeacon {
    PairingBeacon {
        device_class: DeviceClass::Computer,
        flags: PairingFlags::NEARBY_CODE | PairingFlags::QR,
        nonce: PairingNonce([1, 2, 3, 4, 5, 6, 7, 8]),
    }
}

#[test]
fn llts_beacons_live_under_their_own_service_identifiers() {
    assert_eq!(SERVICE_UUID_16, 0xFCF2);
    assert_ne!(SERVICE_UUID_16, 0xFCF1, "0xFCF1 belongs to ffsp");
    assert_eq!(SERVICE_TYPE, "_llts._udp");
}

#[test]
fn an_advertisement_fills_the_legacy_budget_and_round_trips() {
    let original = advertisement(&link(1), NOW);
    let encoded = original.to_bytes();
    assert_eq!(encoded.len(), ADVERTISEMENT_LEN);
    assert_eq!(ADVERTISEMENT_LEN, 24);
    assert_eq!(encoded[0], BEACON_VERSION);
    assert_eq!(Advertisement::parse(&encoded).unwrap(), original);
    assert_eq!(Beacon::parse(&encoded).unwrap(), Beacon::Linked(original));
}

#[test]
fn an_advertisement_of_the_wrong_length_is_rejected() {
    let encoded = advertisement(&link(1), NOW).to_bytes();
    for length in [0, 6, 23, 25] {
        let mut resized = encoded.to_vec();
        resized.resize(length, 0);
        assert_eq!(
            Advertisement::parse(&resized),
            Err(DiscoveryError::Length {
                expected: ADVERTISEMENT_LEN,
                actual: length
            })
        );
    }
}

#[test]
fn a_beacon_from_a_future_layout_or_of_an_unknown_kind_is_rejected() {
    let mut encoded = advertisement(&link(1), NOW).to_bytes();
    encoded[0] = 99;
    assert_eq!(
        Beacon::parse(&encoded),
        Err(DiscoveryError::UnsupportedVersion(99))
    );

    let mut encoded = advertisement(&link(1), NOW).to_bytes();
    encoded[1] = 9;
    assert_eq!(Beacon::parse(&encoded), Err(DiscoveryError::UnknownKind(9)));

    let mut encoded = advertisement(&link(1), NOW).to_bytes();
    encoded[2] = 0;
    assert_eq!(
        Beacon::parse(&encoded),
        Err(DiscoveryError::UnknownDeviceClass(0))
    );

    assert!(matches!(
        Beacon::parse(&[BEACON_VERSION]),
        Err(DiscoveryError::Length { .. })
    ));
}

#[test]
fn an_advertisement_leaks_neither_the_device_id_nor_the_secret() {
    let record = link(7);
    let encoded = advertisement(&record, NOW).to_bytes();
    let secret = DiscoverySecret::derive(&[7 ^ 0xA5; 32]);
    let leaked = |needle: &[u8]| {
        needle
            .windows(4)
            .any(|window| encoded.windows(4).any(|candidate| candidate == window))
    };
    assert!(
        !leaked(&record.device.0),
        "the beacon carries the device id"
    );
    assert!(
        !leaked(&[7 ^ 0xA5; 32]),
        "the beacon carries the link secret"
    );
    assert_eq!(format!("{secret:?}"), "DiscoverySecret(..)");
}

#[test]
fn the_advertised_identifier_rotates_every_epoch_and_differs_per_link() {
    let record = link(3);
    let now = EphemeralId::derive(&record.secret, NOW);
    assert_eq!(now, EphemeralId::derive(&record.secret, NOW));
    assert_ne!(
        now,
        EphemeralId::derive(&record.secret, NOW.shift(1).unwrap())
    );
    assert_ne!(now, EphemeralId::derive(&link(4).secret, NOW));
}

#[test]
fn a_linked_peer_resolves_in_the_previous_current_and_next_epoch() {
    let links = [link(1), link(2)];
    let resolver = resolver(&links, PeerResolver::DEFAULT_TOLERANCE);
    for delta in [-1, 0, 1] {
        let seen = advertisement(&links[1], NOW.shift(delta).unwrap());
        assert_eq!(
            resolver.resolve(&seen),
            Some(links[1].device),
            "delta {delta}"
        );
    }
}

#[test]
fn a_peer_outside_the_epoch_window_is_rejected_until_the_window_widens() {
    let links = [link(1)];
    let narrow = resolver(&links, PeerResolver::DEFAULT_TOLERANCE);
    for delta in [-3, -2, 2, 3] {
        let seen = advertisement(&links[0], NOW.shift(delta).unwrap());
        assert_eq!(narrow.resolve(&seen), None, "delta {delta}");
    }

    let wide = resolver(&links, PeerResolver::SKEWED_TOLERANCE);
    let far = advertisement(&links[0], NOW.shift(3).unwrap());
    assert_eq!(wide.resolve(&far), Some(links[0].device));
}

#[test]
fn an_unlinked_device_is_indistinguishable_from_noise() {
    let resolver = resolver(&[link(1)], 1);
    assert_eq!(resolver.resolve(&advertisement(&link(200), NOW)), None);
    let zeros = Advertisement::new(DeviceClass::Phone, EphemeralId::from_bytes([0; 16]), NOW);
    assert_eq!(resolver.resolve(&zeros), None);
}

#[test]
fn the_resolver_precomputes_one_entry_per_link_and_epoch() {
    let links: Vec<Link> = (0..50).map(link).collect();
    let resolver = resolver(&links, 1);
    assert_eq!(resolver.len(), 50 * 3);
    assert_eq!(
        resolver.resolve(&advertisement(&links[49], NOW)),
        Some(links[49].device)
    );
    assert!(resolver.is_current_for(NOW));
    assert!(!resolver.is_current_for(NOW.shift(1).unwrap()));
}

#[test]
fn an_epoch_byte_outside_the_window_signals_clock_skew() {
    let resolver = resolver(&[link(1)], 1);
    assert!(resolver.covers_epoch_byte(NOW.low_byte()));
    assert!(resolver.covers_epoch_byte(NOW.shift(-1).unwrap().low_byte()));
    assert!(!resolver.covers_epoch_byte(NOW.shift(40).unwrap().low_byte()));
}

#[test]
fn epochs_are_fifteen_minutes_and_do_not_underflow_at_zero() {
    assert_eq!(EPOCH_SECONDS, 900);
    assert_eq!(Epoch::from_unix_seconds(899), Epoch::new(0));
    assert_eq!(Epoch::from_unix_seconds(900), Epoch::new(1));
    assert_eq!(Epoch::new(0).shift(-1), None);
    assert_eq!(Epoch::new(0).window(2).count(), 3);
    assert_eq!(Epoch::new(u64::MAX).shift(1), None);
}

#[test]
fn a_pairing_beacon_round_trips_and_carries_no_identity() {
    let beacon = pairing_beacon();
    let encoded = beacon.to_bytes();
    assert_eq!(encoded.len(), PAIRING_BEACON_LEN);
    assert_eq!(PairingBeacon::parse(&encoded).unwrap(), beacon);
    assert_eq!(Beacon::parse(&encoded).unwrap(), Beacon::Pairing(beacon));
    const { assert!(PAIRING_BEACON_LEN < ADVERTISEMENT_LEN) };
}

#[test]
fn a_pairing_beacon_drops_flags_it_does_not_know() {
    let mut encoded = pairing_beacon().to_bytes();
    encoded[3] = 0b1000_0001;
    assert_eq!(
        PairingBeacon::parse(&encoded).unwrap().flags,
        PairingFlags::NEARBY_CODE
    );
}

#[test]
fn pairing_nonces_are_fresh_each_window() {
    assert_ne!(
        PairingNonce::random().unwrap(),
        PairingNonce::random().unwrap()
    );
}

#[test]
fn the_mdns_record_carries_the_same_beacon_as_bluetooth() {
    for beacon in [
        Beacon::Linked(advertisement(&link(5), NOW)),
        Beacon::Pairing(pairing_beacon()),
    ] {
        let txt = TxtRecord {
            beacon,
            port: 47_000,
        };
        let pairs = txt.to_pairs();
        let parsed = TxtRecord::parse(|key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.as_str())
        })
        .unwrap();
        assert_eq!(parsed, txt);
    }
}

#[test]
fn a_malformed_mdns_record_is_rejected() {
    assert_eq!(
        TxtRecord::parse(|_| None),
        Err(DiscoveryError::MissingTxtKey("b"))
    );
    assert_eq!(
        TxtRecord::parse(|key| if key == "b" { Some("zz") } else { Some("1") }),
        Err(DiscoveryError::MalformedTxtValue { key: "b" })
    );
    let oversized = "00".repeat(ADVERTISEMENT_LEN + 1);
    assert_eq!(
        TxtRecord::parse(|key| if key == "b" {
            Some(&oversized)
        } else {
            Some("1")
        }),
        Err(DiscoveryError::MalformedTxtValue { key: "b" })
    );
    let valid = Beacon::Linked(advertisement(&link(1), NOW)).to_hex();
    assert_eq!(
        TxtRecord::parse(|key| if key == "b" {
            Some(&valid)
        } else {
            Some("not-a-port")
        }),
        Err(DiscoveryError::MalformedTxtValue { key: "p" })
    );
}
