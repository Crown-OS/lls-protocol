use llts_core::link::LinkKind;

use crate::TransportError;

/// What a link can tell the engine about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkProperties {
    pub kind: LinkKind,
    pub max_datagram_size: usize,
}

/// A path that carries datagrams between the two peers.
///
/// Deliberately blocking-free rather than async: the engine is sans-I/O and drives itself from a
/// single thread, so a non-blocking socket plus an explicit poll is both simpler and one fewer
/// scheduling hop than an async runtime would add.
pub trait PacketLink {
    fn properties(&self) -> LinkProperties;

    fn send(&mut self, datagram: &[u8]) -> Result<(), TransportError>;

    /// Reads one datagram, returning `None` when none is waiting.
    fn recv(&mut self, into: &mut [u8]) -> Result<Option<usize>, TransportError>;
}
