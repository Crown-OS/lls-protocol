use llts_wire::packet::{BodyPrefix, PacketHeader, RepairBlockGeometry};

use crate::CoreError;

/// How many payload bytes fit in one datagram on the current path.
///
/// The path MTU is discovered by probing; until it is, [`DatagramSizing::CONSERVATIVE_MTU`] is
/// safe on any Wi-Fi or Ethernet link and matches QUIC's own floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatagramSizing {
    mtu: usize,
}

impl DatagramSizing {
    /// Bytes the AEAD appends to every datagram.
    pub const AUTHENTICATION_TAG_LEN: usize = 16;

    /// The size every path is assumed to carry before probing says otherwise.
    pub const CONSERVATIVE_MTU: usize = 1200;

    /// A parity packet carries the block geometry ahead of its shard, so every payload reserves
    /// room for it. Reed-Solomon needs data and parity shards to be the same size, and the
    /// alternative — letting data packets run 14 bytes longer — would make them different. The
    /// body prefix is reserved at its longest, so a retransmission fits wherever its original did.
    const OVERHEAD: usize = PacketHeader::ENCODED_LEN
        + BodyPrefix::MAX_ENCODED_LEN
        + RepairBlockGeometry::ENCODED_LEN
        + Self::AUTHENTICATION_TAG_LEN;

    /// Two payload bytes is the floor: fragmentation must terminate, and a Reed-Solomon shard
    /// size has to be non-zero and even.
    pub const MIN_MTU: usize = Self::OVERHEAD + 2;

    pub const fn new(mtu: usize) -> Result<Self, CoreError> {
        if mtu < Self::MIN_MTU {
            return Err(CoreError::PayloadExceedsMtu {
                payload_len: Self::MIN_MTU,
                mtu,
            });
        }
        Ok(Self { mtu })
    }

    pub const fn conservative() -> Self {
        Self {
            mtu: Self::CONSERVATIVE_MTU,
        }
    }

    pub const fn mtu(self) -> usize {
        self.mtu
    }

    /// Rounded down to an even number of bytes, because a repair block's shards must all be the
    /// same even size and a shard is exactly one full payload.
    pub const fn max_payload_len(self) -> usize {
        (self.mtu - Self::OVERHEAD) & !1
    }

    /// Fragments a payload of `byte_len` will occupy. A zero-length payload still costs one,
    /// since an empty slice must still carry its flags.
    pub const fn fragment_count(self, byte_len: usize) -> usize {
        if byte_len == 0 {
            return 1;
        }
        byte_len.div_ceil(self.max_payload_len())
    }
}
