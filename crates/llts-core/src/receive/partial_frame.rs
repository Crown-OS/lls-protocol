use alloc::vec::Vec;

use llts_wire::packet::{MediaPacketView, RepairPacketView};
use llts_wire::{FrameId, MediaTimestamp};

use super::{AssembledFrame, FragmentBitmap, RepairBlockState};
use crate::CoreError;
use crate::buffer::{BufferPool, PooledBuffer};
use crate::clock::Instant;
use crate::limits::{MAX_BLOCKS_PER_FRAME, MAX_FRAGMENTS_PER_FRAME};
use crate::repair::RepairBlockDecoder;

/// A frame being reassembled from fragments that may arrive in any order.
#[derive(Debug)]
pub(super) struct PartialFrame {
    frame: FrameId,
    timestamp: MediaTimestamp,
    is_keyframe: bool,
    /// Known once the fragment carrying `FRAME_END` arrives; a lower bound until then.
    total_fragments: Option<u16>,
    highest_index_seen: u16,
    arrived: FragmentBitmap,
    first_arrival: Instant,
    buffer: PooledBuffer,
    blocks: Vec<RepairBlockState>,
}

impl PartialFrame {
    pub(super) fn start(
        frame: FrameId,
        timestamp: MediaTimestamp,
        is_keyframe: bool,
        pool: &BufferPool,
        now: Instant,
    ) -> Self {
        Self {
            frame,
            timestamp,
            is_keyframe,
            total_fragments: None,
            highest_index_seen: 0,
            arrived: FragmentBitmap::new(),
            first_arrival: now,
            buffer: pool.acquire(),
            blocks: Vec::new(),
        }
    }

    pub(super) const fn frame(&self) -> FrameId {
        self.frame
    }

    pub(super) const fn first_arrival(&self) -> Instant {
        self.first_arrival
    }

    /// Fragments still missing below the highest index seen, which is what a NACK asks for.
    pub(super) fn missing_fragments(&self) -> impl Iterator<Item = u16> + '_ {
        let limit = self
            .total_fragments
            .unwrap_or(self.highest_index_seen.saturating_add(1));
        self.arrived.missing_below(limit)
    }

    pub(super) fn contains(&self, index: u16) -> bool {
        self.arrived.contains(index)
    }

    pub(super) const fn fragments_present(&self) -> u16 {
        self.arrived.present_count()
    }

    /// Stores one fragment, reporting whether the frame is now complete.
    ///
    /// Also retries any block still short a shard, because parity that arrived before its data
    /// could not be used at the time.
    pub(super) fn absorb(
        &mut self,
        packet: &MediaPacketView<'_>,
        decoder: &mut RepairBlockDecoder,
    ) -> Result<bool, CoreError> {
        let header = packet.header();
        let index = header.packet_index;

        if usize::from(index) >= MAX_FRAGMENTS_PER_FRAME {
            return Err(CoreError::FrameTooFragmented {
                frame_id: self.frame.get(),
                required: usize::from(index) + 1,
                limit: MAX_FRAGMENTS_PER_FRAME,
            });
        }
        if !self.arrived.insert(index) {
            return Ok(self.is_complete());
        }

        self.highest_index_seen = self.highest_index_seen.max(index);
        if header.is_keyframe() {
            self.is_keyframe = true;
        }
        if header.is_frame_end() {
            self.total_fragments = Some(header.packet_count);
        }

        self.buffer
            .write_at(header.payload_offset as usize, packet.payload())?;

        if !self.is_complete() {
            self.repair_pending_blocks(decoder)?;
        }
        Ok(self.is_complete())
    }

    fn repair_pending_blocks(&mut self, decoder: &mut RepairBlockDecoder) -> Result<(), CoreError> {
        for position in 0..self.blocks.len() {
            self.try_repair(position, decoder)?;
        }
        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.total_fragments
            .is_some_and(|total| self.arrived.present_count() >= total)
    }

    /// Stores a parity shard and attempts recovery if its block is now over-determined.
    ///
    /// Recovery is attempted only when the block is actually short a data shard, so a block that
    /// arrived intact never pays for the decode.
    pub(super) fn absorb_repair(
        &mut self,
        view: &RepairPacketView<'_>,
        pool: &BufferPool,
        decoder: &mut RepairBlockDecoder,
    ) -> Result<bool, CoreError> {
        let geometry = view.geometry();
        let position = match self
            .blocks
            .iter()
            .position(|block| block.first_data_index() == geometry.first_data_index)
        {
            Some(position) => position,
            None => {
                if self.blocks.len() >= MAX_BLOCKS_PER_FRAME {
                    return Ok(self.is_complete());
                }
                self.blocks.push(RepairBlockState::start(view));
                self.blocks.len() - 1
            }
        };

        if view.header().is_frame_end() {
            self.total_fragments = Some(
                geometry
                    .first_data_index
                    .saturating_add(geometry.data_shard_count),
            );
        }

        let Some(block) = self.blocks.get_mut(position) else {
            return Ok(self.is_complete());
        };
        block.absorb(view, pool)?;

        self.repair_pending_blocks(decoder)?;
        Ok(self.is_complete())
    }

    fn try_repair(
        &mut self,
        position: usize,
        decoder: &mut RepairBlockDecoder,
    ) -> Result<(), CoreError> {
        let Some(block) = self.blocks.get(position) else {
            return Ok(());
        };
        if block.is_repaired() {
            return Ok(());
        }

        let geometry = block.geometry();
        let first = geometry.first_data_index;
        let data_shards = geometry.data_shard_count;
        let present = (0..data_shards)
            .filter(|offset| self.arrived.contains(first.saturating_add(*offset)))
            .count();
        let present = u16::try_from(present).unwrap_or(u16::MAX);

        if present == data_shards || present.saturating_add(block.parity_held()) < data_shards {
            return Ok(());
        }

        decoder.begin(block.layout()?, block.parity_total())?;
        for offset in 0..data_shards {
            if !self.arrived.contains(first.saturating_add(offset)) {
                continue;
            }
            let start = geometry.shard_offset(offset) as usize;
            let len = geometry.shard_len(offset) as usize;
            let shard = self
                .buffer
                .filled()
                .get(start..start.saturating_add(len))
                .unwrap_or_default();
            decoder.add_data_shard(offset, shard)?;
        }
        for (index, shard) in block.shards() {
            decoder.add_parity_shard(index, shard)?;
        }
        if !decoder.is_decodable() {
            return Ok(());
        }

        let restored = decoder.decode()?;
        let mut repaired: Vec<u16> = Vec::with_capacity(usize::from(data_shards));
        for (offset, shard) in restored.iter() {
            if offset >= data_shards {
                continue;
            }
            let start = geometry.shard_offset(offset) as usize;
            let len = geometry.shard_len(offset) as usize;
            self.buffer
                .write_at(start, shard.get(..len).unwrap_or_default())?;
            repaired.push(first.saturating_add(offset));
        }
        drop(restored);

        for fragment in repaired {
            self.arrived.insert(fragment);
            self.highest_index_seen = self.highest_index_seen.max(fragment);
        }
        if let Some(block) = self.blocks.get_mut(position) {
            block.mark_repaired();
        }
        Ok(())
    }

    pub(super) fn finish(self, now: Instant) -> AssembledFrame {
        AssembledFrame::new(
            self.frame,
            self.timestamp,
            self.is_keyframe,
            self.first_arrival,
            now,
            self.buffer,
        )
    }
}
