use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::time::Duration;

use llts_wire::packet::{
    BodyPrefix, ControlEnvelope, FeedbackReport, MediaPacketView, NackRequest, PacketFlags,
    PacketHeader, PacketKind, PathProbe, PathProbeAck, RepairPacketView,
};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, StreamClass,
    WireError,
};

use super::stream_table::{StreamChannel, StreamTable};
use super::{
    Action, ClockSync, Event, Liveness, MediaBudget, PathChallenge, PathId, PathManager,
    PeerConfig, SentHistory, SentPacket, SessionControl, SessionPools, TransportFeedback,
    frame_byte_budget,
};
use crate::CoreError;
use crate::buffer::Datagram;
use crate::channel::{Delivery, InputChannel, InputDelivery, ReliableChannel};
use crate::clock::{Instant, RoundTripFilter};
use crate::congestion::{AckedPacket, CongestionController};
use crate::link::LinkQuality;
use crate::send::{LanePacer, PacerLane};
use crate::stats::StatsSnapshot;
use crate::stream::{MediaKind, MediaPools, MediaStream, MediaStreamConfig, StreamKey};
use crate::units::{Bitrate, LossRate};

#[derive(Debug, Clone, Copy)]
struct PendingProbe {
    token: u32,
    sent_at: Instant,
}

/// Where a stream's output goes while the session services it.
struct Outlets<'a> {
    pacer: &'a mut LanePacer,
    actions: &'a mut VecDeque<Action>,
    keyframe_requests: &'a mut Vec<StreamKey>,
    active_path: PathId,
    round_trip: Duration,
}

impl Outlets<'_> {
    /// Paced kinds wait in their class's lane; feedback, NACKs and probes leave at once, since
    /// their whole value is arriving quickly and they are too small to matter to the pacer.
    fn route(&mut self, class: StreamClass, datagram: Datagram) {
        if BodyPrefix::is_carried_by(datagram.header().kind) {
            self.pacer.enqueue(PacerLane::for_class(class), datagram);
        } else {
            self.actions.push_back(Action::Transmit {
                path: self.active_path,
                datagram,
            });
        }
    }
}

/// The sans-I/O session with one peer: every stream, one pacer, one congestion controller.
///
/// Takes `(now, Event)` and yields [`Action`]s. It reads no clock, owns no socket and spawns no
/// thread, which is what makes a run exactly reproducible and lets Linux and Android execute
/// byte-identical logic.
///
/// Streams are opened explicitly and anything addressed to one that is not open is refused.
/// Paced packets from every stream share one departure clock, with control, input and audio
/// ahead of video, and each carries a transport sequence so the peer's arrival reports feed a
/// single delay-based controller whose estimate the session then divides between the streams.
#[derive(Debug)]
pub struct PeerSession {
    config: PeerConfig,
    pools: SessionPools,
    streams: StreamTable,
    pacer: LanePacer,
    congestion: CongestionController,
    round_trip: RoundTripFilter,
    quality: LinkQuality,
    sent: SentHistory,
    feedback: TransportFeedback,
    transport_sequence: SequenceNumber,
    probe_sequence: SequenceNumber,
    pending_probes: [Option<PendingProbe>; Self::PENDING_PROBES],
    next_probe_slot: usize,
    next_probe_token: u32,
    liveness: Liveness,
    clock: ClockSync,
    path: PathManager,
    actions: VecDeque<Action>,
    keyframe_requests: Vec<StreamKey>,
}

impl PeerSession {
    const PACER_CAPACITY_BYTES: usize = 1 << 22;

    /// Reliable fragments a lane may hold before its channel is asked for more, so a channel's
    /// retransmit timer runs from when a fragment nearly leaves, not from when it was queued.
    const RELIABLE_LANE_DEPTH: usize = 4;

    const PENDING_PROBES: usize = 8;

