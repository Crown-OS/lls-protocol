#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use llts_core::CoreError;
use llts_core::repair::{RepairBlockDecoder, RepairBlockEncoder, ShardLayout};

const SHARD_BYTES: u16 = 1_156;

fn block(bytes: usize) -> Vec<u8> {
    (0..bytes)
        .map(|index| (index.wrapping_mul(31) % 251).to_le_bytes()[0])
        .collect()
}

fn data_shards(layout: ShardLayout, source: &[u8]) -> Vec<Vec<u8>> {
    (0..layout.data_shards())
        .map(|index| {
            let start = layout.shard_offset(index);
            let end = start + layout.shard_len(index);
            source[start..end].to_vec()
        })
        .collect()
}

/// Encodes `source`, delivers only the shards named, and returns the reassembled block.
fn round_trip(
    source: &[u8],
    parity_count: u16,
    deliver_data: &[u16],
    deliver_parity: &[u16],
) -> Result<Vec<u8>, CoreError> {
    let layout = ShardLayout::new(u32::try_from(source.len()).unwrap(), SHARD_BYTES)?;
    let shards = data_shards(layout, source);

    let mut encoder = RepairBlockEncoder::new();
    encoder.begin(layout, parity_count)?;
    for shard in &shards {
        encoder.add_data_shard(shard)?;
    }
    let parity: Vec<Vec<u8>> = encoder.encode()?.iter().map(<[u8]>::to_vec).collect();

    let mut decoder = RepairBlockDecoder::new();
    decoder.begin(layout, parity_count)?;
    for index in deliver_data {
        decoder.add_data_shard(*index, &shards[usize::from(*index)])?;
    }
    for index in deliver_parity {
        decoder.add_parity_shard(*index, &parity[usize::from(*index)])?;
    }

    let mut recovered = vec![0u8; source.len()];
    {
        let restored = decoder.decode()?;
        for index in 0..layout.data_shards() {
            let start = layout.shard_offset(index);
            let len = layout.shard_len(index);
            let bytes = match restored.get(index) {
                Some(shard) => &shard[..len],
                None => &shards[usize::from(index)][..len],
            };
            recovered[start..start + len].copy_from_slice(bytes);
        }
    }
    Ok(recovered)
}

#[test]
fn a_block_divides_into_equal_shards_with_a_short_tail() {
    let layout = ShardLayout::new(3_000, SHARD_BYTES).unwrap();

    assert_eq!(layout.data_shards(), 3);
    assert_eq!(layout.shard_len(0), usize::from(SHARD_BYTES));
    assert_eq!(layout.shard_len(1), usize::from(SHARD_BYTES));
    assert_eq!(layout.shard_len(2), 3_000 - 2 * usize::from(SHARD_BYTES));
    assert_eq!(layout.shard_offset(2), 2 * usize::from(SHARD_BYTES));
}

#[test]
fn an_odd_shard_size_is_rejected() {
    assert!(matches!(
        ShardLayout::new(3_000, 1_155).unwrap_err(),
        CoreError::RepairShardSize { .. }
    ));
    assert!(matches!(
        ShardLayout::new(3_000, 0).unwrap_err(),
        CoreError::RepairShardSize { .. }
    ));
}

#[test]
fn a_block_with_nothing_lost_comes_back_unchanged() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let all: Vec<u16> = (0..layout.data_shards()).collect();

    assert_eq!(round_trip(&source, 4, &all, &[]).unwrap(), source);
}

#[test]
fn one_lost_shard_is_recovered_from_one_parity_shard() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let survivors: Vec<u16> = (0..layout.data_shards()).filter(|i| *i != 7).collect();

    assert_eq!(round_trip(&source, 4, &survivors, &[0]).unwrap(), source);
}

#[test]
fn a_lost_tail_shard_is_recovered_and_trimmed() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let last = layout.data_shards() - 1;
    let survivors: Vec<u16> = (0..last).collect();

    let recovered = round_trip(&source, 2, &survivors, &[0]).unwrap();
    assert_eq!(recovered.len(), source.len());
    assert_eq!(recovered, source, "the short final shard came back padded");
}

#[test]
fn losses_up_to_the_parity_count_are_all_recoverable() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let parity_count = 6;

    for lost in 1..=parity_count {
        let survivors: Vec<u16> = (lost..layout.data_shards()).collect();
        let parity: Vec<u16> = (0..lost).collect();
        assert_eq!(
            round_trip(&source, parity_count, &survivors, &parity).unwrap(),
            source,
            "failed with {lost} shards lost"
        );
    }
}

