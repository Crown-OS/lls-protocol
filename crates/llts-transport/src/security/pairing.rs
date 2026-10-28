use zeroize::Zeroizing;

use super::{
    DeviceIdentity, HandshakeKind, Hello, NoiseHandshake, PresharedKey, SessionKeys, StaticKey,
    VerificationCode,
};
use crate::TransportError;
use crate::util::random_bytes;

const NONCE_LEN: usize = 32;

/// What a completed pairing handshake leaves behind.
#[derive(Debug)]
pub struct PairingOutcome {
    pub keys: SessionKeys,
    pub peer: StaticKey,
    pub peer_hello: Hello,
    /// The code both screens must show before the user accepts, for nearby pairing. `None` for
    /// QR pairing, where the scanned one-time key already authenticated the responder.
    pub code: Option<VerificationCode>,
}

fn commitment(nonce: &[u8; NONCE_LEN]) -> [u8; 32] {
    *blake3::hash(nonce).as_bytes()
}

fn split_nonce(payload: &[u8]) -> Result<([u8; NONCE_LEN], &[u8]), TransportError> {
    let (nonce, rest) =
        payload
            .split_at_checked(NONCE_LEN)
            .ok_or(TransportError::HandshakeRejected {
                reason: "the pairing message is missing its nonce",
            })?;
    let nonce =
        <[u8; NONCE_LEN]>::try_from(nonce).map_err(|_| TransportError::HandshakeRejected {
            reason: "the pairing message is missing its nonce",
        })?;
    Ok((nonce, rest))
}

/// Writes `nonce ‖ hello` into a payload buffer.
fn nonce_and_hello(
    nonce: &[u8; NONCE_LEN],
    hello: &Hello,
) -> Result<([u8; NONCE_LEN + Hello::MAX_ENCODED_LEN], usize), TransportError> {
    let mut payload = [0u8; NONCE_LEN + Hello::MAX_ENCODED_LEN];
    let (nonce_slot, hello_slot) = payload.split_at_mut(NONCE_LEN);
    nonce_slot.copy_from_slice(nonce);
    let hello_len = hello.encode_into(hello_slot)?;
    Ok((payload, NONCE_LEN + hello_len))
}

fn payload_of(payload: &[u8], len: usize) -> &[u8] {
    payload.get(..len).unwrap_or_default()
}

/// The device that starts nearby pairing, over `Noise_XX` with a commit-reveal code.
///
/// ```text
/// -> e                  commit = BLAKE3(N_i)
/// <- e, ee, s, es       N_r ‖ Hello
/// -> s, se              N_i ‖ Hello
/// code = 6 digits of H(handshake_hash ‖ N_i ‖ N_r)
/// ```
///
/// The initiator is bound to `N_i` before it has seen `N_r`, and the responder has revealed
/// `N_r` before it sees `N_i`. A man in the middle running a separate handshake with each side
/// must commit to its own nonces before learning the value that would let it steer either code,
/// so it matches the two codes only by the one-in-a-million chance of guessing.
#[derive(Debug)]
pub struct NearbyInitiator {
    noise: NoiseHandshake,
    nonce: Zeroizing<[u8; NONCE_LEN]>,
    hello: Hello,
}

impl NearbyInitiator {
    /// Starts the handshake, writing message one into `out`.
    pub fn start(
        identity: &DeviceIdentity,
        hello: Hello,
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let nonce = Zeroizing::new(random_bytes::<NONCE_LEN>()?);
        Self::start_with_nonce(identity, hello, nonce, out)
    }

    pub(crate) fn start_with_nonce(
        identity: &DeviceIdentity,
        hello: Hello,
        nonce: Zeroizing<[u8; NONCE_LEN]>,
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let mut noise =
            NoiseHandshake::initiator(HandshakeKind::NearbyPairing, identity, None, None)?;
        let len = noise.write_message(&commitment(&nonce), out)?;
        Ok((
            Self {
                noise,
                nonce,
                hello,
            },
            len,
        ))
    }

    /// Reads the responder's nonce and identity, writes message three into `out`, and derives
    /// the code.
    pub fn on_response(
        mut self,
        message: &[u8],
        out: &mut [u8],
    ) -> Result<(PairingOutcome, usize), TransportError> {
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = self.noise.read_message(message, &mut payload)?;
        let (responder_nonce, hello_bytes) = split_nonce(payload_of(&payload, payload_len))?;
        let peer_hello = Hello::decode(hello_bytes)?;
        let peer = self
            .noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the responder did not reveal its identity",
            })?;

        let (reveal, reveal_len) = nonce_and_hello(&self.nonce, &self.hello)?;
        let len = self
            .noise
            .write_message(payload_of(&reveal, reveal_len), out)?;
        let code =
            VerificationCode::derive(&self.noise.handshake_hash(), &self.nonce, &responder_nonce);
        Ok((
            PairingOutcome {
                keys: self.noise.finish()?,
                peer,
                peer_hello,
                code: Some(code),
            },
            len,
        ))
    }
}

