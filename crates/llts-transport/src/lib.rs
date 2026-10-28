//! Socket drivers and session crypto for the LLTS low-latency media link.
//!
//! The protocol engine in `llts-core` is sans-I/O; everything that actually touches a file
//! descriptor lives here.

// `unsafe` is confined to `udp::socket_options`, which reaches for one socket option that
// `socket2` does not wrap. Everything else in this crate is safe.
#![deny(unsafe_code)]

mod error;

pub mod link;
pub mod pairing;
pub mod security;
pub mod session;
pub mod udp;

pub(crate) mod util;

pub use error::TransportError;
pub use link::{LinkProperties, PacketLink};
pub use util::address::canonical as canonical_address;
pub use util::tai64n::Tai64N;
