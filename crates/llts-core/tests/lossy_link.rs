#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::buffer::BufferPool;
use llts_core::clock::Instant;
use llts_core::link::{LinkKind, LinkQuality};
use llts_core::receive::{FragmentAcceptance, FrameAssembler};
use llts_core::repair::{RepairMode, RepairPolicy, RepairRate};
use llts_core::send::{DatagramSizing, EncodedSlice, SendPipeline};
use llts_core::units::LossRate;
use llts_wire::packet::{MediaPacketView, PacketKind, PacketView, RepairPacketView};
use llts_wire::{MediaTimestamp, MonotonicMicros, StreamId};

const MTU: usize = 1200;
const MAX_FRAME_BYTES: usize = 512 * 1024;
const SCREEN: StreamId = StreamId::new(1);
const DEPARTURE: MonotonicMicros = MonotonicMicros::new(1_000);

struct Link {
    send: SendPipeline,
    assembler: FrameAssembler,
    clock: Instant,
}

impl Link {
    fn new() -> Self {
        let sizing = DatagramSizing::new(MTU).unwrap();
        Self {
            send: SendPipeline::new(SCREEN, sizing, BufferPool::new(MTU, 1024), 1 << 22),
            assembler: FrameAssembler::new(
                BufferPool::new(MAX_FRAME_BYTES, 8),
                BufferPool::new(MTU, 512),
            ),
            clock: Instant::SESSION_START,
        }
    }

    /// Submits a slice and collects every datagram the pipeline puts on the wire.
    fn transmit(&mut self, slice: EncodedSlice<'_>, parity_shards: u16) -> Vec<Vec<u8>> {
        let plan = fixed_plan(parity_shards);
        self.send.submit_slice(slice, &plan, DEPARTURE).unwrap();
        drain_paced(&mut self.send)
    }

    fn deliver(&mut self, wire: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for datagram in wire {
            self.clock = Instant::from_micros(self.clock.as_micros() + 100);
            let acceptance = match PacketView::parse(datagram).unwrap() {
                PacketView::Media(_) => {
                    let view = MediaPacketView::parse(datagram).unwrap();
                    self.assembler.accept(&view, self.clock).unwrap()
                }
                PacketView::Repair(_) => {
                    let view = RepairPacketView::parse(datagram).unwrap();
                    self.assembler.accept_repair(&view, self.clock).unwrap()
                }
                other => panic!("unexpected packet {other:?}"),
            };
            if let FragmentAcceptance::Complete(frame) = acceptance {
                frames.push(frame.bytes().to_vec());
            }
        }
        frames
    }
}

/// Collects every datagram the pipeline built, in the order it would put them on the wire.
fn drain_paced(send: &mut SendPipeline) -> Vec<Vec<u8>> {
    send.drain_outgoing()
        .map(|datagram| datagram.bytes().to_vec())
        .collect()
}

fn fixed_plan(parity_shards: u16) -> llts_core::repair::RepairPlan {
    let rate = if parity_shards == 0 {
        RepairRate::NONE
    } else {
        RepairRate::from_percent(50)
    };
    let mut plan = RepairPolicy::new(RepairMode::Fixed(rate)).plan(
        1,
        &LinkQuality {
            kind: LinkKind::Wifi,
            loss_rate: LossRate::NONE,
            round_trip: core::time::Duration::from_millis(2),
        },
    );
    plan.parity_shards = parity_shards;
    plan
}

fn ramp(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add(index.to_le_bytes()[0]))
        .collect()
}

fn kinds(wire: &[Vec<u8>]) -> Vec<PacketKind> {
    wire.iter()
        .map(|bytes| {
            llts_wire::packet::PacketHeader::read_from(bytes)
                .unwrap()
                .0
                .kind
        })
        .collect()
}

#[test]
fn a_protected_slice_emits_data_then_parity() {
    let mut link = Link::new();
    let payload = ramp(20_000, 0);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(90_000)).ending_frame(),
        4,
    );

    let kinds = kinds(&wire);
    let data = kinds.iter().filter(|k| **k == PacketKind::Media).count();
    let parity = kinds.iter().filter(|k| **k == PacketKind::Repair).count();

    assert_eq!(parity, 4);
    assert!(data >= 17, "20 kB should need at least 17 fragments");
    assert!(
        kinds[data..].iter().all(|k| *k == PacketKind::Repair),
        "parity should trail the data it protects"
    );
}

#[test]
fn no_parity_packet_exceeds_the_mtu() {
    let mut link = Link::new();
    let payload = ramp(40_000, 0);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        8,
    );

    let budget = MTU - DatagramSizing::AUTHENTICATION_TAG_LEN;
    for datagram in &wire {
        assert!(
            datagram.len() <= budget,
            "a {}-byte datagram leaves no room for the auth tag",
            datagram.len()
        );
    }
}

#[test]
fn a_lossless_delivery_still_reassembles() {
    let mut link = Link::new();
    let payload = ramp(20_000, 3);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        4,
    );

    assert_eq!(link.deliver(&wire), vec![payload]);
}

#[test]
fn a_dropped_fragment_is_rebuilt_from_parity() {
    let mut link = Link::new();
    let payload = ramp(20_000, 9);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        4,
    );

    let surviving: Vec<Vec<u8>> = wire
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 5)
        .map(|(_, bytes)| bytes.clone())
        .collect();

    assert_eq!(link.deliver(&surviving), vec![payload]);
    assert_eq!(link.assembler.repaired_fragments(), 1);
}

