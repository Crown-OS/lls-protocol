#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that indexes out of bounds should fail loudly, not return an error"
)]

use llts_wire::packet::{
    BodyPrefix, CounterRecord, EcnMark, FeedbackEntry, FeedbackReport, HandshakePattern,
    HandshakeView, InputBatch, InputBatches, MediaPacketView, MissingRange, NackRequest,
    PacketFlags, PacketHeader, PacketKind, PacketView, PathProbe, PathProbeAck,
    RepairBlockGeometry, RepairPacketView, SequenceSpace,
};
use llts_wire::{
    FrameId, MediaTimestamp, MonotonicMicros, ProtocolVersion, SequenceNumber, SessionTag,
    StreamClass, StreamId, WireError,
};

fn sample_header(kind: PacketKind) -> PacketHeader {
    PacketHeader {
        version: ProtocolVersion::CURRENT,
        kind,
        flags: PacketFlags::FRAME_END | PacketFlags::KEYFRAME,
        stream: StreamId::new(0xDEAD_BEEF),
        sequence: SequenceNumber::new(40_000),
        frame: FrameId::new(1234),
        timestamp: MediaTimestamp::new(90_000),
        packet_index: 3,
        packet_count: 32,
        departure: MonotonicMicros::new(123_456_789),
        payload_offset: 3_456,
    }
}

fn encode(header: &PacketHeader, body: &[u8]) -> Vec<u8> {
    let mut datagram = vec![0u8; PacketHeader::ENCODED_LEN + body.len()];
    let written = header.write_into(&mut datagram).unwrap();
    assert_eq!(written, PacketHeader::ENCODED_LEN);
    datagram[written..].copy_from_slice(body);
    datagram
}

/// Prepends the body prefix a paced packet of this header carries.
fn with_prefix(header: &PacketHeader, prefix: BodyPrefix, body: &[u8]) -> Vec<u8> {
    let mut framed = vec![0u8; BodyPrefix::encoded_len(header.kind, header.flags)];
    prefix.write_into(header, &mut framed).unwrap();
    framed.extend_from_slice(body);
    framed
}

#[test]
fn the_header_has_the_documented_size() {
    assert_eq!(PacketHeader::ENCODED_LEN, 28);
    let datagram = encode(&sample_header(PacketKind::Media), &[]);
    assert_eq!(datagram.len(), 28);
}

#[test]
fn header_survives_a_round_trip() {
    let original = sample_header(PacketKind::Media);
    let datagram = encode(&original, b"payload");
    let (decoded, payload) = PacketHeader::read_from(&datagram).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(payload, b"payload");
}

#[test]
fn every_truncation_is_rejected_without_panicking() {
    let datagram = encode(&sample_header(PacketKind::Media), b"payload");
    for length in 0..PacketHeader::ENCODED_LEN {
        let error = PacketHeader::read_from(&datagram[..length]).unwrap_err();
        assert!(
            matches!(error, WireError::TooShort { .. } | WireError::BadMagic),
            "length {length} produced {error:?}"
        );
    }
}

#[test]
fn foreign_traffic_on_the_port_is_rejected() {
    let error = PacketHeader::read_from(&[0xFF; 64]).unwrap_err();
    assert_eq!(error, WireError::BadMagic);
}

#[test]
fn a_mismatched_version_names_both_sides() {
    let mut datagram = encode(&sample_header(PacketKind::Media), &[]);
    datagram[2] = (9 << 4) | PacketKind::Media as u8;
    assert_eq!(
        PacketHeader::read_from(&datagram).unwrap_err(),
        WireError::UnsupportedVersion { peer: 9, local: 1 }
    );
}

#[test]
fn an_index_beyond_the_count_is_malformed() {
    let mut header = sample_header(PacketKind::Media);
    header.packet_index = header.packet_count;
    let datagram = encode(&header, &[]);
    assert!(matches!(
        PacketHeader::read_from(&datagram).unwrap_err(),
        WireError::MalformedField {
            field: "packet_index"
        }
    ));
}

