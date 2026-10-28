use core::fmt;

const CODE_CONTEXT: &str = "crownos.llts.v1 pairing verification code";
const CODE_SPACE: u64 = 1_000_000;

/// The six digits both screens show during nearby pairing.
///
/// Derived from the handshake transcript and both pairing nonces, so a man in the middle —
/// running one handshake with each side — would need two different transcripts to produce the
/// same six digits. The commit-reveal order of the nonces is what stops it choosing its way to
/// a match: see [`super::NearbyInitiator`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerificationCode(u32);

impl VerificationCode {
    pub(crate) fn derive(
        handshake_hash: &[u8; 32],
        initiator_nonce: &[u8; 32],
        responder_nonce: &[u8; 32],
    ) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(CODE_CONTEXT);
        hasher.update(handshake_hash);
        hasher.update(initiator_nonce);
        hasher.update(responder_nonce);
        let mut wide = [0u8; 8];
        hasher.finalize_xof().fill(&mut wide);
        let code = u64::from_le_bytes(wide) % CODE_SPACE;
        Self(u32::try_from(code).unwrap_or(0))
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for VerificationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:06}", self.0)
    }
}

impl fmt::Debug for VerificationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "VerificationCode({self})")
    }
}
