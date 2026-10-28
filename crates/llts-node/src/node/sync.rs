//! State sync: announcing capabilities, subscribing per enabled feature, and keeping every
//! subscriber's copy of this device's documents current.

use llts_signaling::device::DeviceId;
use llts_signaling::message::{Capabilities, Signal, Snapshot, Subscribe, Unsubscribe, Update};
use llts_signaling::state::{
    Applied, Clipboard, DeviceInfo, HybridTimestamp, Published, Topic, TopicPayload, TopicSet,
};

use crate::output::{AppEvent, FeatureFailure};
use crate::sync::{encode, encode_snapshot, topics_for};
use crate::util::stream::{CONTROL, STATE};
use crate::{Node, NodeError, Now};

impl Node {
    /// Publishes this device's own document for `P::TOPIC` to every subscriber.
    ///
    /// # Errors
    ///
    /// Fails only if the value cannot be encoded.
    pub fn publish<'v, P: TopicPayload<'v>>(
        &mut self,
        now: Now,
        value: &P,
    ) -> Result<Published, NodeError> {
        self.now = now;
        let published = self.publisher.publish(value)?;
        if published != Published::Unchanged {
            self.mark_changed(P::TOPIC);
        }
        Ok(published)
    }

    pub(super) fn publish_encoded(
        &mut self,
        topic: Topic,
        payload: &[u8],
    ) -> Result<(), NodeError> {
        if self.publisher.publish_encoded(topic, payload)? != Published::Unchanged {
            self.mark_changed(topic);
        }
        Ok(())
    }

    /// Stamps a clipboard the user just copied on this device, ordered after every clipboard
    /// seen from a peer.
    pub const fn clipboard_stamp(&mut self, now: Now) -> HybridTimestamp {
        self.now = now;
        self.clock.tick(now.unix_ms)
    }

    fn mark_changed(&mut self, topic: Topic) {
        for session in self
            .peers
            .iter_mut()
            .filter_map(|peer| peer.session.as_mut())
        {
            session.outbox.mark_changed(topic);
        }
        if self.flush_at.is_none() {
            self.flush_at = Some(
                self.now
                    .instant
                    .saturating_add(self.config.state_flush_delay),
            );
        }
    }

    /// Sends every subscriber one update per topic that changed since the last flush.
    pub(super) fn flush_state(&mut self) {
        self.flush_at = None;
        let mut messages = Vec::new();
        for peer in self.peers.iter_mut() {
            let Some(session) = peer.session.as_mut() else {
                continue;
            };
            for update in session.outbox.drain(&self.publisher) {
                messages.push((peer.id, encode(&Signal::UpdateV1(update))));
            }
        }
        for (peer, message) in messages {
            match message {
                Ok(message) => self.queue_state(peer, message),
                Err(error) => self.emit(AppEvent::Error(error)),
            }
        }
    }

    fn queue_state(&mut self, peer: DeviceId, message: Vec<u8>) {
        self.command(crate::DriverCommand::Send {
            peer,
            stream: STATE,
            message,
            delivery: llts_core::channel::Delivery::Ordered,
        });
    }

    pub(super) fn on_capabilities(
        &mut self,
        peer: DeviceId,
        capabilities: Capabilities,
    ) -> Result<(), NodeError> {
        let Some(session) = self
            .peers
            .get_mut(&peer)
            .and_then(|entry| entry.session.as_mut())
        else {
            return Ok(());
        };
        let first = session.capabilities.replace(capabilities).is_none();
        if first && session.just_paired {
            self.announce(peer)?;
        }
        self.renegotiate(peer)?;
        self.start_deferred(peer)
    }

    /// Sends this device's capabilities for `peer` again, after they changed or when the first
    /// copy may have been dropped.
    pub(super) fn announce(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        let Some(entry) = self.peers.get(&peer) else {
            return Ok(());
        };
        let capabilities = self.capabilities_for(entry.enabled);
        self.send(peer, CONTROL, &Signal::CapabilitiesV1(capabilities))
    }

