//! Every paired device and where its link stands.

mod backoff;
mod link_state;
mod session;
mod sightings;
mod table;

pub(crate) use backoff::Backoff;
pub(crate) use link_state::LinkState;
pub(crate) use session::Session;
pub(crate) use sightings::Sightings;
pub(crate) use table::{MAX_PEERS, Peer, PeerTable};