    pub fn new(config: PeerConfig, pools: SessionPools) -> Self {
        let congestion = CongestionController::new(
            config.initial_bitrate,
            config.min_bitrate,
            config.max_bitrate,
        );
        let mut streams = StreamTable::new();
        let control = ReliableChannel::new(
            StreamKey::CONTROL,
            config.peer_tag,
            config.sizing,
            pools.datagrams.clone(),
            pools.messages.clone(),
        );
        let _ = streams.insert(
            StreamKey::CONTROL,
            StreamChannel::Reliable(Box::new(control)),
        );
        Self {
            pacer: LanePacer::new(congestion.pacing_rate(), Self::PACER_CAPACITY_BYTES),
            congestion,
            round_trip: RoundTripFilter::new(),
            quality: LinkQuality::pristine(config.link),
            sent: SentHistory::new(),
            feedback: TransportFeedback::new(config.sizing),
            transport_sequence: SequenceNumber::ZERO,
            probe_sequence: SequenceNumber::ZERO,
            pending_probes: [None; Self::PENDING_PROBES],
            next_probe_slot: 0,
            next_probe_token: 1,
            liveness: Liveness::new(),
            clock: ClockSync::new(),
            path: PathManager::new(PathId::default()),
            actions: VecDeque::with_capacity(64),
            keyframe_requests: Vec::with_capacity(StreamTable::MAX_STREAMS),
            streams,
            pools,
            config,
        }
    }

    pub const fn config(&self) -> &PeerConfig {
        &self.config
    }

    pub const fn link_quality(&self) -> LinkQuality {
        self.quality
    }

    pub const fn target_bitrate(&self) -> Bitrate {
        self.congestion.target_bitrate()
    }

    pub const fn active_path(&self) -> PathId {
        self.path.active()
    }

    pub const fn clock(&self) -> &ClockSync {
        &self.clock
    }

    pub const fn is_dead(&self) -> bool {
        self.liveness.is_dead()
    }

    pub fn open_media(
        &mut self,
        key: StreamKey,
        config: MediaStreamConfig,
    ) -> Result<(), CoreError> {
        let pools = MediaPools {
            datagrams: self.pools.datagrams.clone(),
            frames: self.pools.frames.clone(),
            shards: self.pools.shards.clone(),
        };
        let stream = MediaStream::new(key, self.config.peer_tag, config, self.config.sizing, pools)
            .ok_or(CoreError::StreamClassMismatch {
                class: key.class.name(),
                kind: "media",
            })?;
        self.streams
            .insert(key, StreamChannel::Media(Box::new(stream)))?;
        self.emit_rate_actions();
        Ok(())
    }

    pub fn open_reliable(&mut self, key: StreamKey) -> Result<(), CoreError> {
        if !matches!(key.class, StreamClass::Control | StreamClass::Data) {
            return Err(CoreError::StreamClassMismatch {
                class: key.class.name(),
                kind: "reliable",
            });
        }
        let channel = ReliableChannel::new(
            key,
            self.config.peer_tag,
            self.config.sizing,
            self.pools.datagrams.clone(),
            self.pools.messages.clone(),
        );
        self.streams
            .insert(key, StreamChannel::Reliable(Box::new(channel)))?;
        self.emit_rate_actions();
        Ok(())
    }

    pub fn open_input(&mut self, key: StreamKey) -> Result<(), CoreError> {
        if key.class != StreamClass::Input {
            return Err(CoreError::StreamClassMismatch {
                class: key.class.name(),
                kind: "input",
            });
        }
        let channel = InputChannel::new(
            key,
            self.config.peer_tag,
            self.config.sizing,
            self.pools.datagrams.clone(),
        );
        self.streams
            .insert(key, StreamChannel::Input(Box::new(channel)))
    }

    /// Closes a stream; the control stream stays open for the life of the session.
    pub fn close(&mut self, key: StreamKey) -> bool {
        if key == StreamKey::CONTROL {
            return false;
        }
        let closed = self.streams.remove(key).is_some();
        if closed {
            self.emit_rate_actions();
        }
        closed
    }

    pub fn poll_action(&mut self) -> Option<Action> {
        self.actions.pop_front()
    }

