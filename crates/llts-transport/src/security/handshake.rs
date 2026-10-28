use llts_wire::packet::HandshakePattern;
use snow::HandshakeState;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::{
    DeviceIdentity, Direction, KeySchedule, ReceivingCipher, SendingCipher, StaticKey,
    TrafficSecret,
};
use crate::TransportError;

/// The three handshakes the protocol runs, each a fixed Noise pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandshakeKind {
    /// First contact nearby: `XX`, confirmed by comparing a six-digit code.
    NearbyPairing,
    /// First contact through a scanned QR code: `XXpsk3`, authenticated by its one-time key.
    QrPairing,
    /// A device already paired: `IK`, one round trip, both identities hidden from observers.
    Reconnect,
}

impl HandshakeKind {
    pub const fn pattern(self) -> HandshakePattern {
        match self {
            Self::NearbyPairing => HandshakePattern::Xx,
            Self::QrPairing => HandshakePattern::XxPsk3,
            Self::Reconnect => HandshakePattern::Ik,
        }
    }

    pub const fn from_pattern(pattern: HandshakePattern) -> Self {
        match pattern {
            HandshakePattern::Xx => Self::NearbyPairing,
            HandshakePattern::XxPsk3 => Self::QrPairing,
            HandshakePattern::Ik => Self::Reconnect,
        }
    }

    pub const fn noise_name(self) -> &'static str {
        match self {
            Self::NearbyPairing => "Noise_XX_25519_ChaChaPoly_BLAKE2s",
            Self::QrPairing => "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s",
            Self::Reconnect => "Noise_IK_25519_ChaChaPoly_BLAKE2s",
        }
    }

    pub(crate) fn noise_params(self) -> Result<snow::params::NoiseParams, TransportError> {
        self.noise_name()
            .parse()
            .map_err(|source| TransportError::Noise {
                step: "parsing the pattern",
                source,
            })
    }
}

/// A 32-byte secret both sides already hold, from a scanned QR code.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct PresharedKey(pub [u8; 32]);

impl core::fmt::Debug for PresharedKey {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("PresharedKey(redacted)")
    }
}

/// The two directional ciphers a completed handshake produces.
#[derive(Debug)]
pub struct SessionKeys {
    pub sending: SendingCipher,
    pub receiving: ReceivingCipher,
}

impl SessionKeys {
    /// Builds both directions from Noise's split, which names its keys for the initiator.
    pub(crate) fn from_split(
        mut initiator_to_responder: [u8; TrafficSecret::LEN],
        mut responder_to_initiator: [u8; TrafficSecret::LEN],
        is_initiator: bool,
    ) -> Self {
        let outbound = KeySchedule::new(
            TrafficSecret::from_bytes(initiator_to_responder),
            Direction::InitiatorToResponder,
        );
        let inbound = KeySchedule::new(
            TrafficSecret::from_bytes(responder_to_initiator),
            Direction::ResponderToInitiator,
        );
        initiator_to_responder.zeroize();
        responder_to_initiator.zeroize();

        let (sending, receiving) = if is_initiator {
            (outbound, inbound)
        } else {
            (inbound, outbound)
        };
        Self {
            sending: SendingCipher::new(sending),
            receiving: ReceivingCipher::new(receiving),
        }
    }
}

fn noise_error(step: &'static str) -> impl FnOnce(snow::Error) -> TransportError {
    move |source| TransportError::Noise { step, source }
}

/// One Noise handshake, driven a message at a time by whatever link is available.
///
/// Only the handshake runs through Noise. The media path then uses its own AEAD with a nonce
/// derived from the packet header, because Noise's transport mode assumes ordered delivery and
/// datagrams are neither ordered nor complete.
#[derive(Debug)]
pub struct NoiseHandshake {
    state: HandshakeState,
    kind: HandshakeKind,
    initiator: bool,
}

impl NoiseHandshake {
    /// Room for the largest message any of the three patterns produces with a Hello payload.
    pub const MAX_MESSAGE_LEN: usize = 512;

