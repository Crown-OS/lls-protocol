use crate::discovery::{Advertisement, DiscoveryError, PAIRING_BEACON_LEN, PairingBeacon};

/// 16-bit BLE service UUID llts beacons are published under.
///
/// Deliberately not ffsp's `0xFCF1`, so a scanner for one protocol never has to parse the other's
/// beacons. A collision with an unrelated assignment is harmless: the version and kind bytes, and
/// for linked devices the ephemeral identifier, all have to check out first.
pub const SERVICE_UUID_16: u16 = 0xFCF2;

/// Layout version in the first byte of every beacon.
pub const BEACON_VERSION: u8 = 1;

pub(crate) const LINKED_KIND: u8 = 1;
pub(crate) const PAIRING_KIND: u8 = 2;

/// Either beacon a device can be advertising, told apart by the kind byte after the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beacon {
    Linked(Advertisement),
    Pairing(PairingBeacon),
}

impl Beacon {
    /// # Errors
    ///
    /// Returns a [`DiscoveryError`] for a beacon of an unknown version, kind or length.
    pub fn parse(bytes: &[u8]) -> Result<Self, DiscoveryError> {
        match bytes {
            [_, LINKED_KIND, ..] => Advertisement::parse(bytes).map(Self::Linked),
            [_, PAIRING_KIND, ..] => PairingBeacon::parse(bytes).map(Self::Pairing),
            [BEACON_VERSION, kind, ..] => Err(DiscoveryError::UnknownKind(*kind)),
            [version, _, ..] => Err(DiscoveryError::UnsupportedVersion(*version)),
            _ => Err(DiscoveryError::Length {
                expected: PAIRING_BEACON_LEN,
                actual: bytes.len(),
            }),
        }
    }

    pub fn to_hex(&self) -> String {
        match self {
            Self::Linked(advertisement) => {
                data_encoding::HEXLOWER.encode(&advertisement.to_bytes())
            }
            Self::Pairing(pairing) => data_encoding::HEXLOWER.encode(&pairing.to_bytes()),
        }
    }
}

pub(crate) const fn check_version(version: u8) -> Result<(), DiscoveryError> {
    if version == BEACON_VERSION {
        Ok(())
    } else {
        Err(DiscoveryError::UnsupportedVersion(version))
    }
}
