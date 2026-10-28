use llts_signaling::device::Feature;
use llts_signaling::message::{MuxClass, StreamParams, StreamRef};

use crate::feature::{FeatureRequest, MediaRole};

/// How far a feature's stream has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The decoder is waiting for the encoder to propose the stream.
    AwaitingProposal,
    /// The encoder proposed the stream and waits for the decoder to accept it.
    Proposed,
    Running,
}

/// One feature running, or being set up, with one peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveFeature {
    pub(crate) request: FeatureRequest,
    pub(crate) role: MediaRole,
    pub(crate) phase: Phase,
    pub(crate) stream: Option<(StreamRef, StreamParams)>,
}

impl ActiveFeature {
    pub(crate) const fn feature(&self) -> Feature {
        self.request.feature()
    }
}

/// The features of one session. Each feature runs at most once per peer, so the table is
/// bounded by the number of media features.
#[derive(Debug, Default)]
pub(crate) struct ActiveFeatures {
    entries: heapless::Vec<ActiveFeature, MEDIA_FEATURES>,
}

pub(crate) const MEDIA_FEATURES: usize = 4;

impl ActiveFeatures {
    pub(crate) fn get(&self, feature: Feature) -> Option<&ActiveFeature> {
        self.entries.iter().find(|entry| entry.feature() == feature)
    }

    pub(crate) fn get_mut(&mut self, feature: Feature) -> Option<&mut ActiveFeature> {
        self.entries
            .iter_mut()
            .find(|entry| entry.feature() == feature)
    }

    pub(crate) fn by_stream(&self, stream: StreamRef) -> Option<&ActiveFeature> {
        self.entries
            .iter()
            .find(|entry| entry.stream.is_some_and(|(held, _)| held == stream))
    }

    /// Adds `entry`, replacing any earlier attempt at the same feature.
    pub(crate) fn insert(&mut self, entry: ActiveFeature) {
        self.remove(entry.feature());
        let _ = self.entries.push(entry);
    }

    pub(crate) fn remove(&mut self, feature: Feature) -> Option<ActiveFeature> {
        let position = self
            .entries
            .iter()
            .position(|entry| entry.feature() == feature)?;
        Some(self.entries.swap_remove(position))
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = ActiveFeature> + use<> {
        core::mem::take(&mut self.entries).into_iter()
    }

    pub(crate) fn features(&self) -> impl Iterator<Item = Feature> + '_ {
        self.entries.iter().map(ActiveFeature::feature)
    }

    pub(crate) fn running(&self) -> impl Iterator<Item = Feature> + '_ {
        self.entries
            .iter()
            .filter(|entry| entry.phase == Phase::Running)
            .map(ActiveFeature::feature)
    }

    /// The lowest free index of `class` with this device's parity: the device with the lower
    /// id proposes even indices and the other odd ones, so both can propose at once without
    /// colliding.
    pub(crate) fn free_index(&self, class: MuxClass, odd: bool) -> Option<u8> {
        (u8::from(odd)..u8::MAX).step_by(2).find(|index| {
            !self.entries.iter().any(|entry| {
                entry
                    .stream
                    .is_some_and(|(stream, _)| stream.class == class && stream.index == *index)
            })
        })
    }
}
