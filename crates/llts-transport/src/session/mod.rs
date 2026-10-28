//! Tying the socket, the handshakes, the packet ciphers and the sans-I/O sessions together.

mod connection;
mod driver;
mod event;
mod framing;
mod handshaking;

pub use driver::{Driver, DriverConfig};
pub use event::DriverEvent;
