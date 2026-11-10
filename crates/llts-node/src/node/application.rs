//! Application messages: the node checks the session, the lane and the feature gate, then
//! passes them through unread, so a new request or page needs no code here.

use llts_core::channel::{Delivery, MAX_MESSAGE_LEN};
use llts_signaling::SignalingError;
use llts_signaling::device::{DeviceId, Feature};
use llts_signaling::message::{
    AppRoute, Envelope, Gate, Lane, MessageType, RequestError, RequestFailed, RequestId, Signal,
    StreamRef,
};

use crate::output::{AppEvent, DriverCommand};
use crate::{Node, NodeError, Now};

/// What the node answers a fetch whose feature is not shared, without user data.
const NOT_SHARED: &str = "the feature is not shared with this device";

impl Node {
    /// Sends one encoded application message to `peer`, on the lane its type is routed to.
    ///
    /// # Errors
    ///
    /// Returns [`NodeError::NotApplication`] for a type the node handles itself,
    /// [`NodeError::UnknownPeer`], [`NodeError::NotConnected`],
    /// [`NodeError::FeatureUnavailable`] when the gate is closed, and
    /// [`NodeError::MessageTooLarge`] above the reliable-message limit.
    pub fn send_application(
        &mut self,
        now: Now,
        peer: DeviceId,
        message: &[u8],
    ) -> Result<(), NodeError> {
        self.now = now;
        self.queue_application(peer, message)
    }

    pub(super) fn queue_application(
        &mut self,
        peer: DeviceId,
        message: &[u8],
    ) -> Result<(), NodeError> {
        let envelope = Envelope::parse(message)?;
        let route = envelope
            .message_type()
            .and_then(MessageType::app_route)
            .ok_or(NodeError::NotApplication(envelope.msg_type))?;
        if message.len() > MAX_MESSAGE_LEN {
            return Err(NodeError::MessageTooLarge {
                len: message.len(),
                limit: MAX_MESSAGE_LEN,
            });
        }
        let entry = self.peers.get(&peer).ok_or(NodeError::UnknownPeer(peer))?;
        if entry.session.is_none() {
            return Err(NodeError::NotConnected(peer));
        }
        let required = match route.gate {
            Gate::Open => None,
            Gate::Feature(feature) => Some(feature),
            Gate::FromBody => Some(
                body_feature(message)?.ok_or(SignalingError::OutOfRange { field: "feature" })?,
            ),
        };
        if let Some(feature) = required
            && !self.shared_features(entry).contains(feature)
        {
            return Err(NodeError::FeatureUnavailable { peer, feature });
        }
        self.command(DriverCommand::Send {
            peer,
            stream: route.lane.stream(),
            message: message.to_vec(),
            delivery: Delivery::Ordered,
        });
        Ok(())
    }

    /// Hands a peer's application message to the platform if it came on its lane and its
    /// feature is shared; anything else is dropped. A fetch under a feature that is not shared
    /// is answered here, so the requester does not wait for an answer that cannot come.
    pub(super) fn on_application(
        &mut self,
        peer: DeviceId,
        stream: StreamRef,
        route: AppRoute,
        message: &[u8],
    ) -> Result<(), NodeError> {
        if stream != route.lane.stream() {
            return Ok(());
        }
        let shared = self
            .peers
            .get(&peer)
            .map(|entry| self.shared_features(entry))
            .unwrap_or_default();
        match route.gate {
            Gate::Open => {}
            Gate::Feature(feature) if shared.contains(feature) => {}
            Gate::Feature(_) => return Ok(()),
            Gate::FromBody => {
                let Signal::FetchBlobV1(fetch) = Signal::decode(message)? else {
                    return Ok(());
                };
                if !fetch
                    .feature()
                    .is_some_and(|feature| shared.contains(feature))
                {
                    return self.refuse(peer, fetch.request);
                }
            }
        }
        self.emit(AppEvent::Application {
            peer,
            message: message.to_vec(),
        });
        Ok(())
    }

    fn refuse(&mut self, peer: DeviceId, request: RequestId) -> Result<(), NodeError> {
        let failure = Signal::RequestFailedV1(RequestFailed {
            request,
            error: RequestError::PERMISSION_DENIED,
            detail: NOT_SHARED,
        });
        self.send(peer, Lane::Bulk.stream(), &failure)
    }
}

/// The feature a message names in its body, as `FetchBlob` does; `None` for a bit this build
/// does not know.
fn body_feature(message: &[u8]) -> Result<Option<Feature>, NodeError> {
    match Signal::decode(message)? {
        Signal::FetchBlobV1(fetch) => Ok(fetch.feature()),
        _ => Ok(None),
    }
}
