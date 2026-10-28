use core::fmt;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::HandshakeKind;
use crate::TransportError;

/// A device's long-term X25519 public key: the identity a peer is paired with.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StaticKey(pub [u8; StaticKey::LEN]);

impl StaticKey {
    pub const LEN: usize = 32;

    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self, TransportError> {
        <[u8; Self::LEN]>::try_from(bytes).map(Self).map_err(|_| {
            TransportError::HandshakeRejected {
                reason: "the peer's static key has the wrong length",
            }
        })
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for StaticKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "StaticKey(")?;
        for byte in self.0.iter().take(6) {
            write!(formatter, "{byte:02x}")?;
        }
        write!(formatter, "…)")
    }
}

/// A device's long-lived X25519 identity, generated once and kept across sessions.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DeviceIdentity {
    private: [u8; StaticKey::LEN],
    #[zeroize(skip)]
    public: StaticKey,
}

impl DeviceIdentity {
    pub fn generate() -> Result<Self, TransportError> {
        let keypair = snow::Builder::new(HandshakeKind::Reconnect.noise_params()?)
            .generate_keypair()
            .map_err(|source| TransportError::Noise {
                step: "generating a keypair",
                source,
            })?;
        let mut private = keypair.private;
        let identity = Self::from_parts(
            <[u8; StaticKey::LEN]>::try_from(private.as_slice()).map_err(|_| {
                TransportError::HandshakeRejected {
                    reason: "the generated private key has the wrong length",
                }
            })?,
            StaticKey::from_slice(&keypair.public)?,
        );
        private.zeroize();
        Ok(identity)
    }

    pub const fn from_parts(private: [u8; StaticKey::LEN], public: StaticKey) -> Self {
        Self { private, public }
    }

    pub const fn private(&self) -> &[u8; StaticKey::LEN] {
        &self.private
    }

    /// The value the peer stores when the two devices are paired.
    pub const fn public(&self) -> &StaticKey {
        &self.public
    }
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceIdentity")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}
