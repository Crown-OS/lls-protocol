use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::net::UdpSocket;

use crate::discovery::DiscoveredDevice;
use crate::packet::{ConfigurationPacket, FrameRateMode, MediaPacket, PacketType};

pub struct Connection {
    receiver_ip: IpAddr,
    socket: UdpSocket,
}

/// The port media packets are sent to. Discovery and signalling will eventually
/// negotiate this; until they do it is fixed.
pub const MEDIA_PORT: u16 = 5254;

impl Connection {
    /// Bind a local socket and point it at `device`.
    ///
    /// This used to read `UdpSocket::try_from(device.ip_address)`, which does not
    /// compile -- `TryFrom` is implemented for `std::net::UdpSocket`, not for an
    /// `IpAddr`. A UDP socket has to be bound and then connected.
    pub async fn connect(device: DiscoveredDevice) -> io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        socket.connect((device.ip_address, MEDIA_PORT)).await?;
        Ok(Connection {
            receiver_ip: device.ip_address,
            socket,
        })
    }

    pub fn receiver_ip(&self) -> IpAddr {
        self.receiver_ip
    }

    pub fn get_byte_stream(&self) {}
}