#[test]
fn a_zero_packet_count_is_malformed() {
    let mut header = sample_header(PacketKind::Media);
    header.packet_index = 0;
    header.packet_count = 0;
    let datagram = encode(&header, &[]);
    assert!(matches!(
        PacketHeader::read_from(&datagram).unwrap_err(),
        WireError::MalformedField {
            field: "packet_count"
        }
    ));
}

#[test]
fn asking_a_media_parser_for_a_repair_packet_names_both_kinds() {
    let datagram = encode(&sample_header(PacketKind::Repair), &[0; 8]);
    assert_eq!(
        MediaPacketView::parse(&datagram).unwrap_err(),
        WireError::UnexpectedPacketKind {
            expected: "media",
            found: "repair",
        }
    );
}

#[test]
fn a_media_view_borrows_its_payload() {
    let header = sample_header(PacketKind::Media);
    let datagram = encode(
        &header,
        &with_prefix(&header, BodyPrefix::fresh(), b"slice bytes"),
    );
    let view = MediaPacketView::parse(&datagram).unwrap();
    assert_eq!(view.payload(), b"slice bytes");
    assert_eq!(view.media_sequence(), header.sequence);
    assert!(view.header().is_frame_end());
    assert!(view.header().is_keyframe());
    assert!(!view.header().is_discardable());
}

#[test]
fn a_repair_shard_reports_its_parity_index() {
    let mut header = sample_header(PacketKind::Repair);
    header.packet_index = 34;
    header.packet_count = 39;

    let geometry = RepairBlockGeometry {
        first_data_index: 0,
        data_shard_count: 32,
        shard_bytes: 1_156,
        block_byte_offset: 0,
        block_byte_len: 35_000,
    };
    let mut body = vec![0u8; RepairBlockGeometry::ENCODED_LEN + 4];
    geometry.write_into(&mut body).unwrap();
    body[RepairBlockGeometry::ENCODED_LEN..].copy_from_slice(&[1, 2, 3, 4]);

    let datagram = encode(&header, &with_prefix(&header, BodyPrefix::fresh(), &body));
    let view = RepairPacketView::parse(&datagram).unwrap();
    assert_eq!(view.geometry(), geometry);
    assert_eq!(view.parity_index(), 2);
    assert_eq!(view.shard(), &[1, 2, 3, 4]);
}

#[test]
fn a_block_geometry_locates_every_shard_in_its_frame() {
    let geometry = RepairBlockGeometry {
        first_data_index: 4,
        data_shard_count: 3,
        shard_bytes: 1_000,
        block_byte_offset: 4_624,
        block_byte_len: 2_500,
    };

    assert_eq!(geometry.shard_offset(0), 4_624);
    assert_eq!(geometry.shard_offset(2), 6_624);
    assert_eq!(geometry.shard_len(0), 1_000);
    assert_eq!(geometry.shard_len(2), 500, "the final shard is short");
    assert_eq!(geometry.shard_len(3), 0, "past the block there is nothing");
}

#[test]
fn an_odd_shard_size_is_malformed() {
    let mut header = sample_header(PacketKind::Repair);
    header.packet_index = 4;
    header.packet_count = 8;
    let geometry = RepairBlockGeometry {
        first_data_index: 0,
        data_shard_count: 4,
        shard_bytes: 1_155,
        block_byte_offset: 0,
        block_byte_len: 4_000,
    };
    let mut body = vec![0u8; RepairBlockGeometry::ENCODED_LEN];
    geometry.write_into(&mut body).unwrap();
    let body = with_prefix(&header, BodyPrefix::fresh(), &body);

    assert!(matches!(
        RepairPacketView::parse(&encode(&header, &body)).unwrap_err(),
        WireError::MalformedField {
            field: "shard_bytes"
        }
    ));
}

