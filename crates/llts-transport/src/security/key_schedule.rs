use core::time::Duration;

use chacha20poly1305::aead::KeyInit;
use chacha20poly1305::{ChaCha20Poly1305, Key};
use llts_wire::StreamId;
use zeroize::{Zeroize, ZeroizeOnDrop};

const PACKET_KEY_CONTEXT: &str = "crownos.llts.v1 packet key";

/// Which way traffic sealed under a key flows.
///
/// Each direction derives from its own handshake secret, so the two peers never share a counter
/// and neither has to coordinate with the other before sealing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    InitiatorToResponder,
    ResponderToInitiator,
}

impl Direction {
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::InitiatorToResponder => 1,
            Self::ResponderToInitiator => 2,
        }
    }
}

/// Generation of the packet keys, advanced by a rekey and signalled by the `KEY_PHASE` bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyEpoch(u32);

impl KeyEpoch {
    pub const FIRST: Self = Self(0);

    pub const fn get(self) -> u32 {
        self.0
    }

    /// The value the header's `KEY_PHASE` bit carries for this epoch.
    pub const fn phase(self) -> bool {
        self.0 & 1 == 1
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }

    pub const fn previous(self) -> Option<Self> {
        match self.0.checked_sub(1) {
            Some(previous) => Some(Self(previous)),
            None => None,
        }
    }
}

/// One direction's secret from the handshake, from which every packet key is derived.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct TrafficSecret([u8; 32]);

impl TrafficSecret {
    pub const LEN: usize = 32;

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }
}

impl core::fmt::Debug for TrafficSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("TrafficSecret(redacted)")
    }
}

/// Derives a distinct key for every `(direction, stream, epoch)`.
///
/// A key per stream is what lets each stream count its sequences independently: the nonce only
/// has to be unique under one stream's key, so no counter is shared between streams and a
/// stream added mid-session cannot collide with one already running. Rekeying moves every stream
/// to the next epoch together, bounding what any one key ever protects.
#[derive(Debug)]
pub struct KeySchedule {
    secret: TrafficSecret,
    direction: Direction,
}

impl KeySchedule {
    /// A key older than this is retired even if it has sealed little.
    pub const REKEY_INTERVAL: Duration = Duration::from_secs(120);

    /// A key is retired after sealing this many packets, far inside ChaCha20-Poly1305's limits.
    pub const REKEY_PACKET_LIMIT: u64 = 1 << 32;

    /// How long a receiver keeps the previous epoch's keys for packets reordered across a rekey.
    pub const RETIRED_KEY_GRACE: Duration = Duration::from_secs(3);

    pub const fn new(secret: TrafficSecret, direction: Direction) -> Self {
        Self { secret, direction }
    }

    pub const fn direction(&self) -> Direction {
        self.direction
    }

    pub(crate) fn packet_cipher(&self, stream: StreamId, epoch: KeyEpoch) -> ChaCha20Poly1305 {
        let mut hasher = blake3::Hasher::new_derive_key(PACKET_KEY_CONTEXT);
        hasher.update(&self.secret.0);
        hasher.update(&[self.direction.as_byte()]);
        hasher.update(&stream.get().to_le_bytes());
        hasher.update(&epoch.get().to_le_bytes());
        let mut key = *hasher.finalize().as_bytes();
        let cipher = ChaCha20Poly1305::new(&Key::from(key));
        key.zeroize();
        cipher
    }
}
