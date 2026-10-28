use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;

use super::PooledBuffer;

/// Recycler shared between a pool and every buffer it has handed out.
#[derive(Debug)]
pub(super) struct Recycler {
    free: ArrayQueue<Box<[u8]>>,
    buffer_len: usize,
    misses: AtomicU64,
}

impl Recycler {
    pub(super) fn reclaim(&self, buffer: Box<[u8]>) {
        if buffer.len() == self.buffer_len {
            let _ = self.free.push(buffer);
        }
    }
}

/// Fixed-size buffers handed out and taken back without touching the allocator.
///
/// A miss — every buffer checked out at once — falls back to a fresh allocation rather than
/// failing, because dropping a frame is worse than one allocation. [`BufferPool::misses`] counts
/// them, and the loopback bench asserts it stays at zero, which is what keeps the steady-state
/// allocation-free claim honest as the code grows.
#[derive(Debug, Clone)]
pub struct BufferPool {
    recycler: Arc<Recycler>,
}

impl BufferPool {
    /// Preallocates `count` buffers of `buffer_len` bytes each.
    pub fn new(buffer_len: usize, count: usize) -> Self {
        let free = ArrayQueue::new(count.max(1));
        for _ in 0..count {
            let _ = free.push(alloc::vec![0u8; buffer_len].into_boxed_slice());
        }
        Self {
            recycler: Arc::new(Recycler {
                free,
                buffer_len,
                misses: AtomicU64::new(0),
            }),
        }
    }

    pub fn buffer_len(&self) -> usize {
        self.recycler.buffer_len
    }

    pub fn available(&self) -> usize {
        self.recycler.free.len()
    }

    /// Buffers served by allocating because the pool was empty.
    pub fn misses(&self) -> u64 {
        self.recycler.misses.load(Ordering::Relaxed)
    }

    pub fn acquire(&self) -> PooledBuffer {
        let storage = self.recycler.free.pop().unwrap_or_else(|| {
            self.recycler.misses.fetch_add(1, Ordering::Relaxed);
            alloc::vec![0u8; self.recycler.buffer_len].into_boxed_slice()
        });
        PooledBuffer::new(storage, Arc::clone(&self.recycler))
    }
}