    /// The earliest instant at which the session has something to do.
    pub fn next_timeout(&self, now: Instant) -> Option<Instant> {
        let round_trip = self.quality.round_trip;
        let streams = self.streams.iter().map(|channel| match channel {
            StreamChannel::Media(stream) => stream.next_timeout(),
            StreamChannel::Reliable(channel) => channel.next_timeout(now, round_trip),
            StreamChannel::Input(channel) => channel.next_timeout(),
        });
        [
            self.pacer.next_departure(now),
            self.feedback.next_deadline(),
            self.liveness.next_deadline(now),
            self.path.next_deadline(),
        ]
        .into_iter()
        .chain(streams)
        .flatten()
        .min()
    }

    /// Advances time without an external event.
    ///
    /// The pacer releases datagrams on a clock, not on arrivals, so a driver that only called
    /// [`PeerSession::handle`] would leave a frame's tail sitting in the queue until something
    /// else happened to arrive.
    pub fn poll(&mut self, now: Instant) -> Result<(), CoreError> {
        self.pump(now)
    }

    pub fn handle(&mut self, now: Instant, event: &Event<'_>) -> Result<(), CoreError> {
        match event {
            Event::SliceEncoded { stream, slice } => {
                let quality = self.quality;
                match self.stream_mut(*stream)? {
                    StreamChannel::Media(media) => media.on_slice(*slice, &quality, now)?,
                    StreamChannel::Reliable(_) | StreamChannel::Input(_) => {
                        return Err(mismatch(*stream, PacketKind::Media));
                    }
                }
                self.liveness.note_media(now);
            }
            Event::DatagramReceived {
                path,
                bytes,
                arrived_at,
            } => self.on_datagram(*path, bytes, *arrived_at, now)?,
            Event::ControlQueued {
                stream,
                message,
                delivery,
            } => {
                if *stream == StreamKey::CONTROL && SessionControl::is_reserved(message) {
                    return Err(CoreError::ReservedMessageType {
                        message_type: SessionControl::message_type(message).unwrap_or(0),
                    });
                }
                match self.stream_mut(*stream)? {
                    StreamChannel::Reliable(channel) => channel.queue(message, *delivery)?,
                    StreamChannel::Media(_) | StreamChannel::Input(_) => {
                        return Err(mismatch(*stream, PacketKind::Control));
                    }
                }
            }
            Event::InputQueued { stream, events } => match self.stream_mut(*stream)? {
                StreamChannel::Input(channel) => channel.queue_events(events, now)?,
                StreamChannel::Media(_) | StreamChannel::Reliable(_) => {
                    return Err(mismatch(*stream, PacketKind::Media));
                }
            },
            Event::KeyframeNeeded { stream } => match self.stream_mut(*stream)? {
                StreamChannel::Media(media) => media.request_keyframe(),
                StreamChannel::Input(_) | StreamChannel::Reliable(_) => {
                    return Err(mismatch(*stream, PacketKind::Media));
                }
            },
            Event::KeyStateUpdated { stream, state } => match self.stream_mut(*stream)? {
                StreamChannel::Input(channel) => channel.update_key_state(state, now)?,
                StreamChannel::Media(_) | StreamChannel::Reliable(_) => {
                    return Err(mismatch(*stream, PacketKind::Media));
                }
            },
            Event::PathCandidate(path) => {
                let challenge = self.path.begin_validation(*path, now);
                self.send_challenge(challenge, now)?;
            }
            Event::LocalAddressChanged => {
                let token = self.take_probe_token();
                self.send_probe(self.path.active(), PacketFlags::empty(), token, now)?;
            }
            Event::LinkChanged(kind) => {
                self.quality.kind = *kind;
                self.config.link = *kind;
            }
            Event::TimerExpired => {}
        }
        self.pump(now)
    }

    pub fn stats(&self, key: StreamKey) -> Option<StatsSnapshot> {
        match self.streams.get(key)? {
            StreamChannel::Media(stream) => {
                Some(stream.stats(&self.quality, self.congestion.pacing_rate()))
            }
            StreamChannel::Reliable(_) | StreamChannel::Input(_) => None,
        }
    }

