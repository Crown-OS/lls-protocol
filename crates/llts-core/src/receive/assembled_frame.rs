use llts_wire::{FrameId, MediaTimestamp};

use crate::buffer::PooledBuffer;
use crate::clock::Instant;

/// A complete encoded frame, ready for a decoder.
///
/// Owns a pooled buffer rather than a `Vec`, so handing a frame to the decoder and getting it
/// back costs nothing.
#[derive(Debug)]
pub struct AssembledFrame {
    frame: FrameId,
    timestamp: MediaTimestamp,
    is_keyframe: bool,
    first_fragment_arrived_at: Instant,
    completed_at: Instant,
    buffer: PooledBuffer,
}

impl AssembledFrame {
    pub(super) const fn new(
        frame: FrameId,
        timestamp: MediaTimestamp,
        is_keyframe: bool,
        first_fragment_arrived_at: Instant,
        completed_at: Instant,
        buffer: PooledBuffer,
    ) -> Self {
        Self {
            frame,
            timestamp,
            is_keyframe,
            first_fragment_arrived_at,
            completed_at,
            buffer,
        }
    }

    pub const fn frame(&self) -> FrameId {
        self.frame
    }

    pub const fn timestamp(&self) -> MediaTimestamp {
        self.timestamp
    }

    pub const fn is_keyframe(&self) -> bool {
        self.is_keyframe
    }

    /// When the first fragment landed, which with the completion time gives the spread a frame's
    /// burst suffered on the wire.
    pub const fn first_fragment_arrived_at(&self) -> Instant {
        self.first_fragment_arrived_at
    }

    pub const fn completed_at(&self) -> Instant {
        self.completed_at
    }

    pub fn bytes(&self) -> &[u8] {
        self.buffer.filled()
    }

    pub const fn byte_len(&self) -> usize {
        self.buffer.len()
    }
}
