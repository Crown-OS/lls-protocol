//! The orchestrator itself, split by concern: links and autoconnect, pairing, discovery, state
//! sync, features and commands each extend [`Node`] in their own file.

mod commands;
mod features;
mod link;
mod pairing;
mod presence;
mod sync;
mod view;

use std::collections::VecDeque;
use std::net::SocketAddr;

use llts_core::channel::Delivery;
use llts_core::clock::Instant;
use llts_signaling::device::{DeviceClass, DeviceId, FeatureSet};
use llts_signaling::message::{Capabilities, Signal, StreamRef};
use llts_signaling::state::{DeviceInfo, HybridClock, Incarnation, StateCache, StatePublisher};
use llts_transport::security::DeviceIdentity;

pub use view::DeviceView;

use crate::config::{KnownPeer, LocalDevice, NodeConfig};
use crate::input::NodeInput;
use crate::link_secret::LinkSecret;
use crate::output::{AppEvent, DriverCommand, NodeOutput};
use crate::pairing::Pairing;
use crate::peer::{Peer, PeerTable};
use crate::presence::Discovery;
use crate::util::device::device_id;
use crate::util::jitter::Jitter;
use crate::{NodeError, Now, sync as documents};

/// Addresses this device offers in a QR pairing offer, the most a [`QrOffer`] carries.
///
/// [`QrOffer`]: llts_transport::pairing::QrOffer
const MAX_LOCAL_ADDRESSES: usize = 4;

/// One device's view of its CrownConnect mesh; see the [crate documentation](crate).
#[derive(Debug)]
pub struct Node {
    identity: DeviceIdentity,
    id: DeviceId,
    local: LocalDevice,
    config: NodeConfig,
    peers: PeerTable,
    publisher: StatePublisher,
    cache: StateCache,
    clock: HybridClock,
    pairing: Pairing,
    discovery: Discovery,
    jitter: Jitter,
    addresses: heapless::Vec<SocketAddr, MAX_LOCAL_ADDRESSES>,
    flush_at: Option<Instant>,
    now: Now,
    outputs: VecDeque<NodeOutput>,
}

impl Node {
    /// Builds a node for `identity`, which trusts `known` from an earlier run.
    ///
    /// Publishes this device's own [`DeviceInfo`] and queues the first beacons to advertise.
    ///
    /// # Errors
    ///
    /// Fails when the operating system has no entropy for the state incarnation, when `known`
    /// holds more peers than a node tracks, or when the device name cannot be encoded.
    pub fn new(
        identity: &DeviceIdentity,
        local: LocalDevice,
        known: impl IntoIterator<Item = KnownPeer>,
        config: NodeConfig,
        now: Now,
    ) -> Result<Self, NodeError> {
        let mut peers = PeerTable::default();
        for known in known {
            let discovery = LinkSecret::derive(identity, &known.id).discovery_secret();
            let peer = Peer::new(known.id, known.name, known.class, known.enabled, discovery);
            if !peers.insert(peer) {
                return Err(NodeError::TooManyPeers {
                    limit: crate::peer::MAX_PEERS,
                });
            }
        }
        let mut publisher = StatePublisher::new(Incarnation::random()?);
        let _ = publisher.publish(&DeviceInfo {
            name: &local.name,
            model: &local.model,
            class: local.capabilities.device_class,
        })?;
        let discovery = Discovery::new(&peers, now.epoch());
        let mut node = Self {
            id: device_id(*identity.public()),
            identity: identity.clone(),
            local,
            config,
            peers,
            publisher,
            cache: StateCache::default(),
            clock: HybridClock::default(),
            pairing: Pairing::default(),
            discovery,
            jitter: Jitter::new(getrandom::u64()?),
            addresses: heapless::Vec::new(),
            flush_at: None,
            now,
            outputs: VecDeque::new(),
        };
        node.refresh_advertising();
        Ok(node)
    }

    pub const fn id(&self) -> DeviceId {
        self.id
    }

    /// Reacts to one input, then runs every timer that is due at `now`.
    pub fn handle(&mut self, now: Now, input: NodeInput<'_>) {
        self.now = now;
        let result = match input {
            NodeInput::Presence(presence) => {
                self.on_presence(presence);
                Ok(())
            }
            NodeInput::Link(event) => self.on_link(event),
            NodeInput::LocalState { topic, payload } => self.publish_encoded(topic, payload),
            NodeInput::App(command) => self.on_app(command),
            NodeInput::NetworkChanged { addresses, rebind } => {
                self.on_network_changed(addresses, rebind);
                Ok(())
            }
            NodeInput::TimerExpired => Ok(()),
        };
        if let Err(error) = result {
            self.emit(AppEvent::Error(error));
        }
        self.service();
    }

