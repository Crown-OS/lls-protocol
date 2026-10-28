//! An in-memory mesh: every node talks to a fake driver that pairs, dials and carries messages
//! instantly and in order, so a scenario runs in microseconds and the same way every time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use llts_core::clock::Instant;
use llts_core::session::Role;
use llts_node::{
    AppCommand, AppEvent, DriverCommand, FfspLinkExport, KnownPeer, LinkEvent, LocalDevice,
    MediaRequest, Node, NodeConfig, NodeInput, NodeOutput, Now, PersistRequest, Presence,
};
use llts_signaling::device::{DeviceClass, DeviceId, Feature, FeatureSet};
use llts_signaling::discovery::Beacon;
use llts_signaling::message::{
    AudioCodec, Capabilities, CodecList, Envelope, MessageType, StreamParams, StreamRef, VideoCodec,
};
use llts_transport::canonical_address;
use llts_transport::pairing::QrOffer;
use llts_transport::security::{DeviceIdentity, PresharedKey, StaticKey};

pub(crate) const CODE: u32 = 424_242;
const UNIX_ORIGIN_MS: u64 = 1_750_000_000_000;

/// What the fake driver hands a node, owned so it can wait in the queue.
#[derive(Debug)]
enum Wire {
    Connected {
        peer: DeviceId,
        name: String,
        class: DeviceClass,
    },
    Disconnected {
        peer: DeviceId,
    },
    Message {
        peer: DeviceId,
        stream: StreamRef,
        bytes: Vec<u8>,
    },
    PairingCode {
        peer: DeviceId,
        name: String,
    },
    Paired {
        peer: DeviceId,
    },
    HandshakeFailed {
        address: SocketAddr,
    },
    QrOffer(Box<QrOffer>),
}

#[derive(Debug, Default)]
pub(crate) struct FakeSession {
    /// Streams this side opened, with the role and parameters of media streams.
    pub(crate) streams: BTreeMap<StreamRef, Option<(StreamParams, Role)>>,
}

#[derive(Debug)]
pub(crate) struct SimNode {
    pub(crate) node: Node,
    pub(crate) identity: DeviceIdentity,
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    pub(crate) local: LocalDevice,
    pub(crate) address: SocketAddr,
    pub(crate) online: bool,
    pub(crate) trusted: BTreeMap<DeviceId, String>,
    pub(crate) stored_features: BTreeMap<DeviceId, FeatureSet>,
    window: Option<Option<PresharedKey>>,
    awaiting_confirmation: BTreeSet<DeviceId>,
    pub(crate) sessions: BTreeMap<DeviceId, FakeSession>,
    pub(crate) advertised: Vec<Beacon>,
    pub(crate) events: Vec<AppEvent>,
    pub(crate) media: Vec<MediaRequest>,
    pub(crate) exports: Vec<FfspLinkExport>,
    pub(crate) dials: Vec<(Duration, DeviceId)>,
    pub(crate) sent: Vec<(DeviceId, MessageType)>,
}

impl SimNode {
    pub(crate) fn pairing_offer(&self) -> Option<&str> {
        self.events.iter().rev().find_map(|event| match event {
            AppEvent::PairingOffer { uri, .. } => Some(uri.as_str()),
            _ => None,
        })
    }

    pub(crate) fn is_connected_to(&self, peer: DeviceId) -> bool {
        self.node
            .device(&peer)
            .is_some_and(|device| device.connected)
    }

    pub(crate) fn sent_count(&self, peer: DeviceId, kind: MessageType) -> usize {
        self.sent
            .iter()
            .filter(|(to, sent)| *to == peer && *sent == kind)
            .count()
    }
}

pub(crate) fn capabilities(video: &[VideoCodec], class: DeviceClass) -> Capabilities {
    Capabilities {
        video_codecs: CodecList::from_slice(video).unwrap_or_default(),
        audio_codecs: CodecList::from_slice(&[AudioCodec::Opus]).unwrap_or_default(),
        features: Feature::ALL.into_iter().collect(),
        device_class: class,
    }
}

#[derive(Debug)]
pub(crate) struct Sim {
    pub(crate) nodes: Vec<SimNode>,
    now_us: u64,
    queue: VecDeque<(usize, Wire)>,
    /// Messages of this type are parked in `held` instead of delivered.
    holding: Option<MessageType>,
    held: Vec<(usize, Wire)>,
    /// Every linked beacon a node advertises is seen at once at the other nodes' addresses, as
    /// `crownconnect-linux` does for peers configured by address.
    pub(crate) static_peers: bool,
    pub(crate) config: NodeConfig,
}

