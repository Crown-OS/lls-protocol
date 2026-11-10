use llts_signaling::device::DeviceId;

use super::event_loop::EventLoop;
use crate::signal_json::{Inbound, inbound};

impl EventLoop {
    /// Delivers a peer's application message to Kotlin, or answers it here when Rust owns the
    /// answer.
    pub(super) fn on_application(&mut self, peer: DeviceId, message: &[u8]) {
        match inbound(&peer, message) {
            Ok(Inbound::Deliver(event)) => self.emit(event),
            Ok(Inbound::Answer(answer)) => {
                if let Err(error) = self.node.send_application(self.clock.now(), peer, &answer) {
                    self.report(&error);
                }
            }
            Ok(Inbound::Ignore) => {}
            Err(error) => self.report(&error),
        }
    }
}
