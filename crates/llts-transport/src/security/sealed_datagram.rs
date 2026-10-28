use llts_wire::StreamId;
use llts_wire::packet::SequenceSpace;

use super::KeyEpoch;
use super::nonce::{NONCE_LEN, packet_nonce};

/// What sealing one datagram used, for a caller that audits or logs the crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealedDatagram {
    /// Header, body and tag.
    pub len: usize,
    pub stream: StreamId,
    pub epoch: KeyEpoch,
    pub space: SequenceSpace,
    pub counter: u64,
}

impl SealedDatagram {
    /// The nonce this datagram was sealed under. Unique per `(stream, epoch)` by construction.
    pub const fn nonce(&self) -> [u8; NONCE_LEN] {
        packet_nonce(self.space, self.counter)
    }
}
