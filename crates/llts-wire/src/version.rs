use crate::WireError;

/// Wire-format revision, carried in the high nibble of the second header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(u8);

impl ProtocolVersion {
    /// Version this build produces and is willing to parse.
    pub const CURRENT: Self = Self(1);

    const MAX_ENCODABLE: u8 = 0x0F;

    pub const fn new(value: u8) -> Option<Self> {
        if value <= Self::MAX_ENCODABLE {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Both endpoints must agree exactly until the format is stable enough to define a
    /// forward-compatible subset.
    pub const fn accepts(self, peer: Self) -> bool {
        self.0 == peer.0
    }

    pub(crate) const fn check_supported(self) -> Result<Self, WireError> {
        if Self::CURRENT.accepts(self) {
            Ok(self)
        } else {
            Err(WireError::UnsupportedVersion {
                peer: self.0,
                local: Self::CURRENT.0,
            })
        }
    }
}
