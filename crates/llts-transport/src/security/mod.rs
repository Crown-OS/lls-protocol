//! Pairing, reconnection and the per-packet crypto they key.

mod handshake;
mod hello;
mod identity;
mod key_schedule;
mod nonce;
mod pairing;
mod receiving_cipher;
mod reconnect;
mod replay_window;
mod sealed_datagram;
mod sending_cipher;
mod sequence_extender;
mod verification_code;

pub use handshake::{HandshakeKind, NoiseHandshake, PresharedKey, SessionKeys};
pub use hello::{DeviceClass, Hello};
pub use identity::{DeviceIdentity, StaticKey};
pub use key_schedule::{Direction, KeyEpoch, KeySchedule, TrafficSecret};
pub use nonce::{NONCE_LEN, packet_nonce};
pub use pairing::{NearbyInitiator, NearbyResponder, PairingOutcome, QrInitiator, QrResponder};
pub use receiving_cipher::ReceivingCipher;
pub use reconnect::{AcceptedReconnect, Established, ReconnectInitiator, ReconnectResponder};
pub use replay_window::ReplayWindow;
pub use sealed_datagram::SealedDatagram;
pub use sending_cipher::{SendingCipher, TAG_LEN};
pub use sequence_extender::SequenceExtender;
pub use verification_code::VerificationCode;

/// Streams one direction of a session may seal or open. A peer that invents more is refused
/// rather than allowed to grow the tables.
pub const MAX_STREAMS_PER_DIRECTION: usize = 16;
