//! What the app can read back: the device list, feature states and peers' documents.

use llts_signaling::device::{DeviceClass, DeviceId, Feature, FeatureSet};
use llts_signaling::message::{StreamRef, Update};
use llts_signaling::state::{Clipboard, StatePublisher, Topic, TopicPayload};

use crate::feature::{FeatureState, MediaRole};
use crate::peer::Peer;
use crate::{Node, NodeError};

/// One paired device as the user interface lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceView<'n> {
    pub id: DeviceId,
    pub name: &'n str,
    pub class: DeviceClass,
    pub connected: bool,
    /// Features the user allows this device.
    pub enabled: FeatureSet,
    /// Features both ends implement and this device allows, while connected.
    pub available: FeatureSet,
    /// Features running right now.
    pub active: FeatureSet,
}

impl Node {
    pub fn devices(&self) -> impl Iterator<Item = DeviceView<'_>> {
        self.peers.iter().map(|peer| self.view(peer))
    }

    pub fn device(&self, peer: &DeviceId) -> Option<DeviceView<'_>> {
        self.peers.get(peer).map(|entry| self.view(entry))
    }

    fn view<'n>(&'n self, peer: &'n Peer) -> DeviceView<'n> {
        DeviceView {
            id: peer.id,
            name: &peer.name,
            class: peer.class,
            connected: peer.session.is_some(),
            enabled: peer.enabled,
            available: self.shared_features(peer),
            active: peer.session.as_ref().map_or(FeatureSet::EMPTY, |session| {
                session.features.running().collect()
            }),
        }
    }

    pub fn feature_state(&self, peer: &DeviceId, feature: Feature) -> Option<FeatureState> {
        let device = self.device(peer)?;
        Some(if device.active.contains(feature) {
            FeatureState::Active
        } else if device.enabled.contains(feature) {
            FeatureState::Enabled
        } else {
            FeatureState::Disabled
        })
    }

    /// A peer's latest document for `P::TOPIC`, decoded in place.
    ///
    /// # Errors
    ///
    /// Fails if the peer published something this build cannot decode.
    pub fn remote_state<'n, P: TopicPayload<'n>>(
        &'n self,
        peer: &DeviceId,
    ) -> Result<Option<P>, NodeError> {
        Ok(self.cache.get(peer)?)
    }

    /// A peer's latest document for `topic`, still encoded, as the IPC `state` call returns it.
    pub fn remote_update(&self, peer: &DeviceId, topic: Topic) -> Option<Update<'_>> {
        self.cache.update(peer, topic)
    }

    pub const fn local_state(&self) -> &StatePublisher {
        &self.publisher
    }

    /// The clipboard copied most recently on any device, this one included.
    pub fn newest_clipboard(&self) -> Option<(DeviceId, Clipboard<'_>)> {
        let own = self
            .publisher
            .get::<Clipboard<'_>>()
            .ok()
            .flatten()
            .map(|clipboard| (self.id, clipboard));
        own.into_iter()
            .chain(self.cache.newest_clipboard())
            .max_by_key(|(origin, clipboard)| clipboard.order(*origin))
    }

    /// Which feature a media stream serves and which half this device runs, for routing
    /// frames the driver delivers.
    pub fn stream_purpose(
        &self,
        peer: &DeviceId,
        stream: StreamRef,
    ) -> Option<(Feature, MediaRole)> {
        let active = self
            .peers
            .get(peer)?
            .session
            .as_ref()?
            .features
            .by_stream(stream)?;
        Some((active.feature(), active.role))
    }
}