#[test]
fn a_parity_index_inside_the_data_range_is_malformed() {
    let mut header = sample_header(PacketKind::Repair);
    header.packet_index = 5;
    header.packet_count = 39;
    let geometry = RepairBlockGeometry {
        first_data_index: 0,
        data_shard_count: 32,
        shard_bytes: 1_156,
        block_byte_offset: 0,
        block_byte_len: 35_000,
    };
    let mut body = vec![0u8; RepairBlockGeometry::ENCODED_LEN];
    geometry.write_into(&mut body).unwrap();
    let body = with_prefix(&header, BodyPrefix::fresh(), &body);
    assert!(matches!(
        RepairPacketView::parse(&encode(&header, &body)).unwrap_err(),
        WireError::MalformedField {
            field: "packet_index"
        }
    ));
}

#[test]
fn feedback_entries_round_trip_in_order() {
    let entries = [
        FeedbackEntry {
            transport_sequence: SequenceNumber::new(65_534),
            arrival: MonotonicMicros::new(10),
            ecn: EcnMark::NotCapable,
        },
        FeedbackEntry {
            transport_sequence: SequenceNumber::new(65_535),
            arrival: MonotonicMicros::new(1_200),
            ecn: EcnMark::CongestionExperienced,
        },
        FeedbackEntry {
            transport_sequence: SequenceNumber::new(0),
            arrival: MonotonicMicros::new(2_400),
            ecn: EcnMark::Capable,
        },
    ];

    let mut body =
        vec![0u8; FeedbackReport::PREFIX_LEN + entries.len() * FeedbackEntry::ENCODED_LEN];
    FeedbackReport::write_prefix(&mut body, 42).unwrap();
    for (slot, entry) in body[FeedbackReport::PREFIX_LEN..]
        .chunks_exact_mut(FeedbackEntry::ENCODED_LEN)
        .zip(entries)
    {
        FeedbackReport::write_entry(slot, entry).unwrap();
    }

    let datagram = encode(&sample_header(PacketKind::Feedback), &body);
    let report = FeedbackReport::parse(&datagram).unwrap();
    assert_eq!(report.entry_count(), 3);
    assert_eq!(report.entries().collect::<Vec<_>>(), entries);
    assert_eq!(
        report.loss_per_mille(),
        42,
        "the receiver's own loss estimate is the only place the sender can learn it"
    );
}

#[test]
fn a_ragged_feedback_body_is_malformed() {
    // Two bytes of loss prefix plus eight bytes, which is not a whole number of entries.
    let datagram = encode(&sample_header(PacketKind::Feedback), &[0; 10]);
    assert!(matches!(
        FeedbackReport::parse(&datagram).unwrap_err(),
        WireError::MalformedField { field: "entries" }
    ));
}

#[test]
fn a_nack_range_expands_across_wraparound() {
    let range = MissingRange {
        first: SequenceNumber::new(65_534),
        extra: 3,
    };
    assert_eq!(range.sequence_count(), 4);
    let expanded: Vec<u16> = range.sequences().map(SequenceNumber::get).collect();
    assert_eq!(expanded, vec![65_534, 65_535, 0, 1]);
}

#[test]
fn nack_ranges_round_trip() {
    let ranges = [
        MissingRange::single(SequenceNumber::new(100)),
        MissingRange {
            first: SequenceNumber::new(200),
            extra: 7,
        },
    ];
    let mut body = vec![0u8; ranges.len() * MissingRange::ENCODED_LEN];
    for (slot, range) in body.chunks_exact_mut(MissingRange::ENCODED_LEN).zip(ranges) {
        NackRequest::write_range(slot, range).unwrap();
    }

    let datagram = encode(&sample_header(PacketKind::Nack), &body);
    let request = NackRequest::parse(&datagram).unwrap();
    assert_eq!(request.range_count(), 2);
    assert_eq!(request.ranges().collect::<Vec<_>>(), ranges);
}

#[test]
fn a_probe_ack_round_trips() {
    let ack = PathProbeAck {
        token: 0xABCD_1234,
        probe_arrival: MonotonicMicros::new(7),
        ack_departure: MonotonicMicros::new(19),
    };
    let mut body = vec![0u8; PathProbeAck::BODY_LEN];
    ack.write_body(&mut body).unwrap();
    let datagram = encode(&sample_header(PacketKind::ProbeAck), &body);
    assert_eq!(PathProbeAck::parse(&datagram).unwrap(), ack);
}

