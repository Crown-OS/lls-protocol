use alloc::vec::Vec;

use llts_wire::packet::{RepairBlockGeometry, RepairPacketView};

use crate::CoreError;
use crate::buffer::{BufferPool, PooledBuffer};
use crate::limits::MAX_PARITY_SHARDS_PER_BLOCK;
use crate::repair::ShardLayout;

/// Parity collected for one slice's repair block while the receiver waits to see whether it is
/// needed at all.
///
/// Most blocks lose nothing and their parity is discarded unused, so shards are held in pooled
/// buffers and the block never allocates after warm-up.
#[derive(Debug)]
pub(super) struct RepairBlockState {
    geometry: RepairBlockGeometry,
    parity_total: u16,
    shards: Vec<(u16, PooledBuffer)>,
    repaired: bool,
}

impl RepairBlockState {
    pub(super) fn start(view: &RepairPacketView<'_>) -> Self {
        let geometry = view.geometry();
        Self {
            geometry,
            parity_total: view
                .header()
                .packet_count
                .saturating_sub(geometry.data_shard_count),
            shards: Vec::with_capacity(4),
            repaired: false,
        }
    }

    pub(super) const fn geometry(&self) -> RepairBlockGeometry {
        self.geometry
    }

    pub(super) const fn first_data_index(&self) -> u16 {
        self.geometry.first_data_index
    }

    pub(super) const fn parity_total(&self) -> u16 {
        self.parity_total
    }

    pub(super) const fn is_repaired(&self) -> bool {
        self.repaired
    }

    pub(super) const fn mark_repaired(&mut self) {
        self.repaired = true;
    }

    pub(super) fn parity_held(&self) -> u16 {
        u16::try_from(self.shards.len()).unwrap_or(u16::MAX)
    }

    pub(super) fn shards(&self) -> impl Iterator<Item = (u16, &[u8])> {
        self.shards
            .iter()
            .map(|(index, buffer)| (*index, buffer.filled()))
    }

    pub(super) fn layout(&self) -> Result<ShardLayout, CoreError> {
        ShardLayout::new(self.geometry.block_byte_len, self.geometry.shard_bytes)
    }

    /// Stores a parity shard, ignoring one already held or one past the block's capacity.
    pub(super) fn absorb(
        &mut self,
        view: &RepairPacketView<'_>,
        pool: &BufferPool,
    ) -> Result<(), CoreError> {
        let index = view.parity_index();
        if self.shards.len() >= MAX_PARITY_SHARDS_PER_BLOCK
            || self.shards.iter().any(|(held, _)| *held == index)
        {
            return Ok(());
        }
        let mut buffer = pool.acquire();
        buffer.extend_from_slice(view.shard())?;
        self.shards.push((index, buffer));
        Ok(())
    }
}
