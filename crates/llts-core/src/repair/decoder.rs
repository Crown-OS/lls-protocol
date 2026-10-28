use alloc::vec::Vec;
use core::fmt;

use reed_solomon_simd::{DecoderResult, ReedSolomonDecoder};

use super::ShardLayout;
use crate::CoreError;

/// Data shards the decoder reconstructed, borrowed from the decoder that recovered them.
pub struct RestoredShards<'a> {
    decoded: DecoderResult<'a>,
}

impl RestoredShards<'_> {
    /// The reconstructed shard at `index`, still padded to the block's shard size.
    pub fn get(&self, index: u16) -> Option<&[u8]> {
        self.decoded.restored_original(usize::from(index))
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, &[u8])> {
        self.decoded
            .restored_original_iter()
            .map(|(index, shard)| (u16::try_from(index).unwrap_or(u16::MAX), shard))
    }
}

impl fmt::Debug for RestoredShards<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestoredShards")
            .finish_non_exhaustive()
    }
}

/// Recovers a repair block's missing data shards from whatever arrived.
///
/// Reed-Solomon is maximum-distance-separable, so any `data_shards` of the combined data and
/// parity shards suffice, whichever ones they happen to be.
#[derive(Default)]
pub struct RepairBlockDecoder {
    engine: Option<ReedSolomonDecoder>,
    padding: Vec<u8>,
    layout: Option<ShardLayout>,
    parity_shards: u16,
    supplied: u16,
}

impl RepairBlockDecoder {
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

    /// Shards supplied so far, counting data and parity alike.
    pub const fn supplied(&self) -> u16 {
        self.supplied
    }

    /// Whether enough shards have arrived for [`RepairBlockDecoder::decode`] to succeed.
    pub fn is_decodable(&self) -> bool {
        self.layout
            .is_some_and(|layout| self.supplied >= layout.data_shards())
    }

    pub fn begin(&mut self, layout: ShardLayout, parity_shards: u16) -> Result<(), CoreError> {
        let data = usize::from(layout.data_shards());
        let parity = usize::from(parity_shards);
        let shard_bytes = usize::from(layout.shard_bytes());
        let unsupported = || CoreError::UnsupportedRepairGeometry {
            data_shards: layout.data_shards(),
            parity_shards,
        };

        if parity_shards == 0 || !ReedSolomonDecoder::supports(data, parity) {
            return Err(unsupported());
        }

        match self.engine.as_mut() {
            Some(engine) => engine
                .reset(data, parity, shard_bytes)
                .map_err(|_| unsupported())?,
            None => {
                self.engine = Some(
                    ReedSolomonDecoder::new(data, parity, shard_bytes)
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

    pub fn add_data_shard(&mut self, index: u16, bytes: &[u8]) -> Result<(), CoreError> {
        self.add(index, bytes, ShardRole::Data)
    }

    pub fn add_parity_shard(&mut self, index: u16, bytes: &[u8]) -> Result<(), CoreError> {
        self.add(index, bytes, ShardRole::Parity)
    }

    /// Reconstructs the missing data shards.
    pub fn decode(&mut self) -> Result<RestoredShards<'_>, CoreError> {
        let layout = self.layout.ok_or(CoreError::RepairBlockNotStarted)?;
        if self.supplied < layout.data_shards() {
            return Err(CoreError::InsufficientRepairShards {
                required: layout.data_shards(),
                available: self.supplied,
            });
        }
        self.supplied = 0;
        let engine = self
            .engine
            .as_mut()
            .ok_or(CoreError::RepairBlockNotStarted)?;
        let decoded = engine
            .decode()
            .map_err(|_| CoreError::RepairBlockNotStarted)?;
        Ok(RestoredShards { decoded })
    }

    fn add(&mut self, index: u16, bytes: &[u8], role: ShardRole) -> Result<(), CoreError> {
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

        let position = usize::from(index);
        let accepted = match role {
            ShardRole::Data => engine.add_original_shard(position, shard),
            ShardRole::Parity => engine.add_recovery_shard(position, shard),
        };
        accepted.map_err(|_| CoreError::UnsupportedRepairGeometry {
            data_shards: layout.data_shards(),
            parity_shards: self.parity_shards,
        })?;

        *supplied = supplied.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum ShardRole {
    Data,
    Parity,
}

impl fmt::Debug for RepairBlockDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepairBlockDecoder")
            .field("layout", &self.layout)
            .field("parity_shards", &self.parity_shards)
            .field("supplied", &self.supplied)
            .finish_non_exhaustive()
    }
}
