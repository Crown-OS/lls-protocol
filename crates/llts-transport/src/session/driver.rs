use std::collections::VecDeque;
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, RawFd};
use std::time::{Duration, Instant as WallClock, SystemTime, UNIX_EPOCH};

use llts_core::buffer::{BufferPool, Datagram};
use llts_core::channel::MAX_MESSAGE_LEN;
use llts_core::clock::Instant;
use llts_core::link::LinkKind;
use llts_core::session::{Action, Event, PeerConfig, PeerSession, SessionPools};
use llts_wire::SessionTag;
use llts_wire::packet::{HandshakePattern, HandshakeView, PacketHeader, PacketKind};

use super::connection::{CachedReply, Commitment, Connection, Resend};
use super::event::DriverEvent;
use super::framing::handshake_datagram;
use super::handshaking::{InProgress, PendingHandshake};
use crate::pairing::{
    HandshakeRateLimiter, PairingMode, PairingWindow, QrOffer, TrustStore, TrustedPeer,
};
use crate::security::{
    DeviceClass, DeviceIdentity, Established, HandshakeKind, Hello, NearbyInitiator,
    NearbyResponder, NoiseHandshake, PairingOutcome, PresharedKey, QrInitiator, QrResponder,
    ReconnectInitiator, ReconnectResponder, StaticKey,
};
use crate::udp::batch::MAX_BATCH;
use crate::udp::dscp::tos_for;
use crate::udp::{Outgoing, SessionSocket};
use crate::util::{canonical, random_bytes};
use crate::{Tai64N, TransportError};

/// How this device introduces itself.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    pub name: String,
    pub class: DeviceClass,
    pub capabilities: u64,
    pub link: LinkKind,
    pub mtu: usize,
}

impl DriverConfig {
    pub fn new(name: impl Into<String>, class: DeviceClass) -> Self {
        Self {
            name: name.into(),
            class,
            capabilities: 0,
            link: LinkKind::Wifi,
            mtu: llts_core::send::DatagramSizing::CONSERVATIVE_MTU,
        }
    }
}

#[derive(Debug)]
enum Outbound {
    Sealed {
        datagram: Datagram,
        to: SocketAddr,
        tos: u8,
    },
    Handshake {
        bytes: Vec<u8>,
        to: SocketAddr,
    },
}

impl Outbound {
    fn as_outgoing(&self) -> Outgoing<'_> {
        match self {
            Self::Sealed { datagram, to, tos } => Outgoing {
                bytes: datagram.bytes(),
                to: *to,
                tos: *tos,
            },
            Self::Handshake { bytes, to } => Outgoing {
                bytes,
                to: *to,
                tos: tos_for(PacketKind::Handshake, None),
            },
        }
    }
}

/// Everything the driver holds except the socket, so received datagrams can be processed in
/// place while the socket still owns their buffers.
#[derive(Debug)]
struct DriverState<T> {
    identity: DeviceIdentity,
    config: DriverConfig,
    trust: T,
    pools: SessionPools,
    connections: Vec<Connection>,
    handshakes: Vec<PendingHandshake>,
    pairing: PairingWindow,
    limiter: HandshakeRateLimiter,
    events: VecDeque<DriverEvent>,
    outbox: Vec<Outbound>,
    origin: WallClock,
    next_tag: u16,
    refused_sends: u64,
}

/// A small, runtime-free driver for every session on one socket.
///
/// Receives in batches, routes each datagram by its session tag — to a handshake, or to a
/// session's packet cipher and then its sans-I/O core — and sends whatever the cores and
/// handshakes produced in one batch. Call [`Driver::poll`] whenever the socket is readable or
/// [`Driver::next_timeout`] has passed; [`Driver::wait`] does both for a thread that has nothing
/// else to do, and the raw descriptor is exposed for an application that runs its own reactor.
#[derive(Debug)]
pub struct Driver<T> {
    socket: SessionSocket,
    state: DriverState<T>,
}

