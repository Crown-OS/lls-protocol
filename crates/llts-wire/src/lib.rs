//! Byte layout for the LLTS low-latency media protocol.
//!
//! This crate is `no_std`, allocation-free and I/O-free. Every parser is a total function over
//! `&[u8]` that either yields a borrowed view or a [`WireError`], which makes the whole surface
//! cheap to fuzz and impossible to misuse from a hot loop.

#![no_std]
#![forbid(unsafe_code)]

mod error;
mod frame_id;
mod sequence;
mod session_tag;
mod stream_class;
mod stream_id;
mod timestamp;
mod version;

pub mod packet;
pub(crate) mod util;

pub use error::WireError;
pub use frame_id::FrameId;
pub use sequence::SequenceNumber;
pub use session_tag::SessionTag;
pub use stream_class::StreamClass;
pub use stream_id::StreamId;
pub use timestamp::{ClockRate, MediaTimestamp, MonotonicMicros};
pub use version::ProtocolVersion;

#[cfg(test)]
extern crate std;
