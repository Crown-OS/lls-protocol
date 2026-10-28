/// Ordering for counters that wrap at `u16::MAX`.
///
/// Two sequence numbers are comparable only within half the counter space; beyond that the
/// question is meaningless, which is why neither [`crate::SequenceNumber`] nor [`crate::FrameId`]
/// implements [`Ord`].
pub(crate) const fn precedes(earlier: u16, later: u16) -> bool {
    (earlier.wrapping_sub(later) as i16) < 0
}

/// Signed forward distance from `from` to `to`, negative when `to` is behind `from`.
pub(crate) const fn distance(from: u16, to: u16) -> i16 {
    to.wrapping_sub(from) as i16
}

#[cfg(test)]
mod tests {
    use super::{distance, precedes};

    #[test]
    fn ordering_survives_wraparound() {
        assert!(precedes(65_534, 2));
        assert!(!precedes(2, 65_534));
        assert!(precedes(0, 1));
        assert!(!precedes(1, 1));
    }

    #[test]
    fn distance_is_signed_and_wraps() {
        assert_eq!(distance(65_534, 2), 4);
        assert_eq!(distance(2, 65_534), -4);
        assert_eq!(distance(7, 7), 0);
    }
}