    /// Brings subscriptions, served topics and running features in line with what both ends
    /// now share.
    pub(super) fn renegotiate(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        let Some(entry) = self.peers.get(&peer) else {
            return Ok(());
        };
        let shared = self.shared_features(entry);
        let served = topics_for(self.local.capabilities.features.shared_with(entry.enabled));
        let Some(session) = self
            .peers
            .get_mut(&peer)
            .and_then(|entry| entry.session.as_mut())
        else {
            return Ok(());
        };
        let wanted = topics_for(shared);
        let added = wanted.difference(session.subscribed);
        let dropped = session.subscribed.difference(wanted);
        session.subscribed = wanted;
        session
            .outbox
            .unsubscribe(session.outbox.subscribed().difference(served));
        let stale: Vec<_> = session
            .features
            .features()
            .filter(|feature| !shared.contains(*feature))
            .collect();
        if !added.is_empty() {
            self.send(
                peer,
                CONTROL,
                &Signal::SubscribeV1(Subscribe { topics: added }),
            )?;
        }
        if !dropped.is_empty() {
            self.send(
                peer,
                CONTROL,
                &Signal::UnsubscribeV1(Unsubscribe { topics: dropped }),
            )?;
        }
        for feature in stale {
            self.stop_feature_with(peer, feature, Some(FeatureFailure::Unavailable))?;
        }
        self.emit(AppEvent::DevicesChanged);
        Ok(())
    }

    pub(super) fn on_subscribe(
        &mut self,
        peer: DeviceId,
        topics: TopicSet,
    ) -> Result<(), NodeError> {
        let Some(entry) = self.peers.get_mut(&peer) else {
            return Ok(());
        };
        let served = topics_for(self.local.capabilities.features.shared_with(entry.enabled));
        let Some(session) = entry.session.as_mut() else {
            return Ok(());
        };
        let snapshot = session
            .outbox
            .subscribe(topics.intersection(served), &self.publisher);
        let messages = encode_snapshot(snapshot)?;
        for message in messages {
            self.queue_state(peer, message);
        }
        Ok(())
    }

    pub(super) fn on_unsubscribe(&mut self, peer: DeviceId, topics: TopicSet) {
        if let Some(session) = self
            .peers
            .get_mut(&peer)
            .and_then(|entry| entry.session.as_mut())
        {
            session.outbox.unsubscribe(topics);
        }
    }

    fn subscribed(&self, peer: &DeviceId) -> TopicSet {
        self.peers
            .get(peer)
            .and_then(|entry| entry.session.as_ref())
            .map_or(TopicSet::EMPTY, |session| session.subscribed)
    }

    pub(super) fn on_update(&mut self, peer: DeviceId, update: &Update<'_>) {
        if !self.subscribed(&peer).contains(update.topic) {
            return;
        }
        if self.cache.apply_update(peer, update) == Applied::Accepted {
            self.after_remote_change(peer, update.topic);
        }
    }

    pub(super) fn on_snapshot(&mut self, peer: DeviceId, snapshot: &Snapshot<'_>) {
        let subscribed = self.subscribed(&peer);
        let topics = snapshot.topics.intersection(subscribed);
        let accepted = Snapshot {
            topics,
            updates: snapshot
                .updates
                .iter()
                .filter(|update| topics.contains(update.topic))
                .copied()
                .collect(),
        };
        self.cache.apply_snapshot(peer, &accepted);
        for topic in topics.iter() {
            self.after_remote_change(peer, topic);
        }
    }

    fn after_remote_change(&mut self, peer: DeviceId, topic: Topic) {
        match topic {
            Topic::Clipboard => {
                if let Ok(Some(clipboard)) = self.cache.get::<Clipboard<'_>>(&peer) {
                    self.clock.observe(clipboard.stamp, self.now.unix_ms);
                }
            }
            Topic::DeviceInfo => {
                if let Ok(Some(info)) = self.cache.get::<DeviceInfo<'_>>(&peer)
                    && let Some(entry) = self.peers.get_mut(&peer)
                    && entry.name != info.name
                {
                    info.name.clone_into(&mut entry.name);
                    entry.class = info.class;
                    self.emit(AppEvent::DevicesChanged);
                }
            }
            _ => {}
        }
        self.emit(AppEvent::RemoteStateChanged { peer, topic });
    }
}
