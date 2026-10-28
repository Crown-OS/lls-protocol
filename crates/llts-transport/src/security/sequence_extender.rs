use llts_wire::SequenceNumber;

const SEQUENCE_BITS: u32 = 16;

/// Extends a 16-bit wire sequence into the 64-bit counter an AEAD nonce needs.
///
/// A nonce may never repeat under one key, but the header only carries 16 bits, which would wrap
/// every 65 536 packets — about thirty seconds of 1080p60. SRTP solves this with a rollover
/// counter and so does this: the high bits are inferred from how far the sequence sits from the
/// highest one accepted, which is unambiguous as long as reordering stays within half the
/// counter space.
///
/// Inference and acceptance are separate steps. [`SequenceExtender::peek`] only computes; a
/// receiver calls [`SequenceExtender::commit`] once the packet has authenticated, so a forged
/// datagram can never drag the counter somewhere the real sender is not.
#[derive(Debug, Default, Clone, Copy)]
pub struct SequenceExtender {
    highest: Option<u64>,
}

impl SequenceExtender {
    /// One full wrap of the wire sequence, the step a receiver retries by when a guess fails.
    pub const ROLLOVER: u64 = 1 << SEQUENCE_BITS;

    pub const fn new() -> Self {
        Self { highest: None }
    }

    /// The highest counter committed so far.
    pub const fn highest(&self) -> Option<u64> {
        self.highest
    }

    /// The counter `sequence` most plausibly belongs to, without changing any state.
    pub fn peek(&self, sequence: SequenceNumber) -> u64 {
        let wire = u64::from(sequence.get());
        let Some(highest) = self.highest else {
            return wire;
        };
        let highest_sequence = SequenceNumber::new(low_bits(highest));
        let rollovers = highest >> SEQUENCE_BITS;
        let distance = highest_sequence.distance_to(sequence);

        let rollovers = if distance > 0 && sequence.get() < highest_sequence.get() {
            rollovers.saturating_add(1)
        } else if distance < 0 && sequence.get() > highest_sequence.get() {
            rollovers.saturating_sub(1)
        } else {
            rollovers
        };
        (rollovers << SEQUENCE_BITS) | wire
    }

    /// Accepts `counter`, moving the highest mark forward if it is newer.
    ///
    /// Also how an authenticated resync record lands: it names the peer's true counter, which
    /// may be several rollovers past anything this side has seen.
    pub fn commit(&mut self, counter: u64) {
        if self.highest.is_none_or(|highest| counter > highest) {
            self.highest = Some(counter);
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the mask keeps exactly the sixteen wire bits"
)]
const fn low_bits(counter: u64) -> u16 {
    (counter & (SequenceExtender::ROLLOVER - 1)) as u16
}

#[cfg(test)]
mod tests {
    use llts_wire::SequenceNumber;

    use super::SequenceExtender;

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    #[test]
    fn peeking_never_moves_the_counter() {
        let mut extender = SequenceExtender::new();
        extender.commit(extender.peek(sequence(65_530)));
        let ahead = extender.peek(sequence(3));
        assert_eq!(ahead, SequenceExtender::ROLLOVER + 3);
        assert_eq!(extender.highest(), Some(65_530));
    }

    #[test]
    fn a_late_packet_from_before_the_wrap_keeps_its_rollover() {
        let mut extender = SequenceExtender::new();
        extender.commit(SequenceExtender::ROLLOVER + 2);
        assert_eq!(extender.peek(sequence(65_534)), 65_534);
    }

    #[test]
    fn committing_an_older_counter_changes_nothing() {
        let mut extender = SequenceExtender::new();
        extender.commit(500);
        extender.commit(20);
        assert_eq!(extender.highest(), Some(500));
    }
}
