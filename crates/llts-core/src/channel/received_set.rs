use llts_wire::SequenceNumber;

const WORDS: usize = 4;
const WORD_BITS: u16 = 64;
const SPAN_BITS: u16 = 256;

/// Which channel sequences past the cumulative point have arrived.
///
/// A ring of 256 bits over `(cumulative, cumulative + 256]`; the cumulative point advances over
/// every run that has filled in, clearing bits as it goes, so the ring never needs shifting.
#[derive(Debug, Clone)]
pub(crate) struct ReceivedSet {
    cumulative: SequenceNumber,
    bits: [u64; WORDS],
}

impl ReceivedSet {
    /// `WORDS` words of `WORD_BITS` bits.
    pub(crate) const SPAN: u16 = SPAN_BITS;

    /// Nothing received yet: the cumulative point sits one before sequence zero.
    pub(crate) const fn new() -> Self {
        Self {
            cumulative: SequenceNumber::new(u16::MAX),
            bits: [0; WORDS],
        }
    }

    pub(crate) const fn cumulative(&self) -> SequenceNumber {
        self.cumulative
    }

    /// Whether the set can hold `sequence`: ahead of the cumulative point and inside the span.
    pub(crate) const fn is_in_window(&self, sequence: SequenceNumber) -> bool {
        let ahead = self.cumulative.distance_to(sequence);
        ahead > 0 && ahead <= Self::SPAN as i16
    }

    pub(crate) fn contains(&self, sequence: SequenceNumber) -> bool {
        if self.cumulative.distance_to(sequence) <= 0 {
            return true;
        }
        let (word, mask) = Self::slot(sequence);
        self.bits.get(word).is_some_and(|bits| bits & mask != 0)
    }

    /// Whether anything past the cumulative point is held, meaning a gap sits in front of it.
    pub(crate) fn has_gap(&self) -> bool {
        self.bits.iter().any(|bits| *bits != 0)
    }

    pub(crate) fn insert(&mut self, sequence: SequenceNumber) {
        let (word, mask) = Self::slot(sequence);
        if let Some(bits) = self.bits.get_mut(word) {
            *bits |= mask;
        }
        while self.take(self.cumulative.next()) {
            self.cumulative = self.cumulative.next();
        }
    }

    fn take(&mut self, sequence: SequenceNumber) -> bool {
        let (word, mask) = Self::slot(sequence);
        let Some(bits) = self.bits.get_mut(word) else {
            return false;
        };
        let present = *bits & mask != 0;
        *bits &= !mask;
        present
    }

    fn slot(sequence: SequenceNumber) -> (usize, u64) {
        let index = sequence.get() % Self::SPAN;
        (usize::from(index / WORD_BITS), 1 << (index % WORD_BITS))
    }
}
