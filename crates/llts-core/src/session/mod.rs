//! The composition root: one peer session over `(now, event)`, multiplexing every stream.

mod action;
mod budget;
mod clock_sync;
mod config;
mod control_message;
mod event;
mod liveness;
mod path;
mod peer;
mod role;
mod sent_history;
mod stream_table;
mod transport_feedback;

pub use action::Action;
pub use budget::{MediaBudget, frame_byte_budget};
pub use clock_sync::ClockSync;
pub use config::{PeerConfig, SessionPools};
pub use control_message::SessionControl;
pub use event::Event;
pub use liveness::Liveness;
pub use path::{PathChallenge, PathId, PathManager};
pub use peer::PeerSession;
pub use role::Role;
pub use sent_history::{SentHistory, SentPacket};
pub use transport_feedback::TransportFeedback;
