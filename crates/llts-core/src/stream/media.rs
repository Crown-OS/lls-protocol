use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::time::Duration;

use llts_wire::packet::{
    MediaPacketView, MissingRange, NackRequest, PacketFlags, PacketHeader, PacketKind,
    RepairPacketView,
};
use llts_wire::{FrameId, MediaTimestamp, ProtocolVersion, SequenceNumber, SessionTag, StreamId};

use super::{MediaKind, MediaStreamConfig, StreamKey};
use crate::CoreError;
use crate::buffer::{BufferPool, Datagram};
use crate::clock::Instant;
use crate::link::LinkQuality;
use crate::receive::{
    AssembledFrame, FragmentAcceptance, FrameAssembler, GapTracker, NackScheduler, PlayoutBuffer,
};
use crate::repair::{RepairPlan, RepairPolicy, RepairRate};
use crate::send::{DatagramSizing, EncodedSlice, SendPipeline};
use crate::stats::{LatencyHistogram, StatsSnapshot};
use crate::units::Bitrate;

/// Buffers a stream's pools are drawn from.
#[derive(Debug, Clone)]
pub struct MediaPools {
    /// One datagram each, for the fragments, parity and NACKs this stream builds.
    pub datagrams: BufferPool,
    /// Whole reassembled frames.
    pub frames: BufferPool,
    /// Parity shards held while the receiver waits to see whether it needs them.
    pub shards: BufferPool,
}

/// One media stream's sending and receiving halves.
///
/// Owns fragmentation, parity, retransmission and playout for its stream, but neither pacing nor
/// congestion control: every stream to a peer shares one pacer and one bandwidth estimate, so
/// those live in the session, which hands this stream the link quality it should plan against
/// and collects the datagrams it builds.
#[derive(Debug)]
pub struct MediaStream {
    key: StreamKey,
    kind: MediaKind,
    outbound: StreamId,
    config: MediaStreamConfig,
    sizing: DatagramSizing,
    send: SendPipeline,
    assembler: FrameAssembler,
    playout: PlayoutBuffer,
    arrived: VecDeque<AssembledFrame>,
    gaps: GapTracker,
    nacks: NackScheduler,
    policy: RepairPolicy,
    last_plan: RepairPlan,
    latency: LatencyHistogram,
    nack_scratch: Vec<MissingRange>,
    nack_pool: BufferPool,
    nack_sequence: SequenceNumber,
    nacks_outgoing: Vec<Datagram>,
    keyframe_wanted: bool,
    keyframe_seen: bool,
    last_keyframe_request: Option<Instant>,
    incomplete_seen: u64,
    expired_seen: u64,
    frame_completed: bool,
}

impl MediaStream {
    /// How far back the NACK scheduler looks for gaps. At 1080p60 this is roughly two frames of
    /// packets, well beyond anything a retransmission could still repair in time.
    const GAP_SCAN_DEPTH: u16 = 96;

    /// How long a sent datagram stays answerable. Several frame periods: anything older has
    /// missed its playout deadline, so a NACK for it would buy nothing.
    const RETENTION_MICROS: u32 = 200_000;

    /// A receiver asks for a keyframe at most this often while one is on its way.
    const KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_millis(100);

    /// `None` when `key` names a class that does not carry media.
    pub fn new(
        key: StreamKey,
        peer_tag: SessionTag,
        config: MediaStreamConfig,
        sizing: DatagramSizing,
        pools: MediaPools,
    ) -> Option<Self> {
        let kind = MediaKind::for_class(key.class)?;
        let outbound = key.with_tag(peer_tag);
        let policy = RepairPolicy::new(config.repair);
        Some(Self {
            key,
            kind,
            outbound,
            sizing,
            send: SendPipeline::new(outbound, sizing, pools.datagrams.clone(), 1 << 22),
            assembler: FrameAssembler::new(pools.frames, pools.shards),
            playout: PlayoutBuffer::new(config.frame_rate),
            arrived: VecDeque::with_capacity(4),
            gaps: GapTracker::new(),
            nacks: NackScheduler::new(),
            last_plan: policy.plan(1, &LinkQuality::pristine(crate::link::LinkKind::Wifi)),
            policy,
            latency: LatencyHistogram::new(),
            nack_scratch: Vec::with_capacity(16),
            nack_pool: pools.datagrams,
            nack_sequence: SequenceNumber::ZERO,
            nacks_outgoing: Vec::with_capacity(4),
            keyframe_wanted: false,
            keyframe_seen: false,
            last_keyframe_request: None,
            incomplete_seen: 0,
            expired_seen: 0,
            frame_completed: false,
            config,
        })
    }

