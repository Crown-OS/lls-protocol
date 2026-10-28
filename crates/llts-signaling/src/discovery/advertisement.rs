use crate::device::DeviceClass;
use crate::discovery::beacon::{LINKED_KIND, check_version};
use crate::discovery::{BEACON_VERSION, DiscoveryError, EphemeralId, Epoch};

/// Bytes of service data a legacy BLE advertisement can carry.
///
/// A legacy advertising PDU is 31 bytes: three for the flags structure and four for a service-data
/// header carrying a **16-bit** UUID, leaving exactly 24.
pub const ADVERTISEMENT_LEN: usize = 24;

/// What a linked device broadcasts so its peers can recognise it.
///
/// Layout: `version ‖ kind ‖ class ‖ ephemeral_id[16] ‖ epoch_low ‖ reserved[4]`. The reserved
/// bytes are sent as zero and ignored on receipt. The device name is deliberately absent, since
/// anyone in radio range can read this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advertisement {
    pub device_class: DeviceClass,
    pub ephemeral_id: EphemeralId,
    pub epoch_low: u8,
}

impl Advertisement {
    pub const fn new(device_class: DeviceClass, ephemeral_id: EphemeralId, epoch: Epoch) -> Self {
        Self {
            device_class,
            ephemeral_id,
            epoch_low: epoch.low_byte(),
        }
    }

    /// Returns a fixed-size array, so overrunning the 24-byte budget cannot compile.
    ///
    /// BlueZ truncates over-long service data silently, which would otherwise produce a beacon
    /// that looks correct and resolves to nothing.
    pub const fn to_bytes(&self) -> [u8; ADVERTISEMENT_LEN] {
        let mut out = [0; ADVERTISEMENT_LEN];
        let [version, kind, class, id @ .., epoch_low, _, _, _, _] = &mut out;
        *version = BEACON_VERSION;
        *kind = LINKED_KIND;
        *class = self.device_class.as_byte();
        *id = *self.ephemeral_id.as_bytes();
        *epoch_low = self.epoch_low;
        out
    }

    /// # Errors
    ///
    /// Returns a [`DiscoveryError`] for the wrong length, version, kind or device class.
    pub fn parse(bytes: &[u8]) -> Result<Self, DiscoveryError> {
        let bytes: &[u8; ADVERTISEMENT_LEN] =
            bytes.try_into().map_err(|_| DiscoveryError::Length {
                expected: ADVERTISEMENT_LEN,
                actual: bytes.len(),
            })?;
        let &[version, kind, class, ref id @ .., epoch_low, _, _, _, _] = bytes;
        check_version(version)?;
        if kind != LINKED_KIND {
            return Err(DiscoveryError::UnknownKind(kind));
        }
        Ok(Self {
            device_class: DeviceClass::from_byte(class)
                .ok_or(DiscoveryError::UnknownDeviceClass(class))?,
            ephemeral_id: EphemeralId::from_bytes(*id),
            epoch_low,
        })
    }
}
