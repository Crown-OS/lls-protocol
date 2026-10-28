use crate::util::wrapping;

/// Per-stream frame counter that wraps at `u16::MAX`.
///
/// One frame is one FEC block, so this also names the repair block a shard belongs to.
/// Not [`Ord`], for the same reason as [`crate::SequenceNumber`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(u16);

impl FrameId {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    pub const fn precedes(self, other: Self) -> bool {
        wrapping::precedes(self.0, other.0)
    }

    pub const fn distance_to(self, other: Self) -> i16 {
        wrapping::distance(self.0, other.0)
    }
}

impl From<u16> for FrameId {
    fn from(value: u16) -> Self {
        Self(value)
    }
}
