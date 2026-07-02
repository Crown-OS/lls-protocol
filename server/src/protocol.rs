use std::net::IpAddr;
use tokio::net::UdpSocket;
use tokio_stream::Stream;

use crate::discovery::DiscoveredDevice;
use crate::packet::{ConfigurationPacket, FrameRateMode, MediaPacket, PacketType};

pub struct Connection {
    receiver_ip: IpAddr,
    socket: UdpSocket,
}

impl Connection {
    pub fn connect(&mut self, device: DiscoveredDevice) {
        let socket = UdpSocket::try_from(device.ip_address);
        self.receiver_ip = device.ip_address
    }

    pub fn get_byte_stream(&self) {}
}
