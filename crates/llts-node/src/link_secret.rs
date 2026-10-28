use core::fmt;

use curve25519_dalek::montgomery::MontgomeryPoint;
use llts_signaling::device::DeviceId;
use llts_signaling::discovery::DiscoverySecret;
use llts_transport::security::DeviceIdentity;
use zeroize::Zeroizing;

/// The long-term secret two paired devices share without ever sending it.
///
/// It is X25519 of one device's private key with the other's public key, hashed together with
/// both public keys in a fixed order, so either side computes the same value from its own
/// identity and the peer's key alone. Discovery identifiers and the ffsp link are derived from
/// it under their own contexts, so none of them reveals it or each other.
#[derive(Clone, PartialEq, Eq)]
pub struct LinkSecret(Zeroizing<[u8; Self::LEN]>);

impl LinkSecret {
    pub const LEN: usize = 32;
    pub const CONTEXT: &'static str = "crownos.llts.v1 link secret";

    pub fn derive(identity: &DeviceIdentity, peer: &DeviceId) -> Self {
        let private = Zeroizing::new(*identity.private());
        let shared = Zeroizing::new(MontgomeryPoint(peer.0).mul_clamped(*private).to_bytes());
        let own = identity.public().0;
        let (low, high) = if own <= peer.0 {
            (&own, &peer.0)
        } else {
            (&peer.0, &own)
        };
        let mut hasher = blake3::Hasher::new_derive_key(Self::CONTEXT);
        hasher.update(&*shared).update(low).update(high);
        Self(Zeroizing::new(*hasher.finalize().as_bytes()))
    }

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }

    pub fn discovery_secret(&self) -> DiscoverySecret {
        DiscoverySecret::derive(self.as_bytes())
    }
}

impl fmt::Debug for LinkSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinkSecret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::device::device_id;

    #[test]
    fn both_ends_derive_the_same_secret_and_a_third_device_does_not()
    -> Result<(), llts_transport::TransportError> {
        let laptop = DeviceIdentity::generate()?;
        let phone = DeviceIdentity::generate()?;
        let stranger = DeviceIdentity::generate()?;
        let from_laptop = LinkSecret::derive(&laptop, &device_id(*phone.public()));
        let from_phone = LinkSecret::derive(&phone, &device_id(*laptop.public()));
        let from_stranger = LinkSecret::derive(&stranger, &device_id(*phone.public()));
        assert_eq!(from_laptop, from_phone);
        assert_ne!(from_laptop, from_stranger);
        Ok(())
    }
}
