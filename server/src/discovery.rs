use std::net::IpAddr;
use tokio::net::UdpSocket;

pub struct DiscoveredDevice {
    pub ip_address: IpAddr,
}

impl DiscoveredDevice {
    pub fn discover(&self) -> Vec<DiscoveredDevice> {
        Vec::new()
    }
}