    pub const fn key(&self) -> StreamKey {
        self.key
    }

    pub const fn kind(&self) -> MediaKind {
        self.kind
    }

    pub const fn config(&self) -> &MediaStreamConfig {
        &self.config
    }

    /// Parity the stream last planned, which the session subtracts from the encoder's budget.
    pub const fn repair_rate(&self) -> RepairRate {
        self.last_plan.rate
    }

    pub const fn reserved_bitrate(&self) -> Option<Bitrate> {
        self.config.reserved_bitrate
    }

    /// Fragments and protects one encoded slice against the link as it currently looks.
    pub fn on_slice(
        &mut self,
        slice: EncodedSlice<'_>,
        quality: &LinkQuality,
        now: Instant,
    ) -> Result<(), CoreError> {
        if !self.config.role.sends() {
            return Ok(());
        }
        let fragments = self.sizing.fragment_count(slice.byte_len());
        let data_shards = u16::try_from(fragments).unwrap_or(u16::MAX);
        self.last_plan = self.policy.plan(data_shards, quality);

        let departure = now.wire_micros();
        self.send.submit_slice(slice, &self.last_plan, departure)?;
        self.send
            .release_older_than(departure, Self::RETENTION_MICROS);
        Ok(())
    }

    pub fn on_media(&mut self, media: &MediaPacketView<'_>, now: Instant) -> Result<(), CoreError> {
        self.note_arrival(media.media_sequence(), media.header().flags, now);
        let acceptance = self.assembler.accept(media, now)?;
        self.absorb(acceptance, now);
        Ok(())
    }

    pub fn on_repair(
        &mut self,
        repair: &RepairPacketView<'_>,
        now: Instant,
    ) -> Result<(), CoreError> {
        self.note_arrival(repair.media_sequence(), repair.header().flags, now);
        let acceptance = self.assembler.accept_repair(repair, now)?;
        self.absorb(acceptance, now);
        Ok(())
    }

    pub fn on_nack(&mut self, request: &NackRequest<'_>, now: Instant) -> Result<(), CoreError> {
        if self.kind == MediaKind::Video {
            self.send.answer_nack(request.ranges(), now.wire_micros())?;
        }
        Ok(())
    }

