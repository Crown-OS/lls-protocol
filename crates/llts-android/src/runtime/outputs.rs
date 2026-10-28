use llts_core::session::Event;
use llts_node::bridge::execute;
use llts_node::{
    AppEvent, LinkEvent, MediaRequest, NodeInput, NodeOutput, PersistRequest, RemoteCommand,
};
use llts_transport::security::StaticKey;

use super::event_loop::EventLoop;
use crate::error::BridgeError;
use crate::event_json;

impl EventLoop {
    /// Carries out everything the node asked for, including what that in turn makes it ask.
    pub(super) fn drain_node(&mut self) {
        while let Some(output) = self.node.poll_output() {
            match output {
                NodeOutput::Driver(command) => match execute(&mut self.driver, command) {
                    Ok(Some(offer)) => self.node.handle(
                        self.clock.now(),
                        NodeInput::Link(LinkEvent::QrOfferReady { offer: &offer }),
                    ),
                    Ok(None) => {}
                    Err(error) => self.report(&error),
                },
                NodeOutput::App(event) => {
                    self.emit(event_json::app_event(&event, &self.node));
                    if let AppEvent::RemoteCommand {
                        peer,
                        command: RemoteCommand::FileOffer(offer),
                    } = &event
                    {
                        let host = self.driver.peer_address(&StaticKey(peer.0));
                        self.emit(event_json::file_offer(peer, offer.ffsp_transfer_id.0, host));
                    }
                }
                NodeOutput::Media(request) => self.start_or_stop_media(&request),
                NodeOutput::Persist(request) => {
                    if let Err(error) = self.persist(request) {
                        self.report(&error);
                    }
                }
                NodeOutput::ExportFfspLink(export) => {
                    if let Err(error) = self.ffsp.record(&export) {
                        self.report(&error);
                    }
                }
                NodeOutput::Advertise(beacons) => {
                    self.advertisements.replace(&beacons);
                    self.emit(event_json::advertising_changed(&beacons));
                }
            }
        }
    }

    fn start_or_stop_media(&mut self, request: &MediaRequest) {
        let outcome = match *request {
            MediaRequest::Start {
                peer,
                request,
                role,
                stream,
                params,
            } => self.media.start(peer, request, role, stream, params),
            MediaRequest::Stop { peer, stream, .. } => {
                self.media.stop(peer, stream);
                Ok(())
            }
        };
        if let Err(error) = outcome {
            self.report(&error);
        }
        self.emit(event_json::media_request(request));
    }

    fn persist(&mut self, request: PersistRequest) -> Result<(), BridgeError> {
        match request {
            PersistRequest::PeerFeatures { peer, enabled } => self.features.set(peer, enabled),
            PersistRequest::ForgetPeer { peer } => {
                self.ffsp.forget(&peer)?;
                self.features.remove(&peer)
            }
        }
    }

    /// Moves whatever the encoders finished into their llts streams, straight from codec memory.
    pub(super) fn pump_media(&mut self) {
        let driver = &mut self.driver;
        let mut refused = None;
        let pumped = self.media.pump(&mut |peer, stream, slice| {
            if let Err(error) =
                driver.handle(&StaticKey(peer.0), &Event::SliceEncoded { stream, slice })
            {
                refused = Some(error);
            }
        });
        if let Err(error) = pumped {
            self.report(&error);
        }
        if let Some(error) = refused {
            self.report(&error);
        }
    }
}
