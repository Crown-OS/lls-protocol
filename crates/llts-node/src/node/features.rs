//! Feature toggles and the stream negotiation behind media features.

use llts_core::session::Role;
use llts_signaling::device::{DeviceId, Feature};
use llts_signaling::message::{
    AcceptStream, CloseStream, OpenStream, Signal, Stop, StreamParams, StreamRef,
};

use crate::feature::{ActiveFeature, FeatureRequest, FeatureState, MediaRole, PendingStart, Phase};
use crate::output::{
    AppEvent, DriverCommand, FeatureFailure, MediaRequest, NodeOutput, PersistRequest,
};
use crate::peer::Session;
use crate::util::stream::CONTROL;
use crate::{Node, NodeError};

impl Node {
    pub(super) fn set_feature(
        &mut self,
        peer: DeviceId,
        feature: Feature,
        enabled: bool,
    ) -> Result<(), NodeError> {
        let entry = self
            .peers
            .get_mut(&peer)
            .ok_or(NodeError::UnknownPeer(peer))?;
        let updated = if enabled {
            entry.enabled.with(feature)
        } else {
            entry.enabled.without(feature)
        };
        if updated == entry.enabled {
            return Ok(());
        }
        entry.enabled = updated;
        let connected = entry.session.is_some();
        self.push(NodeOutput::Persist(PersistRequest::PeerFeatures {
            peer,
            enabled: updated,
        }));
        self.emit(AppEvent::FeatureStateChanged {
            peer,
            feature,
            state: if enabled {
                FeatureState::Enabled
            } else {
                FeatureState::Disabled
            },
        });
        if connected {
            self.announce(peer)?;
            self.renegotiate(peer)?;
        } else {
            self.emit(AppEvent::DevicesChanged);
        }
        Ok(())
    }

    pub(super) fn start_feature(
        &mut self,
        peer: DeviceId,
        request: FeatureRequest,
    ) -> Result<(), NodeError> {
        let feature = request.feature();
        if self.defer_until_capabilities(peer, request) {
            return Ok(());
        }
        let entry = self.peers.get(&peer).ok_or(NodeError::UnknownPeer(peer))?;
        let failure = match &entry.session {
            None => Some(FeatureFailure::NotConnected),
            Some(_) if !self.shared_features(entry).contains(feature) => {
                Some(FeatureFailure::Unavailable)
            }
            Some(session) if session.features.get(feature).is_some() => {
                Some(FeatureFailure::AlreadyActive)
            }
            Some(_) => None,
        };
        if let Some(reason) = failure {
            self.emit(AppEvent::FeatureFailed {
                peer,
                feature,
                reason,
            });
            return Ok(());
        }
        let role = request.requester_role();
        let proposal = match role {
            MediaRole::Encoder => match self.proposal(peer, &request) {
                Some(proposal) => Some(proposal),
                None => {
                    self.emit(AppEvent::FeatureFailed {
                        peer,
                        feature,
                        reason: FeatureFailure::NoCommonCodec,
                    });
                    return Ok(());
                }
            },
            MediaRole::Decoder => None,
        };
        self.send(peer, CONTROL, &request.as_signal())?;
        match proposal {
            Some((stream, params)) => self.propose(peer, request, stream, params),
            None => {
                self.insert_feature(
                    peer,
                    ActiveFeature {
                        request,
                        role,
                        phase: Phase::AwaitingProposal,
                        stream: None,
                    },
                );
                Ok(())
            }
        }
    }

    /// Parks `request` when the session is up but the peer has not announced what it supports
    /// yet; it is decided once the capabilities arrive or the wait runs out.
    fn defer_until_capabilities(&mut self, peer: DeviceId, request: FeatureRequest) -> bool {
        let expires_at = self
            .now
            .instant
            .saturating_add(self.config.capabilities_wait);
        let Some(session) = self
            .session_mut(&peer)
            .filter(|session| session.capabilities.is_none())
        else {
            return false;
        };
        session.pending_starts.queue(PendingStart {
            request,
            expires_at,
        });
        true
    }

    /// Decides every start that waited for the capabilities that just arrived.
    pub(super) fn start_deferred(&mut self, peer: DeviceId) -> Result<(), NodeError> {
        let Some(session) = self.session_mut(&peer) else {
            return Ok(());
        };
        for pending in session.pending_starts.drain() {
            self.start_feature(peer, pending.request)?;
        }
        Ok(())
    }

