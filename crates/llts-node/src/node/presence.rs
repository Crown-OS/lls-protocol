//! Sightings from the discovery planes: linked peers to dial and pairing windows to show.

use std::net::SocketAddr;

use llts_signaling::discovery::{Advertisement, Beacon, PairingBeacon, PairingFlags};

use crate::Node;
use crate::input::Presence;
use crate::output::AppEvent;
use crate::peer::LinkState;

impl Node {
    pub(super) fn on_presence(&mut self, presence: Presence<'_>) {
        match presence {
            Presence::Seen {
                beacon: Beacon::Linked(advertisement),
                addresses,
            } => self.on_linked_seen(&advertisement, addresses),
            Presence::Lost {
                beacon: Beacon::Linked(advertisement),
            } => {
                if let Some(peer) = self.discovery.resolve(&advertisement, &self.peers)
                    && let Some(entry) = self.peers.get_mut(&peer)
                {
                    entry.sightings.lost();
                }
            }
            Presence::Seen {
                beacon: Beacon::Pairing(beacon),
                addresses,
            } => self.on_pairable_seen(beacon, addresses.first().copied()),
            Presence::Lost {
                beacon: Beacon::Pairing(beacon),
            } => self.pairing.forget_pairable(beacon.nonce),
        }
    }

    /// Autoconnect: a paired device seen anywhere is dialled, unless a dial, a session or a
    /// backoff for it is already running.
    fn on_linked_seen(&mut self, advertisement: &Advertisement, addresses: &[SocketAddr]) {
        let Some(peer) = self.discovery.resolve(advertisement, &self.peers) else {
            return;
        };
        let Some(entry) = self.peers.get_mut(&peer) else {
            return;
        };
        entry.sightings.seen(addresses);
        if entry.link == LinkState::Idle {
            self.dial(peer);
        }
    }

    /// Announces a pairing window nearby. When this device has its own window open too, the
    /// device whose window nonce is lower starts nearby pairing, so exactly one side dials.
    fn on_pairable_seen(&mut self, beacon: PairingBeacon, address: Option<SocketAddr>) {
        if !self.pairing.note_pairable(beacon.nonce) {
            return;
        }
        self.emit(AppEvent::PairableDeviceSeen {
            class: beacon.device_class,
            flags: beacon.flags,
            address,
        });
        let Some(window) = self.pairing.window else {
            return;
        };
        if let Some(address) = address
            && beacon.flags.contains(PairingFlags::NEARBY_CODE)
            && window.nonce.0 < beacon.nonce.0
            && self.pairing.attempt.is_none()
        {
            self.pair_nearby(address);
        }
    }
}