    fn stream_mut(&mut self, key: StreamKey) -> Result<&mut StreamChannel, CoreError> {
        let stream = key.with_tag(self.config.local_tag).get();
        self.streams
            .get_mut(key)
            .ok_or(CoreError::UnknownStream { stream })
    }

    fn on_datagram(
        &mut self,
        path: PathId,
        bytes: &[u8],
        arrived_at: MonotonicMicros,
        now: Instant,
    ) -> Result<(), CoreError> {
        let (header, body) = PacketHeader::read_from(bytes)?;
        let unknown = CoreError::UnknownStream {
            stream: header.stream.get(),
        };
        if header.stream.session_tag() != self.config.local_tag {
            return Err(unknown);
        }
        let key = StreamKey::from_stream_id(header.stream).ok_or(unknown.clone())?;
        if self.streams.get(key).is_none() {
            return Err(unknown);
        }
        if header.kind == PacketKind::Handshake {
            return Ok(());
        }

        self.liveness.note_inbound(now);
        if let Some(challenge) = self.path.observe_arrival(path, now) {
            self.send_challenge(challenge, now)?;
        }
        if BodyPrefix::is_carried_by(header.kind) {
            let (prefix, _) = BodyPrefix::split_from(&header, body)?;
            if self
                .feedback
                .record(prefix.transport_sequence, arrived_at, now)
            {
                self.flush_feedback(now)?;
            }
        }

        match header.kind {
            PacketKind::Feedback if key == StreamKey::CONTROL => {
                self.on_transport_feedback(&FeedbackReport::parse(bytes)?, now);
            }
            PacketKind::Probe if key == StreamKey::CONTROL => {
                self.answer_probe(&header, PathProbe::parse(bytes)?, path, arrived_at, now)?;
            }
            PacketKind::ProbeAck if key == StreamKey::CONTROL => {
                self.on_probe_ack(&header, PathProbeAck::parse(bytes)?, path, now);
            }
            PacketKind::Media | PacketKind::Repair | PacketKind::Nack | PacketKind::Control => {
                self.dispatch_to_stream(key, header.kind, bytes, now)?;
            }
            kind => return Err(mismatch(key, kind)),
        }
        Ok(())
    }

    fn dispatch_to_stream(
        &mut self,
        key: StreamKey,
        kind: PacketKind,
        bytes: &[u8],
        now: Instant,
    ) -> Result<(), CoreError> {
        let mut frame_completed = false;
        match (self.stream_mut(key)?, kind) {
            (StreamChannel::Media(stream), PacketKind::Media) => {
                stream.on_media(&MediaPacketView::parse(bytes)?, now)?;
                frame_completed =
                    stream.take_frame_completed() && stream.kind() == MediaKind::Video;
            }
            (StreamChannel::Media(stream), PacketKind::Repair) => {
                stream.on_repair(&RepairPacketView::parse(bytes)?, now)?;
                frame_completed =
                    stream.take_frame_completed() && stream.kind() == MediaKind::Video;
            }
            (StreamChannel::Media(stream), PacketKind::Nack) => {
                stream.on_nack(&NackRequest::parse(bytes)?, now)?;
            }
            (StreamChannel::Reliable(channel), PacketKind::Control) => {
                channel.on_envelope(&ControlEnvelope::parse(bytes)?, now)?;
            }
            (StreamChannel::Input(channel), PacketKind::Media) => {
                channel.on_media(&MediaPacketView::parse(bytes)?)?;
            }
            _ => return Err(mismatch(key, kind)),
        }
        if matches!(key.class, StreamClass::Video | StreamClass::Audio) {
            self.liveness.note_media(now);
        }
        if frame_completed {
            self.flush_feedback(now)?;
        }
        Ok(())
    }

