#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::buffer::{BufferPool, Datagram};
use llts_core::clock::Instant;
use llts_core::limits::MAX_FRAMES_IN_FLIGHT;
use llts_core::receive::{FragmentAcceptance, FrameAssembler};
use llts_core::send::{DatagramSizing, EncodedSlice, FramePacketizer};
use llts_wire::packet::MediaPacketView;
use llts_wire::{MediaTimestamp, MonotonicMicros, StreamId};

const MTU: usize = 1200;
const MAX_FRAME_BYTES: usize = 256 * 1024;
const SCREEN: StreamId = StreamId::new(1);
const AT_ONE_MS: MonotonicMicros = MonotonicMicros::new(1_000);

struct Link {
    packetizer: FramePacketizer,
    assembler: FrameAssembler,
    clock: Instant,
}

impl Link {
    fn new() -> Self {
        let sizing = DatagramSizing::new(MTU).unwrap();
        Self {
            packetizer: FramePacketizer::new(SCREEN, sizing, BufferPool::new(MTU, 512)),
            assembler: FrameAssembler::new(
                BufferPool::new(MAX_FRAME_BYTES, 8),
                BufferPool::new(MTU, 128),
            ),
            clock: Instant::SESSION_START,
        }
    }

    fn encode(&mut self, slice: EncodedSlice<'_>) -> Vec<Datagram> {
        let mut sent = Vec::new();
        self.packetizer
            .packetize(slice, AT_ONE_MS, &mut sent)
            .unwrap();
        sent
    }

    fn deliver(&mut self, datagrams: &[Datagram]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for datagram in datagrams {
            self.clock = Instant::from_micros(self.clock.as_micros() + 100);
            let packet = MediaPacketView::parse(datagram.bytes()).unwrap();
            if let FragmentAcceptance::Complete(frame) =
                self.assembler.accept(&packet, self.clock).unwrap()
            {
                frames.push(frame.bytes().to_vec());
            }
        }
        frames
    }
}

fn ramp(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add(index.to_le_bytes()[0]))
        .collect()
}

#[test]
fn a_frame_survives_the_round_trip_byte_for_byte() {
    let mut link = Link::new();
    let payload = ramp(40_000, 0);

    let sent = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(90_000)).ending_frame());
    let received = link.deliver(&sent);

    assert_eq!(received.len(), 1);
    assert_eq!(received[0], payload);
}

#[test]
fn reversed_arrival_order_reassembles_correctly() {
    let mut link = Link::new();
    let payload = ramp(40_000, 7);

    let mut sent =
        link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(90_000)).ending_frame());
    sent.reverse();
    let received = link.deliver(&sent);

    assert_eq!(received.len(), 1);
    assert_eq!(received[0], payload);
}

#[test]
fn a_frame_split_into_slices_reassembles_across_the_short_fragments() {
    let mut link = Link::new();
    let timestamp = MediaTimestamp::new(90_000);

    // 2000 is deliberately not a whole number of payloads, so every slice but the first starts
    // at an offset that fragment index alone could not predict.
    let slices: Vec<Vec<u8>> = (0..4u8).map(|index| ramp(2_000, index * 31)).collect();
    let mut sent = Vec::new();
    for (index, body) in slices.iter().enumerate() {
        let slice = EncodedSlice::new(body, timestamp);
        let slice = if index == 3 {
            slice.ending_frame()
        } else {
            slice
        };
        sent.extend(link.encode(slice));
    }

    let received = link.deliver(&sent);
    let expected: Vec<u8> = slices.concat();

    assert_eq!(received.len(), 1);
    assert_eq!(received[0].len(), expected.len());
    assert_eq!(received[0], expected);
}

#[test]
fn slices_arriving_out_of_order_still_reassemble() {
    let mut link = Link::new();
    let timestamp = MediaTimestamp::new(90_000);
    let slices: Vec<Vec<u8>> = (0..3u8).map(|index| ramp(1_700, index * 13)).collect();

    let mut sent = Vec::new();
    for (index, body) in slices.iter().enumerate() {
        let slice = EncodedSlice::new(body, timestamp);
        let slice = if index == 2 {
            slice.ending_frame()
        } else {
            slice
        };
        sent.extend(link.encode(slice));
    }
    sent.rotate_left(3);

    let received = link.deliver(&sent);
    assert_eq!(received, vec![slices.concat()]);
}

#[test]
fn a_duplicate_fragment_is_reported_and_ignored() {
    let mut link = Link::new();
    let payload = ramp(3_000, 0);
    let sent = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame());

    let first = MediaPacketView::parse(sent[0].bytes()).unwrap();
    let now = Instant::from_micros(10);
    assert!(matches!(
        link.assembler.accept(&first, now).unwrap(),
        FragmentAcceptance::Pending
    ));
    assert!(matches!(
        link.assembler.accept(&first, now).unwrap(),
        FragmentAcceptance::Duplicate
    ));
}

