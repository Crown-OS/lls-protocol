#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::CoreError;
use llts_core::buffer::{BufferPool, Datagram};
use llts_core::send::{DatagramSizing, EncodedSlice, FramePacketizer};
use llts_wire::packet::{MediaPacketView, PacketFlags};
use llts_wire::{MediaTimestamp, MonotonicMicros, StreamId};

const SCREEN: StreamId = StreamId::new(1);
const AT_ONE_MS: MonotonicMicros = MonotonicMicros::new(1_000);

fn packetizer(mtu: usize, pool_size: usize) -> (FramePacketizer, BufferPool) {
    let pool = BufferPool::new(mtu, pool_size);
    let sizing = DatagramSizing::new(mtu).unwrap();
    (FramePacketizer::new(SCREEN, sizing, pool.clone()), pool)
}

/// Reassembles the payloads in emission order, which is also sequence order.
fn concatenated_payloads(sent: &[Datagram]) -> Vec<u8> {
    sent.iter()
        .flat_map(|datagram| {
            MediaPacketView::parse(datagram.bytes())
                .unwrap()
                .payload()
                .to_vec()
        })
        .collect()
}

#[test]
fn a_small_frame_is_one_datagram() {
    let (mut packetizer, _pool) = packetizer(1200, 8);
    let mut sent = Vec::new();

    let slice = EncodedSlice::new(b"tiny frame", MediaTimestamp::new(90_000))
        .as_keyframe()
        .ending_frame();
    let range = packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();

    assert_eq!(sent.len(), 1);
    assert_eq!(range.fragment_count, 1);
    assert!(range.frame_closed);

    let header = sent[0].header();
    assert_eq!(header.packet_index, 0);
    assert_eq!(header.packet_count, 1);
    assert!(header.is_frame_end());
    assert!(header.is_keyframe());
}

#[test]
fn a_large_frame_splits_and_reassembles_byte_for_byte() {
    let (mut packetizer, _pool) = packetizer(1200, 256);
    let payload: Vec<u8> = (0..40_000u32).map(|index| index.to_le_bytes()[0]).collect();
    let mut sent = Vec::new();

    let slice = EncodedSlice::new(&payload, MediaTimestamp::new(90_000)).ending_frame();
    let range = packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();

    let expected = DatagramSizing::new(1200).unwrap().fragment_count(40_000);
    assert_eq!(sent.len(), expected);
    assert_eq!(usize::from(range.fragment_count), expected);
    assert_eq!(concatenated_payloads(&sent), payload);
}

#[test]
fn no_datagram_exceeds_the_mtu() {
    let (mut packetizer, _pool) = packetizer(1200, 256);
    let payload = vec![0xABu8; 40_000];
    let mut sent = Vec::new();

    let slice = EncodedSlice::new(&payload, MediaTimestamp::new(90_000)).ending_frame();
    packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();

    let budget = 1200 - DatagramSizing::AUTHENTICATION_TAG_LEN;
    for datagram in &sent {
        assert!(
            datagram.byte_len() <= budget,
            "a {}-byte datagram leaves no room for the auth tag",
            datagram.byte_len()
        );
    }
}

#[test]
fn sequence_numbers_are_contiguous_and_indices_restart_each_frame() {
    let (mut packetizer, _pool) = packetizer(1200, 256);
    let payload = vec![0u8; 3_000];
    let mut sent = Vec::new();

    for tick in 0..3 {
        let slice = EncodedSlice::new(&payload, MediaTimestamp::new(90_000 * tick)).ending_frame();
        packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();
    }

    let per_frame = DatagramSizing::new(1200).unwrap().fragment_count(3_000);
    assert_eq!(sent.len(), per_frame * 3);

    for (position, datagram) in sent.iter().enumerate() {
        let header = datagram.header();
        assert_eq!(header.sequence.get() as usize, position);
        assert_eq!(usize::from(header.packet_index), position % per_frame);
        assert_eq!(header.frame.get() as usize, position / per_frame);
    }
}

#[test]
fn slices_of_one_frame_share_a_frame_id_and_number_continuously() {
    let (mut packetizer, _pool) = packetizer(1200, 64);
    let timestamp = MediaTimestamp::new(90_000);
    let mut sent = Vec::new();

    for index in 0..4u8 {
        let body = vec![index; 2_000];
        let slice = EncodedSlice::new(&body, timestamp);
        let slice = if index == 3 {
            slice.ending_frame()
        } else {
            slice
        };
        packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();
    }

    assert!(sent.iter().all(|d| d.header().frame.get() == 0));
    for (position, datagram) in sent.iter().enumerate() {
        assert_eq!(usize::from(datagram.header().packet_index), position);
    }

    let slice_ends = sent
        .iter()
        .filter(|d| d.header().flags.contains(PacketFlags::SLICE_END))
        .count();
    assert_eq!(slice_ends, 4, "each slice should mark its own boundary");

    let frame_ends = sent.iter().filter(|d| d.header().is_frame_end()).count();
    assert_eq!(frame_ends, 1);
}

#[test]
fn the_closing_packet_carries_the_true_fragment_count() {
    let (mut packetizer, _pool) = packetizer(1200, 64);
    let timestamp = MediaTimestamp::new(90_000);
    let mut sent = Vec::new();

    for index in 0..3u8 {
        let body = vec![index; 2_000];
        let slice = EncodedSlice::new(&body, timestamp);
        let slice = if index == 2 {
            slice.ending_frame()
        } else {
            slice
        };
        packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();
    }

    let total = u16::try_from(sent.len()).unwrap();
    let last = sent.last().unwrap().header();
    assert!(last.is_frame_end());
    assert_eq!(
        last.packet_count, total,
        "the closing packet must name the whole frame"
    );

    for datagram in &sent[..sent.len() - 1] {
        let header = datagram.header();
        assert_eq!(
            header.packet_count,
            header.packet_index + 1,
            "an open frame advertises only a lower bound"
        );
    }
}

#[test]
fn an_empty_slice_still_produces_a_packet() {
    let (mut packetizer, _pool) = packetizer(1200, 4);
    let mut sent = Vec::new();

    let slice = EncodedSlice::new(&[], MediaTimestamp::new(0)).ending_frame();
    packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();

    assert_eq!(sent.len(), 1);
    assert!(sent[0].header().is_frame_end());
}

#[test]
fn an_mtu_too_small_for_a_payload_byte_is_rejected() {
    let floor = DatagramSizing::MIN_MTU;
    assert!(DatagramSizing::new(floor).is_ok());
    assert_eq!(
        DatagramSizing::new(floor - 1).unwrap_err(),
        CoreError::PayloadExceedsMtu {
            payload_len: floor,
            mtu: floor - 1,
        }
    );
}

#[test]
fn packetizing_a_frame_burst_never_allocates_from_outside_the_pool() {
    let (mut packetizer, pool) = packetizer(1200, 64);
    let payload = vec![0u8; 40_000];
    let mut sent = Vec::with_capacity(64);

    for tick in 0..600 {
        let slice = EncodedSlice::new(&payload, MediaTimestamp::new(1_500 * tick)).ending_frame();
        packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();
        sent.clear();
    }

    assert_eq!(pool.misses(), 0, "the packetizer outgrew its pool");
}
