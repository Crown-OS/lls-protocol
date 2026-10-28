//! The pairing flows: QR offers, nearby codes, and forgetting a device again.

use core::time::Duration;
use std::net::SocketAddr;

use llts_signaling::device::DeviceId;
use llts_signaling::discovery::PairingNonce;
use llts_signaling::message::{Goodbye, GoodbyeReason, Signal};
use llts_transport::canonical_address;
use llts_transport::pairing::QrOffer;

use crate::output::{AppEvent, DriverCommand, PersistRequest};
use crate::pairing::{Attempt, PendingCode, Window};
use crate::peer::MAX_PEERS;
use crate::util::device::device_id;
use crate::util::stream::CONTROL;
use crate::{Node, NodeError, NodeOutput};

impl Node {
    pub(super) fn begin_pairing(&mut self, duration: Option<Duration>) -> Result<(), NodeError> {
        let duration = duration.unwrap_or(self.config.pairing_window);
        self.pairing.window = Some(Window {
            expires_at: self.now.instant.saturating_add(duration),
            nonce: PairingNonce::random()?,
        });
        self.pairing.clear_pairable();
        self.command(DriverCommand::OfferQrPairing {
            addresses: self.addresses.to_vec(),
            duration,
        });
        self.refresh_advertising();
        Ok(())
    }

    pub(super) fn on_qr_offer(&mut self, offer: &QrOffer) -> Result<(), NodeError> {
        self.emit(AppEvent::PairingOffer {
            uri: offer.to_uri()?,
            expires_unix_ms: offer.expires_at_unix.saturating_mul(1_000),
        });
        Ok(())
    }

    pub(super) fn cancel_pairing(&mut self) {
        if self.pairing.window.take().is_some() {
            self.command(DriverCommand::ClosePairing);
            self.emit(AppEvent::PairingWindowClosed);
            self.refresh_advertising();
        }
    }

    pub(super) fn pair_with_qr(&mut self, uri: &str) -> Result<(), NodeError> {
        let offer = QrOffer::parse(uri, self.now.unix_seconds())?;
        let address = offer
            .addresses
            .first()
            .copied()
            .map(canonical_address)
            .ok_or(llts_transport::TransportError::InvalidPairingOffer(
                "no address",
            ))?;
        self.pairing.attempt = Some(Attempt::Qr {
            peer: device_id(offer.static_key),
            address,
            until: self.attempt_deadline(),
        });
        self.command(DriverCommand::PairWithOffer {
            offer: Box::new(offer),
        });
        Ok(())
    }

    pub(super) fn pair_nearby(&mut self, address: SocketAddr) {
        let address = canonical_address(address);
        self.pairing.attempt = Some(Attempt::Nearby {
            address,
            until: self.attempt_deadline(),
        });
        self.command(DriverCommand::PairNearby { address });
    }

    /// A pairing handshake gets two dial timeouts: it has a third message and a person to wait
    /// for on the far side's window.
    const fn attempt_deadline(&self) -> llts_core::clock::Instant {
        self.now
            .instant
            .saturating_add(self.config.dial_timeout.saturating_mul(2))
    }

    pub(super) fn on_pairing_code(&mut self, peer: DeviceId, name: &str, code: u32) {
        if matches!(self.pairing.attempt, Some(Attempt::Nearby { .. })) {
            self.pairing.attempt = None;
        }
        self.pairing.add_code(PendingCode {
            peer,
            until: self
                .now
                .instant
                .saturating_add(self.config.code_confirmation),
        });
        self.emit(AppEvent::PairingRequest {
            peer,
            name: name.to_owned(),
            code,
        });
    }

    pub(super) fn confirm_pairing(
        &mut self,
        peer: DeviceId,
        accept: bool,
    ) -> Result<(), NodeError> {
        self.pairing
            .take_code(&peer)
            .ok_or(NodeError::UnknownPeer(peer))?;
        if accept {
            self.command(DriverCommand::ConfirmPairing { peer });
        } else {
            self.reject_pairing(peer);
        }
        Ok(())
    }

