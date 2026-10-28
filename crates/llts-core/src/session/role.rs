/// Which halves of the link a session drives.
///
/// Sender and receiver are runtime roles rather than separate builds, because the same device is
/// a display sink in one direction and a camera source in the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Sender,
    Receiver,
    Bidirectional,
}

impl Role {
    pub const fn sends(self) -> bool {
        matches!(self, Self::Sender | Self::Bidirectional)
    }

    pub const fn receives(self) -> bool {
        matches!(self, Self::Receiver | Self::Bidirectional)
    }
}