/// The device being paired with nearby, answering a [`NearbyInitiator`].
#[derive(Debug)]
pub struct NearbyResponder {
    noise: NoiseHandshake,
    commitment: [u8; 32],
    nonce: Zeroizing<[u8; NONCE_LEN]>,
}

impl NearbyResponder {
    /// Reads message one and writes message two, revealing this side's nonce.
    pub fn accept(
        identity: &DeviceIdentity,
        hello: &Hello,
        message: &[u8],
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let nonce = Zeroizing::new(random_bytes::<NONCE_LEN>()?);
        Self::accept_with_nonce(identity, hello, message, nonce, out)
    }

    pub(crate) fn accept_with_nonce(
        identity: &DeviceIdentity,
        hello: &Hello,
        message: &[u8],
        nonce: Zeroizing<[u8; NONCE_LEN]>,
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let mut noise = NoiseHandshake::responder(HandshakeKind::NearbyPairing, identity, None)?;
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = noise.read_message(message, &mut payload)?;
        let commitment = <[u8; 32]>::try_from(payload_of(&payload, payload_len)).map_err(|_| {
            TransportError::HandshakeRejected {
                reason: "the pairing request carried no commitment",
            }
        })?;
        let (response, response_len) = nonce_and_hello(&nonce, hello)?;
        let len = noise.write_message(payload_of(&response, response_len), out)?;
        Ok((
            Self {
                noise,
                commitment,
                nonce,
            },
            len,
        ))
    }

    /// Reads message three, checks the revealed nonce against its commitment, and derives the
    /// code.
    pub fn on_reveal(mut self, message: &[u8]) -> Result<PairingOutcome, TransportError> {
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = self.noise.read_message(message, &mut payload)?;
        let (initiator_nonce, hello_bytes) = split_nonce(payload_of(&payload, payload_len))?;
        if commitment(&initiator_nonce) != self.commitment {
            return Err(TransportError::HandshakeRejected {
                reason: "the revealed nonce does not match its commitment",
            });
        }
        let peer_hello = Hello::decode(hello_bytes)?;
        let peer = self
            .noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the initiator did not reveal its identity",
            })?;
        let code =
            VerificationCode::derive(&self.noise.handshake_hash(), &initiator_nonce, &self.nonce);
        Ok(PairingOutcome {
            keys: self.noise.finish()?,
            peer,
            peer_hello,
            code: Some(code),
        })
    }
}

/// The device that scanned a QR code, pairing over `Noise_XXpsk3` with the key the code carried.
///
/// The responder's static key is in the QR code too, so after message two the initiator checks
/// it is talking to the device it scanned; the one-time key mixed into message three is what
/// convinces the responder in turn. No code needs comparing.
#[derive(Debug)]
pub struct QrInitiator {
    noise: NoiseHandshake,
    expected_responder: StaticKey,
    hello: Hello,
}

impl QrInitiator {
    pub fn start(
        identity: &DeviceIdentity,
        expected_responder: StaticKey,
        psk: &PresharedKey,
        hello: Hello,
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let mut noise =
            NoiseHandshake::initiator(HandshakeKind::QrPairing, identity, None, Some(psk))?;
        let len = noise.write_message(&[], out)?;
        Ok((
            Self {
                noise,
                expected_responder,
                hello,
            },
            len,
        ))
    }

    pub fn on_response(
        mut self,
        message: &[u8],
        out: &mut [u8],
    ) -> Result<(PairingOutcome, usize), TransportError> {
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = self.noise.read_message(message, &mut payload)?;
        let peer_hello = Hello::decode(payload_of(&payload, payload_len))?;
        let peer = self
            .noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the responder did not reveal its identity",
            })?;
        if peer != self.expected_responder {
            return Err(TransportError::HandshakeRejected {
                reason: "the responder is not the device in the scanned code",
            });
        }
        let mut hello = [0u8; Hello::MAX_ENCODED_LEN];
        let hello_len = self.hello.encode_into(&mut hello)?;
        let len = self
            .noise
            .write_message(payload_of(&hello, hello_len), out)?;
        Ok((
            PairingOutcome {
                keys: self.noise.finish()?,
                peer,
                peer_hello,
                code: None,
            },
            len,
        ))
    }
}

/// The device showing a QR code, answering a [`QrInitiator`].
#[derive(Debug)]
pub struct QrResponder {
    noise: NoiseHandshake,
}

impl QrResponder {
    pub fn accept(
        identity: &DeviceIdentity,
        psk: &PresharedKey,
        hello: &Hello,
        message: &[u8],
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let mut noise = NoiseHandshake::responder(HandshakeKind::QrPairing, identity, Some(psk))?;
        let mut discard = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        noise.read_message(message, &mut discard)?;
        let mut encoded = [0u8; Hello::MAX_ENCODED_LEN];
        let hello_len = hello.encode_into(&mut encoded)?;
        let len = noise.write_message(payload_of(&encoded, hello_len), out)?;
        Ok((Self { noise }, len))
    }

