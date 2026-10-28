//! Resolving the beacons other devices advertise, and choosing the ones this device does.

use llts_signaling::device::{DeviceClass, DeviceId};
use llts_signaling::discovery::{
    Advertisement, Beacon, EphemeralId, Epoch, PairingBeacon, PairingFlags, PeerResolver,
};

use crate::pairing::Window;
use crate::peer::PeerTable;

#[derive(Debug)]
pub(crate) struct Discovery {
    resolver: PeerResolver,
    advertised: Vec<Beacon>,
}

impl Discovery {
    pub(crate) fn new(peers: &PeerTable, epoch: Epoch) -> Self {
        Self {
            resolver: build(peers, epoch, PeerResolver::DEFAULT_TOLERANCE),
            advertised: Vec::new(),
        }
    }

    pub(crate) const fn epoch(&self) -> Epoch {
        self.resolver.epoch()
    }

    /// Recomputes every candidate identifier, after the peer set or the epoch changed.
    pub(crate) fn rebuild(&mut self, peers: &PeerTable, epoch: Epoch) {
        self.resolver = build(peers, epoch, PeerResolver::DEFAULT_TOLERANCE);
    }

    /// The paired device behind an advertisement. A miss whose epoch byte lies outside the
    /// window suggests a skewed clock on one side, so the window widens once before giving up.
    pub(crate) fn resolve(
        &mut self,
        advertisement: &Advertisement,
        peers: &PeerTable,
    ) -> Option<DeviceId> {
        if let Some(peer) = self.resolver.resolve(advertisement) {
            return Some(peer);
        }
        if self.resolver.tolerance() == PeerResolver::SKEWED_TOLERANCE
            || self.resolver.covers_epoch_byte(advertisement.epoch_low)
        {
            return None;
        }
        self.resolver = build(peers, self.epoch(), PeerResolver::SKEWED_TOLERANCE);
        self.resolver.resolve(advertisement)
    }

    /// Stores the beacons to advertise and returns them if they differ from the last set.
    pub(crate) fn refresh(
        &mut self,
        peers: &PeerTable,
        class: DeviceClass,
        window: Option<&Window>,
    ) -> Option<Vec<Beacon>> {
        let epoch = self.epoch();
        let linked = peers.iter().map(|peer| {
            Beacon::Linked(Advertisement::new(
                class,
                EphemeralId::derive(&peer.discovery, epoch),
                epoch,
            ))
        });
        let pairing = window.map(|window| {
            Beacon::Pairing(PairingBeacon {
                device_class: class,
                flags: PairingFlags::NEARBY_CODE | PairingFlags::QR,
                nonce: window.nonce,
            })
        });
        let beacons: Vec<Beacon> = pairing.into_iter().chain(linked).collect();
        if beacons == self.advertised {
            return None;
        }
        self.advertised.clone_from(&beacons);
        Some(beacons)
    }
}

fn build(peers: &PeerTable, epoch: Epoch, tolerance: u8) -> PeerResolver {
    PeerResolver::build(
        peers.iter().map(|peer| (peer.id, &peer.discovery)),
        epoch,
        tolerance,
    )
}