impl Sim {
    pub(crate) fn new(devices: &[(&str, Capabilities)]) -> Self {
        let mut sim = Self {
            nodes: Vec::new(),
            now_us: 1_000_000,
            queue: VecDeque::new(),
            holding: None,
            held: Vec::new(),
            static_peers: false,
            config: NodeConfig::default(),
        };
        for (index, (name, capabilities)) in devices.iter().enumerate() {
            let identity = DeviceIdentity::generate().expect("entropy");
            let local = LocalDevice {
                name: (*name).to_owned(),
                model: "sim".to_owned(),
                capabilities: capabilities.clone(),
            };
            let port = u16::try_from(40_000 + index).unwrap_or(u16::MAX);
            let address = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), port));
            let node = Node::new(&identity, local.clone(), [], sim.config, sim.now())
                .expect("a node builds");
            sim.nodes.push(SimNode {
                id: DeviceId(identity.public().0),
                node,
                identity,
                name: (*name).to_owned(),
                local,
                address,
                online: true,
                trusted: BTreeMap::new(),
                stored_features: BTreeMap::new(),
                window: None,
                awaiting_confirmation: BTreeSet::new(),
                sessions: BTreeMap::new(),
                advertised: Vec::new(),
                events: Vec::new(),
                media: Vec::new(),
                exports: Vec::new(),
                dials: Vec::new(),
                sent: Vec::new(),
            });
        }
        for index in 0..sim.nodes.len() {
            let address = sim.nodes[index].address;
            sim.input(
                index,
                NodeInput::NetworkChanged {
                    addresses: &[address],
                    rebind: None,
                },
            );
        }
        sim.settle();
        sim
    }

    pub(crate) const fn now(&self) -> Now {
        Now::new(
            Instant::from_micros(self.now_us),
            UNIX_ORIGIN_MS + self.now_us / 1_000,
        )
    }

    pub(crate) const fn elapsed(&self) -> Duration {
        Duration::from_micros(self.now_us)
    }

    pub(crate) fn id(&self, index: usize) -> DeviceId {
        self.nodes[index].id
    }

    pub(crate) fn input(&mut self, index: usize, input: NodeInput<'_>) {
        let now = self.now();
        self.nodes[index].node.handle(now, input);
        self.drain(index);
    }

    pub(crate) fn app(&mut self, index: usize, command: AppCommand<'_>) {
        self.input(index, NodeInput::App(command));
        self.settle();
    }

    /// Runs until no node has output and no message is in flight.
    pub(crate) fn settle(&mut self) {
        loop {
            for index in 0..self.nodes.len() {
                self.drain(index);
            }
            let Some((to, wire)) = self.queue.pop_front() else {
                return;
            };
            if !self.nodes[to].online {
                continue;
            }
            self.deliver(to, &wire);
        }
    }

    /// Moves the clock forward by `span`, firing every node timer on the way.
    pub(crate) fn advance(&mut self, span: Duration) {
        let target = self.now_us + u64::try_from(span.as_micros()).unwrap_or(u64::MAX);
        loop {
            self.settle();
            let next = self
                .nodes
                .iter()
                .filter(|node| node.online)
                .filter_map(|node| node.node.next_timeout())
                .map(Instant::as_micros)
                .min();
            match next {
                Some(at) if at <= target => {
                    self.now_us = self.now_us.max(at);
                    for index in 0..self.nodes.len() {
                        let due = self.nodes[index]
                            .node
                            .next_timeout()
                            .is_some_and(|at| at.as_micros() <= self.now_us);
                        if self.nodes[index].online && due {
                            self.input(index, NodeInput::TimerExpired);
                        }
                    }
                }
                _ => break,
            }
        }
        self.now_us = target;
        self.settle();
    }

    /// `observer` sees every beacon `advertiser` is sending, at the advertiser's address.
    pub(crate) fn show(&mut self, advertiser: usize, observer: usize) {
        let address = self.nodes[advertiser].address;
        let beacons = self.nodes[advertiser].advertised.clone();
        for beacon in beacons {
            self.input(
                observer,
                NodeInput::Presence(Presence::Seen {
                    beacon,
                    addresses: &[address],
                }),
            );
        }
        self.settle();
    }

    pub(crate) fn show_all(&mut self) {
        for advertiser in 0..self.nodes.len() {
            for observer in 0..self.nodes.len() {
                if advertiser != observer && self.nodes[advertiser].online {
                    self.show(advertiser, observer);
                }
            }
        }
    }

    /// Pairs two devices by QR: `shower` opens a window, `scanner` scans its code.
    pub(crate) fn pair_by_qr(&mut self, shower: usize, scanner: usize) {
        self.app(shower, AppCommand::BeginPairing { duration: None });
        let uri = self.nodes[shower]
            .pairing_offer()
            .expect("a QR offer is shown")
            .to_owned();
        self.app(scanner, AppCommand::PairWithQr { uri: &uri });
    }

    /// Parks every message of type `kind` sent from now on until [`Sim::release`].
    pub(crate) const fn hold(&mut self, kind: MessageType) {
        self.holding = Some(kind);
    }

    /// Delivers every parked message in the order it was sent, and stops holding.
    pub(crate) fn release(&mut self) {
        self.holding = None;
        self.queue.extend(self.held.drain(..));
        self.settle();
    }

    /// The process stops: its sessions die, and nothing reaches it until [`Sim::restart`].
    pub(crate) fn stop(&mut self, index: usize) {
        self.nodes[index].online = false;
        let id = self.id(index);
        let peers: Vec<DeviceId> = self.nodes[index].sessions.keys().copied().collect();
        self.nodes[index].sessions.clear();
        for peer in peers {
            if let Some(other) = self.index_of(peer) {
                self.nodes[other].sessions.remove(&id);
                self.queue
                    .push_back((other, Wire::Disconnected { peer: id }));
            }
        }
        self.settle();
    }

    /// The process starts again from what it persisted: trust store and feature settings.
    pub(crate) fn restart(&mut self, index: usize) {
        if self.nodes[index].online {
            self.stop(index);
        }
        let now = self.now();
        let sim_node = &mut self.nodes[index];
        let known: Vec<KnownPeer> = sim_node
            .trusted
            .iter()
            .map(|(id, name)| KnownPeer {
                id: *id,
                name: name.clone(),
                class: DeviceClass::Computer,
                enabled: sim_node
                    .stored_features
                    .get(id)
                    .copied()
                    .unwrap_or_default(),
            })
            .collect();
        sim_node.node = Node::new(
            &sim_node.identity,
            sim_node.local.clone(),
            known,
            self.config,
            now,
        )
        .expect("a node rebuilds");
        sim_node.online = true;
        sim_node.window = None;
        sim_node.awaiting_confirmation.clear();
        let address = sim_node.address;
        self.input(
            index,
            NodeInput::NetworkChanged {
                addresses: &[address],
                rebind: None,
            },
        );
        self.settle();
    }

    fn index_of(&self, id: DeviceId) -> Option<usize> {
        self.nodes.iter().position(|node| node.id == id)
    }

    /// Like the real driver, reaches a v4-mapped address at its IPv4 peer.
    fn index_at(&self, address: SocketAddr) -> Option<usize> {
        let address = canonical_address(address);
        self.nodes
            .iter()
            .position(|node| node.address == address && node.online)
    }

    /// Like the real driver, reports a failed handshake at the canonical address.
    fn handshake_failed(&mut self, to: usize, address: SocketAddr) {
        let address = canonical_address(address);
        self.queue
            .push_back((to, Wire::HandshakeFailed { address }));
    }

    fn deliver(&mut self, to: usize, wire: &Wire) {
        let event = match wire {
            Wire::Connected { peer, name, class } => LinkEvent::Connected {
                peer: *peer,
                name,
                class: *class,
            },
            Wire::Disconnected { peer } => LinkEvent::Disconnected { peer: *peer },
            Wire::Message {
                peer,
                stream,
                bytes,
            } => LinkEvent::Message {
                peer: *peer,
                stream: *stream,
                bytes,
            },
            Wire::PairingCode { peer, name } => LinkEvent::PairingCode {
                peer: *peer,
                name,
                code: CODE,
            },
            Wire::Paired { peer } => LinkEvent::Paired { peer: *peer },
            Wire::HandshakeFailed { address } => LinkEvent::HandshakeFailed { address: *address },
            Wire::QrOffer(offer) => LinkEvent::QrOfferReady { offer },
        };
        self.input(to, NodeInput::Link(event));
    }

    fn drain(&mut self, index: usize) {
        while let Some(output) = self.nodes[index].node.poll_output() {
            match output {
                NodeOutput::Driver(command) => self.execute(index, command),
                NodeOutput::App(event) => self.nodes[index].events.push(event),
                NodeOutput::Media(request) => self.nodes[index].media.push(request),
                NodeOutput::Persist(PersistRequest::PeerFeatures { peer, enabled }) => {
                    self.nodes[index].stored_features.insert(peer, enabled);
                }
                NodeOutput::Persist(PersistRequest::ForgetPeer { peer }) => {
                    self.nodes[index].stored_features.remove(&peer);
                }
                NodeOutput::ExportFfspLink(export) => self.nodes[index].exports.push(export),
                NodeOutput::Advertise(beacons) => {
                    if self.static_peers {
                        self.see_statically(index, &beacons);
                    }
                    self.nodes[index].advertised = beacons;
                }
            }
        }
    }

    fn see_statically(&mut self, index: usize, beacons: &[Beacon]) {
        let others: Vec<SocketAddr> = (0..self.nodes.len())
            .filter(|&other| other != index)
            .map(|other| self.nodes[other].address)
            .collect();
        for beacon in beacons
            .iter()
            .filter(|beacon| matches!(beacon, Beacon::Linked(_)))
        {
            self.input(
                index,
                NodeInput::Presence(Presence::Seen {
                    beacon: *beacon,
                    addresses: &others,
                }),
            );
        }
    }

    /// Opens a session between two devices, as a completed handshake does.
    fn connect(&mut self, left: usize, right: usize) {
        let (left_id, right_id) = (self.id(left), self.id(right));
        self.nodes[left]
            .sessions
            .insert(right_id, FakeSession::default());
        self.nodes[right]
            .sessions
            .insert(left_id, FakeSession::default());
    }

    fn connected_event(&self, about: usize) -> Wire {
        let node = &self.nodes[about];
        Wire::Connected {
            peer: node.id,
            name: node.name.clone(),
            class: node.local.capabilities.device_class,
        }
    }

    fn trust(&mut self, truster: usize, trusted: usize) {
        let (id, name) = (self.id(trusted), self.nodes[trusted].name.clone());
        self.nodes[truster].trusted.insert(id, name);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per driver call keeps the fake readable side by side with the real one"
    )]
    fn execute(&mut self, from: usize, command: DriverCommand) {
        let from_id = self.id(from);
        match command {
            DriverCommand::Dial { peer, address } => {
                let elapsed = self.elapsed();
                self.nodes[from].dials.push((elapsed, peer));
                let target = self.index_at(address).filter(|&target| {
                    self.nodes[target].id == peer
                        && self.nodes[from].trusted.contains_key(&peer)
                        && self.nodes[target].trusted.contains_key(&from_id)
                });
                match target {
                    Some(target) => {
                        self.connect(from, target);
                        let (to_from, to_target) =
                            (self.connected_event(target), self.connected_event(from));
                        self.queue.push_back((from, to_from));
                        self.queue.push_back((target, to_target));
                    }
                    None => self.handshake_failed(from, address),
                }
            }
            DriverCommand::OfferQrPairing {
                addresses,
                duration,
            } => {
                let psk = PresharedKey([from.to_le_bytes()[0].wrapping_add(9); 32]);
                let expires = self.now().unix_seconds() + duration.as_secs();
                let offer = QrOffer::new(
                    StaticKey(from_id.0),
                    &psk,
                    addresses,
                    expires,
                    self.nodes[from].name.clone(),
                );
                self.nodes[from].window = Some(Some(psk));
                self.queue.push_back((from, Wire::QrOffer(Box::new(offer))));
            }
            DriverCommand::ClosePairing => self.nodes[from].window = None,
            DriverCommand::PairNearby { address } => {
                match self
                    .index_at(address)
                    .filter(|&target| self.nodes[target].window.is_some())
                {
                    Some(target) => {
                        self.connect(from, target);
                        let target_id = self.id(target);
                        self.nodes[from].awaiting_confirmation.insert(target_id);
                        self.nodes[target].awaiting_confirmation.insert(from_id);
                        let (from_name, target_name) = (
                            self.nodes[from].name.clone(),
                            self.nodes[target].name.clone(),
                        );
                        self.queue.push_back((
                            from,
                            Wire::PairingCode {
                                peer: target_id,
                                name: target_name,
                            },
                        ));
                        self.queue.push_back((
                            target,
                            Wire::PairingCode {
                                peer: from_id,
                                name: from_name,
                            },
                        ));
                    }
                    None => self.handshake_failed(from, address),
                }
            }
            DriverCommand::PairWithOffer { offer } => {
                let address = offer.addresses[0];
                let target = self.index_at(address).filter(|&target| {
                    self.nodes[target].id.0 == offer.static_key.0
                        && self.nodes[target].window.as_ref().and_then(Option::as_ref)
                            == Some(&offer.psk())
                });
                match target {
                    Some(target) => {
                        self.trust(from, target);
                        self.trust(target, from);
                        self.connect(from, target);
                        let target_id = self.id(target);
                        self.queue
                            .push_back((from, Wire::Paired { peer: target_id }));
                        self.queue
                            .push_back((target, Wire::Paired { peer: from_id }));
                        let (to_from, to_target) =
                            (self.connected_event(target), self.connected_event(from));
                        self.queue.push_back((from, to_from));
                        self.queue.push_back((target, to_target));
                    }
                    None => self.handshake_failed(from, address),
                }
            }
            DriverCommand::ConfirmPairing { peer } => {
                if self.nodes[from].awaiting_confirmation.remove(&peer)
                    && let Some(other) = self.index_of(peer)
                {
                    self.trust(from, other);
                    self.queue.push_back((from, Wire::Paired { peer }));
                    let connected = self.connected_event(other);
                    self.queue.push_back((from, connected));
                }
            }
            DriverCommand::RejectPairing { peer } => {
                self.nodes[from].awaiting_confirmation.remove(&peer);
                self.nodes[from].sessions.remove(&peer);
            }
            DriverCommand::Send {
                peer,
                stream,
                message,
                ..
            } => {
                let envelope = Envelope::parse(&message).expect("an envelope");
                assert!(
                    envelope.msg_type >= llts_core::session::SessionControl::RESERVED_BELOW,
                    "the core refuses application messages below 0x0100"
                );
                let kind = envelope.message_type().expect("a registered type");
                self.nodes[from].sent.push((peer, kind));
                let Some(target) = self.index_of(peer) else {
                    return;
                };
                let open = |node: &SimNode, peer: DeviceId| {
                    node.sessions.get(&peer).is_some_and(|session| {
                        stream.class == llts_signaling::message::MuxClass::Control
                            || session.streams.contains_key(&stream)
                    })
                };
                assert!(
                    open(&self.nodes[from], peer),
                    "{stream:?} was never opened by the sender"
                );
                if self.nodes[target].online && self.nodes[target].sessions.contains_key(&from_id) {
                    let delivery = (
                        target,
                        Wire::Message {
                            peer: from_id,
                            stream,
                            bytes: message,
                        },
                    );
                    if self.holding == Some(kind) {
                        self.held.push(delivery);
                    } else {
                        self.queue.push_back(delivery);
                    }
                }
            }
            DriverCommand::OpenReliable { peer, stream } => {
                if let Some(session) = self.nodes[from].sessions.get_mut(&peer) {
                    session.streams.insert(stream, None);
                }
            }
            DriverCommand::OpenMedia {
                peer,
                stream,
                params,
                role,
            } => {
                if let Some(session) = self.nodes[from].sessions.get_mut(&peer) {
                    session.streams.insert(stream, Some((params, role)));
                }
            }
            DriverCommand::CloseStream { peer, stream } => {
                if let Some(session) = self.nodes[from].sessions.get_mut(&peer) {
                    session.streams.remove(&stream);
                }
            }
            DriverCommand::Forget { peer } => {
                self.nodes[from].trusted.remove(&peer);
                if self.nodes[from].sessions.remove(&peer).is_some()
                    && let Some(other) = self.index_of(peer)
                {
                    self.nodes[other].sessions.remove(&from_id);
                    self.queue
                        .push_back((other, Wire::Disconnected { peer: from_id }));
                }
            }
            DriverCommand::Rebind { .. } => {}
        }
    }
}
