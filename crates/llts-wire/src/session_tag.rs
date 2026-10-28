/// The receiver-chosen half of every [`crate::StreamId`] a peer sends to this device.
///
/// Each side picks the tag its peer must stamp on every datagram, so one socket can serve every
/// peer and route an arriving datagram to its session with a single table lookup, before any
/// crypto work. The tag sits inside the authenticated header, so a peer cannot claim another
/// session's tag without that session's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionTag(u16);

impl SessionTag {
    /// Carried by the first handshake message, which is sent before the responder has chosen one.
    pub const UNASSIGNED: Self = Self(0);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn is_assigned(self) -> bool {
        self.0 != Self::UNASSIGNED.0
    }
}

impl From<u16> for SessionTag {
    fn from(value: u16) -> Self {
        Self(value)
    }
}
