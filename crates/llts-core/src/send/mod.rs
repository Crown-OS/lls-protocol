//! Turning encoded media into datagrams, and deciding when they leave.

mod departure_pacer;
mod encoded_slice;
mod lane_pacer;
mod packetizer;
mod pipeline;
mod retransmit_cache;
mod sizing;

pub use departure_pacer::DeparturePacer;
pub use encoded_slice::EncodedSlice;
pub use lane_pacer::{LanePacer, PacerLane};
pub use packetizer::{EmittedRange, FramePacketizer};
pub use pipeline::SendPipeline;
pub use retransmit_cache::RetransmitCache;
pub use sizing::DatagramSizing;
