//! Fixed-size buffers recycled through a pool, so the packet path never touches the allocator.

mod datagram;
mod pool;
mod pooled;

pub use datagram::Datagram;
pub use pool::BufferPool;
pub use pooled::PooledBuffer;
