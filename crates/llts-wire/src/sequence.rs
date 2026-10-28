use crate::util::wrapping;

/// Per-stream packet counter that wraps at `u16::MAX`.
///
/// Deliberately not [`Ord`]: a wrapping counter has no total order, and comparing two sequence
/// numbers more than half a counter space apart is a question without an answer. Use
/// [`SequenceNumber::precedes`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SequenceNumber(u16);

impl SequenceNumber {
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

    pub const fn advanced_by(self, count: u16) -> Self {
        Self(self.0.wrapping_add(count))
    }

    pub const fn precedes(self, other: Self) -> bool {
        wrapping::precedes(self.0, other.0)
    }

    /// Forward distance to `other`; negative when `other` is behind `self`.
    pub const fn distance_to(self, other: Self) -> i16 {
        wrapping::distance(self.0, other.0)
    }
}

impl From<u16> for SequenceNumber {
    fn from(value: u16) -> Self {
        Self(value)
    }
}
