use std::net::{SocketAddr, SocketAddrV6};

/// The family a UDP socket was opened with, which decides how it must be told where to send.
///
/// A dual-stack IPv6 socket reaches IPv4 peers, but only through v4-mapped addresses, and it
/// reports IPv4 sources in that spelling too. Everything above the socket uses [`canonical`]
/// addresses, so one peer has one address whichever family of socket saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SocketFamily {
    Ipv4,
    Ipv6,
}

impl SocketFamily {
    pub(crate) const fn of(local: SocketAddr) -> Self {
        match local {
            SocketAddr::V4(_) => Self::Ipv4,
            SocketAddr::V6(_) => Self::Ipv6,
        }
    }

    /// `to` spelled the way a socket of this family accepts it.
    pub(crate) fn destination(self, to: SocketAddr) -> SocketAddr {
        match (self, to) {
            (Self::Ipv6, SocketAddr::V4(ipv4)) => SocketAddr::V6(SocketAddrV6::new(
                ipv4.ip().to_ipv6_mapped(),
                ipv4.port(),
                0,
                0,
            )),
            (Self::Ipv4, to) => canonical(to),
            (Self::Ipv6, to) => to,
        }
    }
}

/// `address` with a v4-mapped IPv6 address unwrapped to plain IPv4. A real IPv6 address keeps
/// its scope and flow label, which link-local peers need.
pub fn canonical(address: SocketAddr) -> SocketAddr {
    match address {
        SocketAddr::V6(ipv6) => ipv6
            .ip()
            .to_ipv4_mapped()
            .map_or(address, |ipv4| SocketAddr::from((ipv4, ipv6.port()))),
        SocketAddr::V4(_) => address,
    }
}

/// Whether datagrams to `address` leave as IPv4, and so take IPv4 socket options such as
/// `IP_TOS` even on an IPv6 socket.
pub(crate) fn travels_over_ipv4(address: SocketAddr) -> bool {
    canonical(address).is_ipv4()
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    const PORT: u16 = 4_001;

    fn ipv4() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::new(192, 168, 1, 20), PORT))
    }

    fn mapped() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::new(192, 168, 1, 20).to_ipv6_mapped(), PORT))
    }

    fn link_local() -> SocketAddr {
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
            PORT,
            0,
            3,
        ))
    }

    #[test]
    fn a_mapped_address_canonicalises_to_ipv4() {
        assert_eq!(canonical(mapped()), ipv4());
        assert_eq!(canonical(ipv4()), ipv4());
    }

    #[test]
    fn a_real_ipv6_address_keeps_its_scope() {
        assert_eq!(canonical(link_local()), link_local());
    }

    #[test]
    fn an_ipv6_socket_reaches_ipv4_through_a_mapped_address() {
        assert_eq!(SocketFamily::Ipv6.destination(ipv4()), mapped());
        assert_eq!(SocketFamily::Ipv6.destination(link_local()), link_local());
    }

    #[test]
    fn an_ipv4_socket_unwraps_a_mapped_destination() {
        assert_eq!(SocketFamily::Ipv4.destination(mapped()), ipv4());
    }

    #[test]
    fn mapped_traffic_is_ipv4_on_the_wire() {
        assert!(travels_over_ipv4(mapped()));
        assert!(travels_over_ipv4(ipv4()));
        assert!(!travels_over_ipv4(link_local()));
    }
}
