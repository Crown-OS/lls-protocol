use chacha20poly1305::Nonce;
use llts_wire::packet::SequenceSpace;

/// Bytes in a ChaCha20-Poly1305 nonce.
pub const NONCE_LEN: usize = 12;

/// `space:u8 ‖ 0:u24 ‖ counter:u64`.
///
/// Nothing here travels on the wire: the receiver rebuilds the space from the clear header and
/// the counter from the header's sequence, so a packet costs only its tag in crypto overhead.
pub const fn packet_nonce(space: SequenceSpace, counter: u64) -> [u8; NONCE_LEN] {
    let counter = counter.to_le_bytes();
    [
        space.as_byte(),
        0,
        0,
        0,
        counter[0],
        counter[1],
        counter[2],
        counter[3],
        counter[4],
        counter[5],
        counter[6],
        counter[7],
    ]
}

pub(crate) fn chacha_nonce(space: SequenceSpace, counter: u64) -> Nonce {
    Nonce::from(packet_nonce(space, counter))
}