    fn on_transport_feedback(&mut self, report: &FeedbackReport<'_>, now: Instant) {
        for entry in report.entries() {
            let Some(sent) = self.sent.take(entry.transport_sequence) else {
                continue;
            };
            self.round_trip.observe(Duration::from_micros(u64::from(
                now.wire_micros().elapsed_since(sent.departure),
            )));
            self.congestion.observe(
                AckedPacket {
                    departure: sent.departure,
                    arrival: entry.arrival,
                    byte_len: sent.byte_len,
                },
                now,
            );
        }

        // The peer's own measurement, not ours: a gap it recovered by asking us to resend is
        // invisible from this side, and that is exactly the loss the repair policy must price.
        self.quality.loss_rate = LossRate::from_per_mille(report.loss_per_mille());
        self.quality.round_trip = self.round_trip.best();
        self.pacer.set_rate(self.congestion.pacing_rate());
        self.emit_rate_actions();
    }

    fn emit_rate_actions(&mut self) {
        let sending_video = |stream: &&MediaStream| {
            stream.kind() == MediaKind::Video && stream.config().role.sends()
        };
        let reserved_bits_per_second = self
            .streams
            .media()
            .filter(|stream| stream.config().role.sends())
            .filter_map(MediaStream::reserved_bitrate)
            .map(|rate| u64::from(rate.bits_per_second()))
            .sum();
        let reserves_data_share = self.streams.iter().any(|channel| {
            matches!(channel, StreamChannel::Reliable(reliable) if reliable.key().class == StreamClass::Data)
        });
        let video_streams =
            u32::try_from(self.streams.media().filter(sending_video).count()).unwrap_or(u32::MAX);
        let budget = MediaBudget {
            estimate: self.congestion.target_bitrate(),
            reserved_bits_per_second,
            reserves_data_share,
            video_streams,
        };
        for stream in self.streams.media().filter(sending_video) {
            let repair = stream.repair_rate();
            self.actions.push_back(Action::SetTargetBitrate {
                stream: stream.key(),
                bitrate: budget.encoder_target(repair),
            });
            self.actions.push_back(Action::SetFrameByteBudget {
                stream: stream.key(),
                bytes: frame_byte_budget(budget.video_share(), stream.config().frame_rate, repair),
            });
        }
    }

    fn answer_probe(
        &mut self,
        header: &PacketHeader,
        probe: PathProbe,
        path: PathId,
        arrived_at: MonotonicMicros,
        now: Instant,
    ) -> Result<(), CoreError> {
        let ack = PathProbeAck {
            token: probe.token,
            probe_arrival: arrived_at,
            ack_departure: now.wire_micros(),
        };
        let flags = header.flags & PacketFlags::PATH_CHALLENGE;
        let datagram = self.build_session_packet(PacketKind::ProbeAck, flags, now, |body| {
            ack.write_body(body)
        })?;
        self.actions.push_back(Action::Transmit { path, datagram });
        Ok(())
    }

    fn on_probe_ack(
        &mut self,
        header: &PacketHeader,
        ack: PathProbeAck,
        path: PathId,
        now: Instant,
    ) {
        if header.flags.contains(PacketFlags::PATH_CHALLENGE)
            && let Some(switched) = self.path.on_challenge_ack(path, ack.token)
        {
            self.switch_path(switched);
        }
        let Some(probe) = self.take_pending_probe(ack.token) else {
            return;
        };
        let turnaround = Duration::from_micros(u64::from(
            ack.ack_departure.elapsed_since(ack.probe_arrival),
        ));
        self.round_trip.observe(
            now.saturating_elapsed_since(probe.sent_at)
                .saturating_sub(turnaround),
        );
        self.quality.round_trip = self.round_trip.best();
        if self
            .clock
            .observe(probe.sent_at, ack.probe_arrival, ack.ack_departure, now)
        {
            self.actions.push_back(Action::PeerClockUpdated {
                offset_micros: self.clock.offset_micros(now),
                skew_ppb: self.clock.skew_ppb(),
            });
        }
    }

    /// A new path starts from scratch: its capacity has nothing to do with the old one's.
    fn switch_path(&mut self, path: PathId) {
        self.congestion = CongestionController::new(
            self.config.initial_bitrate,
            self.config.min_bitrate,
            self.config.max_bitrate,
        );
        self.sent.clear();
        self.round_trip = RoundTripFilter::new();
        self.quality.round_trip = Duration::ZERO;
        self.pacer.set_rate(self.congestion.pacing_rate());
        self.actions.push_back(Action::PathChanged(path));
        self.emit_rate_actions();
    }

