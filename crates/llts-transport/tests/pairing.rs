#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use llts_core::clock::Instant;
use llts_transport::pairing::{
    FileTrustStore, HandshakeRateLimiter, MemoryTrustStore, PairingMode, PairingWindow, QrOffer,
    TrustStore, TrustedPeer,
};
use llts_transport::security::{
    DeviceClass, DeviceIdentity, Hello, NearbyInitiator, NearbyResponder, NoiseHandshake,
    PresharedKey, QrInitiator, QrResponder, ReconnectInitiator, ReconnectResponder, SessionKeys,
    StaticKey, TAG_LEN,
};
use llts_transport::{Tai64N, TransportError};
use llts_wire::packet::{PacketFlags, PacketHeader, PacketKind};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, StreamId,
};

fn hello(name: &str, tag: u16) -> Hello {
    Hello::new(name, DeviceClass::Phone, 0b101).with_session_tag(tag)
}

fn buffer() -> Vec<u8> {
    vec![0u8; NoiseHandshake::MAX_MESSAGE_LEN]
}

/// Proves two key sets interoperate by sealing one datagram each way.
fn assert_keys_agree(first: &mut SessionKeys, second: &mut SessionKeys) {
    seal_and_open(first, second);
    seal_and_open(second, first);
}

fn seal_and_open(from: &mut SessionKeys, to: &mut SessionKeys) {
    {
        let header = PacketHeader {
            version: ProtocolVersion::CURRENT,
            kind: PacketKind::Media,
            flags: PacketFlags::FRAME_END,
            stream: StreamId::new(0x0101_1000),
            sequence: SequenceNumber::new(1),
            frame: FrameId::ZERO,
            timestamp: MediaTimestamp::ZERO,
            packet_index: 0,
            packet_count: 1,
            departure: MonotonicMicros::ZERO,
            payload_offset: 0,
        };
        let mut datagram = vec![0u8; PacketHeader::ENCODED_LEN + 4 + TAG_LEN];
        header.write_into(&mut datagram).unwrap();
        let sealed = from
            .sending
            .seal(&mut datagram, 4, Instant::SESSION_START)
            .unwrap();
        to.receiving
            .open(&mut datagram[..sealed.len], Instant::SESSION_START)
            .unwrap();
    }
}

#[test]
fn nearby_pairing_shows_the_same_code_on_both_screens() {
    let laptop = DeviceIdentity::generate().unwrap();
    let phone = DeviceIdentity::generate().unwrap();
    let (mut first, mut second, mut third) = (buffer(), buffer(), buffer());

    let (initiator, len) =
        NearbyInitiator::start(&laptop, hello("laptop", 11), &mut first).unwrap();
    let (responder, len) =
        NearbyResponder::accept(&phone, &hello("phone", 22), &first[..len], &mut second).unwrap();
    let (mut laptop_side, len) = initiator.on_response(&second[..len], &mut third).unwrap();
    let mut phone_side = responder.on_reveal(&third[..len]).unwrap();

    assert!(laptop_side.code.is_some());
    assert_eq!(laptop_side.code, phone_side.code);
    assert_eq!(laptop_side.code.unwrap().to_string().len(), 6);
    assert_eq!(laptop_side.peer, *phone.public());
    assert_eq!(phone_side.peer, *laptop.public());
    assert_eq!(laptop_side.peer_hello.session_tag, 22);
    assert_eq!(phone_side.peer_hello.name, "laptop");
    assert_keys_agree(&mut laptop_side.keys, &mut phone_side.keys);
}

