//! The non-media streams: reliable messages and redundant input events.

mod delivery;
mod input;
mod reassembly;
mod received_set;
mod reliable;
mod send_window;

pub use delivery::{Delivery, InputDelivery};
pub use input::InputChannel;
pub use reliable::ReliableChannel;

/// Largest message a reliable channel carries. Anything bigger is a file, and belongs to ffsp.
pub const MAX_MESSAGE_LEN: usize = 64 * 1024;
