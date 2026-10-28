#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant as WallClock, SystemTime, UNIX_EPOCH};

use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, Role};
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_transport::pairing::{MemoryTrustStore, QrOffer, TrustStore, TrustedPeer};
use llts_transport::security::{DeviceClass, DeviceIdentity, StaticKey};
use llts_transport::session::{Driver, DriverConfig, DriverEvent};
use llts_wire::{MediaTimestamp, StreamClass};

const SCREEN: StreamKey = StreamKey::new(StreamClass::Video, 0);

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

fn driver(name: &str) -> Driver<MemoryTrustStore> {
    let identity = DeviceIdentity::generate().unwrap();
    let mut trust = MemoryTrustStore::new();
    trust.set_identity(&identity).unwrap();
    Driver::bind(
        loopback(),
        identity,
        trust,
        DriverConfig::new(name, DeviceClass::Laptop),
    )
    .unwrap()
}

fn trusted(peer: &StaticKey, name: &str) -> TrustedPeer {
    TrustedPeer {
        static_key: *peer,
        name: name.into(),
        class: DeviceClass::Phone,
        last_handshake: None,
        paired_at_unix: 0,
    }
}

/// Two drivers that already trust each other, as after an earlier pairing.
fn paired_drivers() -> (Driver<MemoryTrustStore>, Driver<MemoryTrustStore>) {
    let laptop_identity = DeviceIdentity::generate().unwrap();
    let phone_identity = DeviceIdentity::generate().unwrap();
    let mut laptop_trust = MemoryTrustStore::new();
    laptop_trust.set_identity(&laptop_identity).unwrap();
    laptop_trust
        .upsert(trusted(phone_identity.public(), "phone"))
        .unwrap();
    let mut phone_trust = MemoryTrustStore::new();
    phone_trust.set_identity(&phone_identity).unwrap();
    phone_trust
        .upsert(trusted(laptop_identity.public(), "laptop"))
        .unwrap();
    let laptop = Driver::bind(
        loopback(),
        laptop_identity,
        laptop_trust,
        DriverConfig::new("laptop", DeviceClass::Laptop),
    )
    .unwrap();
    let phone = Driver::bind(
        loopback(),
        phone_identity,
        phone_trust,
        DriverConfig::new("phone", DeviceClass::Phone),
    )
    .unwrap();
    (laptop, phone)
}

#[derive(Default)]
struct Seen {
    connected: u32,
    paired: u32,
    codes: Vec<String>,
    presented: u32,
    failures: u32,
    path_changes: u32,
}

fn drain(driver: &mut Driver<MemoryTrustStore>, seen: &mut Seen) {
    while let Some(event) = driver.poll_event() {
        match event {
            DriverEvent::Connected { .. } => seen.connected += 1,
            DriverEvent::Paired { .. } => seen.paired += 1,
            DriverEvent::PairingCode { code, .. } => seen.codes.push(code.to_string()),
            DriverEvent::Session {
                action: Action::PresentFrame { .. },
                ..
            } => seen.presented += 1,
            DriverEvent::HandshakeFailed { .. } => seen.failures += 1,
            DriverEvent::Session {
                action: Action::PathChanged(_),
                ..
            } => seen.path_changes += 1,
            DriverEvent::Session { .. } => {}
        }
    }
}

/// Polls both drivers until `done` holds or `limit` passes.
fn run_until(
    left: &mut Driver<MemoryTrustStore>,
    right: &mut Driver<MemoryTrustStore>,
    left_seen: &mut Seen,
    right_seen: &mut Seen,
    limit: Duration,
    mut done: impl FnMut(&Seen, &Seen) -> bool,
) -> bool {
    let deadline = WallClock::now() + limit;
    while WallClock::now() < deadline {
        left.poll().unwrap();
        right.poll().unwrap();
        drain(left, left_seen);
        drain(right, right_seen);
        if done(left_seen, right_seen) {
            return true;
        }
        thread::sleep(Duration::from_micros(200));
    }
    false
}

