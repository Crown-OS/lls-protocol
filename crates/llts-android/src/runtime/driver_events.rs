use llts_core::session::Action;
use llts_node::NodeInput;
use llts_node::bridge::link_event;
use llts_signaling::device::DeviceId;
use llts_transport::security::StaticKey;
use llts_transport::session::DriverEvent;

use super::event_loop::{EventLoop, INPUT_STREAM};
use crate::input_json::{input_event, key_state_event};

impl EventLoop {
    /// Hands the node what it understands and the media pipelines the rest.
    pub(super) fn route_driver_events(&mut self) {
        while let Some(event) = self.driver.poll_event() {
            if let DriverEvent::Connected { peer, .. } = &event {
                self.open_input(peer);
            }
            if let Some(link) = link_event(&event) {
                self.node.handle(self.clock.now(), NodeInput::Link(link));
                self.drain_node();
            } else if let DriverEvent::Session { peer, action } = event {
                self.media_action(DeviceId(peer.0), action);
            }
        }
    }

    /// Opens the input stream of a new session, so the peer's input is taken from the start.
    fn open_input(&mut self, peer: &StaticKey) {
        let opened = self
            .driver
            .session_mut(peer)
            .map(|session| session.open_input(INPUT_STREAM));
        if let Some(Err(error)) = opened {
            log::debug!("input stream: {error}");
        }
    }

    fn media_action(&mut self, peer: DeviceId, action: Action) {
        match action {
            Action::PresentFrame { stream, frame } => {
                if let Err(error) = self.media.present(peer, stream, &frame) {
                    self.report(&error);
                }
            }
            Action::SetTargetBitrate { stream, bitrate } => {
                self.media.set_bitrate(peer, stream, bitrate);
            }
            Action::RequestKeyframe { stream } => self.media.request_keyframe(peer, stream),
            Action::Deliver { message, .. } => {
                if let Some(event) = input_event(&peer, message.filled()) {
                    self.emit(event);
                }
            }
            Action::InputKeyState { state, .. } => {
                self.emit(key_state_event(&peer, state.filled()));
            }
            Action::Transmit { .. }
            | Action::SetFrameByteBudget { .. }
            | Action::PeerClockUpdated { .. }
            | Action::PeerDead
            | Action::PathChanged(_)
            | Action::ArmTimer(_) => {}
        }
    }
}
