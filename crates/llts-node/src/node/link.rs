//! Sessions coming and going, and the autoconnect loop that brings them back.

use std::net::SocketAddr;

use llts_signaling::device::{DeviceClass, DeviceId};
use llts_signaling::message::{GoodbyeReason, Signal, StreamRef};

use llts_transport::canonical_address;

use crate::input::LinkEvent;
use crate::output::{AppEvent, DriverCommand, NodeOutput};
use crate::peer::{LinkState, Session};
use crate::util::stream::{CONTROL, STATE};
use crate::{FfspLinkExport, LinkSecret, Node, NodeError};

impl Node {
    pub(super) fn on_link(&mut self, event: LinkEvent<'_>) -> Result<(), NodeError> {
        match event {
            LinkEvent::Connected { peer, name, class } => self.on_connected(peer, name, class),
            LinkEvent::Disconnected { peer } => {
                self.on_disconnected(peer);
                Ok(())
            }
            LinkEvent::Message {
                peer,
                stream,
                bytes,
            } => self.on_message(peer, stream, bytes),
            LinkEvent::PairingCode { peer, name, code } => {
                self.on_pairing_code(peer, name, code);
                Ok(())
            }
            LinkEvent::Paired { peer } => self.on_paired(peer),
            LinkEvent::HandshakeFailed { address } => {
                self.on_handshake_failed(address);
                Ok(())
            }
            LinkEvent::QrOfferReady { offer } => self.on_qr_offer(offer),
        }
    }

    fn on_connected(
        &mut self,
        peer: DeviceId,
        name: &str,
        class: DeviceClass,
    ) -> Result<(), NodeError> {
        if self.peers.get(&peer).is_none() {
            self.adopt(peer)?;
        }
        let just_paired = self.pairing.take_just_paired(&peer);
        let Some(entry) = self.peers.get_mut(&peer) else {
            return Err(NodeError::UnknownPeer(peer));
        };
        let mut replaced = entry.session.take();
        let pending_starts = replaced
            .as_mut()
            .map(|session| core::mem::take(&mut session.pending_starts))
            .unwrap_or_default();
        entry.session = Some(Session {
            just_paired,
            pending_starts,
            ..Session::default()
        });
        if !name.is_empty() {
            name.clone_into(&mut entry.name);
        }
        entry.class = class;
        entry.link = LinkState::Connected;
        entry.backoff.reset();
        let enabled = entry.enabled;
        if let Some(replaced) = replaced {
            self.tear_down_features(peer, replaced, false);
        }
        if just_paired {
            self.export_link(peer);
        }
        self.command(DriverCommand::OpenReliable {
            peer,
            stream: STATE,
        });
        let capabilities = self.capabilities_for(enabled);
        self.send(peer, CONTROL, &Signal::CapabilitiesV1(capabilities))?;
        self.emit(AppEvent::DeviceConnected {
            peer,
            connected: true,
        });
        self.emit(AppEvent::DevicesChanged);
        Ok(())
    }

    pub(super) fn on_disconnected(&mut self, peer: DeviceId) {
        let Some(entry) = self.peers.get_mut(&peer) else {
            return;
        };
        let Some(session) = entry.session.take() else {
            return;
        };
        entry.link = LinkState::Idle;
        self.tear_down_features(peer, session, false);
        self.schedule_retry(peer);
        self.emit(AppEvent::DeviceConnected {
            peer,
            connected: false,
        });
        self.emit(AppEvent::DevicesChanged);
    }

