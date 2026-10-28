//! The sans-IO CrownConnect orchestrator shared by `crownconnect-linux` and `llts-android`.
//!
//! A [`Node`] owns everything above one llts [`Driver`](llts_transport::session::Driver): which
//! devices are paired and whether to dial them, the pairing flows, the state every device
//! publishes about itself and caches about the others, and which features run with whom. It
//! never touches a socket, a radio or a clock. The runtime feeds each [`NodeInput`] to
//! [`Node::handle`], carries out every [`NodeOutput`] that [`Node::poll_output`] yields, and
//! hands [`NodeInput::TimerExpired`] back once [`Node::next_timeout`] passes. [`bridge`] does the
//! translation to and from a real driver, so a runtime is mostly plumbing.

pub mod bridge;

mod clock;
mod config;
mod error;
mod feature;
mod input;
mod link_secret;
mod node;
mod output;
mod pairing;
mod peer;
mod presence;
mod sync;
pub(crate) mod util;

pub use clock::Now;
pub use config::{BackoffPolicy, KnownPeer, LocalDevice, NodeConfig};
pub use error::NodeError;
pub use feature::{FeatureRequest, FeatureState, MediaRole};
pub use input::{AppCommand, LinkEvent, NodeInput, OutgoingCommand, Presence};
pub use link_secret::LinkSecret;
pub use node::{DeviceView, Node};
pub use output::{
    AppEvent, CallCommand, DriverCommand, FeatureFailure, FfspLinkExport, MediaRequest, NodeOutput,
    PersistRequest, RemoteCommand,
};