    /// Datagrams built since the last call: NACKs first, then fragments, parity and
    /// retransmissions in the order they must leave.
    pub fn drain_outgoing(&mut self) -> impl Iterator<Item = Datagram> + '_ {
        let Self {
            nacks_outgoing,
            send,
            ..
        } = self;
        nacks_outgoing.drain(..).chain(send.drain_outgoing())
    }

    /// The next frame due for the decoder. Audio frames are released as they complete; video
    /// waits in the one-slot playout buffer for its presentation instant.
    pub fn poll_present(&mut self, now: Instant) -> Option<AssembledFrame> {
        if let Some(frame) = self.arrived.pop_front() {
            return Some(frame);
        }
        self.playout.poll_present(now)
    }

    /// Sends whatever NACKs have come due since the last arrival.
    pub fn poll(&mut self, now: Instant) {
        if self.kind == MediaKind::Video && self.config.role.receives() {
            self.schedule_retransmissions(now);
        }
    }

    pub fn next_timeout(&self) -> Option<Instant> {
        if !self.arrived.is_empty() {
            return Some(Instant::SESSION_START);
        }
        let nack = self
            .last_plan
            .retransmission
            .and_then(|window| self.nacks.next_due(&window));
        match (self.playout.next_presentation(), nack) {
            (Some(presentation), Some(nack)) => Some(presentation.min(nack)),
            (presentation, nack) => presentation.or(nack),
        }
    }

    /// Whether a frame completed since the last call, which is when an arrival report is worth
    /// sending without waiting for the batch to fill.
    pub const fn take_frame_completed(&mut self) -> bool {
        let completed = self.frame_completed;
        self.frame_completed = false;
        completed
    }

    /// The application lost frames the session had already delivered — a decoder that fell
    /// behind, or one that joined late — so the next frame worth decoding is a keyframe.
    pub fn request_keyframe(&mut self) {
        if self.kind == MediaKind::Video && self.config.role.receives() {
            self.keyframe_wanted = true;
            self.last_keyframe_request = None;
        }
    }

    /// Whether the decoder has lost its reference and a keyframe should be asked for now.
    pub fn take_keyframe_request(&mut self, now: Instant) -> bool {
        self.notice_decode_gaps();
        if !self.keyframe_wanted {
            return false;
        }
        if self.last_keyframe_request.is_some_and(|last| {
            now.saturating_elapsed_since(last) < Self::KEYFRAME_REQUEST_INTERVAL
        }) {
            return false;
        }
        self.last_keyframe_request = Some(now);
        true
    }

    pub fn stats(&self, path: &LinkQuality, pacing_rate: Bitrate) -> StatsSnapshot {
        let loss_rate = if self.config.role.sends() {
            path.loss_rate
        } else {
            self.gaps.loss_rate()
        };
        StatsSnapshot {
            frames_presented: self.playout.presented(),
            frames_superseded: self.playout.superseded(),
            frames_expired: self.playout.expired(),
            fragments_repaired: self.assembler.repaired_fragments(),
            frames_incomplete: self.assembler.evicted_incomplete(),
            retransmissions_requested: self.nacks.requested(),
            retransmissions_abandoned: self.nacks.abandoned(),
            loss_rate,
            round_trip: path.round_trip,
            playout_delay: self.playout.target_delay(),
            pacing_rate,
            protocol_latency_p50: self.latency.quantile(50),
            protocol_latency_p95: self.latency.quantile(95),
            protocol_latency_p99: self.latency.quantile(99),
        }
    }

    fn absorb(&mut self, acceptance: FragmentAcceptance, now: Instant) {
        let FragmentAcceptance::Complete(frame) = acceptance else {
            return;
        };
        self.latency
            .record(now.saturating_elapsed_since(frame.first_fragment_arrived_at()));
        if frame.is_keyframe() {
            self.keyframe_wanted = false;
            self.keyframe_seen = true;
        } else if !self.keyframe_seen && self.kind == MediaKind::Video {
            self.keyframe_wanted = true;
        }
        self.frame_completed = true;
        match self.kind {
            MediaKind::Audio => self.arrived.push_back(frame),
            MediaKind::Video => self.playout.accept(frame, now),
        }
    }

    fn notice_decode_gaps(&mut self) {
        if self.kind != MediaKind::Video {
            return;
        }
        let incomplete = self.assembler.evicted_incomplete();
        let expired = self.playout.expired();
        if incomplete > self.incomplete_seen || expired > self.expired_seen {
            self.keyframe_wanted = true;
        }
        self.incomplete_seen = incomplete;
        self.expired_seen = expired;
    }

    fn note_arrival(&mut self, sequence: SequenceNumber, flags: PacketFlags, now: Instant) {
        self.gaps
            .record(sequence, flags.contains(PacketFlags::RETRANSMISSION));
        self.nacks.resolve(sequence);
        if self.kind == MediaKind::Video {
            self.schedule_retransmissions(now);
        }
    }

    fn schedule_retransmissions(&mut self, now: Instant) {
        let Some(window) = self.last_plan.retransmission else {
            return;
        };
        self.nacks
            .note_gaps(self.gaps.gaps_within(Self::GAP_SCAN_DEPTH), now);

        self.nack_scratch.clear();
        let deadline = now.saturating_add(self.config.frame_rate.period());
        self.nacks
            .collect_due(now, deadline, &window, &mut self.nack_scratch);
        if self.nack_scratch.is_empty() {
            return;
        }
        if let Ok(datagram) = self.build_nack(now) {
            self.nacks_outgoing.push(datagram);
        }
    }

    fn build_nack(&mut self, now: Instant) -> Result<Datagram, CoreError> {
        let header = PacketHeader {
            version: ProtocolVersion::CURRENT,
            kind: PacketKind::Nack,
            flags: PacketFlags::empty(),
            stream: self.outbound,
            sequence: self.nack_sequence,
            frame: FrameId::ZERO,
            timestamp: MediaTimestamp::ZERO,
            packet_index: 0,
            packet_count: 1,
            departure: now.wire_micros(),
            payload_offset: 0,
        };
        self.nack_sequence = self.nack_sequence.next();

        let mut buffer = self.nack_pool.acquire();
        let header_len = header.write_into(buffer.spare_capacity_mut())?;
        buffer.set_filled(header_len)?;

        let mut scratch = [0u8; MissingRange::ENCODED_LEN];
        for range in self.nack_scratch.drain(..) {
            NackRequest::write_range(&mut scratch, range)?;
            buffer.extend_from_slice(&scratch)?;
        }
        Ok(Datagram::from_buffer(header, buffer))
    }
}