    /// Gives up on starts whose peer never announced its capabilities in time.
    pub(super) fn service_deferred_starts(&mut self) {
        let now = self.now.instant;
        let expired: Vec<(DeviceId, Feature)> = self
            .peers
            .iter_mut()
            .filter_map(|entry| Some((entry.id, entry.session.as_mut()?)))
            .flat_map(|(peer, session)| {
                session
                    .pending_starts
                    .take_expired(now)
                    .map(move |pending| (peer, pending.request.feature()))
            })
            .collect();
        for (peer, feature) in expired {
            self.emit(AppEvent::FeatureFailed {
                peer,
                feature,
                reason: FeatureFailure::Unavailable,
            });
        }
    }

    pub(super) fn on_feature_request(
        &mut self,
        peer: DeviceId,
        signal: &Signal<'_>,
    ) -> Result<(), NodeError> {
        let Some(request) = FeatureRequest::from_peer(signal) else {
            return Ok(());
        };
        let feature = request.feature();
        let Some(entry) = self.peers.get(&peer) else {
            return Ok(());
        };
        let allowed = self.shared_features(entry).contains(feature)
            && entry
                .session
                .as_ref()
                .is_some_and(|session| session.features.get(feature).is_none());
        if !allowed {
            return self.send(peer, CONTROL, &Signal::StopV1(Stop { feature }));
        }
        match request.responder_role() {
            MediaRole::Encoder => match self.proposal(peer, &request) {
                Some((stream, params)) => self.propose(peer, request, stream, params),
                None => self.send(peer, CONTROL, &Signal::StopV1(Stop { feature })),
            },
            MediaRole::Decoder => {
                self.insert_feature(
                    peer,
                    ActiveFeature {
                        request,
                        role: MediaRole::Decoder,
                        phase: Phase::AwaitingProposal,
                        stream: None,
                    },
                );
                Ok(())
            }
        }
    }

    /// The stream this device would propose as the encoder: codecs from its own preference
    /// order among what the peer decodes, and an index of its own parity.
    fn proposal(
        &self,
        peer: DeviceId,
        request: &FeatureRequest,
    ) -> Option<(StreamRef, StreamParams)> {
        let session = self.peers.get(&peer)?.session.as_ref()?;
        let theirs = session.capabilities.as_ref()?;
        let ours = &self.local.capabilities;
        let params = request.params(
            ours.video_codec_with(theirs),
            ours.audio_codec_with(theirs),
            self.config.video_bitrate_kbps,
        )?;
        let class = params.class();
        let index = session.features.free_index(class, self.id > peer)?;
        Some((StreamRef { class, index }, params))
    }

    fn propose(
        &mut self,
        peer: DeviceId,
        request: FeatureRequest,
        stream: StreamRef,
        params: StreamParams,
    ) -> Result<(), NodeError> {
        self.send(
            peer,
            CONTROL,
            &Signal::OpenStreamV1(OpenStream {
                index: stream.index,
                purpose: request.feature(),
                params,
            }),
        )?;
        self.command(DriverCommand::OpenMedia {
            peer,
            stream,
            params,
            role: Role::Sender,
        });
        self.insert_feature(
            peer,
            ActiveFeature {
                request,
                role: MediaRole::Encoder,
                phase: Phase::Proposed,
                stream: Some((stream, params)),
            },
        );
        Ok(())
    }

    pub(super) fn on_open_stream(
        &mut self,
        peer: DeviceId,
        open: OpenStream,
    ) -> Result<(), NodeError> {
        let stream = open.stream();
        let decodable = match open.params {
            StreamParams::Video(video) => {
                self.local.capabilities.video_codecs.contains(&video.codec)
            }
            StreamParams::Audio(audio) => {
                self.local.capabilities.audio_codecs.contains(&audio.codec)
            }
        };
        let Some(active) = self.session_mut(&peer).and_then(|session| {
            session.features.get_mut(open.purpose).filter(|active| {
                active.role == MediaRole::Decoder && active.phase == Phase::AwaitingProposal
            })
        }) else {
            return self.send(
                peer,
                CONTROL,
                &Signal::CloseStreamV1(CloseStream { stream }),
            );
        };
        if !decodable {
            let feature = active.feature();
            self.send(
                peer,
                CONTROL,
                &Signal::CloseStreamV1(CloseStream { stream }),
            )?;
            return self.stop_feature_with(peer, feature, Some(FeatureFailure::NoCommonCodec));
        }
        active.phase = Phase::Running;
        active.stream = Some((stream, open.params));
        let request = active.request;
        self.command(DriverCommand::OpenMedia {
            peer,
            stream,
            params: open.params,
            role: Role::Receiver,
        });
        self.send(
            peer,
            CONTROL,
            &Signal::AcceptStreamV1(AcceptStream {
                stream,
                codec: open.params.codec(),
            }),
        )?;
        self.started(peer, request, MediaRole::Decoder, stream, open.params);
        Ok(())
    }