    fn on_message(
        &mut self,
        peer: DeviceId,
        stream: StreamRef,
        bytes: &[u8],
    ) -> Result<(), NodeError> {
        if self
            .peers
            .get(&peer)
            .is_none_or(|entry| entry.session.is_none())
        {
            return Ok(());
        }
        let signal = Signal::decode(bytes)?;
        match signal {
            Signal::CapabilitiesV1(capabilities) => self.on_capabilities(peer, capabilities),
            Signal::GoodbyeV1(goodbye) if goodbye.reason == GoodbyeReason::Unpaired => {
                self.forget_locally(peer);
                Ok(())
            }
            Signal::GoodbyeV1(_) => {
                self.on_disconnected(peer);
                Ok(())
            }
            Signal::SubscribeV1(subscribe) => self.on_subscribe(peer, subscribe.topics),
            Signal::UnsubscribeV1(unsubscribe) => {
                self.on_unsubscribe(peer, unsubscribe.topics);
                Ok(())
            }
            Signal::UpdateV1(update) => {
                self.on_update(peer, &update);
                Ok(())
            }
            Signal::SnapshotV1(snapshot) => {
                self.on_snapshot(peer, &snapshot);
                Ok(())
            }
            Signal::OpenStreamV1(open) => self.on_open_stream(peer, open),
            Signal::AcceptStreamV1(accept) => {
                self.on_accept_stream(peer, accept.stream);
                Ok(())
            }
            Signal::CloseStreamV1(close) => {
                self.on_close_stream(peer, close.stream);
                Ok(())
            }
            Signal::StopV1(stop) => {
                self.on_stop(peer, stop.feature);
                Ok(())
            }
            Signal::StartMirrorV1(_)
            | Signal::StartCameraV1(_)
            | Signal::StartMicV1(_)
            | Signal::StartMonitorV1(_) => self.on_feature_request(peer, &signal),
            signal => {
                self.on_command(peer, &signal, stream);
                Ok(())
            }
        }
    }

    fn on_handshake_failed(&mut self, address: SocketAddr) {
        let address = canonical_address(address);
        self.fail_pairing_attempt(address);
        let failed: Vec<DeviceId> = self
            .peers
            .iter()
            .filter(|peer| matches!(peer.link, LinkState::Dialing { address: dialled, .. } if dialled == address))
            .map(|peer| peer.id)
            .collect();
        for peer in failed {
            self.schedule_retry(peer);
        }
    }

    /// Dials `peer` at its next known address, unless a dial or session is already under way.
    pub(super) fn dial(&mut self, peer: DeviceId) {
        if self.pairing.is_just_paired(&peer) {
            return;
        }
        let until = self.now.instant.saturating_add(self.config.dial_timeout);
        let Some(entry) = self.peers.get_mut(&peer) else {
            return;
        };
        if matches!(entry.link, LinkState::Dialing { .. } | LinkState::Connected) {
            return;
        }
        let Some(address) = entry.sightings.next_address() else {
            entry.link = LinkState::Idle;
            return;
        };
        entry.link = LinkState::Dialing { address, until };
        self.command(DriverCommand::Dial { peer, address });
    }

    /// Backs off before the next attempt, or waits for a sighting if nothing sees the peer.
    fn schedule_retry(&mut self, peer: DeviceId) {
        let Some(entry) = self.peers.get_mut(&peer) else {
            return;
        };
        if matches!(entry.link, LinkState::Connected) {
            return;
        }
        let delay = entry
            .backoff
            .next_delay(&self.config.backoff, &mut self.jitter);
        entry.link = LinkState::Backoff {
            until: self.now.instant.saturating_add(delay),
        };
    }

    pub(super) fn service_links(&mut self) {
        let now = self.now.instant;
        let due: Vec<(DeviceId, LinkState)> = self
            .peers
            .iter()
            .filter(|peer| peer.link.deadline().is_some_and(|at| !at.is_after(now)))
            .map(|peer| (peer.id, peer.link))
            .collect();
        for (peer, link) in due {
            match link {
                LinkState::Dialing { .. } => self.schedule_retry(peer),
                LinkState::Backoff { .. } => self.retry(peer),
                LinkState::Idle | LinkState::Connected => {}
            }
        }
    }

    /// A backoff ran out: dial again if anything still sees the peer.
    fn retry(&mut self, peer: DeviceId) {
        let Some(entry) = self.peers.get_mut(&peer) else {
            return;
        };
        entry.link = LinkState::Idle;
        if entry.sightings.is_present() {
            self.dial(peer);
        }
    }

    /// The network changed, which makes every waiting peer worth trying at once.
    pub(super) fn retry_now(&mut self) {
        let waiting: Vec<DeviceId> = self
            .peers
            .iter_mut()
            .filter(|peer| matches!(peer.link, LinkState::Backoff { .. }))
            .map(|peer| {
                peer.backoff.reset();
                peer.id
            })
            .collect();
        for peer in waiting {
            self.retry(peer);
        }
    }

    fn export_link(&mut self, peer: DeviceId) {
        let Some(entry) = self.peers.get(&peer) else {
            return;
        };
        let export = FfspLinkExport {
            peer,
            name: entry.name.clone(),
            class: entry.class,
            link_secret: LinkSecret::derive(&self.identity, &peer),
        };
        self.push(NodeOutput::ExportFfspLink(export));
    }
}
