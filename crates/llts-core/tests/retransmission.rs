#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use core::time::Duration;

use llts_core::buffer::BufferPool;
use llts_core::clock::Instant;
use llts_core::limits::MAX_PENDING_RETRANSMISSIONS;
use llts_core::receive::NackScheduler;
use llts_core::repair::RetransmissionWindow;
use llts_core::send::{DatagramSizing, EncodedSlice, FramePacketizer, RetransmitCache};
use llts_wire::packet::{MediaPacketView, MissingRange, PacketFlags};
use llts_wire::{MediaTimestamp, MonotonicMicros, SequenceNumber, StreamId};

const MTU: usize = 1200;
const SCREEN: StreamId = StreamId::new(1);

const WINDOW: RetransmissionWindow = RetransmissionWindow {
    gap_settle: Duration::from_micros(500),
    abandon_before_deadline: Duration::from_millis(3),
};

const fn at(micros: u64) -> Instant {
    Instant::from_micros(micros)
}

const fn sequence(value: u16) -> SequenceNumber {
    SequenceNumber::new(value)
}

fn due(scheduler: &mut NackScheduler, now: Instant, deadline: Instant) -> Vec<MissingRange> {
    let mut ranges = Vec::new();
    scheduler.collect_due(now, deadline, &WINDOW, &mut ranges);
    ranges
}

#[test]
fn a_gap_is_not_chased_until_reordering_has_had_time_to_settle() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));

    assert!(
        due(&mut scheduler, at(400), at(20_000)).is_empty(),
        "asked before the gap could have been mere reordering"
    );
    assert_eq!(
        due(&mut scheduler, at(600), at(20_000)),
        vec![MissingRange::single(sequence(10))]
    );
}

#[test]
fn a_repeat_request_waits_a_whole_round_trip() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));
    assert_eq!(due(&mut scheduler, at(600), at(40_000)).len(), 1);

    assert!(
        due(&mut scheduler, at(2_000), at(40_000)).is_empty(),
        "asked again before the first request could possibly have been answered"
    );
    assert_eq!(due(&mut scheduler, at(3_700), at(40_000)).len(), 1);
}

#[test]
fn a_resolved_sequence_is_never_asked_for_again() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));
    scheduler.resolve(sequence(10));

    assert!(due(&mut scheduler, at(1_000), at(40_000)).is_empty());
    assert_eq!(scheduler.pending_count(), 0);
}

#[test]
fn requests_stop_once_the_deadline_is_too_close_to_answer() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));

    let deadline = at(10_000);
    assert!(
        due(&mut scheduler, at(8_000), deadline).is_empty(),
        "asked with less than a round trip left before playout"
    );
    assert_eq!(scheduler.pending_count(), 0);
    assert_eq!(scheduler.abandoned(), 1);
}

#[test]
fn a_sequence_is_given_up_on_after_a_few_attempts() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));

    let deadline = at(10_000_000);
    let mut asks = 0;
    for tick in 0..10 {
        asks += due(&mut scheduler, at(600 + tick * 4_000), deadline).len();
    }

    assert_eq!(asks, usize::from(NackScheduler::MAX_ATTEMPTS));
    assert_eq!(scheduler.pending_count(), 0);
}

#[test]
fn a_lost_burst_collapses_into_one_range() {
    let mut scheduler = NackScheduler::new();
    let lost = [40, 41, 42, 43, 44];
    scheduler.note_gaps(lost.into_iter().map(sequence), at(0));

    let ranges = due(&mut scheduler, at(600), at(40_000));
    assert_eq!(ranges.len(), 1, "a contiguous burst should be one range");
    assert_eq!(ranges[0].first, sequence(40));
    assert_eq!(ranges[0].sequence_count(), 5);
}

#[test]
fn separated_losses_stay_separate_ranges() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([10, 11, 20].into_iter().map(sequence), at(0));

    let ranges = due(&mut scheduler, at(600), at(40_000));
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].sequence_count(), 2);
    assert_eq!(ranges[1].first, sequence(20));
}

#[test]
fn the_same_gap_is_only_registered_once() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps([sequence(10)].into_iter(), at(0));
    scheduler.note_gaps([sequence(10)].into_iter(), at(100));

    assert_eq!(scheduler.pending_count(), 1);
}

#[test]
fn the_backlog_is_bounded() {
    let mut scheduler = NackScheduler::new();
    scheduler.note_gaps((0..500).map(sequence), at(0));

    assert_eq!(scheduler.pending_count(), MAX_PENDING_RETRANSMISSIONS);
}

fn cached_frame(cache: &mut RetransmitCache, pool: &BufferPool, bytes: usize) -> usize {
    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut packetizer = FramePacketizer::new(SCREEN, sizing, pool.clone());
    let payload: Vec<u8> = (0..bytes).map(|index| index.to_le_bytes()[0]).collect();
    let mut sent = Vec::new();
    packetizer
        .packetize(
            EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
            MonotonicMicros::new(1_000),
            &mut sent,
        )
        .unwrap();
    let count = sent.len();
    for datagram in sent {
        cache.store(datagram);
    }
    count
}