#[test]
fn a_paired_device_reconnects_and_streams_frames() {
    let (mut laptop, mut phone) = paired_drivers();
    let phone_key = *phone.identity().public();
    let laptop_key = *laptop.identity().public();
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());

    laptop
        .connect(phone_key, phone.local_addr().unwrap())
        .unwrap();
    assert!(run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_secs(5),
        |left, right| left.connected == 1 && right.connected == 1
    ));

    laptop
        .session_mut(&phone_key)
        .unwrap()
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    phone
        .session_mut(&laptop_key)
        .unwrap()
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Receiver))
        .unwrap();

    stream_frames(
        &mut laptop,
        &mut phone,
        &phone_key,
        &mut laptop_seen,
        &mut phone_seen,
        0..30,
    );
    assert!(
        phone_seen.presented >= 28,
        "only {} of 30 frames arrived",
        phone_seen.presented
    );
    assert_eq!(laptop.connected_peers().count(), 1);
    assert!(
        phone
            .trust()
            .peer(&laptop_key)
            .unwrap()
            .last_handshake
            .is_some(),
        "the reconnect timestamp was never recorded"
    );
}

fn stream_frames(
    sender: &mut Driver<MemoryTrustStore>,
    receiver: &mut Driver<MemoryTrustStore>,
    receiver_key: &StaticKey,
    sender_seen: &mut Seen,
    receiver_seen: &mut Seen,
    ticks: std::ops::Range<u32>,
) {
    let payload = vec![0x5Au8; 12_000];
    for tick in ticks {
        sender
            .handle(
                receiver_key,
                &Event::SliceEncoded {
                    stream: SCREEN,
                    slice: EncodedSlice::new(&payload, MediaTimestamp::new(tick * 1_500))
                        .as_keyframe()
                        .ending_frame(),
                },
            )
            .unwrap();
        run_until(
            sender,
            receiver,
            sender_seen,
            receiver_seen,
            Duration::from_millis(16),
            |_, _| false,
        );
    }
    run_until(
        sender,
        receiver,
        sender_seen,
        receiver_seen,
        Duration::from_millis(100),
        |_, _| false,
    );
}

#[test]
fn a_simultaneous_dial_ends_with_exactly_one_session() {
    let (mut laptop, mut phone) = paired_drivers();
    let phone_key = *phone.identity().public();
    let laptop_key = *laptop.identity().public();
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());

    laptop
        .connect(phone_key, phone.local_addr().unwrap())
        .unwrap();
    phone
        .connect(laptop_key, laptop.local_addr().unwrap())
        .unwrap();
    run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_secs(2),
        |_, _| false,
    );

    assert_eq!(laptop.connected_peers().count(), 1);
    assert_eq!(phone.connected_peers().count(), 1);
    assert_eq!(
        laptop_seen.connected, 1,
        "the laptop saw more than one session come up"
    );
    assert_eq!(
        phone_seen.connected, 1,
        "the phone saw more than one session come up"
    );
    assert_eq!(laptop_seen.failures + phone_seen.failures, 0);
}

#[test]
fn a_rebound_socket_keeps_frames_flowing() {
    let (mut laptop, mut phone) = paired_drivers();
    let phone_key = *phone.identity().public();
    let laptop_key = *laptop.identity().public();
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());
    laptop
        .connect(phone_key, phone.local_addr().unwrap())
        .unwrap();
    assert!(run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_secs(5),
        |left, right| left.connected == 1 && right.connected == 1
    ));
    laptop
        .session_mut(&phone_key)
        .unwrap()
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
        .unwrap();
    phone
        .session_mut(&laptop_key)
        .unwrap()
        .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Receiver))
        .unwrap();

    stream_frames(
        &mut laptop,
        &mut phone,
        &phone_key,
        &mut laptop_seen,
        &mut phone_seen,
        0..20,
    );
    let before = phone_seen.presented;
    assert_eq!(before, 20);

    // The receiver moves: until the sender has validated the new address, its media goes to a
    // socket that no longer exists.
    let old_address = phone.local_addr().unwrap();
    phone.rebind(loopback()).unwrap();
    assert_ne!(phone.local_addr().unwrap(), old_address);

    stream_frames(
        &mut laptop,
        &mut phone,
        &phone_key,
        &mut laptop_seen,
        &mut phone_seen,
        20..60,
    );
    let after = phone_seen.presented - before;
    assert_eq!(
        laptop_seen.path_changes, 1,
        "the sender never moved to the new address"
    );
    assert!(
        after >= 34,
        "only {after} of 40 frames arrived after the rebind"
    );
}

#[test]
fn qr_pairing_between_two_drivers_trusts_both_ways() {
    let mut laptop = driver("laptop");
    let mut phone = driver("phone");
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());

    let offer = laptop
        .offer_qr_pairing(vec![laptop.local_addr().unwrap()], Duration::from_secs(60))
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let scanned = QrOffer::parse(&offer.to_uri().unwrap(), now).unwrap();
    phone.pair_with_offer(&scanned).unwrap();

    assert!(run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_secs(5),
        |left, right| left.paired == 1
            && right.paired == 1
            && left.connected == 1
            && right.connected == 1
    ));
    assert!(laptop.trust().peer(phone.identity().public()).is_some());
    assert!(phone.trust().peer(laptop.identity().public()).is_some());
}

