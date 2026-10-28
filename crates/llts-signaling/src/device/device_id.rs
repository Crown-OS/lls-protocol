use core::fmt;

use serde::{Deserialize, Serialize};

/// A device's long-term X25519 public key, which is also its identity on every CrownOS link.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 32]);

impl fmt::Debug for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "DeviceId({self})")
    }
}

/// Renders the leading four bytes, which is enough to tell devices apart in a log.
impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .take(4)
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}