#[test]
fn any_combination_of_shards_will_do() {
    let source = block(20_000);
    let layout = ShardLayout::new(20_000, SHARD_BYTES).unwrap();
    let total = layout.data_shards();

    // Drop three scattered data shards and recover from three scattered parity shards.
    let survivors: Vec<u16> = (0..total).filter(|i| ![1, 5, 11].contains(i)).collect();
    assert_eq!(
        round_trip(&source, 8, &survivors, &[2, 4, 7]).unwrap(),
        source
    );
}

#[test]
fn losing_more_than_the_parity_count_is_reported_not_guessed() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let survivors: Vec<u16> = (3..layout.data_shards()).collect();

    assert!(matches!(
        round_trip(&source, 2, &survivors, &[0, 1]).unwrap_err(),
        CoreError::InsufficientRepairShards { .. }
    ));
}

#[test]
fn a_single_shard_block_still_codes() {
    let source = block(500);
    let layout = ShardLayout::new(500, SHARD_BYTES).unwrap();
    assert_eq!(layout.data_shards(), 1);

    assert_eq!(round_trip(&source, 1, &[], &[0]).unwrap(), source);
}

#[test]
fn a_geometry_the_codec_cannot_handle_is_refused() {
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let mut encoder = RepairBlockEncoder::new();

    assert!(matches!(
        encoder.begin(layout, 0).unwrap_err(),
        CoreError::UnsupportedRepairGeometry {
            parity_shards: 0,
            ..
        }
    ));
}

#[test]
fn encoding_before_every_shard_arrives_is_refused() {
    let source = block(40_000);
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let shards = data_shards(layout, &source);

    let mut encoder = RepairBlockEncoder::new();
    encoder.begin(layout, 4).unwrap();
    encoder.add_data_shard(&shards[0]).unwrap();

    assert!(matches!(
        encoder.encode().unwrap_err(),
        CoreError::InsufficientRepairShards { available: 1, .. }
    ));
}

#[test]
fn feeding_the_codec_without_opening_a_block_is_refused() {
    let mut encoder = RepairBlockEncoder::new();
    assert!(matches!(
        encoder.add_data_shard(&[0; 16]).unwrap_err(),
        CoreError::RepairBlockNotStarted
    ));

    let mut decoder = RepairBlockDecoder::new();
    assert!(matches!(
        decoder.add_parity_shard(0, &[0; 16]).unwrap_err(),
        CoreError::RepairBlockNotStarted
    ));
}

#[test]
fn an_oversized_shard_is_refused() {
    let layout = ShardLayout::new(40_000, SHARD_BYTES).unwrap();
    let mut encoder = RepairBlockEncoder::new();
    encoder.begin(layout, 2).unwrap();

    assert!(matches!(
        encoder
            .add_data_shard(&vec![0; usize::from(SHARD_BYTES) + 1])
            .unwrap_err(),
        CoreError::RepairShardSize { .. }
    ));
}

#[test]
fn the_decoder_reports_when_it_has_enough() {
    let source = block(20_000);
    let layout = ShardLayout::new(20_000, SHARD_BYTES).unwrap();
    let shards = data_shards(layout, &source);

    let mut decoder = RepairBlockDecoder::new();
    decoder.begin(layout, 4).unwrap();
    assert!(!decoder.is_decodable());

    for index in 0..layout.data_shards() {
        decoder
            .add_data_shard(index, &shards[usize::from(index)])
            .unwrap();
    }
    assert!(decoder.is_decodable());
    assert_eq!(decoder.supplied(), layout.data_shards());
}

#[test]
fn one_codec_instance_handles_a_run_of_differently_shaped_blocks() {
    let mut encoder = RepairBlockEncoder::new();
    let mut decoder = RepairBlockDecoder::new();

    for frame in 1..=200u16 {
        let bytes = 1_000 + (usize::from(frame) % 37) * 900;
        let source = block(bytes);
        let layout = ShardLayout::new(u32::try_from(bytes).unwrap(), SHARD_BYTES).unwrap();
        let shards = data_shards(layout, &source);
        let parity_count = 2;

        encoder.begin(layout, parity_count).unwrap();
        for shard in &shards {
            encoder.add_data_shard(shard).unwrap();
        }
        let parity: Vec<Vec<u8>> = encoder
            .encode()
            .unwrap()
            .iter()
            .map(<[u8]>::to_vec)
            .collect();

        decoder.begin(layout, parity_count).unwrap();
        let dropped = frame % layout.data_shards();
        for index in (0..layout.data_shards()).filter(|i| *i != dropped) {
            decoder
                .add_data_shard(index, &shards[usize::from(index)])
                .unwrap();
        }
        decoder.add_parity_shard(0, &parity[0]).unwrap();

        let restored = decoder.decode().unwrap();
        let len = layout.shard_len(dropped);
        assert_eq!(
            &restored.get(dropped).unwrap()[..len],
            &shards[usize::from(dropped)][..],
            "frame {frame} recovered the wrong bytes"
        );
    }
}