    pub fn poll_output(&mut self) -> Option<NodeOutput> {
        self.outputs.pop_front()
    }

    /// The earliest moment [`NodeInput::TimerExpired`] has work to do.
    pub fn next_timeout(&self) -> Option<Instant> {
        let links = self.peers.iter().filter_map(|peer| peer.link.deadline());
        let deferred_starts = self
            .peers
            .iter()
            .filter_map(|peer| peer.session.as_ref()?.pending_starts.next_deadline());
        links
            .chain(deferred_starts)
            .chain(self.pairing.next_deadline())
            .chain(self.flush_at)
            .chain(Some(self.now.next_epoch_at()))
            .min()
    }

    fn service(&mut self) {
        if self.now.epoch() != self.discovery.epoch() {
            self.discovery.rebuild(&self.peers, self.now.epoch());
            self.refresh_advertising();
        }
        self.service_pairing();
        self.service_links();
        self.service_deferred_starts();
        if self
            .flush_at
            .is_some_and(|at| !at.is_after(self.now.instant))
        {
            self.flush_state();
        }
    }

    fn on_app(&mut self, command: crate::AppCommand<'_>) -> Result<(), NodeError> {
        use crate::AppCommand;
        match command {
            AppCommand::BeginPairing { duration } => self.begin_pairing(duration),
            AppCommand::CancelPairing => {
                self.cancel_pairing();
                Ok(())
            }
            AppCommand::PairWithQr { uri } => self.pair_with_qr(uri),
            AppCommand::PairNearby { address } => {
                self.pair_nearby(address);
                Ok(())
            }
            AppCommand::ConfirmPairing { peer, accept } => self.confirm_pairing(peer, accept),
            AppCommand::Forget { peer } => self.forget(peer),
            AppCommand::SetFeature {
                peer,
                feature,
                enabled,
            } => self.set_feature(peer, feature, enabled),
            AppCommand::StartFeature { peer, request } => self.start_feature(peer, request),
            AppCommand::StopFeature { peer, feature } => self.stop_feature(peer, feature),
            AppCommand::Send { peer, command } => self.send_command(peer, command),
        }
    }

    fn on_network_changed(&mut self, addresses: &[SocketAddr], rebind: Option<SocketAddr>) {
        self.addresses = addresses
            .iter()
            .copied()
            .take(MAX_LOCAL_ADDRESSES)
            .collect();
        if let Some(local) = rebind {
            self.command(DriverCommand::Rebind { local });
        }
        self.retry_now();
    }

    /// What this device announces to `peer`: everything it implements that the user allows it.
    fn capabilities_for(&self, enabled: FeatureSet) -> Capabilities {
        Capabilities {
            features: self.local.capabilities.features.shared_with(enabled),
            ..self.local.capabilities.clone()
        }
    }

    /// Features both ends implement and this device allows `peer`, for a live session.
    fn shared_features(&self, peer: &Peer) -> FeatureSet {
        peer.session.as_ref().map_or(FeatureSet::EMPTY, |session| {
            self.local
                .capabilities
                .features
                .shared_with(peer.enabled)
                .shared_with(session.peer_features())
        })
    }

    fn send(
        &mut self,
        peer: DeviceId,
        stream: StreamRef,
        signal: &Signal<'_>,
    ) -> Result<(), NodeError> {
        let message = documents::encode(signal)?;
        self.command(DriverCommand::Send {
            peer,
            stream,
            message,
            delivery: Delivery::Ordered,
        });
        Ok(())
    }

    fn command(&mut self, command: DriverCommand) {
        self.outputs.push_back(NodeOutput::Driver(command));
    }

    fn emit(&mut self, event: AppEvent) {
        self.outputs.push_back(NodeOutput::App(event));
    }

    fn push(&mut self, output: NodeOutput) {
        self.outputs.push_back(output);
    }

    fn refresh_advertising(&mut self) {
        let class = self.local.capabilities.device_class;
        if let Some(beacons) =
            self.discovery
                .refresh(&self.peers, class, self.pairing.window.as_ref())
        {
            self.push(NodeOutput::Advertise(beacons));
        }
    }

    fn new_peer(&self, id: DeviceId) -> Peer {
        let discovery = LinkSecret::derive(&self.identity, &id).discovery_secret();
        Peer::new(
            id,
            String::new(),
            DeviceClass::Computer,
            self.config.default_features,
            discovery,
        )
    }
}
