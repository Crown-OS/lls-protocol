const WORD_BITS: u64 = 64;
const WORDS: usize = 16;
const TOTAL_BITS: u64 = WORD_BITS * WORDS as u64;

/// Remembers which of the last 1024 counters have been accepted, so each is accepted once.
///
/// A ring of bits indexed by counter, cleared a word at a time as the window slides — the same
/// layout WireGuard uses, which never shifts the whole bitmap. One word is kept as slack so that
/// clearing ahead never erases a live bit, which puts the usable depth at 960 counters: far more
/// reordering than one Wi-Fi hop produces, and old enough that anything beyond it is stale.
#[derive(Debug, Clone)]
pub struct ReplayWindow {
    highest: u64,
    bits: [u64; WORDS],
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWindow {
    /// Counters this far behind the highest are refused outright.
    pub const DEPTH: u64 = TOTAL_BITS - WORD_BITS;

    pub const fn new() -> Self {
        Self {
            highest: 0,
            bits: [0; WORDS],
        }
    }

    /// Whether `counter` would be accepted now: neither stale nor already seen.
    pub fn admits(&self, counter: u64) -> bool {
        if counter > self.highest {
            return true;
        }
        if counter.saturating_add(Self::DEPTH) < self.highest {
            return false;
        }
        let (word, mask) = Self::locate(counter);
        self.bits.get(word).is_some_and(|bits| bits & mask == 0)
    }

    /// Marks `counter` as seen, sliding the window forward if it is the newest yet.
    pub fn record(&mut self, counter: u64) {
        if counter > self.highest {
            let current_word = self.highest / WORD_BITS;
            let advance = (counter / WORD_BITS - current_word).min(WORDS as u64);
            for step in 1..=advance {
                if let Some(bits) = self.bits.get_mut(Self::word_index(current_word + step)) {
                    *bits = 0;
                }
            }
            self.highest = counter;
        }
        let (word, mask) = Self::locate(counter);
        if let Some(bits) = self.bits.get_mut(word) {
            *bits |= mask;
        }
    }

    const fn locate(counter: u64) -> (usize, u64) {
        (
            Self::word_index(counter / WORD_BITS),
            1 << (counter % WORD_BITS),
        )
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "the modulus keeps the index below WORDS"
    )]
    const fn word_index(word: u64) -> usize {
        (word % WORDS as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::ReplayWindow;

    #[test]
    fn each_counter_is_accepted_once() {
        let mut window = ReplayWindow::new();
        for counter in [0, 5, 3, 900, 4] {
            assert!(window.admits(counter));
            window.record(counter);
            assert!(!window.admits(counter));
        }
    }

    #[test]
    fn a_counter_past_the_depth_is_stale() {
        let mut window = ReplayWindow::new();
        window.record(10_000);
        assert!(!window.admits(10_000 - ReplayWindow::DEPTH - 1));
        assert!(window.admits(10_000 - ReplayWindow::DEPTH + 1));
    }

    #[test]
    fn a_long_jump_forgets_everything_behind_it() {
        let mut window = ReplayWindow::new();
        window.record(7);
        window.record(1 << 40);
        assert!(!window.admits(7));
        assert!(window.admits((1 << 40) - 1));
    }
}
