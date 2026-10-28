use serde::{Deserialize, Serialize};

/// What kind of hardware a device is, matching the `crownconnect` IPC `DeviceClass`.
///
/// The discriminants are what discovery beacons carry, so they never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DeviceClass {
    Phone = 1,
    Tablet = 2,
    Watch = 3,
    Computer = 4,
}

impl DeviceClass {
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Phone),
            2 => Some(Self::Tablet),
            3 => Some(Self::Watch),
            4 => Some(Self::Computer),
            _ => None,
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}
