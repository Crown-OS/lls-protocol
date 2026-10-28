//! Sans-IO signalling shared by `crownconnect-linux` and `llts-android`.
//!
//! Media transport lives in the other `llts-*` crates. This crate owns what travels around it:
//! the postcard control messages carried on the reliable channel, the versioned state each device
//! publishes to its peers, and the private discovery beacons that let linked devices find each
//! other without telling anyone else who is nearby. Nothing here performs I/O.

pub mod device;
pub mod discovery;
pub mod input;
pub mod message;
pub mod state;

mod error;
pub(crate) mod util;

pub use error::SignalingError;
