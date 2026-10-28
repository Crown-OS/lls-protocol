//! Sans-I/O protocol engine for the LLTS low-latency media link.
//!
//! Everything here is a pure state machine over `(now, event)` that lets the whole protocol be exercised against a simulated lossy network
//! in milliseconds, and lets Linux and Android run byte-identical logic.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(test)]
extern crate std;

mod error;

pub mod buffer;
pub mod channel;
pub mod clock;
pub mod congestion;
pub mod limits;
pub mod link;
pub mod receive;
pub mod repair;
pub mod send;
pub mod session;
pub mod stats;
pub mod stream;
pub mod units;

pub(crate) mod util;

pub use error::CoreError;
