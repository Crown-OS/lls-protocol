use alloc::vec::Vec;
use core::fmt;

use reed_solomon_simd::{EncoderResult, ReedSolomonEncoder};

use super::ShardLayout;
use crate::CoreError;

/// Parity shards for one block, borrowed from the encoder that produced them.
pub struct ParityShards<'a> {
    encoded: EncoderResult<'a>,
    count: u16,
}

impl ParityShards<'_> {
    pub const fn count(&self) -> u16 {
        self.count
    }

    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.encoded.recovery_iter()
    }

    pub fn get(&self, index: u16) -> Option<&[u8]> {
        self.encoded.recovery(usize::from(index))
    }
}

impl fmt::Debug for ParityShards<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParityShards")
            .field("count", &self.count)
            .finish()
    }
}

/// Produces parity shards for one repair block at a time.
///
/// Holds a single `reed-solomon-simd` engine across blocks and reconfigures it in place, because
/// constructing one allocates its working tables. That engine selects AVX2, SSSE3 or Neon through
/// `cpufeatures`, which needs no `std`, so this crate keeps its `no_std` guarantee and still codes
/// at gigabytes per second.
#[derive(Default)]
pub struct RepairBlockEncoder {
    engine: Option<ReedSolomonEncoder>,
    padding: Vec<u8>,
    layout: Option<ShardLayout>,
    parity_shards: u16,
    supplied: u16,
}

impl RepairBlockEncoder {
    pub const fn new() -> Self {
        Self {
            engine: None,
            padding: Vec::new(),
            layout: None,
            parity_shards: 0,
            supplied: 0,
        }
    }

    pub const fn open_layout(&self) -> Option<ShardLayout> {
        self.layout
    }

    /// Configures the encoder for a new block, discarding any half-finished one.
    pub fn begin(&mut self, layout: ShardLayout, parity_shards: u16) -> Result<(), CoreError> {
        let data = usize::from(layout.data_shards());
        let parity = usize::from(parity_shards);
        let shard_bytes = usize::from(layout.shard_bytes());
        let unsupported = || CoreError::UnsupportedRepairGeometry {
            data_shards: layout.data_shards(),
            parity_shards,
        };

        if parity_shards == 0 || !ReedSolomonEncoder::supports(data, parity) {
            return Err(unsupported());
        }

        match self.engine.as_mut() {
            Some(engine) => engine
                .reset(data, parity, shard_bytes)
                .map_err(|_| unsupported())?,
            None => {
                self.engine = Some(
                    ReedSolomonEncoder::new(data, parity, shard_bytes)
                        .map_err(|_| unsupported())?,
                );
            }
        }

        self.padding.clear();
        self.padding.resize(shard_bytes, 0);
        self.layout = Some(layout);
        self.parity_shards = parity_shards;
        self.supplied = 0;
        Ok(())
    }

    /// Feeds the next data shard in index order, zero-padding a short final shard.
    pub fn add_data_shard(&mut self, bytes: &[u8]) -> Result<(), CoreError> {
        let layout = self.layout.ok_or(CoreError::RepairBlockNotStarted)?;
        let shard_bytes = usize::from(layout.shard_bytes());
        if bytes.len() > shard_bytes {
            return Err(CoreError::RepairShardSize {
                expected: shard_bytes,
                actual: bytes.len(),
            });
        }

        let Self {
            engine,
            padding,
            supplied,
            ..
        } = self;
        let engine = engine.as_mut().ok_or(CoreError::RepairBlockNotStarted)?;

        let shard = if bytes.len() == shard_bytes {
            bytes
        } else {
            padding.fill(0);
            let head = padding
                .get_mut(..bytes.len())
                .ok_or(CoreError::RepairShardSize {
                    expected: shard_bytes,
                    actual: bytes.len(),
                })?;
            head.copy_from_slice(bytes);
            padding.as_slice()
        };

        engine
            .add_original_shard(shard)
            .map_err(|_| CoreError::RepairShardSize {
                expected: shard_bytes,
                actual: bytes.len(),
            })?;
        *supplied = supplied.saturating_add(1);
        Ok(())
    }

    /// Computes the parity shards once every data shard has been supplied.
    pub fn encode(&mut self) -> Result<ParityShards<'_>, CoreError> {
        let layout = self.layout.ok_or(CoreError::RepairBlockNotStarted)?;
        if self.supplied != layout.data_shards() {
            return Err(CoreError::InsufficientRepairShards {
                required: layout.data_shards(),
                available: self.supplied,
            });
        }
        self.supplied = 0;
        let count = self.parity_shards;
        let engine = self
            .engine
            .as_mut()
            .ok_or(CoreError::RepairBlockNotStarted)?;
        let encoded = engine
            .encode()
            .map_err(|_| CoreError::RepairBlockNotStarted)?;
        Ok(ParityShards { encoded, count })
    }
}

impl fmt::Debug for RepairBlockEncoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepairBlockEncoder")
            .field("layout", &self.layout)
            .field("parity_shards", &self.parity_shards)
            .field("supplied", &self.supplied)
            .finish_non_exhaustive()
    }
}