#[test]
fn dispatch_picks_the_right_view_for_every_kind() {
    let cases = [
        (PacketKind::Media, vec![0u8; 4]),
        (
            PacketKind::Feedback,
            vec![0u8; FeedbackReport::PREFIX_LEN + FeedbackEntry::ENCODED_LEN],
        ),
        (PacketKind::Nack, vec![0u8; MissingRange::ENCODED_LEN]),
        (PacketKind::Control, vec![0u8; 6]),
        (PacketKind::Probe, vec![0u8; PathProbe::BODY_LEN]),
        (PacketKind::ProbeAck, vec![0u8; PathProbeAck::BODY_LEN]),
        (PacketKind::Handshake, vec![0u8; HandshakeView::PREFIX_LEN]),
    ];
    for (kind, body) in cases {
        let datagram = encode(&sample_header(kind), &body);
        let view = PacketView::parse(&datagram).unwrap();
        assert!(
            matches!(
                (kind, view),
                (PacketKind::Media, PacketView::Media(_))
                    | (PacketKind::Feedback, PacketView::Feedback(_))
                    | (PacketKind::Nack, PacketView::Nack(_))
                    | (PacketKind::Control, PacketView::Control(_))
                    | (PacketKind::Probe, PacketView::Probe(_))
                    | (PacketKind::ProbeAck, PacketView::ProbeAck(_))
                    | (PacketKind::Handshake, PacketView::Handshake(_))
            ),
            "{kind:?} dispatched to the wrong view"
        );
    }
}

#[test]
fn an_unknown_kind_is_rejected() {
    let mut datagram = encode(&sample_header(PacketKind::Media), &[]);
    datagram[2] = (1 << 4) | 0x0F;
    assert_eq!(
        PacketHeader::read_from(&datagram).unwrap_err(),
        WireError::UnknownPacketKind(0x0F)
    );
}

#[test]
fn a_retransmission_carries_the_sequence_it_replaces() {
    let mut header = sample_header(PacketKind::Media);
    header.flags |= PacketFlags::RETRANSMISSION;
    header.sequence = SequenceNumber::new(9);
    let prefix = BodyPrefix::retransmitting(SequenceNumber::new(40_000));

    let datagram = encode(&header, &with_prefix(&header, prefix, b"again"));
    let view = MediaPacketView::parse(&datagram).unwrap();
    assert_eq!(view.payload(), b"again");
    assert_eq!(view.media_sequence(), SequenceNumber::new(40_000));
    assert_eq!(view.header().sequence, SequenceNumber::new(9));
}

#[test]
fn a_retransmission_without_its_original_cannot_be_written() {
    let mut header = sample_header(PacketKind::Media);
    header.flags |= PacketFlags::RETRANSMISSION;
    let mut destination = [0u8; BodyPrefix::MAX_ENCODED_LEN];
    assert!(
        BodyPrefix::fresh()
            .write_into(&header, &mut destination)
            .is_err()
    );
}

#[test]
fn the_transport_sequence_is_stamped_in_place() {
    let header = sample_header(PacketKind::Media);
    let mut datagram = encode(&header, &with_prefix(&header, BodyPrefix::fresh(), b"x"));
    BodyPrefix::stamp_transport_sequence(&mut datagram, SequenceNumber::new(777)).unwrap();
    let view = MediaPacketView::parse(&datagram).unwrap();
    assert_eq!(view.prefix().transport_sequence, SequenceNumber::new(777));
    assert_eq!(view.payload(), b"x");
}