#[test]
fn a_rebuilt_datagram_carries_the_original_payload() {
    let pool = BufferPool::new(MTU, 256);
    let mut cache = RetransmitCache::new(1 << 20);
    cached_frame(&mut cache, &pool, 20_000);

    let original_payload = {
        let rebuilt = cache
            .rebuild(sequence(3), sequence(0), MonotonicMicros::new(5_000), &pool)
            .unwrap()
            .unwrap();
        MediaPacketView::parse(rebuilt.bytes())
            .unwrap()
            .payload()
            .to_vec()
    };

    let again = cache
        .rebuild(sequence(3), sequence(1), MonotonicMicros::new(9_000), &pool)
        .unwrap()
        .unwrap();
    let packet = MediaPacketView::parse(again.bytes()).unwrap();

    assert_eq!(packet.payload(), original_payload);
    assert_eq!(
        cache.served(),
        2,
        "a second NACK should still be answerable"
    );
}

#[test]
fn a_rebuilt_datagram_is_stamped_and_redated() {
    let pool = BufferPool::new(MTU, 256);
    let mut cache = RetransmitCache::new(1 << 20);
    cached_frame(&mut cache, &pool, 20_000);

    let rebuilt = cache
        .rebuild(
            sequence(2),
            sequence(40),
            MonotonicMicros::new(7_500),
            &pool,
        )
        .unwrap()
        .unwrap();
    let header = rebuilt.header();

    assert!(header.flags.contains(PacketFlags::RETRANSMISSION));
    assert_eq!(header.departure, MonotonicMicros::new(7_500));
    assert_eq!(
        header.sequence,
        sequence(40),
        "a retransmission must not reuse the original's sequence, and so its nonce"
    );
    assert_eq!(rebuilt.media_sequence(), sequence(2));
    assert_eq!(
        MediaPacketView::parse(rebuilt.bytes())
            .unwrap()
            .media_sequence(),
        sequence(2)
    );
}

#[test]
fn asking_for_something_never_sent_yields_nothing() {
    let pool = BufferPool::new(MTU, 256);
    let mut cache = RetransmitCache::new(1 << 20);
    cached_frame(&mut cache, &pool, 5_000);

    assert!(
        cache
            .rebuild(sequence(9_000), sequence(0), MonotonicMicros::new(0), &pool)
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_cache_evicts_the_oldest_to_stay_within_its_budget() {
    let pool = BufferPool::new(MTU, 256);
    let budget = MTU * 8;
    let mut cache = RetransmitCache::new(budget);
    let stored = cached_frame(&mut cache, &pool, 40_000);

    assert!(cache.cached_bytes() <= budget);
    assert!(cache.cached_datagrams() < stored);
    assert!(
        !cache.contains(sequence(0)),
        "the oldest entry survived eviction"
    );
    assert!(cache.contains(SequenceNumber::new(u16::try_from(stored).unwrap() - 1)));
}

#[test]
fn releasing_acknowledged_sequences_frees_the_front() {
    let pool = BufferPool::new(MTU, 256);
    let mut cache = RetransmitCache::new(1 << 20);
    let stored = cached_frame(&mut cache, &pool, 20_000);

    cache.release_through(sequence(4));

    assert_eq!(cache.cached_datagrams(), stored - 5);
    assert!(!cache.contains(sequence(4)));
    assert!(cache.contains(sequence(5)));
    assert_eq!(cache.oldest_cached(), Some(sequence(5)));
}

#[test]
fn a_restarted_sequence_run_drops_the_stale_contents() {
    let pool = BufferPool::new(MTU, 256);
    let mut cache = RetransmitCache::new(1 << 20);
    cached_frame(&mut cache, &pool, 5_000);
    assert!(cache.contains(sequence(0)));

    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut restarted = FramePacketizer::new(SCREEN, sizing, pool.clone());
    let mut sent = Vec::new();
    restarted
        .packetize(
            EncodedSlice::new(b"fresh run", MediaTimestamp::new(0)).ending_frame(),
            MonotonicMicros::new(0),
            &mut sent,
        )
        .unwrap();
    for datagram in sent {
        cache.store(datagram);
    }

    assert_eq!(cache.cached_datagrams(), 1);
    assert_eq!(cache.oldest_cached(), Some(sequence(0)));
}

#[test]
fn caching_a_steady_stream_never_outgrows_the_pool() {
    let pool = BufferPool::new(MTU, 256);
    let sizing = DatagramSizing::new(MTU).unwrap();
    let mut packetizer = FramePacketizer::new(SCREEN, sizing, pool.clone());
    let mut cache = RetransmitCache::new(MTU * 64);
    let payload = vec![0u8; 40_000];
    let mut sent = Vec::with_capacity(64);

    for tick in 0..600u32 {
        sent.clear();
        packetizer
            .packetize(
                EncodedSlice::new(&payload, MediaTimestamp::new(1_500 * tick)).ending_frame(),
                MonotonicMicros::new(0),
                &mut sent,
            )
            .unwrap();
        for datagram in sent.drain(..) {
            cache.store(datagram);
        }
    }

    assert_eq!(
        pool.misses(),
        0,
        "the cache held more buffers than the pool has"
    );
    assert!(cache.cached_bytes() <= MTU * 64);
}