#[test]
fn an_incomplete_frame_reports_exactly_what_is_missing() {
    let mut link = Link::new();
    let payload = ramp(5_000, 0);
    let sent = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame());
    assert!(sent.len() >= 5);

    // Drop fragment 2 and deliver the rest, including the closing one so the total is known.
    for (index, datagram) in sent.iter().enumerate() {
        if index == 2 {
            continue;
        }
        let packet = MediaPacketView::parse(datagram.bytes()).unwrap();
        link.assembler
            .accept(&packet, Instant::from_micros(index as u64))
            .unwrap();
    }

    let missing: Vec<u16> = link
        .assembler
        .missing_fragments()
        .map(|(_, index)| index)
        .collect();
    assert_eq!(missing, vec![2]);
}

#[test]
fn the_missing_fragment_completes_the_frame_when_it_arrives_late() {
    let mut link = Link::new();
    let payload = ramp(5_000, 3);
    let sent = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame());

    let mut ordered: Vec<&Datagram> = sent.iter().collect();
    let delayed = ordered.remove(1);
    ordered.push(delayed);

    let mut completed = Vec::new();
    for (tick, datagram) in ordered.iter().enumerate() {
        let packet = MediaPacketView::parse(datagram.bytes()).unwrap();
        if let FragmentAcceptance::Complete(frame) = link
            .assembler
            .accept(&packet, Instant::from_micros(tick as u64))
            .unwrap()
        {
            completed.push(frame.bytes().to_vec());
        }
    }

    assert_eq!(completed, vec![payload]);
}

#[test]
fn a_fragment_for_an_already_delivered_frame_is_obsolete() {
    let mut link = Link::new();
    let payload = ramp(1_000, 0);
    let sent = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame());

    let packet = MediaPacketView::parse(sent[0].bytes()).unwrap();
    let now = Instant::from_micros(1);
    assert!(matches!(
        link.assembler.accept(&packet, now).unwrap(),
        FragmentAcceptance::Complete(_)
    ));
    assert!(matches!(
        link.assembler.accept(&packet, now).unwrap(),
        FragmentAcceptance::Obsolete
    ));
}

#[test]
fn a_stalled_frame_is_evicted_once_newer_frames_need_the_slot() {
    let mut link = Link::new();
    let payload = ramp(3_000, 0);

    // Open one frame with a single fragment and never finish it.
    let stalled = link.encode(EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame());
    let opener = MediaPacketView::parse(stalled[0].bytes()).unwrap();
    link.assembler
        .accept(&opener, Instant::from_micros(1))
        .unwrap();

    let overflow_by_one = u32::try_from(MAX_FRAMES_IN_FLIGHT).unwrap() + 1;
    for tick in 1..=overflow_by_one {
        let body = ramp(3_000, tick.to_le_bytes()[0]);
        let sent =
            link.encode(EncodedSlice::new(&body, MediaTimestamp::new(1_500 * tick)).ending_frame());
        let head = MediaPacketView::parse(sent[0].bytes()).unwrap();
        link.assembler
            .accept(&head, Instant::from_micros(u64::from(tick) * 1_000))
            .unwrap();
    }

    assert!(link.assembler.in_flight() <= MAX_FRAMES_IN_FLIGHT);
    assert!(
        link.assembler.evicted_incomplete() >= 1,
        "the stalled frame was never reclaimed"
    );
}

#[test]
fn sustained_traffic_never_outgrows_either_pool() {
    let sizing = DatagramSizing::new(MTU).unwrap();
    let datagram_pool = BufferPool::new(MTU, 128);
    let frame_pool = BufferPool::new(MAX_FRAME_BYTES, 8);
    let mut packetizer = FramePacketizer::new(SCREEN, sizing, datagram_pool.clone());
    let mut assembler = FrameAssembler::new(frame_pool.clone(), BufferPool::new(MTU, 128));
    let payload = ramp(40_000, 0);
    let mut sent = Vec::with_capacity(64);

    for tick in 0..600u32 {
        sent.clear();
        let slice = EncodedSlice::new(&payload, MediaTimestamp::new(1_500 * tick)).ending_frame();
        packetizer.packetize(slice, AT_ONE_MS, &mut sent).unwrap();
        for datagram in &sent {
            let packet = MediaPacketView::parse(datagram.bytes()).unwrap();
            assembler
                .accept(&packet, Instant::from_micros(u64::from(tick) * 16_667))
                .unwrap();
        }
    }

    assert_eq!(datagram_pool.misses(), 0, "the datagram pool overflowed");
    assert_eq!(frame_pool.misses(), 0, "the frame pool overflowed");
}
