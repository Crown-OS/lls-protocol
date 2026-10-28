#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use llts_core::buffer::{BufferPool, Datagram};
use llts_core::clock::Instant;
use llts_core::link::LinkKind;
use llts_core::repair::RepairMode;
use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, PathId, PeerConfig, PeerSession, Role, SessionPools};
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_transport::security::{
    DeviceClass, DeviceIdentity, Hello, KeyEpoch, NoiseHandshake, ReconnectInitiator,
    ReconnectResponder, SessionKeys, TAG_LEN,
};
use llts_transport::udp::UdpLink;
use llts_transport::{PacketLink, Tai64N, TransportError};
use llts_wire::packet::{PacketFlags, PacketHeader, PacketKind, PathProbe, SequenceSpace};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, SessionTag,
    StreamClass, StreamId,
};

const MTU: usize = 1200;
const SCREEN: StreamId = StreamId::compose(SessionTag::new(7), StreamClass::Video, 0);
const CONTROL: StreamId = StreamId::compose(SessionTag::new(7), StreamClass::Control, 0);
const SCREEN_KEY: StreamKey = StreamKey::new(StreamClass::Video, 0);
const START: Instant = Instant::SESSION_START;

const fn loopback(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

const fn at_millis(millis: u64) -> Instant {
    Instant::from_micros(millis * 1_000)
}

const fn header_of(kind: PacketKind, stream: StreamId, sequence: u16) -> PacketHeader {
    PacketHeader {
        version: ProtocolVersion::CURRENT,
        kind,
        flags: PacketFlags::FRAME_END,
        stream,
        sequence: SequenceNumber::new(sequence),
        frame: FrameId::new(3),
        timestamp: MediaTimestamp::new(90_000),
        packet_index: 0,
        packet_count: 1,
        departure: MonotonicMicros::new(1_234),
        payload_offset: 0,
    }
}

const fn header(sequence: u16) -> PacketHeader {
    header_of(PacketKind::Media, SCREEN, sequence)
}

/// Builds a datagram with room for the authentication tag, as a pooled buffer would have.
fn framed_with(header: &PacketHeader, body: &[u8]) -> (Vec<u8>, usize) {
    let mut datagram = vec![0u8; PacketHeader::ENCODED_LEN + body.len() + TAG_LEN];
    header.write_into(&mut datagram).unwrap();
    datagram[PacketHeader::ENCODED_LEN..PacketHeader::ENCODED_LEN + body.len()]
        .copy_from_slice(body);
    (datagram, body.len())
}

fn framed(sequence: u16, body: &[u8]) -> (Vec<u8>, usize) {
    framed_with(&header(sequence), body)
}

/// Seals a media packet and returns just the sealed bytes.
fn sealed_media(keys: &mut SessionKeys, sequence: u16, now: Instant) -> Vec<u8> {
    let (mut datagram, body_len) = framed(sequence, &sequence.to_le_bytes());
    let sealed = keys.sending.seal(&mut datagram, body_len, now).unwrap();
    datagram.truncate(sealed.len);
    datagram
}

/// A probe with room for the sender's counter records.
fn sealed_probe(keys: &mut SessionKeys, sequence: u16, now: Instant) -> Vec<u8> {
    let mut datagram = vec![0u8; 512];
    header_of(PacketKind::Probe, CONTROL, sequence)
        .write_into(&mut datagram)
        .unwrap();
    let body_len = PathProbe { token: 99 }
        .write_body(&mut datagram[PacketHeader::ENCODED_LEN..])
        .unwrap();
    let sealed = keys.sending.seal(&mut datagram, body_len, now).unwrap();
    datagram.truncate(sealed.len);
    datagram
}

fn opens(keys: &mut SessionKeys, datagram: &[u8], now: Instant) -> Result<usize, TransportError> {
    let mut copy = datagram.to_vec();
    keys.receiving.open(&mut copy, now)
}

fn hello() -> Hello {
    Hello::new("device", DeviceClass::Laptop, 0).with_session_tag(7)
}

/// Runs the Noise IK exchange to completion and returns both sides' keys.
fn paired_sessions() -> (SessionKeys, SessionKeys) {
    let responder_identity = DeviceIdentity::generate().unwrap();
    let initiator_identity = DeviceIdentity::generate().unwrap();

    let mut first = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
    let mut second = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
    let (initiator, len) = ReconnectInitiator::start(
        &initiator_identity,
        responder_identity.public(),
        &hello(),
        Tai64N::now(),
        &mut first,
    )
    .unwrap();
    let (accepted, len) = ReconnectResponder::accept(
        &responder_identity,
        &hello(),
        &first[..len],
        &mut second,
        |_, _| Ok(()),
    )
    .unwrap();
    let established = initiator.on_response(&second[..len]).unwrap();
    (established.keys, accepted.established.keys)
}

#[test]
fn a_paired_handshake_agrees_on_both_directions() {
    let (mut initiator, mut responder) = paired_sessions();

    let (mut datagram, body_len) = framed(1, b"upstream payload");
    let sealed = initiator
        .sending
        .seal(&mut datagram, body_len, START)
        .unwrap();
    let opened = responder
        .receiving
        .open(&mut datagram[..sealed.len], START)
        .unwrap();

    assert_eq!(
        &datagram[PacketHeader::ENCODED_LEN..opened],
        b"upstream payload"
    );
}

#[test]
fn the_reverse_direction_uses_its_own_key() {
    let (mut initiator, mut responder) = paired_sessions();

    let (mut datagram, body_len) = framed(1, b"downstream payload");
    let sealed = responder
        .sending
        .seal(&mut datagram, body_len, START)
        .unwrap();
    let opened = initiator
        .receiving
        .open(&mut datagram[..sealed.len], START)
        .unwrap();

    assert_eq!(
        &datagram[PacketHeader::ENCODED_LEN..opened],
        b"downstream payload"
    );
}

#[test]
fn a_datagram_cannot_be_reflected_back_to_its_sender() {
    let (mut initiator, _) = paired_sessions();
    let sealed = sealed_media(&mut initiator, 1, START);
    assert!(matches!(
        opens(&mut initiator, &sealed, START),
        Err(TransportError::Unauthenticated)
    ));
}

#[test]
fn sealing_costs_exactly_the_tag() {
    let (mut initiator, _) = paired_sessions();
    let body = vec![0u8; 1_138];
    let (mut datagram, body_len) = framed(1, &body);

    let sealed = initiator
        .sending
        .seal(&mut datagram, body_len, START)
        .unwrap();
    assert_eq!(sealed.len, PacketHeader::ENCODED_LEN + body_len + TAG_LEN);
    assert!(
        sealed.len <= MTU,
        "a sealed datagram must still fit the path"
    );
}

#[test]
fn a_tampered_header_is_rejected() {
    let (mut initiator, mut responder) = paired_sessions();
    let mut datagram = sealed_media(&mut initiator, 1, START);

    // Flip a flag bit. It is not part of the nonce, so only the AAD covers it.
    datagram[3] ^= PacketFlags::FRAME_END.bits();

    assert!(matches!(
        responder.receiving.open(&mut datagram, START),
        Err(TransportError::Unauthenticated)
    ));
}

#[test]
fn a_datagram_moved_to_another_stream_is_rejected() {
    let (mut initiator, mut responder) = paired_sessions();
    let mut datagram = sealed_media(&mut initiator, 1, START);
    let other = StreamId::compose(SessionTag::new(7), StreamClass::Video, 1);
    datagram[4..8].copy_from_slice(&other.get().to_le_bytes());

    assert!(matches!(
        responder.receiving.open(&mut datagram, START),
        Err(TransportError::Unauthenticated)
    ));
}

#[test]
fn a_tampered_body_is_rejected() {
    let (mut initiator, mut responder) = paired_sessions();
    let mut datagram = sealed_media(&mut initiator, 1, START);

    datagram[PacketHeader::ENCODED_LEN] ^= 0xFF;

    assert!(matches!(
        responder.receiving.open(&mut datagram, START),
        Err(TransportError::Unauthenticated)
    ));
}

#[test]
fn an_unpaired_peer_cannot_complete_the_handshake() {
    let responder_identity = DeviceIdentity::generate().unwrap();
    let initiator_identity = DeviceIdentity::generate().unwrap();
    let stranger = DeviceIdentity::generate().unwrap();

    let mut first = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
    let mut second = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
    let (_, len) = ReconnectInitiator::start(
        &initiator_identity,
        stranger.public(),
        &hello(),
        Tai64N::now(),
        &mut first,
    )
    .unwrap();

    assert!(
        ReconnectResponder::accept(
            &responder_identity,
            &hello(),
            &first[..len],
            &mut second,
            |_, _| Ok(())
        )
        .is_err(),
        "a responder accepted a handshake addressed to a different device"
    );
}

#[test]
fn sequences_that_wrap_never_reuse_a_nonce() {
    let (mut initiator, mut responder) = paired_sessions();
    let mut nonces = HashSet::new();

    // Cross the 16-bit wraparound. A repeated nonce here would be a catastrophic key reuse, and
    // it would show up as the two sides disagreeing.
    for sequence in [65_530u16, 65_533, 65_535, 0, 2, 5] {
        let body = sequence.to_le_bytes();
        let (mut datagram, body_len) = framed(sequence, &body);
        let sealed = initiator
            .sending
            .seal(&mut datagram, body_len, START)
            .unwrap();
        assert!(nonces.insert(sealed.nonce()));
        let opened = responder
            .receiving
            .open(&mut datagram[..sealed.len], START)
            .unwrap();
        assert_eq!(&datagram[PacketHeader::ENCODED_LEN..opened], &body);
    }
}

#[test]
fn a_sequence_that_does_not_advance_is_refused_before_sealing() {
    let (mut initiator, _) = paired_sessions();
    sealed_media(&mut initiator, 5, START);

    for repeated in [5u16, 4] {
        let (mut datagram, body_len) = framed(repeated, b"again");
        assert!(matches!(
            initiator.sending.seal(&mut datagram, body_len, START),
            Err(TransportError::SequenceRegressed {
                space: SequenceSpace::Media,
                ..
            })
        ));
    }
}

#[test]
fn a_reopened_stream_starting_over_keeps_advancing_its_nonces() {
    let (mut initiator, mut responder) = paired_sessions();
    for sequence in 0..300u16 {
        let datagram = sealed_media(&mut initiator, sequence, START);
        opens(&mut responder, &datagram, START).unwrap();
    }
    for sequence in 0..3u16 {
        let datagram = sealed_media(&mut initiator, sequence, START);
        opens(&mut responder, &datagram, START).unwrap();
    }
}

#[test]
fn one_sequence_number_in_two_spaces_gets_two_nonces() {
    let (mut initiator, mut responder) = paired_sessions();

    let (mut media, media_len) = framed_with(&header_of(PacketKind::Media, SCREEN, 5), b"m");
    let (mut feedback, feedback_len) = framed_with(
        &header_of(PacketKind::Feedback, SCREEN, 5),
        &[0, 0, 5, 0, 1, 0, 0, 0, 0],
    );
    let media_sealed = initiator
        .sending
        .seal(&mut media, media_len, START)
        .unwrap();
    let feedback_sealed = initiator
        .sending
        .seal(&mut feedback, feedback_len, START)
        .unwrap();

    assert_ne!(media_sealed.nonce(), feedback_sealed.nonce());
    assert_eq!(feedback_sealed.space, SequenceSpace::Feedback);
    responder
        .receiving
        .open(&mut media[..media_sealed.len], START)
        .unwrap();
    responder
        .receiving
        .open(&mut feedback[..feedback_sealed.len], START)
        .unwrap();
}

#[test]
fn a_forged_far_future_sequence_does_not_move_the_receiver() {
    let (mut initiator, mut responder) = paired_sessions();
    for sequence in 1..=100 {
        let sealed = sealed_media(&mut initiator, sequence, START);
        opens(&mut responder, &sealed, START).unwrap();
    }

    // Two forgeries, each half a counter space further on. Had either moved the extender, the
    // next genuine packet would be read as a rollover later than it is and fail to open.
    for forged_sequence in [32_100u16, 64_100] {
        let (mut forged, body_len) = framed(forged_sequence, b"forged");
        forged.truncate(PacketHeader::ENCODED_LEN + body_len + TAG_LEN);
        assert!(matches!(
            opens(&mut responder, &forged, START),
            Err(TransportError::Unauthenticated)
        ));
    }

    let genuine = sealed_media(&mut initiator, 101, START);
    opens(&mut responder, &genuine, START).unwrap();
}

#[test]
fn a_replayed_datagram_is_refused() {
    let (mut initiator, mut responder) = paired_sessions();
    let first = sealed_media(&mut initiator, 1, START);
    opens(&mut responder, &first, START).unwrap();
    assert!(matches!(
        opens(&mut responder, &first, START),
        Err(TransportError::Replayed)
    ));

    for sequence in 2..2_000 {
        let sealed = sealed_media(&mut initiator, sequence, START);
        opens(&mut responder, &sealed, START).unwrap();
    }
    assert!(
        opens(&mut responder, &first, START).is_err(),
        "a datagram far older than the replay window was accepted"
    );
}

#[test]
fn a_reordered_datagram_inside_the_window_still_opens() {
    let (mut initiator, mut responder) = paired_sessions();
    let late = sealed_media(&mut initiator, 1, START);
    for sequence in 2..500 {
        let sealed = sealed_media(&mut initiator, sequence, START);
        opens(&mut responder, &sealed, START).unwrap();
    }
    opens(&mut responder, &late, START).unwrap();
}

/// Seals `count` datagrams that never arrive, as a Wi-Fi blackout would lose them.
fn black_out(keys: &mut SessionKeys, from: u16, count: u32) -> u16 {
    let mut sequence = from;
    for _ in 0..count {
        sealed_media(keys, sequence, START);
        sequence = sequence.wrapping_add(1);
    }
    sequence
}

#[test]
fn a_ten_second_blackout_at_nine_thousand_packets_a_second_recovers_by_itself() {
    let (mut initiator, mut responder) = paired_sessions();
    let first = sealed_media(&mut initiator, 0, START);
    opens(&mut responder, &first, START).unwrap();

    let resumed = black_out(&mut initiator, 1, 90_000);

    let after = sealed_media(&mut initiator, resumed, START);
    opens(&mut responder, &after, START).unwrap();
    let next = sealed_media(&mut initiator, resumed.wrapping_add(1), START);
    opens(&mut responder, &next, START).unwrap();
}

#[test]
fn a_longer_blackout_is_closed_by_the_counters_a_probe_carries() {
    let (mut initiator, mut responder) = paired_sessions();
    let first = sealed_media(&mut initiator, 0, START);
    opens(&mut responder, &first, START).unwrap();

    let resumed = black_out(&mut initiator, 1, 180_000);
    let lost_in_step = sealed_media(&mut initiator, resumed, START);
    assert!(
        opens(&mut responder, &lost_in_step, START).is_err(),
        "two rollovers behind should be beyond what inference alone can recover"
    );

    let probe = sealed_probe(&mut initiator, 0, START);
    opens(&mut responder, &probe, START).unwrap();

    let after = sealed_media(&mut initiator, resumed.wrapping_add(1), START);
    opens(&mut responder, &after, START).unwrap();
}

#[test]
fn the_key_phase_flips_on_rekey_and_the_old_key_lingers_briefly() {
    let (mut initiator, mut responder) = paired_sessions();
    let before = sealed_media(&mut initiator, 1, START);
    opens(&mut responder, &before, START).unwrap();

    let straggler = sealed_media(&mut initiator, 2, at_millis(119_000));
    let late_straggler = sealed_media(&mut initiator, 3, at_millis(119_500));
    let rekeyed_at = at_millis(121_000);
    let rekeyed = sealed_media(&mut initiator, 4, rekeyed_at);
    assert_eq!(initiator.sending.epoch(), KeyEpoch::FIRST.next().unwrap());
    assert!(PacketFlags::from_bits_retain(rekeyed[3]).contains(PacketFlags::KEY_PHASE));

    opens(&mut responder, &rekeyed, rekeyed_at).unwrap();
    assert_eq!(responder.receiving.epoch(), initiator.sending.epoch());

    opens(&mut responder, &straggler, at_millis(122_000)).unwrap();
    assert!(
        opens(&mut responder, &late_straggler, at_millis(125_000)).is_err(),
        "a retired key was still accepted after its grace period"
    );
}

/// A sending and a receiving session with the packet ciphers between them, dropping one datagram
/// in `drop_every` so NACKs and retransmissions run as they would on Wi-Fi.
struct SealedLoopback {
    sender: PeerSession,
    receiver: PeerSession,
    sender_keys: SessionKeys,
    receiver_keys: SessionKeys,
    now: Instant,
    drop_every: u64,
    sealed: u64,
    retransmissions: u64,
    nonces: HashSet<(bool, StreamId, KeyEpoch, [u8; 12])>,
}

impl SealedLoopback {
    fn new(drop_every: u64) -> Self {
        let session = |local, peer, role| {
            let mut session = PeerSession::new(
                PeerConfig::lan(
                    SessionTag::new(local),
                    SessionTag::new(peer),
                    LinkKind::Wifi,
                ),
                SessionPools {
                    datagrams: BufferPool::new(MTU, 1_024),
                    frames: BufferPool::new(64 * 1024, 8),
                    shards: BufferPool::new(MTU, 64),
                    messages: BufferPool::new(64 * 1024, 2),
                },
            );
            let mut config = MediaStreamConfig::screen_share(role);
            // Without parity every lost fragment has to be retransmitted, which is the path
            // whose nonce handling this exercises.
            config.repair = RepairMode::Off;
            session.open_media(SCREEN_KEY, config).unwrap();
            session
        };
        let (sender_keys, receiver_keys) = paired_sessions();
        Self {
            sender: session(0x0007, 0x0007, Role::Sender),
            receiver: session(0x0007, 0x0007, Role::Receiver),
            sender_keys,
            receiver_keys,
            now: START,
            drop_every,
            sealed: 0,
            retransmissions: 0,
            nonces: HashSet::new(),
        }
    }

    /// Seals the datagram in place, records its nonce, and opens it on the far side, returning
    /// the plaintext or `None` when the loss model eats it.
    fn carry(&mut self, mut datagram: Datagram, upstream: bool) -> Option<Vec<u8>> {
        let (sealing, opening) = if upstream {
            (&mut self.sender_keys, &mut self.receiver_keys)
        } else {
            (&mut self.receiver_keys, &mut self.sender_keys)
        };
        let (buffer, filled) = datagram.sealable_mut();
        let body_len = filled - PacketHeader::ENCODED_LEN;
        let sealed = sealing.sending.seal(buffer, body_len, self.now).unwrap();
        assert!(
            self.nonces
                .insert((upstream, sealed.stream, sealed.epoch, sealed.nonce())),
            "nonce reused: {sealed:?}"
        );
        if sealed.space == SequenceSpace::Retransmission {
            self.retransmissions += 1;
        }
        self.sealed += 1;
        if self.sealed.is_multiple_of(self.drop_every) {
            return None;
        }
        let mut wire = buffer[..sealed.len].to_vec();
        let opened = opening.receiving.open(&mut wire, self.now).unwrap();
        wire.truncate(opened);
        Some(wire)
    }

    fn pump(&mut self) -> bool {
        let mut progressed = false;
        while let Some(action) = self.sender.poll_action() {
            if let Action::Transmit { datagram, .. } = action {
                progressed = true;
                if let Some(plaintext) = self.carry(datagram, true) {
                    self.receiver
                        .handle(
                            self.now,
                            &Event::DatagramReceived {
                                path: PathId::default(),
                                bytes: &plaintext,
                                arrived_at: self.now.wire_micros(),
                            },
                        )
                        .unwrap();
                }
            }
        }
        while let Some(action) = self.receiver.poll_action() {
            if let Action::Transmit { datagram, .. } = action {
                progressed = true;
                if let Some(plaintext) = self.carry(datagram, false) {
                    self.sender
                        .handle(
                            self.now,
                            &Event::DatagramReceived {
                                path: PathId::default(),
                                bytes: &plaintext,
                                arrived_at: self.now.wire_micros(),
                            },
                        )
                        .unwrap();
                }
            }
        }
        progressed
    }

    fn settle(&mut self, until: Instant) {
        loop {
            if self.pump() {
                continue;
            }
            let next = [
                self.sender.next_timeout(self.now),
                self.receiver.next_timeout(self.now),
            ]
            .into_iter()
            .flatten()
            .filter(|deadline| deadline.is_after(self.now) && !deadline.is_after(until))
            .min();
            let Some(next) = next else {
                self.now = until;
                return;
            };
            self.now = next;
            self.sender.poll(self.now).unwrap();
            self.receiver.poll(self.now).unwrap();
        }
    }
}

#[test]
fn two_hundred_thousand_mixed_packets_never_reuse_a_nonce() {
    let mut link = SealedLoopback::new(50);
    let payload = vec![0x5A; 6_000];
    let period = Duration::from_micros(16_667);
    let mut tick = 0u32;

    while link.sealed < 200_000 {
        let slice = EncodedSlice::new(&payload, MediaTimestamp::new(tick.wrapping_mul(1_500)))
            .ending_frame();
        link.sender
            .handle(
                link.now,
                &Event::SliceEncoded {
                    stream: SCREEN_KEY,
                    slice,
                },
            )
            .unwrap();
        let until = link.now.saturating_add(period);
        link.settle(until);
        tick = tick.wrapping_add(1);
    }

    assert!(
        link.retransmissions > 1_000,
        "only {} retransmissions: the run never exercised the retransmission space",
        link.retransmissions
    );
    assert!(
        link.sender_keys.sending.epoch() > KeyEpoch::FIRST,
        "the run never crossed a rekey"
    );
    let stats = link.receiver.stats(SCREEN_KEY).unwrap();
    assert!(
        stats.frames_presented > u64::from(tick) * 9 / 10,
        "the sealed link stopped carrying media: {tick} frames sent, {stats:?}"
    );
}

#[test]
fn two_udp_links_carry_a_datagram_over_loopback() {
    let left_addr = loopback(0);
    let mut left = UdpLink::connect(left_addr, loopback(1), LinkKind::Loopback, MTU).unwrap();
    let left_port = left.local_addr().unwrap().port();

    let mut right =
        UdpLink::connect(loopback(0), loopback(left_port), LinkKind::Loopback, MTU).unwrap();
    let right_port = right.local_addr().unwrap().port();

    // Re-point the left link now that the right one has a real port.
    left = UdpLink::connect(
        loopback(left_port),
        loopback(right_port),
        LinkKind::Loopback,
        MTU,
    )
    .unwrap();

    left.send(b"hello over the wire").unwrap();

    let mut received = [0u8; MTU];
    let mut len = None;
    for _ in 0..1_000 {
        if let Some(bytes) = right.recv(&mut received).unwrap() {
            len = Some(bytes);
            break;
        }
        std::thread::yield_now();
    }

    assert_eq!(&received[..len.unwrap()], b"hello over the wire");
    assert_eq!(right.properties().kind, LinkKind::Loopback);
}

#[test]
fn an_oversized_datagram_is_refused_rather_than_fragmented() {
    let mut link = UdpLink::connect(loopback(0), loopback(9), LinkKind::Wifi, MTU).unwrap();
    assert!(matches!(
        link.send(&vec![0u8; MTU + 1]),
        Err(TransportError::DatagramTooLarge { .. })
    ));
}

#[test]
fn a_quiet_socket_reports_nothing_rather_than_blocking() {
    let mut link = UdpLink::connect(loopback(0), loopback(9), LinkKind::Wifi, MTU).unwrap();
    let mut buffer = [0u8; MTU];
    assert_eq!(link.recv(&mut buffer).unwrap(), None);
}
