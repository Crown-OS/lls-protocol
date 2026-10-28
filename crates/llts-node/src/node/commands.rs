//! Commands one device asks another to carry out: calls, media keys, replies, and the rest.

use llts_signaling::device::DeviceId;
use llts_signaling::message::{Signal, StreamRef};

use crate::input::OutgoingCommand;
use crate::output::{AppEvent, RemoteCommand, command_feature};
use crate::util::stream::CONTROL;
use crate::{Node, NodeError};

impl Node {
    pub(super) fn send_command(
        &mut self,
        peer: DeviceId,
        command: OutgoingCommand<'_>,
    ) -> Result<(), NodeError> {
        let entry = self.peers.get(&peer).ok_or(NodeError::UnknownPeer(peer))?;
        if entry.session.is_none() {
            return Err(NodeError::NotConnected(peer));
        }
        let signal = command.as_signal();
        if let Some(feature) = command_feature(&signal)
            && !self.shared_features(entry).contains(feature)
        {
            return Err(NodeError::FeatureUnavailable { peer, feature });
        }
        self.send(peer, CONTROL, &signal)
    }

    /// Hands a peer's command to the app, if this device allows what it needs. Anything else
    /// on the stream is a message this build has no use for and is skipped.
    pub(super) fn on_command(&mut self, peer: DeviceId, signal: &Signal<'_>, stream: StreamRef) {
        if stream != CONTROL {
            return;
        }
        let Some(command) = RemoteCommand::from_signal(signal) else {
            return;
        };
        let allowed = command_feature(signal).is_none_or(|feature| {
            self.peers
                .get(&peer)
                .is_some_and(|entry| self.shared_features(entry).contains(feature))
        });
        if allowed {
            self.emit(AppEvent::RemoteCommand { peer, command });
        }
    }
}
