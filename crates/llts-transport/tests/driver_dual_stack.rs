#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

//! A driver bound to the IPv6 wildcard must reach IPv4 peers through the same socket, since
//! desktops bind `[::]` by default and phones on Wi-Fi are usually reachable only over IPv4.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::thread;
use std::time::{Duration, Instant as WallClock, SystemTime, UNIX_EPOCH};

use llts_core::channel::Delivery;
use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, Role};
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_transport::pairing::{MemoryTrustStore, QrOffer, TrustStore, TrustedPeer};
use llts_transport::security::{DeviceClass, DeviceIdentity, StaticKey};
use llts_transport::session::{Driver, DriverConfig, DriverEvent};
use llts_wire::{MediaTimestamp, StreamClass};

const SCREEN: StreamKey = StreamKey::new(StreamClass::Video, 0);
const IPV4_LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const IPV6_LOOPBACK: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

type TestDriver = Driver<MemoryTrustStore>;

fn dual_stack() -> SocketAddr {
    SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
}

fn ipv4_only() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
}

const fn ipv6_loopback() -> SocketAddr {
    SocketAddr::new(IPV6_LOOPBACK, 0)
}

/// Where `driver` can be reached over `ip`, whatever wildcard it is bound to.
fn reachable(driver: &TestDriver, ip: IpAddr) -> SocketAddr {
    SocketAddr::new(ip, driver.local_addr().unwrap().port())
}

fn driver(name: &str, local: SocketAddr) -> TestDriver {
    driver_with(name, local, DeviceIdentity::generate().unwrap(), None)
}

fn driver_with(
    name: &str,
    local: SocketAddr,
    identity: DeviceIdentity,
    trusting: Option<StaticKey>,
) -> TestDriver {
    let mut trust = MemoryTrustStore::new();
    trust.set_identity(&identity).unwrap();
    if let Some(peer) = trusting {
        trust
            .upsert(TrustedPeer {
                static_key: peer,
                name: "peer".into(),
                class: DeviceClass::Phone,
                last_handshake: None,
                paired_at_unix: 0,
            })
            .unwrap();
    }
    Driver::bind(
        local,
        identity,
        trust,
        DriverConfig::new(name, DeviceClass::Laptop),
    )
    .unwrap()
}

/// Two drivers that already trust each other, bound to the given addresses.
fn paired_drivers(left: SocketAddr, right: SocketAddr) -> (TestDriver, TestDriver) {
    let left_identity = DeviceIdentity::generate().unwrap();
    let right_identity = DeviceIdentity::generate().unwrap();
    let (left_key, right_key) = (*left_identity.public(), *right_identity.public());
    (
        driver_with("left", left, left_identity, Some(right_key)),
        driver_with("right", right, right_identity, Some(left_key)),
    )
}

#[derive(Default)]
struct Seen {
    connected: u32,
    paired: u32,
    presented: u32,
    delivered: Vec<Vec<u8>>,
    failures: u32,
}

fn drain(driver: &mut TestDriver, seen: &mut Seen) {
    while let Some(event) = driver.poll_event() {
        match event {
            DriverEvent::Connected { .. } => seen.connected += 1,
            DriverEvent::Paired { .. } => seen.paired += 1,
            DriverEvent::HandshakeFailed { .. } => seen.failures += 1,
            DriverEvent::Session { action, .. } => match action {
                Action::PresentFrame { .. } => seen.presented += 1,
                Action::Deliver { stream, message } if stream == StreamKey::CONTROL => {
                    seen.delivered.push(message.filled().to_vec());
                }
                _ => {}
            },
            DriverEvent::PairingCode { .. } => {}
        }
    }
}

struct Pair {
    left: TestDriver,
    right: TestDriver,
    left_seen: Seen,
    right_seen: Seen,
}

impl Pair {
    fn new(left: TestDriver, right: TestDriver) -> Self {
        Self {
            left,
            right,
            left_seen: Seen::default(),
            right_seen: Seen::default(),
        }
    }

    fn run_until(&mut self, limit: Duration, mut done: impl FnMut(&Seen, &Seen) -> bool) -> bool {
        let deadline = WallClock::now() + limit;
        while WallClock::now() < deadline {
            self.left.poll().unwrap();
            self.right.poll().unwrap();
            drain(&mut self.left, &mut self.left_seen);
            drain(&mut self.right, &mut self.right_seen);
            if done(&self.left_seen, &self.right_seen) {
                return true;
            }
            thread::sleep(Duration::from_micros(200));
        }
        false
    }

    fn run_for(&mut self, duration: Duration) {
        self.run_until(duration, |_, _| false);
    }

    fn both_connected(&mut self) -> bool {
        self.run_until(Duration::from_secs(5), |left, right| {
            left.connected == 1 && right.connected == 1
        })
    }

    const fn left_key(&self) -> StaticKey {
        *self.left.identity().public()
    }

    const fn right_key(&self) -> StaticKey {
        *self.right.identity().public()
    }

    /// `offerer` shows a QR code naming itself at `ip`; the other side scans it.
    fn pair_by_qr(&mut self, offerer: Side, ip: IpAddr) {
        let (showing, scanning) = match offerer {
            Side::Left => (&mut self.left, &mut self.right),
            Side::Right => (&mut self.right, &mut self.left),
        };
        let offer = showing
            .offer_qr_pairing(vec![reachable(showing, ip)], Duration::from_secs(60))
            .unwrap();
        let scanned = QrOffer::parse(&offer.to_uri().unwrap(), unix_now()).unwrap();
        scanning.pair_with_offer(&scanned).unwrap();

        let paired = self.run_until(Duration::from_secs(5), |left, right| {
            left.paired == 1 && right.paired == 1 && left.connected == 1 && right.connected == 1
        });
        assert!(paired, "QR pairing never completed");
        assert!(self.left.trust().peer(&self.right_key()).is_some());
        assert!(self.right.trust().peer(&self.left_key()).is_some());
        assert_eq!(self.left_seen.failures + self.right_seen.failures, 0);
    }

