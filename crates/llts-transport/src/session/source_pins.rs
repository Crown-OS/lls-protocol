//! The source addresses a peer introduced over another authenticated channel may send from.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use crate::security::StaticKey;
use crate::util::canonical;

/// Peers without a pin are admitted from anywhere, so paired devices keep roaming.
#[derive(Debug, Default)]
pub(super) struct SourcePins(HashMap<StaticKey, Vec<IpAddr>>);

impl SourcePins {
    /// Admits `peer` only from the hosts of `sources`; no sources lifts the pin.
    pub(super) fn pin(&mut self, peer: StaticKey, sources: &[SocketAddr]) {
        if sources.is_empty() {
            self.0.remove(&peer);
        } else {
            let hosts = sources
                .iter()
                .map(|source| canonical(*source).ip())
                .collect();
            self.0.insert(peer, hosts);
        }
    }

    pub(super) fn lift(&mut self, peer: &StaticKey) {
        self.0.remove(peer);
    }

    pub(super) fn admits(&self, peer: &StaticKey, from: SocketAddr) -> bool {
        self.0
            .get(peer)
            .is_none_or(|hosts| hosts.contains(&canonical(from).ip()))
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const PEER: StaticKey = StaticKey([7; StaticKey::LEN]);

    #[test]
    fn a_pinned_peer_is_admitted_only_from_its_hosts_on_any_port() {
        let mut pins = SourcePins::default();
        pins.pin(PEER, &[SocketAddr::from(([100, 64, 0, 2], 47601))]);
        assert!(pins.admits(&PEER, SocketAddr::from(([100, 64, 0, 2], 5000))));
        let mapped = SocketAddr::from((Ipv4Addr::new(100, 64, 0, 2).to_ipv6_mapped(), 47601));
        assert!(pins.admits(&PEER, mapped));
        assert!(!pins.admits(&PEER, SocketAddr::from(([100, 64, 0, 3], 47601))));
        assert!(pins.admits(
            &StaticKey([8; StaticKey::LEN]),
            SocketAddr::from(([1, 1, 1, 1], 1))
        ));
        pins.lift(&PEER);
        assert!(pins.admits(&PEER, SocketAddr::from(([100, 64, 0, 3], 47601))));
    }
}
