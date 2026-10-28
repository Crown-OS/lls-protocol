use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// The per-link key a device's rotating identifiers are derived under.
///
/// It is derived from the secret a pairing agreed on rather than being that secret, so a leaked
/// discovery key lets someone recognise the device but never decrypt its traffic.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DiscoverySecret([u8; Self::LEN]);

impl DiscoverySecret {
    pub const LEN: usize = 32;
    const CONTEXT: &'static str = "crownos.llts.v1 discovery secret";

    pub fn derive(link_secret: &[u8; 32]) -> Self {
        Self(blake3::derive_key(Self::CONTEXT, link_secret))
    }

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for DiscoverySecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DiscoverySecret(..)")
    }
}