#[test]
fn nearby_pairing_is_refused_while_the_window_is_closed() {
    let mut laptop = driver("laptop");
    let mut phone = driver("phone");
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());
    phone.pair_nearby(laptop.local_addr().unwrap()).unwrap();
    run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_millis(600),
        |_, _| false,
    );
    assert!(phone_seen.codes.is_empty() && laptop_seen.codes.is_empty());
}

#[test]
fn a_qr_window_also_accepts_nearby_pairing_and_forget_drops_the_peer() {
    let mut laptop = driver("laptop");
    let mut phone = driver("phone");
    let (mut laptop_seen, mut phone_seen) = (Seen::default(), Seen::default());
    let _offer = laptop
        .offer_qr_pairing(vec![laptop.local_addr().unwrap()], Duration::from_secs(60))
        .unwrap();
    phone.pair_nearby(laptop.local_addr().unwrap()).unwrap();
    assert!(run_until(
        &mut laptop,
        &mut phone,
        &mut laptop_seen,
        &mut phone_seen,
        Duration::from_secs(5),
        |left, right| !left.codes.is_empty() && !right.codes.is_empty()
    ));
    assert_eq!(laptop_seen.codes, phone_seen.codes);

    let phone_key = *phone.identity().public();
    laptop.confirm_pairing(&phone_key).unwrap();
    assert!(laptop.trust().peer(&phone_key).is_some());
    assert!(laptop.forget(&phone_key).unwrap());
    assert!(laptop.trust().peer(&phone_key).is_none());
    assert!(laptop.session_mut(&phone_key).is_none());
    assert!(!laptop.forget(&phone_key).unwrap());
}

/// Each device runs its own driver on its own thread; they meet only through the sockets.
#[test]
fn two_threads_pair_nearby_over_localhost() {
    let (address_tx, address_rx) = mpsc::channel();
    let (code_tx, code_rx) = mpsc::channel();
    let (confirm_left_tx, confirm_left_rx) = mpsc::channel::<()>();
    let (confirm_right_tx, confirm_right_rx) = mpsc::channel::<()>();
    let phone_codes = code_tx.clone();

    let responder = thread::spawn(move || {
        let mut laptop = driver("laptop");
        laptop.open_nearby_pairing(Duration::from_secs(30));
        address_tx.send(laptop.local_addr().unwrap()).unwrap();
        run_device(&mut laptop, &code_tx, &confirm_left_rx)
    });
    let initiator = thread::spawn(move || {
        let mut phone = driver("phone");
        phone.pair_nearby(address_rx.recv().unwrap()).unwrap();
        run_device(&mut phone, &phone_codes, &confirm_right_rx)
    });

    let first = code_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = code_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first, second, "the two screens showed different codes");
    confirm_left_tx.send(()).unwrap();
    confirm_right_tx.send(()).unwrap();

    let laptop_trusts = responder.join().unwrap();
    let phone_trusts = initiator.join().unwrap();
    assert!(laptop_trusts && phone_trusts);
}

/// Polls one device until pairing is confirmed and the session is up, reporting its code.
fn run_device(
    device: &mut Driver<MemoryTrustStore>,
    codes: &mpsc::Sender<String>,
    confirm: &mpsc::Receiver<()>,
) -> bool {
    let deadline = WallClock::now() + Duration::from_secs(10);
    let mut peer = None;
    let mut confirmed = false;
    while WallClock::now() < deadline {
        device.wait(Duration::from_millis(5)).unwrap();
        while let Some(event) = device.poll_event() {
            match event {
                DriverEvent::PairingCode {
                    peer: key, code, ..
                } => {
                    peer = Some(key);
                    codes.send(code.to_string()).unwrap();
                }
                DriverEvent::Paired { .. } => confirmed = true,
                _ => {}
            }
        }
        if let Some(key) = peer
            && !confirmed
            && confirm.try_recv().is_ok()
        {
            device.confirm_pairing(&key).unwrap();
        }
        if confirmed {
            return peer.is_some_and(|key| device.trust().peer(&key).is_some());
        }
    }
    false
}

#[test]
#[should_panic(expected = "UnknownPeer")]
fn connecting_to_an_unpaired_device_is_refused_locally() {
    let mut laptop = driver("laptop");
    laptop.connect(StaticKey([1u8; 32]), loopback()).unwrap();
}
