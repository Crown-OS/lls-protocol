use llts_wire::SequenceNumber;

const BITS_PER_WORD: usize = u64::BITS as usize;

/// One source of truth for the window size, so the slot mask and the distance bound cannot drift
/// apart and no cast is needed to compare them.
const WINDOW_LOG2: u32 = 10;
const WINDOW_LEN: usize = 1 << WINDOW_LOG2;
const WINDOW_DISTANCE: i16 = 1 << WINDOW_LOG2;
const WORD_COUNT: usize = WINDOW_LEN / BITS_PER_WORD;

/// What a sequence number turned out to be when the window saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// At or beyond the highest sequence seen so far.
    Advanced,
    /// Filled a hole the window was already tracking.
    Backfilled,
    Duplicate,
    /// Older than the window still remembers, so it cannot be told from a duplicate.
    Forgotten,
}

/// Sliding bitmap over the most recent [`SequenceWindow::CAPACITY`] sequence numbers.
///
/// The same anti-replay structure TCP SACK and IPsec use: a fixed 128 bytes, constant-time
/// insertion, and correct across the 16-bit wraparound because every comparison goes through
/// [`SequenceNumber::distance_to`].
#[derive(Debug)]
pub struct SequenceWindow {
    arrived: [u64; WORD_COUNT],
    highest: Option<SequenceNumber>,
}

impl Default for SequenceWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceWindow {
    pub const CAPACITY: u16 = 1 << WINDOW_LOG2;

    pub const fn new() -> Self {
        Self {
            arrived: [0; WORD_COUNT],
            highest: None,
        }
    }

    pub const fn highest(&self) -> Option<SequenceNumber> {
        self.highest
    }

    pub fn insert(&mut self, sequence: SequenceNumber) -> Arrival {
        let Some(highest) = self.highest else {
            // Everything before the first sequence is treated as already delivered, so a session
            // that starts mid-stream does not report the entire counter space as lost.
            self.arrived = [u64::MAX; WORD_COUNT];
            self.mark(sequence);
            self.highest = Some(sequence);
            return Arrival::Advanced;
        };

        let distance = highest.distance_to(sequence);
        if distance == 0 {
            return Arrival::Duplicate;
        }
        if distance < 0 {
            return self.backfill(sequence, distance);
        }

        self.advance_to(sequence, distance);
        Arrival::Advanced
    }

    pub fn contains(&self, sequence: SequenceNumber) -> bool {
        let Some(highest) = self.highest else {
            return false;
        };
        let distance = highest.distance_to(sequence);
        if distance > 0 || -distance >= WINDOW_DISTANCE {
            return false;
        }
        self.is_marked(sequence)
    }

    /// Sequences still missing within `depth` of the highest seen, oldest first.
    ///
    /// Bounded by design: a retransmission that would arrive after the playout deadline is not
    /// worth asking for, so the scheduler only ever looks a few milliseconds back.
    pub fn gaps_within(&self, depth: u16) -> impl Iterator<Item = SequenceNumber> + '_ {
        let highest = self.highest;
        let depth = depth.min(Self::CAPACITY);
        (1..depth).rev().filter_map(move |back| {
            let candidate = highest?.advanced_by(0u16.wrapping_sub(back));
            (!self.is_marked(candidate)).then_some(candidate)
        })
    }

    fn backfill(&mut self, sequence: SequenceNumber, distance: i16) -> Arrival {
        if -distance >= WINDOW_DISTANCE {
            return Arrival::Forgotten;
        }
        if self.is_marked(sequence) {
            return Arrival::Duplicate;
        }
        self.mark(sequence);
        Arrival::Backfilled
    }

    fn advance_to(&mut self, sequence: SequenceNumber, distance: i16) {
        if distance >= WINDOW_DISTANCE {
            self.arrived = [0; WORD_COUNT];
        } else {
            let mut vacated = self.highest.unwrap_or(sequence);
            for _ in 0..distance {
                vacated = vacated.next();
                self.clear(vacated);
            }
        }
        self.mark(sequence);
        self.highest = Some(sequence);
    }

    fn slot(sequence: SequenceNumber) -> (usize, u64) {
        let index = usize::from(sequence.get()) & (WINDOW_LEN - 1);
        (index / BITS_PER_WORD, 1u64 << (index % BITS_PER_WORD))
    }

    fn mark(&mut self, sequence: SequenceNumber) {
        let (word, mask) = Self::slot(sequence);
        if let Some(slot) = self.arrived.get_mut(word) {
            *slot |= mask;
        }
    }

    fn clear(&mut self, sequence: SequenceNumber) {
        let (word, mask) = Self::slot(sequence);
        if let Some(slot) = self.arrived.get_mut(word) {
            *slot &= !mask;
        }
    }

    fn is_marked(&self, sequence: SequenceNumber) -> bool {
        let (word, mask) = Self::slot(sequence);
        self.arrived.get(word).is_some_and(|slot| slot & mask != 0)
    }
}
