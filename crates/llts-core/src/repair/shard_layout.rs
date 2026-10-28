use crate::CoreError;

/// How one repair block's bytes divide into equal shards.
///
/// Reed-Solomon needs every shard the same size, and `reed-solomon-simd` additionally requires
/// that size to be even. Only the final shard of a block is short, and it is zero-padded for the
/// coding and trimmed back on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardLayout {
    shard_bytes: u16,
    data_shards: u16,
    block_bytes: u32,
}

impl ShardLayout {
    pub fn new(block_bytes: u32, shard_bytes: u16) -> Result<Self, CoreError> {
        if !shard_bytes.is_multiple_of(2) || shard_bytes == 0 {
            return Err(CoreError::RepairShardSize {
                expected: usize::from(shard_bytes.saturating_add(shard_bytes % 2).max(2)),
                actual: usize::from(shard_bytes),
            });
        }
        let data_shards = block_bytes.div_ceil(u32::from(shard_bytes)).max(1);
        Ok(Self {
            shard_bytes,
            data_shards: u16::try_from(data_shards).unwrap_or(u16::MAX),
            block_bytes,
        })
    }

    pub const fn shard_bytes(&self) -> u16 {
        self.shard_bytes
    }

    pub const fn data_shards(&self) -> u16 {
        self.data_shards
    }

    pub const fn block_bytes(&self) -> u32 {
        self.block_bytes
    }

    /// Real, unpadded length of shard `index`; zero once past the end of the block.
    pub const fn shard_len(&self, index: u16) -> usize {
        let consumed = (index as u32).saturating_mul(self.shard_bytes as u32);
        let remaining = self.block_bytes.saturating_sub(consumed);
        if remaining > self.shard_bytes as u32 {
            self.shard_bytes as usize
        } else {
            remaining as usize
        }
    }

    /// Byte offset of shard `index` within the block.
    pub const fn shard_offset(&self, index: u16) -> usize {
        (index as usize).saturating_mul(self.shard_bytes as usize)
    }
}