#[test]
fn a_burst_up_to_the_parity_count_is_rebuilt() {
    let mut link = Link::new();
    let payload = ramp(30_000, 11);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        6,
    );

    let lost = [2usize, 3, 4, 5, 6, 7];
    let surviving: Vec<Vec<u8>> = wire
        .iter()
        .enumerate()
        .filter(|(index, _)| !lost.contains(index))
        .map(|(_, bytes)| bytes.clone())
        .collect();

    assert_eq!(link.deliver(&surviving), vec![payload]);
    assert_eq!(link.assembler.repaired_fragments(), 6);
}

#[test]
fn a_lost_final_fragment_is_rebuilt_and_trimmed() {
    let mut link = Link::new();
    let payload = ramp(20_001, 4);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        4,
    );

    let last_media = kinds(&wire)
        .iter()
        .rposition(|kind| *kind == PacketKind::Media)
        .unwrap();
    let surviving: Vec<Vec<u8>> = wire
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != last_media)
        .map(|(_, bytes)| bytes.clone())
        .collect();

    let received = link.deliver(&surviving);
    assert_eq!(received.len(), 1);
    assert_eq!(
        received[0].len(),
        payload.len(),
        "trimmed to the wrong length"
    );
    assert_eq!(received[0], payload);
}

#[test]
fn losing_more_than_the_parity_count_leaves_the_frame_incomplete() {
    let mut link = Link::new();
    let payload = ramp(20_000, 1);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        2,
    );

    let lost = [1usize, 2, 3];
    let surviving: Vec<Vec<u8>> = wire
        .iter()
        .enumerate()
        .filter(|(index, _)| !lost.contains(index))
        .map(|(_, bytes)| bytes.clone())
        .collect();

    assert!(
        link.deliver(&surviving).is_empty(),
        "a frame was delivered with unrecoverable holes"
    );
}

#[test]
fn each_slice_is_protected_by_its_own_block() {
    let mut link = Link::new();
    let timestamp = MediaTimestamp::new(90_000);
    let slices: Vec<Vec<u8>> = (0..3u8).map(|index| ramp(5_000, index * 17)).collect();

    let mut wire = Vec::new();
    for (index, body) in slices.iter().enumerate() {
        let slice = EncodedSlice::new(body, timestamp);
        let slice = if index == 2 {
            slice.ending_frame()
        } else {
            slice
        };
        let mut emitted = link.transmit(slice, 2);
        // Drop the first data fragment of every slice; each block must repair its own.
        emitted.remove(0);
        wire.append(&mut emitted);
    }

    assert_eq!(link.deliver(&wire), vec![slices.concat()]);
    assert_eq!(link.assembler.repaired_fragments(), 3);
}

#[test]
fn parity_arriving_before_the_data_it_protects_still_repairs() {
    let mut link = Link::new();
    let payload = ramp(20_000, 6);
    let wire = link.transmit(
        EncodedSlice::new(&payload, MediaTimestamp::new(0)).ending_frame(),
        4,
    );

    let (data, parity): (Vec<_>, Vec<_>) = wire
        .iter()
        .cloned()
        .zip(kinds(&wire))
        .partition(|(_, kind)| *kind == PacketKind::Media);

    let mut reordered: Vec<Vec<u8>> = parity.into_iter().map(|(bytes, _)| bytes).collect();
    reordered.extend(data.into_iter().map(|(bytes, _)| bytes).skip(1));

    assert_eq!(link.deliver(&reordered), vec![payload]);
}

#[test]
fn a_protected_stream_never_outgrows_its_pools() {
    let sizing = DatagramSizing::new(MTU).unwrap();
    let datagram_pool = BufferPool::new(MTU, 512);
    let frame_pool = BufferPool::new(MAX_FRAME_BYTES, 8);
    let shard_pool = BufferPool::new(MTU, 256);

    let mut send = SendPipeline::new(SCREEN, sizing, datagram_pool.clone(), MTU * 128);
    let mut assembler = FrameAssembler::new(frame_pool.clone(), shard_pool.clone());
    let payload = ramp(40_000, 0);
    let plan = fixed_plan(4);

    for tick in 0..300u32 {
        send.submit_slice(
            EncodedSlice::new(&payload, MediaTimestamp::new(1_500 * tick)).ending_frame(),
            &plan,
            DEPARTURE,
        )
        .unwrap();

        let wire = drain_paced(&mut send);

        for datagram in &wire {
            let now = Instant::from_micros(u64::from(tick) * 16_667);
            match PacketView::parse(datagram).unwrap() {
                PacketView::Media(_) => {
                    let view = MediaPacketView::parse(datagram).unwrap();
                    assembler.accept(&view, now).unwrap();
                }
                PacketView::Repair(_) => {
                    let view = RepairPacketView::parse(datagram).unwrap();
                    assembler.accept_repair(&view, now).unwrap();
                }
                other => panic!("unexpected packet {other:?}"),
            }
        }
    }

    assert_eq!(datagram_pool.misses(), 0, "the datagram pool overflowed");
    assert_eq!(frame_pool.misses(), 0, "the frame pool overflowed");
    assert_eq!(shard_pool.misses(), 0, "the parity pool overflowed");
}