    /// Streams frames and control messages from left to right, asserting both arrive.
    fn assert_left_delivers_media_and_control(&mut self) {
        let (left_key, right_key) = (self.left_key(), self.right_key());
        self.left
            .session_mut(&right_key)
            .unwrap()
            .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Sender))
            .unwrap();
        self.right
            .session_mut(&left_key)
            .unwrap()
            .open_media(SCREEN, MediaStreamConfig::screen_share(Role::Receiver))
            .unwrap();

        let payload = vec![0x5Au8; 12_000];
        let mut sent_messages = Vec::new();
        for tick in 0..20u32 {
            self.left
                .handle(
                    &right_key,
                    &Event::SliceEncoded {
                        stream: SCREEN,
                        slice: EncodedSlice::new(&payload, MediaTimestamp::new(tick * 1_500))
                            .as_keyframe()
                            .ending_frame(),
                    },
                )
                .unwrap();
            let message = [0x00, 0x40, tick.to_le_bytes()[0]].to_vec();
            self.left
                .handle(
                    &right_key,
                    &Event::ControlQueued {
                        stream: StreamKey::CONTROL,
                        message: &message,
                        delivery: Delivery::Ordered,
                    },
                )
                .unwrap();
            sent_messages.push(message);
            self.run_for(Duration::from_millis(16));
        }
        self.run_until(Duration::from_secs(2), |_, right| {
            right.delivered.len() == sent_messages.len()
        });

        assert!(
            self.right_seen.presented >= 18,
            "only {} of 20 frames arrived",
            self.right_seen.presented
        );
        assert_eq!(self.right_seen.delivered, sent_messages);
    }
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn a_dual_stack_offerer_pairs_with_an_ipv4_scanner() {
    let mut pair = Pair::new(
        driver("desktop", dual_stack()),
        driver("phone", ipv4_only()),
    );
    pair.pair_by_qr(Side::Left, IPV4_LOOPBACK);
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn a_dual_stack_scanner_pairs_with_an_ipv4_offerer() {
    let mut pair = Pair::new(
        driver("desktop", dual_stack()),
        driver("phone", ipv4_only()),
    );
    pair.pair_by_qr(Side::Right, IPV4_LOOPBACK);
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn an_ipv4_peer_receives_media_and_control_from_a_dual_stack_peer_it_paired_with() {
    let mut pair = Pair::new(
        driver("phone", ipv4_only()),
        driver("desktop", dual_stack()),
    );
    pair.pair_by_qr(Side::Right, IPV4_LOOPBACK);
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn a_dual_stack_driver_reconnects_to_an_ipv4_peer() {
    let (left, right) = paired_drivers(dual_stack(), ipv4_only());
    let mut pair = Pair::new(left, right);
    let address = reachable(&pair.right, IPV4_LOOPBACK);
    pair.left.connect(pair.right_key(), address).unwrap();
    assert!(pair.both_connected(), "the IK reconnect never completed");
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn an_ipv4_driver_reconnects_to_a_dual_stack_peer() {
    let (left, right) = paired_drivers(ipv4_only(), dual_stack());
    let mut pair = Pair::new(left, right);
    let address = reachable(&pair.right, IPV4_LOOPBACK);
    pair.left.connect(pair.right_key(), address).unwrap();
    assert!(pair.both_connected(), "the IK reconnect never completed");
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn a_dual_stack_driver_accepts_an_ipv4_mapped_dial_address() {
    let (left, right) = paired_drivers(dual_stack(), ipv4_only());
    let mut pair = Pair::new(left, right);
    let mapped = IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped());
    let address = reachable(&pair.right, mapped);
    pair.left.connect(pair.right_key(), address).unwrap();
    assert!(
        pair.both_connected(),
        "a v4-mapped dial address never connected"
    );
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn ipv6_loopback_and_dual_stack_pair_both_ways() {
    let mut pair = Pair::new(
        driver("desktop", dual_stack()),
        driver("phone", ipv6_loopback()),
    );
    pair.pair_by_qr(Side::Left, IPV6_LOOPBACK);
    pair.assert_left_delivers_media_and_control();

    let mut reverse = Pair::new(
        driver("desktop", dual_stack()),
        driver("phone", ipv6_loopback()),
    );
    reverse.pair_by_qr(Side::Right, IPV6_LOOPBACK);
    reverse.assert_left_delivers_media_and_control();
}

#[test]
fn a_dual_stack_driver_reconnects_over_ipv6() {
    let (left, right) = paired_drivers(dual_stack(), ipv6_loopback());
    let mut pair = Pair::new(left, right);
    let address = reachable(&pair.right, IPV6_LOOPBACK);
    pair.left.connect(pair.right_key(), address).unwrap();
    assert!(pair.both_connected(), "the IK reconnect never completed");
    pair.assert_left_delivers_media_and_control();
}

#[test]
fn an_ipv4_driver_counts_a_datagram_it_cannot_send_to_ipv6() {
    let (mut left, right) = paired_drivers(ipv4_only(), ipv6_loopback());
    let unreachable = reachable(&right, IPV6_LOOPBACK);
    let refusal = left.connect(*right.identity().public(), unreachable);
    assert!(
        matches!(refusal, Err(llts_transport::TransportError::SendFailed { to, .. }) if to == unreachable),
        "an unsendable dial was not reported: {refusal:?}"
    );
    assert_eq!(left.refused_sends(), 1);
}
