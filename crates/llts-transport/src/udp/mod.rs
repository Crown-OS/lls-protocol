//! Plain UDP, which is the fastest thing available when both ends are ours.

pub mod batch;
mod connected_link;
pub mod dscp;
mod socket;
pub mod socket_options;

pub use batch::Outgoing;
pub use connected_link::UdpLink;
pub use socket::SessionSocket;
