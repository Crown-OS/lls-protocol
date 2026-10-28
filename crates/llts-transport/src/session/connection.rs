use std::net::SocketAddr;
use std::time::Duration;

use llts_core::clock::Instant;
use llts_core::session::{PathId, PeerSession};
use llts_wire::SessionTag;

use crate::Tai64N;
use crate::security::{Hello, SessionKeys, StaticKey};

/// A handshake message resent until the peer shows it arrived.
#[derive(Debug)]
pub(crate) struct Resend {
    pub(crate) bytes: Vec<u8>,
    pub(crate) to: SocketAddr,
    pub(crate) next_at: Instant,
    pub(crate) attempts: u8,
}

impl Resend {
    pub(crate) const INTERVAL: Duration = Duration::from_millis(250);
    pub(crate) const MAX_ATTEMPTS: u8 = 12;

    pub(crate) const fn new(bytes: Vec<u8>, to: SocketAddr, now: Instant) -> Self {
        Self {
            bytes,
            to,
            next_at: now.saturating_add(Self::INTERVAL),
            attempts: 0,
        }
    }

    /// Whether to send again now, advancing the schedule if so.
    pub(crate) const fn take_due(&mut self, now: Instant) -> bool {
        if self.next_at.is_after(now) || self.attempts >= Self::MAX_ATTEMPTS {
            return false;
        }
        self.attempts += 1;
        self.next_at = now.saturating_add(Self::INTERVAL);
        true
    }
}

/// The reply a responder sent to a first message, kept so a retransmitted first message gets
/// the same reply instead of a second session.
#[derive(Debug)]
pub(crate) struct CachedReply {
    pub(crate) from: SocketAddr,
    pub(crate) sender_tag: SessionTag,
    pub(crate) bytes: Vec<u8>,
}

/// Where a session stands in proving itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Commitment {
    /// Carrying traffic.
    Committed,
    /// A reconnect answered but not yet proven: it replaces nothing until the initiator's first
    /// authenticated packet arrives, so a replayed or abandoned first message costs nothing.
    AwaitingFirstPacket(Tai64N),
}

/// One established session: its keys, its sans-I/O core and the addresses it has used.
#[derive(Debug)]
pub(crate) struct Connection {
    pub(crate) peer: StaticKey,
    pub(crate) peer_hello: Hello,
    pub(crate) local_tag: SessionTag,
    pub(crate) keys: SessionKeys,
    pub(crate) core: PeerSession,
    pub(crate) commitment: Commitment,
    /// Nearby pairing that the user has not confirmed yet.
    pub(crate) awaiting_confirmation: bool,
    pub(crate) resend: Option<Resend>,
    pub(crate) reply: Option<CachedReply>,
    paths: Vec<SocketAddr>,
}

impl Connection {
    /// Addresses remembered per session; a peer roaming further than this reuses the oldest.
    const MAX_PATHS: usize = 4;

    #[allow(
        clippy::too_many_arguments,
        reason = "every field is required and distinct; a builder would only restate them"
    )]
    pub(crate) fn new(
        peer: StaticKey,
        peer_hello: Hello,
        local_tag: SessionTag,
        keys: SessionKeys,
        core: PeerSession,
        commitment: Commitment,
        first_path: SocketAddr,
    ) -> Self {
        Self {
            peer,
            peer_hello,
            local_tag,
            keys,
            core,
            commitment,
            awaiting_confirmation: false,
            resend: None,
            reply: None,
            paths: vec![first_path],
        }
    }

    /// The path an authenticated datagram from `address` arrived on, remembering it if new.
    pub(crate) fn path_for(&mut self, address: SocketAddr) -> PathId {
        if let Some(position) = self.paths.iter().position(|known| *known == address) {
            return PathId(u8::try_from(position).unwrap_or(0));
        }
        if self.paths.len() >= Self::MAX_PATHS {
            let active = usize::from(self.core.active_path().0);
            let evict = (0..self.paths.len())
                .find(|index| *index != active)
                .unwrap_or(0);
            if let Some(slot) = self.paths.get_mut(evict) {
                *slot = address;
            }
            return PathId(u8::try_from(evict).unwrap_or(0));
        }
        self.paths.push(address);
        PathId(u8::try_from(self.paths.len() - 1).unwrap_or(0))
    }

    pub(crate) fn address_of(&self, path: PathId) -> Option<SocketAddr> {
        self.paths.get(usize::from(path.0)).copied()
    }
}