impl<T: TrustStore> Driver<T> {
    pub fn bind(
        local: SocketAddr,
        identity: DeviceIdentity,
        trust: T,
        config: DriverConfig,
    ) -> Result<Self, TransportError> {
        let pools = SessionPools {
            datagrams: BufferPool::new(config.mtu, 2_048),
            frames: BufferPool::new(1 << 20, 8),
            shards: BufferPool::new(config.mtu, 512),
            messages: BufferPool::new(MAX_MESSAGE_LEN, 4),
        };
        let first_tag = u16::from_le_bytes(random_bytes::<2>()?);
        Ok(Self {
            socket: SessionSocket::bind(local)?,
            state: DriverState {
                identity,
                config,
                trust,
                pools,
                connections: Vec::new(),
                handshakes: Vec::new(),
                pairing: PairingWindow::new(),
                limiter: HandshakeRateLimiter::new(),
                events: VecDeque::new(),
                outbox: Vec::with_capacity(MAX_BATCH),
                origin: WallClock::now(),
                next_tag: first_tag,
                refused_sends: 0,
            },
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.socket.local_addr()
    }

    /// Datagrams the kernel refused to send since this driver was bound, such as to an
    /// unreachable network. Each was skipped so the rest of its batch still went out.
    pub const fn refused_sends(&self) -> u64 {
        self.state.refused_sends
    }

    pub const fn identity(&self) -> &DeviceIdentity {
        &self.state.identity
    }

    pub const fn trust(&self) -> &T {
        &self.state.trust
    }

    /// Peers with a session carrying traffic.
    pub fn connected_peers(&self) -> impl Iterator<Item = &StaticKey> {
        self.state
            .connections
            .iter()
            .filter(|connection| connection.commitment == Commitment::Committed)
            .map(|connection| &connection.peer)
    }

    /// The address `peer`'s session sends to now, for a side channel such as ffsp that has to
    /// dial the same device.
    pub fn peer_address(&self, peer: &StaticKey) -> Option<SocketAddr> {
        let connection = self
            .state
            .connections
            .get(self.state.committed_index(peer)?)?;
        connection.address_of(connection.core.active_path())
    }

    /// The session with `peer`, for opening streams or reading its state.
    pub fn session_mut(&mut self, peer: &StaticKey) -> Option<&mut PeerSession> {
        self.state
            .committed_mut(peer)
            .map(|connection| &mut connection.core)
    }

    /// Reconnects to a paired device.
    pub fn connect(&mut self, peer: StaticKey, address: SocketAddr) -> Result<(), TransportError> {
        if self.state.trust.peer(&peer).is_none() {
            return Err(TransportError::UnknownPeer);
        }
        let now = self.state.now();
        let tag = self.state.allocate_tag();
        let hello = self.state.hello(tag);
        let mut message = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let (initiator, len) = ReconnectInitiator::start(
            &self.state.identity,
            &peer,
            &hello,
            Tai64N::now(),
            &mut message,
        )?;
        let state = InProgress::Reconnect { initiator, peer };
        self.state.begin(
            tag,
            state,
            HandshakeKind::Reconnect,
            message.get(..len).unwrap_or_default(),
            address,
            now,
        )?;
        self.flush()
    }

    /// Starts nearby pairing with a device that has its pairing window open.
    pub fn pair_nearby(&mut self, address: SocketAddr) -> Result<(), TransportError> {
        let now = self.state.now();
        let tag = self.state.allocate_tag();
        let mut message = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let (initiator, len) =
            NearbyInitiator::start(&self.state.identity, self.state.hello(tag), &mut message)?;
        self.state.begin(
            tag,
            InProgress::Nearby(initiator),
            HandshakeKind::NearbyPairing,
            message.get(..len).unwrap_or_default(),
            address,
            now,
        )?;
        self.flush()
    }

    /// Pairs with the device that showed `offer`.
    pub fn pair_with_offer(&mut self, offer: &QrOffer) -> Result<(), TransportError> {
        let address = *offer
            .addresses
            .first()
            .ok_or(TransportError::InvalidPairingOffer("no address"))?;
        let now = self.state.now();
        let tag = self.state.allocate_tag();
        let mut message = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let (initiator, len) = QrInitiator::start(
            &self.state.identity,
            offer.static_key,
            &offer.psk(),
            self.state.hello(tag),
            &mut message,
        )?;
        self.state.begin(
            tag,
            InProgress::Qr(initiator),
            HandshakeKind::QrPairing,
            message.get(..len).unwrap_or_default(),
            address,
            now,
        )?;
        self.flush()
    }

    /// Accepts nearby pairing requests for `duration`.
    pub fn open_nearby_pairing(&mut self, duration: Duration) {
        let now = self.state.now();
        self.state.pairing.open(PairingMode::Nearby, now, duration);
    }

    /// Accepts QR pairing for `duration` and returns the offer to show. Nearby pairing is
    /// accepted in the same window, since each still needs the user to confirm its code.
    pub fn offer_qr_pairing(
        &mut self,
        addresses: Vec<SocketAddr>,
        duration: Duration,
    ) -> Result<QrOffer, TransportError> {
        let now = self.state.now();
        let psk = PresharedKey(random_bytes::<32>()?);
        let offer = QrOffer::new(
            *self.state.identity.public(),
            &psk,
            addresses,
            unix_now().saturating_add(duration.as_secs()),
            self.state.config.name.clone(),
        );
        self.state.pairing.open(PairingMode::Qr(psk), now, duration);
        Ok(offer)
    }

    pub fn close_pairing(&mut self) {
        self.state.pairing.close();
    }

    /// The user saw matching codes: trust the peer from now on.
    pub fn confirm_pairing(&mut self, peer: &StaticKey) -> Result<(), TransportError> {
        let connection = self
            .state
            .connections
            .iter_mut()
            .find(|connection| connection.peer == *peer && connection.awaiting_confirmation)
            .ok_or(TransportError::UnknownPeer)?;
        connection.awaiting_confirmation = false;
        let hello = connection.peer_hello.clone();
        self.state.trust_peer(*peer, &hello)?;
        self.state
            .events
            .push_back(DriverEvent::Connected { peer: *peer, hello });
        Ok(())
    }

    /// Unpairs `peer`: drops it from the trust store along with any session or handshake with
    /// it. Returns whether it was trusted.
    pub fn forget(&mut self, peer: &StaticKey) -> Result<bool, TransportError> {
        self.state
            .connections
            .retain(|connection| connection.peer != *peer);
        self.state
            .handshakes
            .retain(|handshake| handshake.state.reconnecting_to() != Some(peer));
        self.state.trust.remove(peer)
    }

    /// The codes did not match, or the user declined: forget the peer.
    pub fn reject_pairing(&mut self, peer: &StaticKey) {
        self.state
            .connections
            .retain(|connection| !(connection.peer == *peer && connection.awaiting_confirmation));
    }

    /// Hands an application event to the session with `peer`.
    pub fn handle(&mut self, peer: &StaticKey, event: &Event<'_>) -> Result<(), TransportError> {
        let now = self.state.now();
        let index = self
            .state
            .committed_index(peer)
            .ok_or(TransportError::UnknownPeer)?;
        if let Some(connection) = self.state.connections.get_mut(index) {
            connection.core.handle(now, event)?;
        }
        self.state.drain_connection(index, now)?;
        self.flush()
    }

    pub fn poll_event(&mut self) -> Option<DriverEvent> {
        self.state.events.pop_front()
    }

    /// Reads everything waiting, advances every session and handshake, and sends what they
    /// produced.
    pub fn poll(&mut self) -> Result<(), TransportError> {
        let now = self.state.now();
        loop {
            let count = self.socket.receive_batch()?;
            for (bytes, from) in self.socket.received_mut() {
                self.state.on_datagram(bytes, from, now);
            }
            if count < MAX_BATCH {
                break;
            }
        }
        self.state.service(now)?;
        self.flush()
    }

    /// How long until the driver has something to do without new input.
    pub fn next_timeout(&self) -> Option<Duration> {
        let now = self.state.now();
        let sessions = self.state.connections.iter().filter_map(|connection| {
            let resend = connection.resend.as_ref().map(|resend| resend.next_at);
            [connection.core.next_timeout(now), resend]
                .into_iter()
                .flatten()
                .min()
        });
        let handshakes = self
            .state
            .handshakes
            .iter()
            .map(PendingHandshake::next_deadline);
        sessions
            .chain(handshakes)
            .min()
            .map(|deadline| deadline.saturating_elapsed_since(now))
    }

    /// Blocks until a datagram arrives, the next timeout passes, or `limit` elapses, then polls.
    pub fn wait(&mut self, limit: Duration) -> Result<(), TransportError> {
        let timeout = self.next_timeout().map_or(limit, |next| next.min(limit));
        if !timeout.is_zero() {
            self.socket.wait_readable(Some(timeout))?;
        }
        self.poll()
    }

    /// Moves to a fresh socket, as after a network change. Peers see traffic from the new
    /// address, challenge it, and follow.
    pub fn rebind(&mut self, local: SocketAddr) -> Result<(), TransportError> {
        self.socket = SessionSocket::bind(local)?;
        let now = self.state.now();
        for index in 0..self.state.connections.len() {
            if let Some(connection) = self.state.connections.get_mut(index)
                && connection.commitment == Commitment::Committed
            {
                connection.core.handle(now, &Event::LocalAddressChanged)?;
            }
            self.state.drain_connection(index, now)?;
        }
        self.flush()
    }

    /// Sends the outbox. A datagram the kernel refuses is counted in [`Driver::refused_sends`]
    /// and skipped, so one unreachable peer never holds up the others; the first refusal is
    /// still returned once everything else has gone.
    fn flush(&mut self) -> Result<(), TransportError> {
        let mut outbox = std::mem::take(&mut self.state.outbox);
        let mut first_refusal = None;
        for chunk in outbox.chunks(MAX_BATCH) {
            let batch: Vec<Outgoing<'_>> = chunk.iter().map(Outbound::as_outgoing).collect();
            let mut sent = 0;
            while let Some(rest) = batch.get(sent..).filter(|rest| !rest.is_empty()) {
                match self.socket.send_batch(rest) {
                    Ok(0) => break,
                    Ok(accepted) => sent += accepted,
                    Err(refusal) => {
                        self.state.refused_sends += 1;
                        first_refusal.get_or_insert(refusal);
                        sent += 1;
                    }
                }
            }
        }
        outbox.clear();
        self.state.outbox = outbox;
        first_refusal.map_or(Ok(()), Err)
    }
}

impl<T> AsRawFd for Driver<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl<T: TrustStore> DriverState<T> {
    fn now(&self) -> Instant {
        Instant::from_micros(u64::try_from(self.origin.elapsed().as_micros()).unwrap_or(u64::MAX))
    }

    fn hello(&self, tag: SessionTag) -> Hello {
        Hello::new(
            self.config.name.clone(),
            self.config.class,
            self.config.capabilities,
        )
        .with_session_tag(tag.get())
    }

    /// A tag no live session or handshake uses, never zero.
    fn allocate_tag(&mut self) -> SessionTag {
        loop {
            self.next_tag = self.next_tag.wrapping_add(1);
            let tag = SessionTag::new(self.next_tag);
            let taken = !tag.is_assigned()
                || self
                    .connections
                    .iter()
                    .any(|connection| connection.local_tag == tag)
                || self
                    .handshakes
                    .iter()
                    .any(|handshake| handshake.local_tag == tag);
            if !taken {
                return tag;
            }
        }
    }

    fn committed_index(&self, peer: &StaticKey) -> Option<usize> {
        self.connections.iter().position(|connection| {
            connection.peer == *peer && connection.commitment == Commitment::Committed
        })
    }

    fn committed_mut(&mut self, peer: &StaticKey) -> Option<&mut Connection> {
        let index = self.committed_index(peer)?;
        self.connections.get_mut(index)
    }

    fn begin(
        &mut self,
        tag: SessionTag,
        state: InProgress,
        kind: HandshakeKind,
        message: &[u8],
        to: SocketAddr,
        now: Instant,
    ) -> Result<(), TransportError> {
        let to = canonical(to);
        let bytes = handshake_datagram(SessionTag::UNASSIGNED, kind.pattern(), 0, tag, message)?;
        self.outbox.push(Outbound::Handshake {
            bytes: bytes.clone(),
            to,
        });
        self.handshakes
            .push(PendingHandshake::new(tag, state, bytes, to, None, now));
        Ok(())
    }

    fn new_session(&self, local_tag: SessionTag, peer_hello: &Hello) -> PeerSession {
        let mut config = PeerConfig::lan(
            local_tag,
            SessionTag::new(peer_hello.session_tag),
            self.config.link,
        );
        config.sizing = llts_core::send::DatagramSizing::new(self.config.mtu)
            .unwrap_or_else(|_| llts_core::send::DatagramSizing::conservative());
        PeerSession::new(config, self.pools.clone())
    }

    fn trust_peer(&mut self, peer: StaticKey, hello: &Hello) -> Result<(), TransportError> {
        let last_handshake = self
            .trust
            .peer(&peer)
            .and_then(|known| known.last_handshake);
        self.trust.upsert(TrustedPeer {
            static_key: peer,
            name: hello.name.clone(),
            class: hello.class,
            last_handshake,
            paired_at_unix: unix_now(),
        })?;
        self.events.push_back(DriverEvent::Paired { peer });
        Ok(())
    }

    fn on_datagram(&mut self, bytes: &mut [u8], from: SocketAddr, now: Instant) {
        let Ok((header, _)) = PacketHeader::read_from(bytes) else {
            return;
        };
        if header.kind == PacketKind::Handshake {
            if let Ok(view) = HandshakeView::parse(bytes) {
                let _ = self.on_handshake(&view, from, now);
            }
            return;
        }
        let tag = header.stream.session_tag();
        let Some(index) = self
            .connections
            .iter()
            .position(|connection| connection.local_tag == tag)
        else {
            return;
        };
        let Some(connection) = self.connections.get_mut(index) else {
            return;
        };
        let Ok(len) = connection.keys.receiving.open(bytes, now) else {
            return;
        };
        connection.resend = None;
        let path = connection.path_for(from);
        let _ = connection.core.handle(
            now,
            &Event::DatagramReceived {
                path,
                bytes: bytes.get(..len).unwrap_or_default(),
                arrived_at: now.wire_micros(),
            },
        );
        if let Commitment::AwaitingFirstPacket(timestamp) = connection.commitment {
            self.commit(index, timestamp);
        }
    }

    /// A reconnect proved itself: it replaces any older session with the same peer.
    fn commit(&mut self, index: usize, timestamp: Tai64N) {
        let Some(connection) = self.connections.get_mut(index) else {
            return;
        };
        connection.commitment = Commitment::Committed;
        connection.reply = None;
        let peer = connection.peer;
        let local_tag = connection.local_tag;
        let hello = connection.peer_hello.clone();
        let _ = self.trust.record_handshake(&peer, timestamp);
        self.connections
            .retain(|other| other.peer != peer || other.local_tag == local_tag);
        self.events
            .push_back(DriverEvent::Connected { peer, hello });
    }

    fn on_handshake(
        &mut self,
        view: &HandshakeView<'_>,
        from: SocketAddr,
        now: Instant,
    ) -> Result<(), TransportError> {
        if view.message_index() == 0 {
            return self.on_first_message(view, from, now);
        }
        let tag = view.header().stream.session_tag();
        let Some(position) = self
            .handshakes
            .iter()
            .position(|handshake| handshake.local_tag == tag && handshake.address() == from)
        else {
            return Ok(());
        };
        let pending = self.handshakes.swap_remove(position);
        let result = self.advance(pending, view, from, now);
        if let Err(error) = &result {
            self.events.push_back(DriverEvent::HandshakeFailed {
                address: from,
                error: TransportError::HandshakeRejected {
                    reason: failure_reason(error),
                },
            });
        }
        result
    }

    fn on_first_message(
        &mut self,
        view: &HandshakeView<'_>,
        from: SocketAddr,
        now: Instant,
    ) -> Result<(), TransportError> {
        let sender_tag = view.sender_tag();
        if let Some(reply) = self.cached_reply(from, sender_tag) {
            self.outbox.push(Outbound::Handshake {
                bytes: reply,
                to: from,
            });
            return Ok(());
        }
        if !self.limiter.allow(from.ip(), now) {
            return Err(TransportError::RateLimited);
        }
        let mut out = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        match HandshakeKind::from_pattern(view.pattern()) {
            HandshakeKind::Reconnect => self.accept_reconnect(view, from),
            HandshakeKind::NearbyPairing => {
                if self.pairing.mode(now).is_none() {
                    return Err(TransportError::PairingClosed);
                }
                let tag = self.allocate_tag();
                let (responder, len) = NearbyResponder::accept(
                    &self.identity,
                    &self.hello(tag),
                    view.message(),
                    &mut out,
                )?;
                self.answer(
                    tag,
                    InProgress::NearbyAnswer(responder),
                    view,
                    out.get(..len).unwrap_or_default(),
                    from,
                    now,
                )
            }
            HandshakeKind::QrPairing => {
                let Some(PairingMode::Qr(psk)) = self.pairing.mode(now).cloned() else {
                    return Err(TransportError::PairingClosed);
                };
                let tag = self.allocate_tag();
                let (responder, len) = QrResponder::accept(
                    &self.identity,
                    &psk,
                    &self.hello(tag),
                    view.message(),
                    &mut out,
                )?;
                self.answer(
                    tag,
                    InProgress::QrAnswer(responder),
                    view,
                    out.get(..len).unwrap_or_default(),
                    from,
                    now,
                )
            }
        }
    }

    fn cached_reply(&self, from: SocketAddr, sender_tag: SessionTag) -> Option<Vec<u8>> {
        let from_connection = self.connections.iter().find_map(|connection| {
            connection
                .reply
                .as_ref()
                .filter(|reply| reply.from == from && reply.sender_tag == sender_tag)
                .map(|reply| reply.bytes.clone())
        });
        from_connection.or_else(|| {
            self.handshakes
                .iter()
                .find(|handshake| {
                    handshake.answering == Some(sender_tag) && handshake.address() == from
                })
                .map(|handshake| handshake.message.bytes.clone())
        })
    }

    fn answer(
        &mut self,
        tag: SessionTag,
        state: InProgress,
        view: &HandshakeView<'_>,
        message: &[u8],
        from: SocketAddr,
        now: Instant,
    ) -> Result<(), TransportError> {
        let bytes = handshake_datagram(view.sender_tag(), view.pattern(), 1, tag, message)?;
        self.outbox.push(Outbound::Handshake {
            bytes: bytes.clone(),
            to: from,
        });
        self.handshakes.push(PendingHandshake::new(
            tag,
            state,
            bytes,
            from,
            Some(view.sender_tag()),
            now,
        ));
        Ok(())
    }

    fn accept_reconnect(
        &mut self,
        view: &HandshakeView<'_>,
        from: SocketAddr,
    ) -> Result<(), TransportError> {
        let tag = self.allocate_tag();
        let hello = self.hello(tag);
        let mut out = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let (trust, connections) = (&self.trust, &self.connections);
        let (accepted, len) = ReconnectResponder::accept(
            &self.identity,
            &hello,
            view.message(),
            &mut out,
            |peer, timestamp| {
                if trust.peer(peer).is_none() {
                    return Err(TransportError::UnknownPeer);
                }
                let pending_newer = connections.iter().any(|connection| {
                    connection.peer == *peer
                        && matches!(connection.commitment, Commitment::AwaitingFirstPacket(held) if held >= timestamp)
                });
                if pending_newer || !trust.admits(peer, timestamp) {
                    return Err(TransportError::Replayed);
                }
                Ok(())
            },
        )?;
        let Established {
            keys,
            peer,
            peer_hello,
        } = accepted.established;
        if peer_hello.session_tag != view.sender_tag().get() {
            return Err(TransportError::HandshakeRejected {
                reason: "the Hello names a different session tag than the handshake",
            });
        }

        if let Some(position) = self
            .handshakes
            .iter()
            .position(|handshake| handshake.state.reconnecting_to() == Some(&peer))
        {
            // Both sides dialled at once. The session the lower static key initiated wins, so
            // both ends pick the same one without another round trip.
            if self.identity.public() < &peer {
                return Ok(());
            }
            self.handshakes.swap_remove(position);
        }

        let reply = handshake_datagram(
            view.sender_tag(),
            HandshakePattern::Ik,
            1,
            tag,
            out.get(..len).unwrap_or_default(),
        )?;
        self.outbox.push(Outbound::Handshake {
            bytes: reply.clone(),
            to: from,
        });
        let core = self.new_session(tag, &peer_hello);
        let mut connection = Connection::new(
            peer,
            peer_hello,
            tag,
            keys,
            core,
            Commitment::AwaitingFirstPacket(accepted.timestamp),
            from,
        );
        connection.reply = Some(CachedReply {
            from,
            sender_tag: view.sender_tag(),
            bytes: reply,
        });
        self.connections.push(connection);
        Ok(())
    }

    /// Feeds a reply to the handshake it answers.
    fn advance(
        &mut self,
        pending: PendingHandshake,
        view: &HandshakeView<'_>,
        from: SocketAddr,
        now: Instant,
    ) -> Result<(), TransportError> {
        let mut out = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let tag = pending.local_tag;
        match (pending.state, view.message_index()) {
            (InProgress::Reconnect { initiator, peer }, 1) => {
                let established = initiator.on_response(view.message())?;
                if established.peer != peer {
                    return Err(TransportError::UnknownPeer);
                }
                self.establish(tag, established, from, view)
            }
            (InProgress::Nearby(initiator), 1) => {
                let (outcome, len) = initiator.on_response(view.message(), &mut out)?;
                let confirmation =
                    self.confirm(view, tag, out.get(..len).unwrap_or_default(), from)?;
                self.paired(
                    tag,
                    outcome,
                    from,
                    view,
                    Some(Resend::new(confirmation, from, now)),
                )
            }
            (InProgress::Qr(initiator), 1) => {
                let (outcome, len) = initiator.on_response(view.message(), &mut out)?;
                let confirmation =
                    self.confirm(view, tag, out.get(..len).unwrap_or_default(), from)?;
                self.paired(
                    tag,
                    outcome,
                    from,
                    view,
                    Some(Resend::new(confirmation, from, now)),
                )
            }
            (InProgress::NearbyAnswer(responder), 2) => {
                let outcome = responder.on_reveal(view.message())?;
                self.paired(tag, outcome, from, view, None)
            }
            (InProgress::QrAnswer(responder), 2) => {
                let outcome = responder.on_confirmation(view.message())?;
                self.paired(tag, outcome, from, view, None)
            }
            _ => Err(TransportError::HandshakeRejected {
                reason: "a handshake message arrived out of order",
            }),
        }
    }

    /// Sends the initiator's third message and returns it for resending.
    fn confirm(
        &mut self,
        view: &HandshakeView<'_>,
        tag: SessionTag,
        message: &[u8],
        to: SocketAddr,
    ) -> Result<Vec<u8>, TransportError> {
        let bytes = handshake_datagram(view.sender_tag(), view.pattern(), 2, tag, message)?;
        self.outbox.push(Outbound::Handshake {
            bytes: bytes.clone(),
            to,
        });
        Ok(bytes)
    }

    fn establish(
        &mut self,
        tag: SessionTag,
        established: Established,
        from: SocketAddr,
        view: &HandshakeView<'_>,
    ) -> Result<(), TransportError> {
        check_tag(&established.peer_hello, view)?;
        let core = self.new_session(tag, &established.peer_hello);
        let peer = established.peer;
        let hello = established.peer_hello.clone();
        self.connections
            .retain(|connection| connection.peer != peer);
        self.connections.push(Connection::new(
            peer,
            established.peer_hello,
            tag,
            established.keys,
            core,
            Commitment::Committed,
            from,
        ));
        self.events
            .push_back(DriverEvent::Connected { peer, hello });
        Ok(())
    }

    fn paired(
        &mut self,
        tag: SessionTag,
        outcome: PairingOutcome,
        from: SocketAddr,
        view: &HandshakeView<'_>,
        resend: Option<Resend>,
    ) -> Result<(), TransportError> {
        check_tag(&outcome.peer_hello, view)?;
        let PairingOutcome {
            keys,
            peer,
            peer_hello,
            code,
        } = outcome;
        let core = self.new_session(tag, &peer_hello);
        self.connections
            .retain(|connection| connection.peer != peer);
        let mut connection = Connection::new(
            peer,
            peer_hello.clone(),
            tag,
            keys,
            core,
            Commitment::Committed,
            from,
        );
        connection.resend = resend;
        match code {
            Some(code) => {
                connection.awaiting_confirmation = true;
                self.events.push_back(DriverEvent::PairingCode {
                    peer,
                    hello: peer_hello,
                    code,
                });
            }
            None => {
                self.trust_peer(peer, &peer_hello)?;
                self.events.push_back(DriverEvent::Connected {
                    peer,
                    hello: peer_hello,
                });
            }
        }
        self.connections.push(connection);
        Ok(())
    }

    /// Resends overdue handshake messages, expires abandoned handshakes, and collects every
    /// session's output.
    fn service(&mut self, now: Instant) -> Result<(), TransportError> {
        let mut index = 0;
        while let Some(handshake) = self.handshakes.get_mut(index) {
            if !handshake.expires_at.is_after(now) {
                let expired = self.handshakes.swap_remove(index);
                if expired.answering.is_none() {
                    self.events.push_back(DriverEvent::HandshakeFailed {
                        address: expired.address(),
                        error: TransportError::HandshakeRejected {
                            reason: "the peer never answered",
                        },
                    });
                }
                continue;
            }
            if handshake.message.take_due(now) {
                self.outbox.push(Outbound::Handshake {
                    bytes: handshake.message.bytes.clone(),
                    to: handshake.message.to,
                });
            }
            index += 1;
        }

        for index in 0..self.connections.len() {
            if let Some(connection) = self.connections.get_mut(index) {
                if let Some(resend) = connection.resend.as_mut()
                    && resend.take_due(now)
                {
                    self.outbox.push(Outbound::Handshake {
                        bytes: resend.bytes.clone(),
                        to: resend.to,
                    });
                }
                if connection.commitment == Commitment::Committed {
                    connection.core.poll(now)?;
                }
            }
            self.drain_connection(index, now)?;
        }
        Ok(())
    }

    /// Seals and queues everything a session wants sent, and passes the rest to the
    /// application.
    fn drain_connection(&mut self, index: usize, now: Instant) -> Result<(), TransportError> {
        let Some(connection) = self.connections.get_mut(index) else {
            return Ok(());
        };
        while let Some(action) = connection.core.poll_action() {
            match action {
                Action::Transmit { path, mut datagram } => {
                    let Some(to) = connection.address_of(path) else {
                        continue;
                    };
                    let header = *datagram.header();
                    let tos = tos_for(header.kind, header.stream.class());
                    let (buffer, filled) = datagram.sealable_mut();
                    let sealed = connection.keys.sending.seal(
                        buffer,
                        filled.saturating_sub(PacketHeader::ENCODED_LEN),
                        now,
                    )?;
                    datagram.set_sealed_len(sealed.len)?;
                    self.outbox.push(Outbound::Sealed { datagram, to, tos });
                }
                Action::ArmTimer(_) => {}
                action => self.events.push_back(DriverEvent::Session {
                    peer: connection.peer,
                    action,
                }),
            }
        }
        Ok(())
    }
}

const fn check_tag(hello: &Hello, view: &HandshakeView<'_>) -> Result<(), TransportError> {
    if hello.session_tag == view.sender_tag().get() {
        Ok(())
    } else {
        Err(TransportError::HandshakeRejected {
            reason: "the Hello names a different session tag than the handshake",
        })
    }
}

const fn failure_reason(error: &TransportError) -> &'static str {
    match error {
        TransportError::HandshakeRejected { reason } => reason,
        TransportError::UnknownPeer => "the peer is not paired with this device",
        TransportError::Replayed => "the handshake was a replay",
        _ => "the handshake failed",
    }
}