    pub fn initiator(
        kind: HandshakeKind,
        identity: &DeviceIdentity,
        responder: Option<&StaticKey>,
        psk: Option<&PresharedKey>,
    ) -> Result<Self, TransportError> {
        let mut builder = snow::Builder::new(kind.noise_params()?)
            .local_private_key(identity.private())
            .map_err(noise_error("loading the local key"))?;
        if let Some(responder) = responder {
            builder = builder
                .remote_public_key(responder.as_bytes())
                .map_err(noise_error("loading the peer key"))?;
        }
        if let Some(psk) = psk {
            builder = builder
                .psk(3, &psk.0)
                .map_err(noise_error("loading the pre-shared key"))?;
        }
        Ok(Self {
            state: builder
                .build_initiator()
                .map_err(noise_error("building the initiator"))?,
            kind,
            initiator: true,
        })
    }

    pub fn responder(
        kind: HandshakeKind,
        identity: &DeviceIdentity,
        psk: Option<&PresharedKey>,
    ) -> Result<Self, TransportError> {
        let mut builder = snow::Builder::new(kind.noise_params()?)
            .local_private_key(identity.private())
            .map_err(noise_error("loading the local key"))?;
        if let Some(psk) = psk {
            builder = builder
                .psk(3, &psk.0)
                .map_err(noise_error("loading the pre-shared key"))?;
        }
        Ok(Self {
            state: builder
                .build_responder()
                .map_err(noise_error("building the responder"))?,
            kind,
            initiator: false,
        })
    }

    pub const fn kind(&self) -> HandshakeKind {
        self.kind
    }

    pub const fn is_initiator(&self) -> bool {
        self.initiator
    }

    pub fn is_complete(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// The peer's static key, once the pattern has revealed it.
    pub fn remote_static(&self) -> Option<StaticKey> {
        self.state
            .get_remote_static()
            .and_then(|bytes| StaticKey::from_slice(bytes).ok())
    }

    /// The transcript hash both sides now share, which binds a verification code to this exact
    /// exchange.
    pub fn handshake_hash(&self) -> [u8; 32] {
        let mut hash = [0u8; 32];
        let transcript = self.state.get_handshake_hash();
        for (slot, byte) in hash.iter_mut().zip(transcript) {
            *slot = *byte;
        }
        hash
    }

    /// Produces the next handshake message, carrying `payload`.
    pub fn write_message(
        &mut self,
        payload: &[u8],
        into: &mut [u8],
    ) -> Result<usize, TransportError> {
        self.state
            .write_message(payload, into)
            .map_err(noise_error("writing a message"))
    }

    /// Consumes the peer's handshake message, returning the payload length written to `payload`.
    pub fn read_message(
        &mut self,
        message: &[u8],
        payload: &mut [u8],
    ) -> Result<usize, TransportError> {
        self.state
            .read_message(message, payload)
            .map_err(noise_error("reading a message"))
    }

    /// Derives the directional keys once every message has been exchanged.
    ///
    /// Exports raw key material rather than using Noise's own transport mode, because that mode
    /// assumes an ordered, complete stream and media datagrams are neither. Taking the split and
    /// running our own AEAD with a header-derived nonce is the same construction WireGuard and
    /// DTLS-SRTP use, and it is what lets a packet be decrypted out of order or not at all.
    ///
    /// Each split key becomes the root of one direction's [`KeySchedule`], from which every
    /// stream and epoch derives its own packet key.
    pub fn finish(mut self) -> Result<SessionKeys, TransportError> {
        if !self.state.is_handshake_finished() {
            return Err(TransportError::HandshakeRejected {
                reason: "the handshake has not finished",
            });
        }
        let (initiator_key, responder_key) = self.state.dangerously_get_raw_split();
        Ok(SessionKeys::from_split(
            initiator_key,
            responder_key,
            self.initiator,
        ))
    }
}
