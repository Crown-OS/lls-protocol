//! Where the pairing flows stand: an open window, attempts this device started, and codes
//! waiting for the user.

use std::net::SocketAddr;

use llts_core::clock::Instant;
use llts_signaling::device::DeviceId;
use llts_signaling::discovery::PairingNonce;

/// Pairing requests one window accepts before the oldest is dropped.
const MAX_PENDING_CODES: usize = 4;
/// Pairing beacons remembered, so repeated sightings of one window are announced once.
const MAX_PAIRABLE: usize = 8;

/// The user has this device in pairing mode until `expires_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    pub(crate) expires_at: Instant,
    pub(crate) nonce: PairingNonce,
}

/// A pairing this device started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// Scanned a QR offer, so the peer's identity is known from the start.
    Qr {
        peer: DeviceId,
        address: SocketAddr,
        until: Instant,
    },
    Nearby {
        address: SocketAddr,
        until: Instant,
    },
}

impl Attempt {
    pub(crate) const fn until(self) -> Instant {
        match self {
            Self::Qr { until, .. } | Self::Nearby { until, .. } => until,
        }
    }

    pub(crate) const fn address(self) -> SocketAddr {
        match self {
            Self::Qr { address, .. } | Self::Nearby { address, .. } => address,
        }
    }

    pub(crate) const fn peer(self) -> Option<DeviceId> {
        match self {
            Self::Qr { peer, .. } => Some(peer),
            Self::Nearby { .. } => None,
        }
    }
}

/// A nearby pairing whose code is on screen, waiting for the user until `until`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingCode {
    pub(crate) peer: DeviceId,
    pub(crate) until: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct Pairing {
    pub(crate) window: Option<Window>,
    pub(crate) attempt: Option<Attempt>,
    pub(crate) codes: heapless::Vec<PendingCode, MAX_PENDING_CODES>,
    /// Peers paired since their session last came up; their first session gets extra care.
    pub(crate) just_paired: heapless::Vec<DeviceId, MAX_PENDING_CODES>,
    pairable: heapless::Vec<PairingNonce, MAX_PAIRABLE>,
}

impl Pairing {
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let window = self.window.map(|window| window.expires_at);
        let attempt = self.attempt.map(Attempt::until);
        let codes = self.codes.iter().map(|code| code.until);
        window.into_iter().chain(attempt).chain(codes).min()
    }

    pub(crate) fn add_code(&mut self, code: PendingCode) {
        self.take_code(&code.peer);
        if self.codes.is_full() {
            self.codes.remove(0);
        }
        let _ = self.codes.push(code);
    }

    pub(crate) fn take_code(&mut self, peer: &DeviceId) -> Option<PendingCode> {
        let position = self.codes.iter().position(|code| code.peer == *peer)?;
        Some(self.codes.remove(position))
    }

    pub(crate) fn mark_just_paired(&mut self, peer: DeviceId) {
        if self.just_paired.contains(&peer) {
            return;
        }
        if self.just_paired.is_full() {
            self.just_paired.remove(0);
        }
        let _ = self.just_paired.push(peer);
    }

    /// Whether `peer` paired and its first session has not come up yet; the transport
    /// announces that session right after the pairing, so it needs no dial.
    pub(crate) fn is_just_paired(&self, peer: &DeviceId) -> bool {
        self.just_paired.contains(peer)
    }

    pub(crate) fn take_just_paired(&mut self, peer: &DeviceId) -> bool {
        let before = self.just_paired.len();
        self.just_paired.retain(|held| held != peer);
        before != self.just_paired.len()
    }

    /// Records a pairing beacon; `true` the first time this window is seen.
    pub(crate) fn note_pairable(&mut self, nonce: PairingNonce) -> bool {
        if self.pairable.contains(&nonce) {
            return false;
        }
        if self.pairable.is_full() {
            self.pairable.remove(0);
        }
        let _ = self.pairable.push(nonce);
        true
    }

    pub(crate) fn forget_pairable(&mut self, nonce: PairingNonce) {
        self.pairable.retain(|held| *held != nonce);
    }

    pub(crate) fn clear_pairable(&mut self) {
        self.pairable.clear();
    }
}