    fn reject_pairing(&mut self, peer: DeviceId) {
        self.command(DriverCommand::RejectPairing { peer });
        self.emit(AppEvent::PairingResult {
            peer: Some(peer),
            accepted: false,
        });
    }

    /// The transport trusts `peer` now: remember it, close the window it came through, and
    /// start advertising to it.
    pub(super) fn on_paired(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        let _ = self.pairing.take_code(&peer);
        if self
            .pairing
            .attempt
            .is_some_and(|attempt| attempt.peer().is_none_or(|target| target == peer))
        {
            self.pairing.attempt = None;
        }
        self.adopt(peer)?;
        self.pairing.mark_just_paired(peer);
        if self.pairing.window.take().is_some() {
            self.command(DriverCommand::ClosePairing);
            self.emit(AppEvent::PairingWindowClosed);
        }
        self.emit(AppEvent::PairingResult {
            peer: Some(peer),
            accepted: true,
        });
        self.emit(AppEvent::DevicesChanged);
        self.refresh_advertising();
        Ok(())
    }

    /// Adds a newly trusted peer with the default features, keeping the settings of one that
    /// was already known.
    pub(super) fn adopt(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        if self.peers.get(&peer).is_some() {
            return Ok(());
        }
        let entry = self.new_peer(peer);
        let enabled = entry.enabled;
        if !self.peers.insert(entry) {
            return Err(NodeError::TooManyPeers { limit: MAX_PEERS });
        }
        self.discovery.rebuild(&self.peers, self.now.epoch());
        self.refresh_advertising();
        self.push(NodeOutput::Persist(PersistRequest::PeerFeatures {
            peer,
            enabled,
        }));
        Ok(())
    }

    pub(super) fn fail_pairing_attempt(&mut self, address: SocketAddr) {
        let Some(attempt) = self.pairing.attempt else {
            return;
        };
        if attempt.address() == canonical_address(address) {
            self.pairing.attempt = None;
            self.emit(AppEvent::PairingResult {
                peer: attempt.peer(),
                accepted: false,
            });
        }
    }

    pub(super) fn service_pairing(&mut self) {
        let now = self.now.instant;
        if self
            .pairing
            .window
            .is_some_and(|window| !window.expires_at.is_after(now))
        {
            self.cancel_pairing();
        }
        if let Some(attempt) = self.pairing.attempt
            && !attempt.until().is_after(now)
        {
            self.pairing.attempt = None;
            self.emit(AppEvent::PairingResult {
                peer: attempt.peer(),
                accepted: false,
            });
        }
        while let Some(code) = self
            .pairing
            .codes
            .iter()
            .find(|code| !code.until.is_after(now))
            .copied()
        {
            let _ = self.pairing.take_code(&code.peer);
            self.reject_pairing(code.peer);
        }
    }

    /// Unpairs `peer`, telling it first so it forgets this device too.
    pub(super) fn forget(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        let entry = self.peers.get(&peer).ok_or(NodeError::UnknownPeer(peer))?;
        if entry.session.is_some() {
            self.send(
                peer,
                CONTROL,
                &Signal::GoodbyeV1(Goodbye {
                    reason: GoodbyeReason::Unpaired,
                }),
            )?;
        }
        self.forget_locally(peer);
        Ok(())
    }

    pub(super) fn forget_locally(&mut self, peer: DeviceId) {
        let Some(mut entry) = self.peers.remove(&peer) else {
            return;
        };
        let connected = entry.session.is_some();
        if let Some(session) = entry.session.take() {
            self.tear_down_features(peer, session, false);
        }
        let _ = self.cache.forget(&peer);
        let _ = self.pairing.take_just_paired(&peer);
        self.discovery.rebuild(&self.peers, self.now.epoch());
        self.command(DriverCommand::Forget { peer });
        self.push(NodeOutput::Persist(PersistRequest::ForgetPeer { peer }));
        if connected {
            self.emit(AppEvent::DeviceConnected {
                peer,
                connected: false,
            });
        }
        self.emit(AppEvent::DevicesChanged);
        self.refresh_advertising();
    }
}
