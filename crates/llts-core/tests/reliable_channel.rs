#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

mod support;

use std::collections::HashSet;
use std::time::Duration;

use llts_core::CoreError;
use llts_core::channel::{Delivery, MAX_MESSAGE_LEN};
use llts_core::session::Event;
use llts_core::stream::StreamKey;
use llts_wire::StreamClass;
use support::{Lcg, LinkModel, Pair};

const DATA: StreamKey = StreamKey::new(StreamClass::Data, 0);

const LOSSY_AND_REORDERING: LinkModel = LinkModel {
    one_way: Duration::from_millis(1),
    jitter: Duration::from_millis(4),
    loss_per_mille: 200,
    drop_every: 0,
};

/// A message whose first two bytes put it outside the session's reserved types and whose body
/// says which message it is.
fn message(index: u32, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len.max(6)];
    bytes[..2].copy_from_slice(&0x4000u16.to_le_bytes());
    bytes[2..6].copy_from_slice(&index.to_le_bytes());
    for (position, byte) in bytes.iter_mut().enumerate().skip(6) {
        *byte = position.to_le_bytes()[0] ^ index.to_le_bytes()[0];
    }
    bytes
}

fn data_pair(model: LinkModel) -> Pair {
    let mut pair = Pair::new(model);
    pair.left.open_reliable(DATA).unwrap();
    pair.right.open_reliable(DATA).unwrap();
    pair
}

fn delivered_on(pair: &Pair, stream: StreamKey) -> Vec<Vec<u8>> {
    pair.right_seen
        .delivered
        .iter()
        .filter(|(key, _)| *key == stream)
        .map(|(_, bytes)| bytes.clone())
        .collect()
}

#[test]
fn ordered_messages_arrive_exactly_once_and_in_order_through_heavy_loss_and_reordering() {
    let mut pair = data_pair(LOSSY_AND_REORDERING);
    let mut sizes = Lcg::new(7);
    let mut sent = Vec::new();

    for index in 0..300u32 {
        let len = match sizes.below(10) {
            0 => 20_000,
            1..=2 => 3_000,
            _ => 40 + sizes.below(400) as usize,
        };
        let bytes = message(index, len);
        for stream in [StreamKey::CONTROL, DATA] {
            pair.left_event(&Event::ControlQueued {
                stream,
                message: &bytes,
                delivery: Delivery::Ordered,
            });
        }
        sent.push(bytes);
        pair.run_for(Duration::from_millis(5));
    }
    pair.run_for(Duration::from_secs(10));

    assert!(pair.dropped() > 500, "the loss model barely fired");
    for stream in [StreamKey::CONTROL, DATA] {
        let delivered = delivered_on(&pair, stream);
        assert_eq!(
            delivered.len(),
            sent.len(),
            "{stream:?} lost or duplicated messages"
        );
        assert!(
            delivered == sent,
            "{stream:?} delivered out of order or corrupted"
        );
    }
}

#[test]
fn unordered_messages_arrive_exactly_once() {
    let mut pair = data_pair(LOSSY_AND_REORDERING);
    let mut sent = HashSet::new();
    for index in 0..200u32 {
        let bytes = message(index, 200 + (index as usize % 5) * 1_500);
        pair.left_event(&Event::ControlQueued {
            stream: DATA,
            message: &bytes,
            delivery: Delivery::Unordered,
        });
        sent.insert(bytes);
        pair.run_for(Duration::from_millis(3));
    }
    pair.run_for(Duration::from_secs(10));

    let delivered = delivered_on(&pair, DATA);
    let unique: HashSet<_> = delivered.iter().cloned().collect();
    assert_eq!(
        delivered.len(),
        unique.len(),
        "a message was delivered twice"
    );
    assert_eq!(unique, sent);
}

#[test]
fn a_message_past_the_limit_is_refused() {
    let mut pair = data_pair(LinkModel::CLEAN);
    let oversized = message(0, MAX_MESSAGE_LEN + 1);
    let refused = pair.left.handle(
        pair.now,
        &Event::ControlQueued {
            stream: DATA,
            message: &oversized,
            delivery: Delivery::Ordered,
        },
    );
    assert!(matches!(refused, Err(CoreError::MessageTooLarge { .. })));
}

#[test]
fn the_largest_message_crosses_intact() {
    let mut pair = data_pair(LinkModel::CLEAN);
    let limit = pair.left.config().sizing.max_payload_len() - 4;
    let largest = message(1, (limit * 57).min(MAX_MESSAGE_LEN));
    pair.left_event(&Event::ControlQueued {
        stream: DATA,
        message: &largest,
        delivery: Delivery::Ordered,
    });
    pair.run_for(Duration::from_millis(200));
    assert_eq!(delivered_on(&pair, DATA), vec![largest]);
}

#[test]
fn the_application_cannot_forge_a_session_message() {
    let mut pair = data_pair(LinkModel::CLEAN);
    let refused = pair.left.handle(
        pair.now,
        &Event::ControlQueued {
            stream: StreamKey::CONTROL,
            message: &[0x01, 0x00, 0x10, 0x00],
            delivery: Delivery::Ordered,
        },
    );
    assert!(matches!(
        refused,
        Err(CoreError::ReservedMessageType {
            message_type: 0x0001
        })
    ));
}
