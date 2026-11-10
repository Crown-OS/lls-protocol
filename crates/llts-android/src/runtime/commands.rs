use std::borrow::Cow;
use std::net::SocketAddr;

use llts_core::session::Event;
use llts_node::{AppCommand, NodeError, NodeInput, Presence};
use llts_signaling::device::DeviceId;
use llts_signaling::state::Topic;
use llts_transport::security::StaticKey;

use super::event_loop::{EventLoop, Flow, INPUT_STREAM};
use crate::command::{Command, NodeCommand};
use crate::error::BridgeError;
use crate::state_json::{LocalDocument, Stamp, restamp};

impl EventLoop {
    pub(super) fn execute(&mut self, command: Command) -> Flow {
        let outcome = match command {
            Command::Stop => return Flow::Stop,
            Command::PublishState { topic, document } => self.publish_state(topic, &document),
            Command::SendApplication {
                peer,
                message,
                outcome,
            } => {
                let sent = self.node.send_application(self.clock.now(), peer, &message);
                let _ = outcome.send(sent);
                Ok(())
            }
            Command::SendInput { peer, batch } => self.send_input(peer, &batch),
            Command::AttachSurface {
                peer,
                index,
                role,
                surface,
            } => self
                .media
                .attach(peer, index, role, surface)
                .map_err(BridgeError::from),
            Command::DetachSurface { peer, index } => {
                self.media.detach(peer, index);
                Ok(())
            }
            Command::Node(command) => {
                self.handle_node_command(&command);
                Ok(())
            }
        };
        if let Err(error) = outcome {
            self.report(&error);
        }
        self.drain_node();
        Flow::Continue
    }

    fn handle_node_command(&mut self, command: &NodeCommand) {
        let now = self.clock.now();
        let app = |command| NodeInput::App(command);
        match command {
            NodeCommand::BeginPairing { duration } => self.node.handle(
                now,
                app(AppCommand::BeginPairing {
                    duration: *duration,
                }),
            ),
            NodeCommand::CancelPairing => self.node.handle(now, app(AppCommand::CancelPairing)),
            NodeCommand::PairWithQr { uri } => {
                self.node.handle(now, app(AppCommand::PairWithQr { uri }));
            }
            NodeCommand::PairNearby { address } => self
                .node
                .handle(now, app(AppCommand::PairNearby { address: *address })),
            NodeCommand::ConfirmPairing { peer, accept } => self.node.handle(
                now,
                app(AppCommand::ConfirmPairing {
                    peer: *peer,
                    accept: *accept,
                }),
            ),
            NodeCommand::Forget { peer } => self
                .node
                .handle(now, app(AppCommand::Forget { peer: *peer })),
            NodeCommand::SetFeature {
                peer,
                feature,
                enabled,
            } => self.node.handle(
                now,
                app(AppCommand::SetFeature {
                    peer: *peer,
                    feature: *feature,
                    enabled: *enabled,
                }),
            ),
            NodeCommand::StartFeature { peer, request } => self.node.handle(
                now,
                app(AppCommand::StartFeature {
                    peer: *peer,
                    request: *request,
                }),
            ),
            NodeCommand::StopFeature { peer, feature } => self.node.handle(
                now,
                app(AppCommand::StopFeature {
                    peer: *peer,
                    feature: *feature,
                }),
            ),
            NodeCommand::Send { peer, command } => self.node.handle(
                now,
                app(AppCommand::Send {
                    peer: *peer,
                    command: command.as_outgoing(),
                }),
            ),
            NodeCommand::Presence {
                beacon,
                addresses,
                seen,
            } => {
                let presence = if *seen {
                    Presence::Seen {
                        beacon: *beacon,
                        addresses,
                    }
                } else {
                    Presence::Lost { beacon: *beacon }
                };
                self.node.handle(now, NodeInput::Presence(presence));
            }
            NodeCommand::NetworkChanged { addresses } => {
                let reachable: Vec<SocketAddr> = addresses
                    .iter()
                    .filter(|address| address.is_ipv4())
                    .map(|address| SocketAddr::new(*address, self.port))
                    .collect();
                self.node.handle(
                    now,
                    NodeInput::NetworkChanged {
                        addresses: &reachable,
                        rebind: None,
                    },
                );
            }
        }
    }

    fn publish_state(&mut self, topic: Topic, document: &LocalDocument) -> Result<(), BridgeError> {
        let now = self.clock.now();
        let payload = match document.stamp {
            Stamp::AsWritten => Cow::Borrowed(document.payload.as_slice()),
            Stamp::Fresh => Cow::Owned(restamp(topic, &document.payload, self.node.stamp(now))?),
        };
        self.node.handle(
            now,
            NodeInput::LocalState {
                topic,
                payload: &payload,
            },
        );
        Ok(())
    }

    /// Queues one input batch for `peer` on the input stream its session opened.
    fn send_input(&mut self, peer: DeviceId, batch: &[u8]) -> Result<(), BridgeError> {
        let key = StaticKey(peer.0);
        if self.driver.session_mut(&key).is_none() {
            return Err(NodeError::NotConnected(peer).into());
        }
        self.driver.handle(
            &key,
            &Event::InputQueued {
                stream: INPUT_STREAM,
                events: batch,
            },
        )?;
        Ok(())
    }
}