    /// Reads message three. A wrong pre-shared key fails here, inside Noise.
    pub fn on_confirmation(mut self, message: &[u8]) -> Result<PairingOutcome, TransportError> {
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = self.noise.read_message(message, &mut payload)?;
        let peer_hello = Hello::decode(payload_of(&payload, payload_len))?;
        let peer = self
            .noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the initiator did not reveal its identity",
            })?;
        Ok(PairingOutcome {
            keys: self.noise.finish()?,
            peer,
            peer_hello,
            code: None,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test that fails a precondition should fail loudly"
    )]

    use zeroize::Zeroizing;

    use super::{NONCE_LEN, NearbyInitiator, NearbyResponder, commitment};
    use crate::TransportError;
    use crate::security::{DeviceClass, DeviceIdentity, HandshakeKind, Hello, NoiseHandshake};

    fn hello(tag: u16) -> Hello {
        Hello::new("device", DeviceClass::Laptop, 0).with_session_tag(tag)
    }

    /// A man in the middle that passes the initiator's commitment through to the responder,
    /// hoping to reveal a nonce of its own choosing once it has seen the responder's.
    #[test]
    fn a_relayed_commitment_cannot_be_reopened_to_steer_the_code() {
        let alice = DeviceIdentity::generate().unwrap();
        let bob = DeviceIdentity::generate().unwrap();
        let mallory = DeviceIdentity::generate().unwrap();
        let mut wire = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let mut reply = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];

        let (_alice, len) = NearbyInitiator::start(&alice, hello(1), &mut wire).unwrap();
        let mut from_alice =
            NoiseHandshake::responder(HandshakeKind::NearbyPairing, &mallory, None).unwrap();
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = from_alice.read_message(&wire[..len], &mut payload).unwrap();
        let alice_commitment = payload[..payload_len].to_vec();

        let mut to_bob =
            NoiseHandshake::initiator(HandshakeKind::NearbyPairing, &mallory, None, None).unwrap();
        let len = to_bob.write_message(&alice_commitment, &mut wire).unwrap();
        let (bob_side, len) =
            NearbyResponder::accept(&bob, &hello(2), &wire[..len], &mut reply).unwrap();
        to_bob.read_message(&reply[..len], &mut payload).unwrap();

        let chosen = [0x42u8; NONCE_LEN];
        let mut reveal = chosen.to_vec();
        let mut encoded = [0u8; Hello::MAX_ENCODED_LEN];
        let hello_len = hello(3).encode_into(&mut encoded).unwrap();
        reveal.extend_from_slice(&encoded[..hello_len]);
        let len = to_bob.write_message(&reveal, &mut wire).unwrap();

        assert!(matches!(
            bob_side.on_reveal(&wire[..len]),
            Err(TransportError::HandshakeRejected { .. })
        ));
    }

    /// A man in the middle that runs an honest-looking handshake with each side and chooses its
    /// own nonces with everything it can see.
    #[test]
    fn an_adaptive_man_in_the_middle_cannot_make_the_codes_agree() {
        let alice = DeviceIdentity::generate().unwrap();
        let bob = DeviceIdentity::generate().unwrap();
        let mallory = DeviceIdentity::generate().unwrap();
        let mut wire = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let mut reply = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];

        let mut matches = 0;
        for attempt in 0..32u8 {
            let (alice_side, len) = NearbyInitiator::start(&alice, hello(1), &mut wire).unwrap();
            let alice_first = wire[..len].to_vec();

            let (mallory_to_bob, len) = NearbyInitiator::start_with_nonce(
                &mallory,
                hello(3),
                Zeroizing::new([attempt; NONCE_LEN]),
                &mut wire,
            )
            .unwrap();
            let (bob_side, len) =
                NearbyResponder::accept(&bob, &hello(2), &wire[..len], &mut reply).unwrap();
            let (bob_leg, len) = mallory_to_bob
                .on_response(&reply[..len], &mut wire)
                .unwrap();
            let bob_outcome = bob_side.on_reveal(&wire[..len]).unwrap();
            let bob_code = bob_outcome.code.unwrap();
            assert_eq!(bob_leg.code, Some(bob_code));

            // Mallory now knows Bob's code and picks the nonce it shows Alice to aim at it, but
            // Alice's own nonce is still hidden behind her commitment.
            let aimed = *blake3::hash(&bob_code.value().to_le_bytes()).as_bytes();
            let (mallory_from_alice, len) = NearbyResponder::accept_with_nonce(
                &mallory,
                &hello(4),
                &alice_first,
                Zeroizing::new(aimed),
                &mut reply,
            )
            .unwrap();
            let (alice_outcome, len) = alice_side.on_response(&reply[..len], &mut wire).unwrap();
            let alice_leg = mallory_from_alice.on_reveal(&wire[..len]).unwrap();
            assert_eq!(alice_leg.code, alice_outcome.code);

            assert_eq!(alice_outcome.peer, *mallory.public());
            assert_eq!(bob_outcome.peer, *mallory.public());
            if alice_outcome.code == Some(bob_code) {
                matches += 1;
            }
        }
        assert_eq!(matches, 0, "a man in the middle forced matching codes");
    }

    #[test]
    fn the_commitment_is_the_hash_of_the_nonce() {
        let nonce = [7u8; NONCE_LEN];
        assert_eq!(commitment(&nonce), *blake3::hash(&nonce).as_bytes());
    }
}