#[test]
fn two_pairings_between_the_same_devices_show_different_codes() {
    let laptop = DeviceIdentity::generate().unwrap();
    let phone = DeviceIdentity::generate().unwrap();
    let mut codes = Vec::new();
    for _ in 0..2 {
        let (mut first, mut second, mut third) = (buffer(), buffer(), buffer());
        let (initiator, len) =
            NearbyInitiator::start(&laptop, hello("laptop", 11), &mut first).unwrap();
        let (_, len2) =
            NearbyResponder::accept(&phone, &hello("phone", 22), &first[..len], &mut second)
                .unwrap();
        let (outcome, _) = initiator.on_response(&second[..len2], &mut third).unwrap();
        codes.push(outcome.code.unwrap());
    }
    assert_ne!(codes[0], codes[1]);
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn qr_pairing_needs_no_code_and_survives_the_round_trip_through_text() {
    let laptop = DeviceIdentity::generate().unwrap();
    let phone = DeviceIdentity::generate().unwrap();
    let psk = PresharedKey([9u8; 32]);
    let offer = QrOffer::new(
        *laptop.public(),
        &psk,
        vec![SocketAddr::from((Ipv4Addr::new(192, 168, 1, 20), 47_000))],
        now_unix() + 120,
        "laptop",
    );
    let uri = offer.to_uri().unwrap();
    assert!(uri.starts_with("llts1:"));
    assert!(
        uri[6..]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    );

    let scanned = QrOffer::parse(&uri, now_unix()).unwrap();
    assert_eq!(scanned, offer);

    let (mut first, mut second, mut third) = (buffer(), buffer(), buffer());
    let (initiator, len) = QrInitiator::start(
        &phone,
        scanned.static_key,
        &scanned.psk(),
        hello("phone", 5),
        &mut first,
    )
    .unwrap();
    let (responder, len) = QrResponder::accept(
        &laptop,
        &psk,
        &hello("laptop", 6),
        &first[..len],
        &mut second,
    )
    .unwrap();
    let (mut phone_side, len) = initiator.on_response(&second[..len], &mut third).unwrap();
    let mut laptop_side = responder.on_confirmation(&third[..len]).unwrap();

    assert_eq!(phone_side.code, None);
    assert_eq!(laptop_side.peer, *phone.public());
    assert_keys_agree(&mut phone_side.keys, &mut laptop_side.keys);
}

#[test]
fn qr_pairing_with_the_wrong_key_fails() {
    let laptop = DeviceIdentity::generate().unwrap();
    let phone = DeviceIdentity::generate().unwrap();
    let (mut first, mut second, mut third) = (buffer(), buffer(), buffer());
    let (initiator, len) = QrInitiator::start(
        &phone,
        *laptop.public(),
        &PresharedKey([1u8; 32]),
        hello("phone", 5),
        &mut first,
    )
    .unwrap();
    let (responder, len) = QrResponder::accept(
        &laptop,
        &PresharedKey([2u8; 32]),
        &hello("laptop", 6),
        &first[..len],
        &mut second,
    )
    .unwrap();
    let outcome = initiator
        .on_response(&second[..len], &mut third)
        .and_then(|(_, len)| responder.on_confirmation(&third[..len]));
    assert!(
        outcome.is_err(),
        "a responder accepted the wrong pre-shared key"
    );
}

#[test]
fn an_expired_or_foreign_offer_is_refused() {
    let offer = QrOffer::new(
        StaticKey([3u8; 32]),
        &PresharedKey([4u8; 32]),
        vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 1))],
        1_000,
        "old",
    );
    let uri = offer.to_uri().unwrap();
    assert!(matches!(
        QrOffer::parse(&uri, 1_000),
        Err(TransportError::PairingOfferExpired)
    ));
    assert!(QrOffer::parse("https://example.com", 0).is_err());
    assert!(QrOffer::parse("llts1:!!!", 0).is_err());
}

fn temporary_store_path(name: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("llts-{name}-{}-{unique}.store", std::process::id()))
}

fn trust(store: &mut impl TrustStore, peer: &DeviceIdentity, name: &str) {
    store
        .upsert(TrustedPeer {
            static_key: *peer.public(),
            name: name.into(),
            class: DeviceClass::Laptop,
            last_handshake: None,
            paired_at_unix: now_unix(),
        })
        .unwrap();
}

/// Runs a reconnect from `initiator` to the device behind `responder_store`, recording the
/// timestamp as the driver does once the session proves itself.
fn reconnect(
    initiator: &DeviceIdentity,
    responder_store: &mut impl TrustStore,
    timestamp: Tai64N,
) -> Result<Vec<u8>, TransportError> {
    let responder = responder_store.identity().unwrap();
    let (mut first, mut second) = (buffer(), buffer());
    let (initiator_side, len) = ReconnectInitiator::start(
        initiator,
        responder.public(),
        &hello("laptop", 7),
        timestamp,
        &mut first,
    )?;
    let message_one = first[..len].to_vec();
    let (accepted, len) = ReconnectResponder::accept(
        &responder,
        &hello("phone", 8),
        &message_one,
        &mut second,
        |peer, stamp| {
            if responder_store.admits(peer, stamp) {
                Ok(())
            } else {
                Err(TransportError::Replayed)
            }
        },
    )?;
    let mut established = initiator_side.on_response(&second[..len])?;
    let mut accepted_keys = accepted.established.keys;
    assert_keys_agree(&mut established.keys, &mut accepted_keys);
    responder_store.record_handshake(&accepted.established.peer, accepted.timestamp)?;
    Ok(message_one)
}

