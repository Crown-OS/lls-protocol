use llts_signaling::device::{DeviceClass, DeviceId, FeatureSet};
use llts_signaling::discovery::DiscoverySecret;

use crate::peer::{Backoff, LinkState, Session, Sightings};

/// Most devices one node trusts at once, which bounds every per-peer table.
pub(crate) const MAX_PEERS: usize = 64;

/// One paired device.
#[derive(Debug)]
pub(crate) struct Peer {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    pub(crate) class: DeviceClass,
    /// Features the user allows this device.
    pub(crate) enabled: FeatureSet,
    pub(crate) discovery: DiscoverySecret,
    pub(crate) link: LinkState,
    pub(crate) sightings: Sightings,
    pub(crate) backoff: Backoff,
    pub(crate) session: Option<Session>,
}

impl Peer {
    pub(crate) fn new(
        id: DeviceId,
        name: String,
        class: DeviceClass,
        enabled: FeatureSet,
        discovery: DiscoverySecret,
    ) -> Self {
        Self {
            id,
            name,
            class,
            enabled,
            discovery,
            link: LinkState::Idle,
            sightings: Sightings::default(),
            backoff: Backoff::default(),
            session: None,
        }
    }
}

/// The paired devices, looked up by linear scan: [`MAX_PEERS`] is small enough that a hash
/// table would cost more than it saves.
#[derive(Debug, Default)]
pub(crate) struct PeerTable {
    peers: Vec<Peer>,
}

impl PeerTable {
    pub(crate) fn get(&self, id: &DeviceId) -> Option<&Peer> {
        self.peers.iter().find(|peer| peer.id == *id)
    }

    pub(crate) fn get_mut(&mut self, id: &DeviceId) -> Option<&mut Peer> {
        self.peers.iter_mut().find(|peer| peer.id == *id)
    }

    /// Adds `peer`, or replaces the entry with its id; `false` when the table is full.
    pub(crate) fn insert(&mut self, peer: Peer) -> bool {
        if let Some(held) = self.get_mut(&peer.id) {
            *held = peer;
            return true;
        }
        if self.peers.len() >= MAX_PEERS {
            return false;
        }
        self.peers.push(peer);
        true
    }

    pub(crate) fn remove(&mut self, id: &DeviceId) -> Option<Peer> {
        let position = self.peers.iter().position(|peer| peer.id == *id)?;
        Some(self.peers.swap_remove(position))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Peer> {
        self.peers.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Peer> {
        self.peers.iter_mut()
    }
}
