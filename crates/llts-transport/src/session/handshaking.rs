use std::net::SocketAddr;
use std::time::Duration;

use llts_core::clock::Instant;
use llts_wire::SessionTag;

use super::connection::Resend;
use crate::security::{
    NearbyInitiator, NearbyResponder, QrInitiator, QrResponder, ReconnectInitiator, StaticKey,
};

/// A handshake that has sent its latest message and waits for the next.
#[derive(Debug)]
pub(crate) enum InProgress {
    Reconnect {
        initiator: ReconnectInitiator,
        peer: StaticKey,
    },
    Nearby(NearbyInitiator),
    NearbyAnswer(NearbyResponder),
    Qr(QrInitiator),
    QrAnswer(QrResponder),
}

impl InProgress {
    /// Whether this side sent the first message, and so resends until answered. A responder
    /// only answers, and forgets the attempt once it has been idle too long.
    const fn is_initiator(&self) -> bool {
        matches!(self, Self::Reconnect { .. } | Self::Nearby(_) | Self::Qr(_))
    }

    pub(crate) const fn reconnecting_to(&self) -> Option<&StaticKey> {
        match self {
            Self::Reconnect { peer, .. } => Some(peer),
            _ => None,
        }
    }
}

/// A handshake in flight, keyed by the session tag this side announced for it.
#[derive(Debug)]
pub(crate) struct PendingHandshake {
    pub(crate) local_tag: SessionTag,
    pub(crate) state: InProgress,
    pub(crate) message: Resend,
    /// For a responder, the initiator's tag, so a resent first message is recognised.
    pub(crate) answering: Option<SessionTag>,
    pub(crate) expires_at: Instant,
}

impl PendingHandshake {
    const RESPONDER_PATIENCE: Duration = Duration::from_secs(10);

    pub(crate) fn new(
        local_tag: SessionTag,
        state: InProgress,
        bytes: Vec<u8>,
        to: SocketAddr,
        answering: Option<SessionTag>,
        now: Instant,
    ) -> Self {
        let patience = if state.is_initiator() {
            Resend::INTERVAL.saturating_mul(u32::from(Resend::MAX_ATTEMPTS) + 1)
        } else {
            Self::RESPONDER_PATIENCE
        };
        let mut message = Resend::new(bytes, to, now);
        if !state.is_initiator() {
            message.attempts = Resend::MAX_ATTEMPTS;
        }
        Self {
            local_tag,
            state,
            message,
            answering,
            expires_at: now.saturating_add(patience),
        }
    }

    pub(crate) const fn address(&self) -> SocketAddr {
        self.message.to
    }

    pub(crate) fn next_deadline(&self) -> Instant {
        if self.message.attempts < Resend::MAX_ATTEMPTS {
            self.message.next_at.min(self.expires_at)
        } else {
            self.expires_at
        }
    }
}