#[test]
fn a_paired_device_reconnects_after_both_sides_restart() {
    let laptop_path = temporary_store_path("laptop");
    let phone_path = temporary_store_path("phone");
    {
        let mut laptop = FileTrustStore::open(&laptop_path).unwrap();
        let mut phone = FileTrustStore::open(&phone_path).unwrap();
        let laptop_identity = laptop.identity_or_generate().unwrap();
        let phone_identity = phone.identity_or_generate().unwrap();
        trust(&mut laptop, &phone_identity, "phone");
        trust(&mut phone, &laptop_identity, "laptop");
    }

    let laptop = FileTrustStore::open(&laptop_path).unwrap();
    let mut phone = FileTrustStore::open(&phone_path).unwrap();
    let laptop_identity = laptop.identity().unwrap();
    assert_eq!(laptop.peers().len(), 1);
    reconnect(&laptop_identity, &mut phone, Tai64N::now()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&phone_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the trust store is readable by others");
    }
    let _ = std::fs::remove_file(laptop_path);
    let _ = std::fs::remove_file(phone_path);
}

#[test]
fn a_replayed_reconnect_is_refused_even_after_a_restart() {
    let phone_path = temporary_store_path("replay");
    let laptop_identity = DeviceIdentity::generate().unwrap();
    let stamp = Tai64N::now();
    let captured = {
        let mut phone = FileTrustStore::open(&phone_path).unwrap();
        phone.identity_or_generate().unwrap();
        trust(&mut phone, &laptop_identity, "laptop");
        reconnect(&laptop_identity, &mut phone, stamp).unwrap()
    };

    let phone = FileTrustStore::open(&phone_path).unwrap();
    let phone_identity = phone.identity().unwrap();
    let mut out = buffer();
    let replayed = ReconnectResponder::accept(
        &phone_identity,
        &hello("phone", 8),
        &captured,
        &mut out,
        |peer, stamp| {
            if phone.admits(peer, stamp) {
                Ok(())
            } else {
                Err(TransportError::Replayed)
            }
        },
    );
    assert!(matches!(replayed, Err(TransportError::Replayed)));
    let _ = std::fs::remove_file(phone_path);
}

#[test]
fn an_unpaired_device_cannot_reconnect() {
    let stranger = DeviceIdentity::generate().unwrap();
    let mut phone = MemoryTrustStore::new();
    phone
        .set_identity(&DeviceIdentity::generate().unwrap())
        .unwrap();
    assert!(matches!(
        reconnect(&stranger, &mut phone, Tai64N::now()),
        Err(TransportError::Replayed)
    ));
}

#[test]
fn the_pairing_window_closes_on_time() {
    let mut window = PairingWindow::new();
    let start = Instant::SESSION_START;
    window.open(PairingMode::Nearby, start, Duration::from_secs(60));
    assert_eq!(
        window.mode(start.saturating_add(Duration::from_secs(59))),
        Some(&PairingMode::Nearby)
    );
    assert_eq!(
        window.mode(start.saturating_add(Duration::from_secs(60))),
        None
    );
}

#[test]
fn a_flood_of_handshakes_from_one_source_is_throttled() {
    let mut limiter = HandshakeRateLimiter::new();
    let source = Ipv4Addr::new(10, 0, 0, 9).into();
    let now = Instant::from_micros(1_000_000);
    let allowed = (0..20).filter(|_| limiter.allow(source, now)).count();
    assert_eq!(allowed, 4);
    let later = now.saturating_add(Duration::from_millis(600));
    assert!(limiter.allow(source, later));
    assert!(limiter.allow(Ipv4Addr::new(10, 0, 0, 10).into(), now));
}
