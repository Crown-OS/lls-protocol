use alloc::vec::Vec;

use llts_wire::packet::{MediaPacketView, RepairPacketView};
use llts_wire::{FrameId, MediaTimestamp};

use super::{AssembledFrame, PartialFrame};
use crate::CoreError;
use crate::buffer::BufferPool;
use crate::clock::Instant;
use crate::limits::MAX_FRAMES_IN_FLIGHT;
use crate::repair::RepairBlockDecoder;

/// What happened to a fragment the assembler was handed.
#[derive(Debug)]
pub enum FragmentAcceptance {
    /// Stored; the frame is still waiting on other fragments.
    Pending,
    /// The last missing fragment arrived.
    Complete(AssembledFrame),
    /// A fragment already held, most often a retransmission that raced its original.
    Duplicate,
    /// Belongs to a frame already delivered or already evicted as too old.
    Obsolete,
}

/// Reassembles frames from fragments that arrive out of order, late, or twice.
///
/// Tracks a fixed [`MAX_FRAMES_IN_FLIGHT`] frames; a fragment for a newer frame evicts the oldest
/// still incomplete, which is the receiver-side half of "present the freshest frame".
#[derive(Debug)]
pub struct FrameAssembler {
    partials: Vec<PartialFrame>,
    frame_pool: BufferPool,
    shard_pool: BufferPool,
    decoder: RepairBlockDecoder,
    highest_delivered: Option<FrameId>,
    evicted_incomplete: u64,
    repaired_fragments: u64,
}

impl FrameAssembler {
    /// `frame_pool` holds whole reassembled frames; `shard_pool` holds parity shards while the
    /// receiver waits to see whether they are needed.
    pub fn new(frame_pool: BufferPool, shard_pool: BufferPool) -> Self {
        Self {
            partials: Vec::with_capacity(MAX_FRAMES_IN_FLIGHT),
            frame_pool,
            shard_pool,
            decoder: RepairBlockDecoder::new(),
            highest_delivered: None,
            evicted_incomplete: 0,
            repaired_fragments: 0,
        }
    }

    /// Fragments recovered from parity rather than received.
    pub const fn repaired_fragments(&self) -> u64 {
        self.repaired_fragments
    }

    /// Stores a parity shard, recovering the block's missing fragments when enough have arrived.
    pub fn accept_repair(
        &mut self,
        view: &RepairPacketView<'_>,
        now: Instant,
    ) -> Result<FragmentAcceptance, CoreError> {
        let frame = view.header().frame;
        if self.is_obsolete(frame) {
            return Ok(FragmentAcceptance::Obsolete);
        }
        let header = view.header();
        let position = match self.position_of(frame) {
            Some(position) => position,
            None => self.open_slot(frame, header.timestamp, header.is_keyframe(), now),
        };

        let Self {
            partials,
            shard_pool,
            decoder,
            repaired_fragments,
            ..
        } = self;
        let Some(partial) = partials.get_mut(position) else {
            return Ok(FragmentAcceptance::Pending);
        };

        let before = partial.fragments_present();
        let complete = partial.absorb_repair(view, shard_pool, decoder)?;
        *repaired_fragments =
            repaired_fragments.saturating_add(u64::from(partial.fragments_present() - before));

        if !complete {
            return Ok(FragmentAcceptance::Pending);
        }
        let finished = self.partials.swap_remove(position);
        self.deliver_through(frame);
        Ok(FragmentAcceptance::Complete(finished.finish(now)))
    }

    /// Frames abandoned because a newer one needed the slot.
    pub const fn evicted_incomplete(&self) -> u64 {
        self.evicted_incomplete
    }

    pub const fn in_flight(&self) -> usize {
        self.partials.len()
    }

    pub fn accept(
        &mut self,
        packet: &MediaPacketView<'_>,
        now: Instant,
    ) -> Result<FragmentAcceptance, CoreError> {
        let frame = packet.header().frame;

        if self.is_obsolete(frame) {
            return Ok(FragmentAcceptance::Obsolete);
        }

        let header = packet.header();
        let position = match self.position_of(frame) {
            Some(position) => position,
            None => self.open_slot(frame, header.timestamp, header.is_keyframe(), now),
        };

        let Self {
            partials,
            decoder,
            repaired_fragments,
            ..
        } = self;
        let Some(partial) = partials.get_mut(position) else {
            return Ok(FragmentAcceptance::Obsolete);
        };

        if partial.contains(packet.header().packet_index) {
            return Ok(FragmentAcceptance::Duplicate);
        }
        let before = partial.fragments_present();
        let complete = partial.absorb(packet, decoder)?;
        *repaired_fragments = repaired_fragments.saturating_add(u64::from(
            partial.fragments_present().saturating_sub(before + 1),
        ));
        if !complete {
            return Ok(FragmentAcceptance::Pending);
        }

        let completed = self.partials.swap_remove(position);
        self.deliver_through(frame);
        Ok(FragmentAcceptance::Complete(completed.finish(now)))
    }

    /// Fragments the receiver is still waiting on, as `(frame, fragment_index)` pairs, for the
    /// NACK scheduler to turn into retransmission requests.
    pub fn missing_fragments(&self) -> impl Iterator<Item = (FrameId, u16)> + '_ {
        self.partials.iter().flat_map(|partial| {
            let frame = partial.frame();
            partial.missing_fragments().map(move |index| (frame, index))
        })
    }

    /// Abandons partial frames whose first fragment arrived before `deadline`, because no
    /// retransmission can still reach the presenter in time.
    pub fn discard_older_than(&mut self, deadline: Instant) {
        let before = self.partials.len();
        self.partials
            .retain(|partial| partial.first_arrival() >= deadline);
        self.evicted_incomplete = self
            .evicted_incomplete
            .saturating_add((before - self.partials.len()) as u64);
    }

    /// Marks `frame` delivered and abandons every older partial, which can no longer be shown and
    /// whose absence is what tells the decoder it needs a keyframe.
    fn deliver_through(&mut self, frame: FrameId) {
        self.highest_delivered = Some(frame);
        let before = self.partials.len();
        self.partials
            .retain(|partial| !partial.frame().precedes(frame));
        self.evicted_incomplete = self
            .evicted_incomplete
            .saturating_add((before - self.partials.len()) as u64);
    }

    fn is_obsolete(&self, frame: FrameId) -> bool {
        self.highest_delivered
            .is_some_and(|delivered| frame.precedes(delivered) || frame == delivered)
    }

    fn position_of(&self, frame: FrameId) -> Option<usize> {
        self.partials
            .iter()
            .position(|partial| partial.frame() == frame)
    }

    fn open_slot(
        &mut self,
        frame: FrameId,
        timestamp: MediaTimestamp,
        is_keyframe: bool,
        now: Instant,
    ) -> usize {
        if self.partials.len() == MAX_FRAMES_IN_FLIGHT {
            self.evict_oldest();
        }
        self.partials.push(PartialFrame::start(
            frame,
            timestamp,
            is_keyframe,
            &self.frame_pool,
            now,
        ));
        self.partials.len() - 1
    }

    fn evict_oldest(&mut self) {
        let Some(oldest) = self
            .partials
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| left.first_arrival().cmp(&right.first_arrival()))
            .map(|(position, _)| position)
        else {
            return;
        };
        self.partials.swap_remove(oldest);
        self.evicted_incomplete = self.evicted_incomplete.saturating_add(1);
    }
}
