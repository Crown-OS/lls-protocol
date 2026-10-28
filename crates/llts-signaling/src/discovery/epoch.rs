/// How long one advertising identifier stays valid.
///
/// Fifteen minutes is long enough that a scanner throttled by Android's background limits still
/// wakes inside the window, and short enough that a stationary device is not trackable for long.
pub const EPOCH_SECONDS: u64 = 900;

/// Counts [`EPOCH_SECONDS`] periods since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    pub const fn from_unix_seconds(seconds: u64) -> Self {
        Self(seconds / EPOCH_SECONDS)
    }

    pub const fn new(counter: u64) -> Self {
        Self(counter)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// The low byte travels in the beacon so a scanner can tell clock skew apart from a device
    /// it does not know.
    pub const fn low_byte(self) -> u8 {
        let [low, ..] = self.0.to_le_bytes();
        low
    }

    pub const fn shift(self, delta: i64) -> Option<Self> {
        match self.0.checked_add_signed(delta) {
            Some(counter) => Some(Self(counter)),
            None => None,
        }
    }

    /// The epochs a scanner tries, centred on its own clock.
    pub fn window(self, tolerance: u8) -> impl Iterator<Item = Self> {
        let tolerance = i64::from(tolerance);
        (-tolerance..=tolerance).filter_map(move |delta| self.shift(delta))
    }
}