    pub(super) fn on_accept_stream(&mut self, peer: DeviceId, stream: StreamRef) {
        let Some(active) = self.session_mut(&peer).and_then(|session| {
            session
                .features
                .by_stream(stream)
                .map(ActiveFeature::feature)
                .and_then(|feature| session.features.get_mut(feature))
                .filter(|active| active.phase == Phase::Proposed)
        }) else {
            return;
        };
        active.phase = Phase::Running;
        let request = active.request;
        let Some((stream, params)) = active.stream else {
            return;
        };
        self.started(peer, request, MediaRole::Encoder, stream, params);
    }

    fn started(
        &mut self,
        peer: DeviceId,
        request: FeatureRequest,
        role: MediaRole,
        stream: StreamRef,
        params: StreamParams,
    ) {
        self.push(NodeOutput::Media(MediaRequest::Start {
            peer,
            request,
            role,
            stream,
            params,
        }));
        self.emit(AppEvent::FeatureStateChanged {
            peer,
            feature: request.feature(),
            state: FeatureState::Active,
        });
        self.emit(AppEvent::DevicesChanged);
    }

    pub(super) fn stop_feature(
        &mut self,
        peer: DeviceId,
        feature: Feature,
    ) -> Result<(), NodeError> {
        if self.peers.get(&peer).is_none() {
            return Err(NodeError::UnknownPeer(peer));
        }
        if self
            .session_mut(&peer)
            .and_then(|session| session.pending_starts.cancel(feature))
            .is_some()
        {
            return Ok(());
        }
        self.stop_feature_with(peer, feature, None)
    }

    /// Stops `feature` on both ends, reporting `failure` if it never got to run.
    pub(super) fn stop_feature_with(
        &mut self,
        peer: DeviceId,
        feature: Feature,
        failure: Option<FeatureFailure>,
    ) -> Result<(), NodeError> {
        let Some(active) = self
            .session_mut(&peer)
            .and_then(|session| session.features.remove(feature))
        else {
            return Ok(());
        };
        self.send(peer, CONTROL, &Signal::StopV1(Stop { feature }))?;
        self.finish(peer, active, true, failure);
        Ok(())
    }

    pub(super) fn on_stop(&mut self, peer: DeviceId, feature: Feature) {
        if let Some(active) = self
            .session_mut(&peer)
            .and_then(|session| session.features.remove(feature))
        {
            self.finish(peer, active, true, Some(FeatureFailure::Refused));
        }
    }

    pub(super) fn on_close_stream(&mut self, peer: DeviceId, stream: StreamRef) {
        let Some(active) = self.session_mut(&peer).and_then(|session| {
            let feature = session.features.by_stream(stream)?.feature();
            session.features.remove(feature)
        }) else {
            return;
        };
        self.finish(peer, active, true, Some(FeatureFailure::Refused));
    }

    /// Ends every feature of a session that is over or being replaced.
    pub(super) fn tear_down_features(&mut self, peer: DeviceId, mut session: Session, alive: bool) {
        for pending in session.pending_starts.drain() {
            self.emit(AppEvent::FeatureFailed {
                peer,
                feature: pending.request.feature(),
                reason: FeatureFailure::NotConnected,
            });
        }
        for active in session.features.drain() {
            self.finish(peer, active, alive, Some(FeatureFailure::NotConnected));
        }
    }

    /// Releases what `active` held: its stream while the session lives, its pipeline if it ran.
    /// A feature that never ran reports `failure` instead of a state change.
    fn finish(
        &mut self,
        peer: DeviceId,
        active: ActiveFeature,
        alive: bool,
        failure: Option<FeatureFailure>,
    ) {
        let feature = active.feature();
        if let Some((stream, _)) = active.stream {
            if alive {
                self.command(DriverCommand::CloseStream { peer, stream });
            }
            if active.phase == Phase::Running {
                self.push(NodeOutput::Media(MediaRequest::Stop {
                    peer,
                    feature,
                    stream,
                }));
            }
        }
        if active.phase == Phase::Running {
            let enabled = self
                .peers
                .get(&peer)
                .is_some_and(|entry| entry.enabled.contains(feature));
            self.emit(AppEvent::FeatureStateChanged {
                peer,
                feature,
                state: if enabled {
                    FeatureState::Enabled
                } else {
                    FeatureState::Disabled
                },
            });
            self.emit(AppEvent::DevicesChanged);
        } else if let Some(reason) = failure {
            self.emit(AppEvent::FeatureFailed {
                peer,
                feature,
                reason,
            });
        }
    }

    fn insert_feature(&mut self, peer: DeviceId, active: ActiveFeature) {
        if let Some(session) = self.session_mut(&peer) {
            session.features.insert(active);
        }
    }

    fn session_mut(&mut self, peer: &DeviceId) -> Option<&mut Session> {
        self.peers.get_mut(peer)?.session.as_mut()
    }
}