    fn send_challenge(&mut self, challenge: PathChallenge, now: Instant) -> Result<(), CoreError> {
        self.send_probe(
            challenge.path,
            PacketFlags::PATH_CHALLENGE,
            challenge.token,
            now,
        )
    }

    fn send_probe(
        &mut self,
        path: PathId,
        flags: PacketFlags,
        token: u32,
        now: Instant,
    ) -> Result<(), CoreError> {
        let probe = PathProbe { token };
        let datagram = self
            .build_session_packet(PacketKind::Probe, flags, now, |body| probe.write_body(body))?;
        if let Some(slot) = self.pending_probes.get_mut(self.next_probe_slot) {
            *slot = Some(PendingProbe {
                token,
                sent_at: now,
            });
        }
        self.next_probe_slot = (self.next_probe_slot + 1) % Self::PENDING_PROBES;
        self.actions.push_back(Action::Transmit { path, datagram });
        Ok(())
    }

    fn take_pending_probe(&mut self, token: u32) -> Option<PendingProbe> {
        self.pending_probes
            .iter_mut()
            .find(|slot| slot.is_some_and(|probe| probe.token == token))
            .and_then(Option::take)
    }

    /// A probe or probe acknowledgement on the control stream, numbered in the probe space.
    fn build_session_packet(
        &mut self,
        kind: PacketKind,
        flags: PacketFlags,
        now: Instant,
        write_body: impl FnOnce(&mut [u8]) -> Result<usize, WireError>,
    ) -> Result<Datagram, CoreError> {
        let header = PacketHeader {
            version: ProtocolVersion::CURRENT,
            kind,
            flags,
            stream: StreamKey::CONTROL.with_tag(self.config.peer_tag),
            sequence: self.probe_sequence,
            frame: FrameId::ZERO,
            timestamp: MediaTimestamp::ZERO,
            packet_index: 0,
            packet_count: 1,
            departure: now.wire_micros(),
            payload_offset: 0,
        };
        self.probe_sequence = self.probe_sequence.next();
        let mut datagram = Datagram::assemble(&header, &[], &self.pools.datagrams)?;
        datagram.append_with(self.config.sizing.max_payload_len(), write_body)?;
        Ok(datagram)
    }

    fn flush_feedback(&mut self, now: Instant) -> Result<(), CoreError> {
        let stream = StreamKey::CONTROL.with_tag(self.config.peer_tag);
        if let Some(datagram) = self.feedback.build(stream, now, &self.pools.datagrams)? {
            self.actions.push_back(Action::Transmit {
                path: self.path.active(),
                datagram,
            });
        }
        Ok(())
    }

    /// Drains whatever is due: stream output, reports, probes, paced datagrams, and the timer
    /// for whichever comes next.
    fn pump(&mut self, now: Instant) -> Result<(), CoreError> {
        self.service_streams(now)?;
        if self.feedback.is_due(now) {
            self.flush_feedback(now)?;
        }
        if self.liveness.take_newly_dead(now) {
            self.actions.push_back(Action::PeerDead);
        }
        if self.liveness.take_probe_due(now) {
            let token = self.take_probe_token();
            self.send_probe(self.path.active(), PacketFlags::empty(), token, now)?;
        }
        if let Some(challenge) = self.path.poll(now) {
            self.send_challenge(challenge, now)?;
        }
        self.release_paced(now)?;
        if let Some(deadline) = self.next_timeout(now) {
            self.actions.push_back(Action::ArmTimer(deadline));
        }
        Ok(())
    }

