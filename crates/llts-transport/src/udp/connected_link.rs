use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};

use llts_core::link::LinkKind;

use super::socket_options::{self, DSCP_VIDEO};
use crate::TransportError;
use crate::link::{LinkProperties, PacketLink};
use crate::util::{SocketFamily, canonical};

/// A connected UDP socket carrying one session.
///
/// Raw UDP rather than QUIC for the LAN case: both ends are ours, so QUIC's interoperability buys
/// nothing, while its congestion controller would gate our datagrams against a second control
/// loop fighting the one in the engine.
#[derive(Debug)]
pub struct UdpLink {
    socket: UdpSocket,
    peer: SocketAddr,
    kind: LinkKind,
    max_datagram_size: usize,
}

impl UdpLink {
    /// Binds `local` and connects to `peer`, so the kernel filters foreign traffic for us.
    pub fn connect(
        local: SocketAddr,
        peer: SocketAddr,
        kind: LinkKind,
        max_datagram_size: usize,
    ) -> Result<Self, TransportError> {
        let socket = socket_options::udp_socket(local)?;
        socket.set_reuse_address(true)?;
        socket.bind(&local.into())?;
        socket_options::apply_low_latency_options(&socket, DSCP_VIDEO)?;
        socket.connect(&SocketFamily::of(local).destination(peer).into())?;

        Ok(Self {
            socket: socket.into(),
            peer: canonical(peer),
            kind,
            max_datagram_size,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.socket.local_addr()?)
    }

    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }
}

impl PacketLink for UdpLink {
    fn properties(&self) -> LinkProperties {
        LinkProperties {
            kind: self.kind,
            max_datagram_size: self.max_datagram_size,
        }
    }

    fn send(&mut self, datagram: &[u8]) -> Result<(), TransportError> {
        if datagram.len() > self.max_datagram_size {
            return Err(TransportError::DatagramTooLarge {
                len: datagram.len(),
                limit: self.max_datagram_size,
            });
        }
        match self.socket.send(datagram) {
            Ok(_) => Ok(()),
            // A full transmit queue means the link is saturated; the pacer will offer it again.
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn recv(&mut self, into: &mut [u8]) -> Result<Option<usize>, TransportError> {
        match self.socket.recv(into) {
            Ok(len) => Ok(Some(len)),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            // An ICMP port-unreachable surfaces here on Linux and says nothing durable about the
            // path, so it is not worth tearing a session down for.
            Err(error) if error.kind() == ErrorKind::ConnectionRefused => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}
