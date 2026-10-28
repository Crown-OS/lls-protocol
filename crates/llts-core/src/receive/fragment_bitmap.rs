use crate::limits::MAX_FRAGMENTS_PER_FRAME;

const BITS_PER_WORD: usize = u64::BITS as usize;
const WORD_COUNT: usize = MAX_FRAGMENTS_PER_FRAME / BITS_PER_WORD;

/// Which fragments of a frame have arrived.
///
/// A fixed 128-byte bitmap, so tracking an in-flight frame costs no allocation and answering
/// "what is still missing" is a handful of word scans rather than a set lookup per index.
#[derive(Debug, Clone)]
pub struct FragmentBitmap {
    words: [u64; WORD_COUNT],
    present: u16,
}

impl Default for FragmentBitmap {
    fn default() -> Self {
        Self::new()
    }
}

impl FragmentBitmap {
    pub const CAPACITY: usize = MAX_FRAGMENTS_PER_FRAME;

    pub const fn new() -> Self {
        Self {
            words: [0; WORD_COUNT],
            present: 0,
        }
    }

    /// Records `index`, reporting whether it was new.
    pub fn insert(&mut self, index: u16) -> bool {
        let Some((word, mask)) = Self::locate(index) else {
            return false;
        };
        let Some(slot) = self.words.get_mut(word) else {
            return false;
        };
        if *slot & mask != 0 {
            return false;
        }
        *slot |= mask;
        self.present = self.present.saturating_add(1);
        true
    }

    pub fn contains(&self, index: u16) -> bool {
        Self::locate(index)
            .and_then(|(word, mask)| self.words.get(word).map(|slot| slot & mask != 0))
            .unwrap_or(false)
    }

    pub const fn present_count(&self) -> u16 {
        self.present
    }

    pub const fn is_empty(&self) -> bool {
        self.present == 0
    }

    pub const fn clear(&mut self) {
        self.words = [0; WORD_COUNT];
        self.present = 0;
    }

    /// Indices below `limit` that have not arrived, in ascending order.
    pub fn missing_below(&self, limit: u16) -> impl Iterator<Item = u16> + '_ {
        (0..limit).filter(|index| !self.contains(*index))
    }

    const fn locate(index: u16) -> Option<(usize, u64)> {
        let index = index as usize;
        if index >= MAX_FRAGMENTS_PER_FRAME {
            return None;
        }
        Some((index / BITS_PER_WORD, 1u64 << (index % BITS_PER_WORD)))
    }
}
