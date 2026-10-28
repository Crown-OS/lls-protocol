#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::CoreError;
use llts_core::buffer::BufferPool;

const DATAGRAM_LEN: usize = 1200;

#[test]
fn a_dropped_buffer_returns_to_the_pool() {
    let pool = BufferPool::new(DATAGRAM_LEN, 4);
    assert_eq!(pool.available(), 4);

    {
        let _first = pool.acquire();
        let _second = pool.acquire();
        assert_eq!(pool.available(), 2);
    }

    assert_eq!(pool.available(), 4);
    assert_eq!(pool.misses(), 0);
}

#[test]
fn steady_state_traffic_never_allocates() {
    let pool = BufferPool::new(DATAGRAM_LEN, 32);

    for _frame in 0..10_000 {
        let burst: Vec<_> = (0..32).map(|_| pool.acquire()).collect();
        assert_eq!(burst.len(), 32);
    }

    assert_eq!(pool.misses(), 0, "the pool fell back to the allocator");
    assert_eq!(pool.available(), 32);
}

#[test]
fn exhausting_the_pool_allocates_rather_than_failing() {
    let pool = BufferPool::new(DATAGRAM_LEN, 2);
    let held: Vec<_> = (0..5).map(|_| pool.acquire()).collect();

    assert_eq!(held.len(), 5);
    assert_eq!(pool.misses(), 3);

    drop(held);
    assert!(
        pool.available() <= 2,
        "the pool grew past its configured size"
    );
}

#[test]
fn writing_past_the_end_reports_the_capacity_it_had() {
    let pool = BufferPool::new(8, 1);
    let mut buffer = pool.acquire();

    assert_eq!(
        buffer.extend_from_slice(&[0; 9]).unwrap_err(),
        CoreError::BufferOverflow {
            requested: 9,
            capacity: 8,
        }
    );
    assert!(buffer.is_empty(), "a failed write left the buffer dirty");
}

#[test]
fn fragments_may_arrive_out_of_order() {
    let pool = BufferPool::new(64, 1);
    let mut buffer = pool.acquire();

    buffer.write_at(16, b"third").unwrap();
    assert_eq!(buffer.len(), 21);
    buffer.write_at(0, b"first").unwrap();
    assert_eq!(buffer.len(), 21, "an earlier write shortened the buffer");

    assert_eq!(&buffer.filled()[..5], b"first");
    assert_eq!(&buffer.filled()[16..21], b"third");
}

#[test]
fn appending_tracks_the_filled_length() {
    let pool = BufferPool::new(32, 1);
    let mut buffer = pool.acquire();

    buffer.extend_from_slice(b"abc").unwrap();
    buffer.extend_from_slice(b"de").unwrap();
    assert_eq!(buffer.filled(), b"abcde");
    assert_eq!(buffer.len(), 5);

    buffer.clear();
    assert!(buffer.is_empty());
    assert_eq!(buffer.capacity(), 32);
}

#[test]
fn a_recycled_buffer_is_usable_immediately() {
    let pool = BufferPool::new(16, 1);

    let mut first = pool.acquire();
    first.extend_from_slice(b"stale").unwrap();
    drop(first);

    let second = pool.acquire();
    assert!(
        second.is_empty(),
        "a recycled buffer reported the previous tenant's length"
    );
    assert_eq!(pool.misses(), 0);
}

#[test]
fn a_pool_handle_can_be_shared_across_threads() {
    let pool = BufferPool::new(DATAGRAM_LEN, 64);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let pool = pool.clone();
            std::thread::spawn(move || {
                for _ in 0..1_000 {
                    let mut buffer = pool.acquire();
                    buffer.extend_from_slice(&[7; 32]).unwrap();
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(pool.available(), 64);
}