    fn service_streams(&mut self, now: Instant) -> Result<(), CoreError> {
        let mut outlets = Outlets {
            pacer: &mut self.pacer,
            actions: &mut self.actions,
            keyframe_requests: &mut self.keyframe_requests,
            active_path: self.path.active(),
            round_trip: self.quality.round_trip,
        };
        for channel in self.streams.iter_mut() {
            service_channel(channel, &mut outlets, now)?;
        }
        if self.keyframe_requests.is_empty() {
            return Ok(());
        }
        let Some(control) = self.streams.get_mut(StreamKey::CONTROL) else {
            return Ok(());
        };
        if let StreamChannel::Reliable(channel) = &mut *control {
            for stream in self.keyframe_requests.drain(..) {
                let (bytes, len) = SessionControl::KeyframeRequest(stream).encode();
                channel.queue(bytes.get(..len).unwrap_or_default(), Delivery::Ordered)?;
            }
        }
        let mut outlets = Outlets {
            pacer: &mut self.pacer,
            actions: &mut self.actions,
            keyframe_requests: &mut self.keyframe_requests,
            active_path: self.path.active(),
            round_trip: self.quality.round_trip,
        };
        service_channel(control, &mut outlets, now)
    }

    fn release_paced(&mut self, now: Instant) -> Result<(), CoreError> {
        while let Some((_, mut datagram)) = self.pacer.poll_departure(now) {
            let mut header = *datagram.header();
            header.departure = now.wire_micros();
            datagram.rewrite_header(&header)?;
            let transport_sequence = self.transport_sequence;
            datagram.stamp_transport_sequence(transport_sequence)?;
            self.transport_sequence = transport_sequence.next();
            self.sent.record(SentPacket {
                transport_sequence,
                departure: header.departure,
                byte_len: u32::try_from(datagram.byte_len()).unwrap_or(u32::MAX),
            });
            self.actions.push_back(Action::Transmit {
                path: self.path.active(),
                datagram,
            });
        }
        Ok(())
    }

    fn take_probe_token(&mut self) -> u32 {
        let token = self.next_probe_token;
        self.next_probe_token = token.wrapping_add(1).max(1);
        token
    }
}

fn service_channel(
    channel: &mut StreamChannel,
    outlets: &mut Outlets<'_>,
    now: Instant,
) -> Result<(), CoreError> {
    match channel {
        StreamChannel::Media(stream) => {
            let key = stream.key();
            stream.poll(now);
            for datagram in stream.drain_outgoing() {
                outlets.route(key.class, datagram);
            }
            while let Some(frame) = stream.poll_present(now) {
                outlets
                    .actions
                    .push_back(Action::PresentFrame { stream: key, frame });
            }
            if stream.take_keyframe_request(now) {
                outlets.keyframe_requests.push(key);
            }
        }
        StreamChannel::Reliable(channel) => {
            let key = channel.key();
            let lane = PacerLane::for_class(key.class);
            while outlets.pacer.queued_datagrams(lane) < PeerSession::RELIABLE_LANE_DEPTH {
                let Some(datagram) = channel.poll_transmit(now, outlets.round_trip)? else {
                    break;
                };
                outlets.pacer.enqueue(lane, datagram);
            }
            while let Some(message) = channel.poll_delivery() {
                if key == StreamKey::CONTROL && SessionControl::is_reserved(message.filled()) {
                    if let Some(SessionControl::KeyframeRequest(stream)) =
                        SessionControl::decode(message.filled())
                    {
                        outlets
                            .actions
                            .push_back(Action::RequestKeyframe { stream });
                    }
                    continue;
                }
                outlets.actions.push_back(Action::Deliver {
                    stream: key,
                    message,
                });
            }
        }
        StreamChannel::Input(channel) => {
            let key = channel.key();
            channel.poll(now)?;
            for datagram in channel.drain_outgoing() {
                outlets.route(key.class, datagram);
            }
            while let Some(delivery) = channel.poll_delivery() {
                outlets.actions.push_back(match delivery {
                    InputDelivery::Events(message) => Action::Deliver {
                        stream: key,
                        message,
                    },
                    InputDelivery::KeyState(state) => Action::InputKeyState { stream: key, state },
                });
            }
        }
    }
    Ok(())
}

const fn mismatch(key: StreamKey, kind: PacketKind) -> CoreError {
    CoreError::StreamClassMismatch {
        class: key.class.name(),
        kind: kind.name(),
    }
}
