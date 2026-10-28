use core::time::Duration;

use llts_core::clock::Instant;

use crate::security::PresharedKey;

/// Which pairing requests are being accepted right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingMode {
    /// Nearby pairing, confirmed by comparing codes.
    Nearby,
    /// QR pairing with this one-time key, and nearby pairing alongside it.
    Qr(PresharedKey),
}

/// Gates pairing: a device only answers a first-contact handshake while the user has asked it
/// to, and only for as long as they asked.
///
/// Without the gate any device in radio range could make this one run an X25519 handshake on
/// demand and put a pairing prompt on screen.
#[derive(Debug, Default)]
pub struct PairingWindow {
    open: Option<(PairingMode, Instant)>,
}

impl PairingWindow {
    pub const fn new() -> Self {
        Self { open: None }
    }

    pub fn open(&mut self, mode: PairingMode, now: Instant, duration: Duration) {
        self.open = Some((mode, now.saturating_add(duration)));
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// The mode in force at `now`, closing the window if it has lapsed.
    pub fn mode(&mut self, now: Instant) -> Option<&PairingMode> {
        if self
            .open
            .as_ref()
            .is_some_and(|(_, until)| !until.is_after(now))
        {
            self.open = None;
        }
        self.open.as_ref().map(|(mode, _)| mode)
    }
}
