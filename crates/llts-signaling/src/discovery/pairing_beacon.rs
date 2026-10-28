use bitflags::bitflags;

use crate::device::DeviceClass;
use crate::discovery::beacon::{PAIRING_KIND, check_version};
use crate::discovery::{BEACON_VERSION, DiscoveryError};

/// Bytes of a pairing beacon: `version ‖ kind ‖ class ‖ flags ‖ nonce[8]`.
pub const PAIRING_BEACON_LEN: usize = 12;

bitflags! {
    /// The pairing methods a device in pairing mode is accepting.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PairingFlags: u8 {
        /// Noise XX confirmed by a six-digit code shown on both screens.
        const NEARBY_CODE = 1 << 0;
        /// Noise XXpsk3 keyed by a QR code this device is showing.
        const QR = 1 << 1;
    }
}

/// A fresh random value per pairing window.
///
/// It lets a scanner fold repeated BLE and mDNS sightings of one pairing window into one entry,
/// and it changes with every window, so it identifies the window rather than the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PairingNonce(pub [u8; 8]);

impl PairingNonce {
    /// # Errors
    ///
    /// Returns [`DiscoveryError::Entropy`] if the operating system has no randomness to give.
    pub fn random() -> Result<Self, DiscoveryError> {
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce)?;
        Ok(Self(nonce))
    }
}

/// What a device advertises while the user has it in pairing mode.
///
/// It carries no identity: an unpaired scanner learns only that some phone or computer nearby
/// is willing to pair, and the identity is proven inside the Noise handshake that follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PairingBeacon {
    pub device_class: DeviceClass,
    pub flags: PairingFlags,
    pub nonce: PairingNonce,
}

impl PairingBeacon {
    pub const fn to_bytes(&self) -> [u8; PAIRING_BEACON_LEN] {
        let mut out = [0; PAIRING_BEACON_LEN];
        let [version, kind, class, flags, nonce @ ..] = &mut out;
        *version = BEACON_VERSION;
        *kind = PAIRING_KIND;
        *class = self.device_class.as_byte();
        *flags = self.flags.bits();
        *nonce = self.nonce.0;
        out
    }

    /// Unknown flag bits are dropped rather than rejected, so a newer pairing method does not hide
    /// the device from older scanners that support the others.
    ///
    /// # Errors
    ///
    /// Returns a [`DiscoveryError`] for the wrong length, version, kind or device class.
    pub fn parse(bytes: &[u8]) -> Result<Self, DiscoveryError> {
        let bytes: &[u8; PAIRING_BEACON_LEN] =
            bytes.try_into().map_err(|_| DiscoveryError::Length {
                expected: PAIRING_BEACON_LEN,
                actual: bytes.len(),
            })?;
        let &[version, kind, class, flags, nonce @ ..] = bytes;
        check_version(version)?;
        if kind != PAIRING_KIND {
            return Err(DiscoveryError::UnknownKind(kind));
        }
        Ok(Self {
            device_class: DeviceClass::from_byte(class)
                .ok_or(DiscoveryError::UnknownDeviceClass(class))?,
            flags: PairingFlags::from_bits_truncate(flags),
            nonce: PairingNonce(nonce),
        })
    }
}
