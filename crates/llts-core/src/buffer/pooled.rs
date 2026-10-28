use alloc::boxed::Box;
use alloc::sync::Arc;
use core::fmt;
use core::mem;

use super::pool::Recycler;
use crate::CoreError;

/// A buffer on loan from a [`super::BufferPool`], returned when it is dropped.
///
/// Deliberately not [`Clone`]: two owners would mean either a copy on the packet path or a
/// recycler that hands the same storage out twice.
pub struct PooledBuffer {
    storage: Box<[u8]>,
    filled: usize,
    recycler: Arc<Recycler>,
}

impl PooledBuffer {
    pub(super) const fn new(storage: Box<[u8]>, recycler: Arc<Recycler>) -> Self {
        Self {
            storage,
            filled: 0,
            recycler,
        }
    }

    pub const fn capacity(&self) -> usize {
        self.storage.len()
    }

    pub const fn len(&self) -> usize {
        self.filled
    }

    pub const fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// The bytes written so far.
    pub fn filled(&self) -> &[u8] {
        self.storage.get(..self.filled).unwrap_or_default()
    }

    /// The whole buffer, for a writer that will call [`PooledBuffer::set_filled`] afterwards.
    pub fn spare_capacity_mut(&mut self) -> &mut [u8] {
        &mut self.storage
    }

    pub fn set_filled(&mut self, filled: usize) -> Result<(), CoreError> {
        if filled > self.storage.len() {
            return Err(CoreError::BufferOverflow {
                requested: filled,
                capacity: self.storage.len(),
            });
        }
        self.filled = filled;
        Ok(())
    }

    /// Appends `source`, failing rather than reallocating when the buffer is too small.
    pub fn extend_from_slice(&mut self, source: &[u8]) -> Result<(), CoreError> {
        let start = self.filled;
        let end = start.saturating_add(source.len());
        self.write_at(start, source)?;
        self.filled = end;
        Ok(())
    }

    /// Copies `source` to a fixed offset, growing the filled length if it now reaches further.
    ///
    /// Used by the frame assembler, which receives fragments out of order.
    pub fn write_at(&mut self, offset: usize, source: &[u8]) -> Result<(), CoreError> {
        let capacity = self.storage.len();
        let end = offset.saturating_add(source.len());
        let destination = self
            .storage
            .get_mut(offset..end)
            .ok_or(CoreError::BufferOverflow {
                requested: end,
                capacity,
            })?;
        destination.copy_from_slice(source);
        self.filled = self.filled.max(end);
        Ok(())
    }

    pub const fn clear(&mut self) {
        self.filled = 0;
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        // An empty `Box<[u8]>` allocates nothing, so taking the storage out costs nothing either.
        self.recycler.reclaim(mem::take(&mut self.storage));
    }
}

impl fmt::Debug for PooledBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PooledBuffer")
            .field("filled", &self.filled)
            .field("capacity", &self.storage.len())
            .finish()
    }
}
