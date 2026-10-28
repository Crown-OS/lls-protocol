use std::net::SocketAddr;

use llts_transport::canonical_address;

/// Most addresses remembered for one peer; a device rarely has more than one per network.
pub(crate) const MAX_ADDRESSES: usize = 4;

/// Where a peer was last seen, and whether any discovery plane still sees it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sightings {
    addresses: heapless::Vec<SocketAddr, MAX_ADDRESSES>,
    present: bool,
    next: usize,
}

impl Sightings {
    /// Records a sighting; the newest addresses go first and push out the oldest. Addresses are
    /// kept canonical, as the driver reports them, so a v4-mapped sighting matches its IPv4 peer.
    pub(crate) fn seen(&mut self, addresses: &[SocketAddr]) {
        self.present = true;
        for address in addresses
            .iter()
            .rev()
            .map(|&address| canonical_address(address))
        {
            if self.addresses.first() == Some(&address) {
                continue;
            }
            self.addresses.retain(|held| *held != address);
            if self.addresses.is_full() {
                self.addresses.pop();
            }
            let _ = self.addresses.insert(0, address);
            self.next = 0;
        }
    }

    pub(crate) const fn lost(&mut self) {
        self.present = false;
    }

    pub(crate) const fn is_present(&self) -> bool {
        self.present
    }

    /// The address to dial next, rotating through every known one across attempts.
    pub(crate) fn next_address(&mut self) -> Option<SocketAddr> {
        let len = self.addresses.len();
        let address = self.addresses.get(self.next % len.max(1)).copied()?;
        self.next = (self.next + 1) % len;
        Some(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(port: u16) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, 1], port))
    }

    #[test]
    fn dialling_rotates_through_known_addresses_newest_first() {
        let mut sightings = Sightings::default();
        sightings.seen(&[address(1), address(2)]);
        assert_eq!(sightings.next_address(), Some(address(1)));
        assert_eq!(sightings.next_address(), Some(address(2)));
        assert_eq!(sightings.next_address(), Some(address(1)));
    }

    #[test]
    fn the_address_list_stays_bounded() {
        let mut sightings = Sightings::default();
        for port in 0..10 {
            sightings.seen(&[address(port)]);
        }
        let dialled: Vec<_> = (0..MAX_ADDRESSES)
            .filter_map(|_| sightings.next_address())
            .collect();
        assert_eq!(dialled, [address(9), address(8), address(7), address(6)]);
        assert_eq!(Sightings::default().next_address(), None);
    }

    #[test]
    fn a_v4_mapped_sighting_is_kept_as_its_ipv4_address() {
        let mapped = SocketAddr::from((std::net::Ipv4Addr::new(10, 0, 0, 1).to_ipv6_mapped(), 7));
        let mut sightings = Sightings::default();
        sightings.seen(&[mapped, address(7)]);
        assert_eq!(sightings.next_address(), Some(address(7)));
        assert_eq!(sightings.next_address(), Some(address(7)));
    }
}