#[test]
fn every_kind_maps_to_one_sequence_space() {
    let plain = PacketFlags::empty();
    assert_eq!(
        SequenceSpace::of(PacketKind::Media, plain),
        Some(SequenceSpace::Media)
    );
    assert_eq!(
        SequenceSpace::of(PacketKind::Repair, plain),
        Some(SequenceSpace::Media)
    );
    assert_eq!(
        SequenceSpace::of(PacketKind::Media, PacketFlags::RETRANSMISSION),
        Some(SequenceSpace::Retransmission)
    );
    assert_eq!(
        SequenceSpace::of(PacketKind::Nack, plain),
        Some(SequenceSpace::Feedback)
    );
    assert_eq!(
        SequenceSpace::of(PacketKind::ProbeAck, plain),
        Some(SequenceSpace::Probe)
    );
    assert_eq!(
        SequenceSpace::of(PacketKind::Control, plain),
        Some(SequenceSpace::Control)
    );
    assert_eq!(SequenceSpace::of(PacketKind::Handshake, plain), None);
    for space in SequenceSpace::ALL {
        assert_eq!(SequenceSpace::from_byte(space.as_byte()), Some(space));
    }
}

#[test]
fn probe_counter_records_round_trip_and_stop_at_capacity() {
    let stream = StreamId::compose(SessionTag::new(4), StreamClass::Video, 0);
    let records: Vec<_> = (0..10u64)
        .map(|counter| CounterRecord {
            stream,
            space: SequenceSpace::Media,
            counter: counter << 40,
        })
        .collect();

    let mut body = vec![0u8; PathProbe::BODY_LEN + 3 * CounterRecord::ENCODED_LEN + 2];
    PathProbe { token: 5 }.write_body(&mut body).unwrap();
    let reached = PathProbe::write_counter_records(&mut body, records.iter().copied()).unwrap();
    assert_eq!(
        reached,
        PathProbe::BODY_LEN + 3 * CounterRecord::ENCODED_LEN
    );

    let decoded: Vec<_> = PathProbe::counter_records(&body).unwrap().collect();
    assert_eq!(decoded, records[..3]);
}

#[test]
fn a_handshake_message_round_trips() {
    let mut header = sample_header(PacketKind::Handshake);
    header.flags = PacketFlags::empty();
    let mut body = vec![0u8; HandshakeView::PREFIX_LEN];
    HandshakeView::write_prefix(&mut body, HandshakePattern::Ik, 1, SessionTag::new(0x4242))
        .unwrap();
    body.extend_from_slice(b"noise");

    let datagram = encode(&header, &body);
    let view = HandshakeView::parse(&datagram).unwrap();
    assert_eq!(view.pattern(), HandshakePattern::Ik);
    assert_eq!(view.message_index(), 1);
    assert_eq!(view.sender_tag(), SessionTag::new(0x4242));
    assert_eq!(view.message(), b"noise");
}

#[test]
fn a_handshake_message_index_past_the_pattern_is_malformed() {
    let header = sample_header(PacketKind::Handshake);
    let mut body = vec![0u8; HandshakeView::PREFIX_LEN];
    HandshakeView::write_prefix(&mut body, HandshakePattern::Ik, 2, SessionTag::UNASSIGNED)
        .unwrap();
    assert!(HandshakeView::parse(&encode(&header, &body)).is_err());
}

#[test]
fn input_batches_round_trip_newest_first_and_stop_at_capacity() {
    let batches = [
        InputBatch {
            sequence: SequenceNumber::new(9),
            events: b"newest",
        },
        InputBatch {
            sequence: SequenceNumber::new(8),
            events: b"older",
        },
        InputBatch {
            sequence: SequenceNumber::new(7),
            events: b"oldest, does not fit",
        },
    ];
    let mut payload = vec![0u8; 1 + batches[0].encoded_len() + batches[1].encoded_len() + 3];
    let written = InputBatches::write(&mut payload, batches.iter().copied()).unwrap();
    assert_eq!(
        written,
        1 + batches[0].encoded_len() + batches[1].encoded_len()
    );

    let parsed = InputBatches::parse(&payload[..written]).unwrap();
    assert_eq!(parsed.count(), 2);
    assert_eq!(parsed.iter().collect::<Vec<_>>(), batches[..2]);
}
