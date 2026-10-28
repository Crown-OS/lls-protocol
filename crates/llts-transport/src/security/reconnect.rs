use super::{DeviceIdentity, HandshakeKind, Hello, NoiseHandshake, SessionKeys, StaticKey};
use crate::TransportError;
use crate::util::tai64n::Tai64N;

/// A reconnect handshake that has produced keys.
#[derive(Debug)]
pub struct Established {
    pub keys: SessionKeys,
    pub peer: StaticKey,
    pub peer_hello: Hello,
}

fn payload_of(payload: &[u8], len: usize) -> &[u8] {
    payload.get(..len).unwrap_or_default()
}

/// Reconnecting to a device already paired, over `Noise_IK`: one round trip.
///
/// Message one carries a TAI64N timestamp ahead of the Hello. The responder accepts a first
/// message only if its timestamp is newer than the last one it accepted from that peer, which
/// makes a captured first message worthless to replay: IK's first message is otherwise valid
/// forever, since it depends on nothing the responder chose.
#[derive(Debug)]
pub struct ReconnectInitiator {
    noise: NoiseHandshake,
}

impl ReconnectInitiator {
    pub fn start(
        identity: &DeviceIdentity,
        peer: &StaticKey,
        hello: &Hello,
        timestamp: Tai64N,
        out: &mut [u8],
    ) -> Result<(Self, usize), TransportError> {
        let mut noise =
            NoiseHandshake::initiator(HandshakeKind::Reconnect, identity, Some(peer), None)?;
        let mut payload = [0u8; Tai64N::LEN + Hello::MAX_ENCODED_LEN];
        let (stamp, rest) = payload.split_at_mut(Tai64N::LEN);
        stamp.copy_from_slice(&timestamp.to_bytes());
        let hello_len = hello.encode_into(rest)?;
        let len = noise.write_message(payload_of(&payload, Tai64N::LEN + hello_len), out)?;
        Ok((Self { noise }, len))
    }

    pub fn on_response(mut self, message: &[u8]) -> Result<Established, TransportError> {
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = self.noise.read_message(message, &mut payload)?;
        let peer_hello = Hello::decode(payload_of(&payload, payload_len))?;
        let peer = self
            .noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the responder's identity is unknown",
            })?;
        Ok(Established {
            keys: self.noise.finish()?,
            peer,
            peer_hello,
        })
    }
}

/// A reconnect accepted by the responder: keys, and the timestamp that must be remembered once
/// the session proves itself with a first authenticated packet.
#[derive(Debug)]
pub struct AcceptedReconnect {
    pub established: Established,
    pub timestamp: Tai64N,
}

/// Answers a [`ReconnectInitiator`].
#[derive(Debug)]
pub struct ReconnectResponder;

impl ReconnectResponder {
    /// Reads message one and, if `authorize` accepts the peer and its timestamp, writes message
    /// two into `out`.
    ///
    /// `authorize` sees the initiator's static key and timestamp before anything is sent back,
    /// and is where the trust store and the per-peer timestamp check live.
    pub fn accept(
        identity: &DeviceIdentity,
        hello: &Hello,
        message: &[u8],
        out: &mut [u8],
        authorize: impl FnOnce(&StaticKey, Tai64N) -> Result<(), TransportError>,
    ) -> Result<(AcceptedReconnect, usize), TransportError> {
        let mut noise = NoiseHandshake::responder(HandshakeKind::Reconnect, identity, None)?;
        let mut payload = [0u8; NoiseHandshake::MAX_MESSAGE_LEN];
        let payload_len = noise.read_message(message, &mut payload)?;
        let payload = payload_of(&payload, payload_len);
        let (stamp, hello_bytes) =
            payload
                .split_at_checked(Tai64N::LEN)
                .ok_or(TransportError::HandshakeRejected {
                    reason: "the reconnect carried no timestamp",
                })?;
        let timestamp = Tai64N::from_bytes(<[u8; Tai64N::LEN]>::try_from(stamp).map_err(|_| {
            TransportError::HandshakeRejected {
                reason: "the reconnect carried no timestamp",
            }
        })?);
        let peer = noise
            .remote_static()
            .ok_or(TransportError::HandshakeRejected {
                reason: "the initiator did not reveal its identity",
            })?;
        authorize(&peer, timestamp)?;
        let peer_hello = Hello::decode(hello_bytes)?;

        let mut encoded = [0u8; Hello::MAX_ENCODED_LEN];
        let hello_len = hello.encode_into(&mut encoded)?;
        let len = noise.write_message(payload_of(&encoded, hello_len), out)?;
        Ok((
            AcceptedReconnect {
                established: Established {
                    keys: noise.finish()?,
                    peer,
                    peer_hello,
                },
                timestamp,
            },
            len,
        ))
    }
}
