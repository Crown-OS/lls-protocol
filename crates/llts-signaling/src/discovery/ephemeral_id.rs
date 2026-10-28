use core::fmt;

use crate::discovery::{DiscoverySecret, Epoch};

/// The rotating value a linked device advertises instead of a stable identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EphemeralId([u8; Self::LEN]);

impl EphemeralId {
    pub const LEN: usize = 16;

    /// Keyed BLAKE3 over the epoch counter, read through the XOF so the identifier is exactly
    /// [`Self::LEN`] bytes without truncating a longer digest.
    pub fn derive(secret: &DiscoverySecret, epoch: Epoch) -> Self {
        let mut id = [0; Self::LEN];
        blake3::Hasher::new_keyed(secret.as_bytes())
            .update(&epoch.as_u64().to_le_bytes())
            .finalize_xof()
            .fill(&mut id);
        Self(id)
    }

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for EphemeralId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EphemeralId(")?;
        self.0
            .iter()
            .take(4)
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))?;
        formatter.write_str("..)")
    }
}
