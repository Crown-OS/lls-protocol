//! Fixed ceilings that keep every buffer in the engine statically sized.
//!
//! Real-time media has natural bounds — a frame that needs more than a thousand datagrams, or a
//! receiver holding more than a handful of frames, has already missed its deadline. Encoding
//! those bounds as constants is what lets the whole receive path avoid the allocator.

/// Datagrams one frame may occupy.
///
/// At the conservative 1200-byte MTU this caps a frame at roughly 1.1 MB, several times any
/// 1080p or 1440p keyframe a hardware encoder will produce under a per-frame byte budget.
pub const MAX_FRAGMENTS_PER_FRAME: usize = 1024;

/// Frames the assembler tracks at once.
///
/// Anything older is past its playout deadline, so holding more would cost memory to deliver
/// something the presenter would drop anyway.
pub const MAX_FRAMES_IN_FLIGHT: usize = 4;

/// Lost sequences the receiver will chase at once.
///
/// A backlog larger than this means the link is failing faster than retransmission can repair it,
/// at which point the repair policy has already switched to parity.
pub const MAX_PENDING_RETRANSMISSIONS: usize = 64;

/// Repair blocks tracked per frame, one per slice the encoder emitted.
pub const MAX_BLOCKS_PER_FRAME: usize = 8;

/// Parity shards held for one block while waiting to see whether they are needed.
pub const MAX_PARITY_SHARDS_PER_BLOCK: usize = 256;
