//! Security tests for storage::segment (SegmentPacker, SegmentHeader, PackedSegment, SegmentEntry).

use claudefs_storage::block::{BlockId, BlockRef, BlockSize, PlacementHint};
use claudefs_storage::checksum::{compute, verify, ChecksumAlgorithm};
use claudefs_storage::segment::{
    PackedSegment, SegmentEntry, SegmentHeader, SegmentPacker, SegmentPackerConfig,
    SegmentPackerStats, SEGMENT_MAGIC, SEGMENT_SIZE,
};

fn test_block_ref() -> BlockRef {
    BlockRef {
        id: BlockId::new(0, 100),
        size: BlockSize::B4K,
    }
}

#[test]
fn test_stor_seg_sec_magic_number_is_correct() {
    assert_eq!(SEGMENT_MAGIC, 0x43534547);
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.header.magic, SEGMENT_MAGIC);
}

#[test]
fn test_stor_seg_sec_version_is_one() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.header.version, 1);
}

#[test]
fn test_stor_seg_sec_sealed_at_secs_reasonable() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    let year_2020_secs: u64 = 1577836800;
    assert!(segment.header.sealed_at_secs > year_2020_secs);
}

#[test]
fn test_stor_seg_sec_segment_size_is_2mb() {
    assert_eq!(SEGMENT_SIZE, 2 * 1024 * 1024);
}

#[test]
fn test_stor_seg_sec_overflow_triggers_auto_seal() {
    let config = SegmentPackerConfig {
        target_size: 100,
        checksum_algorithm: ChecksumAlgorithm::Crc32c,
    };
    let packer = SegmentPacker::new(config);
    packer
        .add_entry(1, test_block_ref(), vec![1u8; 60], PlacementHint::Journal)
        .unwrap();
    let result = packer
        .add_entry(2, test_block_ref(), vec![2u8; 60], PlacementHint::Journal)
        .unwrap();
    assert!(result.is_some());
    let segment = result.unwrap();
    assert_eq!(segment.entries.len(), 1);
    assert_eq!(segment.header.first_sequence, 1);
}

#[test]
fn test_stor_seg_sec_zero_byte_entry_succeeds() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let result = packer
        .add_entry(1, test_block_ref(), vec![], PlacementHint::Journal)
        .unwrap();
    assert!(result.is_none());
    assert_eq!(packer.pending_count(), 1);
    assert_eq!(packer.pending_bytes(), 0);
}

#[test]
fn test_stor_seg_sec_one_byte_entry_succeeds() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let result = packer
        .add_entry(1, test_block_ref(), vec![42], PlacementHint::Journal)
        .unwrap();
    assert!(result.is_none());
    assert_eq!(packer.pending_count(), 1);
    assert_eq!(packer.pending_bytes(), 1);
}

#[test]
fn test_stor_seg_sec_small_target_frequent_seals() {
    let config = SegmentPackerConfig {
        target_size: 100,
        checksum_algorithm: ChecksumAlgorithm::Crc32c,
    };
    let packer = SegmentPacker::new(config);
    let mut sealed_count = 0;
    for i in 0..10 {
        let result = packer
            .add_entry(
                i,
                test_block_ref(),
                vec![i as u8; 60],
                PlacementHint::Journal,
            )
            .unwrap();
        if result.is_some() {
            sealed_count += 1;
        }
    }
    assert!(sealed_count >= 4);
}

#[test]
fn test_stor_seg_sec_zero_target_seals_every_entry() {
    let config = SegmentPackerConfig {
        target_size: 0,
        checksum_algorithm: ChecksumAlgorithm::Crc32c,
    };
    let packer = SegmentPacker::new(config);
    let result1 = packer
        .add_entry(1, test_block_ref(), vec![1u8; 10], PlacementHint::Journal)
        .unwrap();
    assert!(result1.is_none());
    let result2 = packer
        .add_entry(2, test_block_ref(), vec![2u8; 10], PlacementHint::Journal)
        .unwrap();
    assert!(result2.is_some());
}

#[test]
fn test_stor_seg_sec_seal_empty_returns_none() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let result = packer.seal().unwrap();
    assert!(result.is_none());
}

#[test]
fn test_stor_seg_sec_seal_one_entry_valid() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(42, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.entries.len(), 1);
    assert_eq!(segment.header.entry_count, 1);
    assert_eq!(segment.header.first_sequence, 42);
    assert_eq!(segment.header.last_sequence, 42);
}

#[test]
fn test_stor_seg_sec_seal_preserves_entry_order() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    for seq in [10, 20, 30, 40].iter() {
        packer
            .add_entry(
                *seq,
                test_block_ref(),
                vec![*seq as u8; 50],
                PlacementHint::Journal,
            )
            .unwrap();
    }
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.header.first_sequence, 10);
    assert_eq!(segment.header.last_sequence, 40);
    assert!(segment.header.first_sequence < segment.header.last_sequence);
    for (i, expected_seq) in [10u64, 20, 30, 40].iter().enumerate() {
        assert_eq!(segment.entries[i].sequence, *expected_seq);
    }
}

#[test]
fn test_stor_seg_sec_multiple_seals_incrementing_ids() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let mut prev_id = 0u64;
    for i in 1..=5 {
        packer
            .add_entry(
                i,
                test_block_ref(),
                vec![i as u8; 100],
                PlacementHint::Journal,
            )
            .unwrap();
        let segment = packer.seal().unwrap().unwrap();
        assert!(segment.header.segment_id > prev_id);
        prev_id = segment.header.segment_id;
    }
}

#[test]
fn test_stor_seg_sec_data_buffer_matches_entries() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let data1 = vec![0xAA; 100];
    let data2 = vec![0xBB; 200];
    let data3 = vec![0xCC; 300];
    packer
        .add_entry(1, test_block_ref(), data1.clone(), PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(2, test_block_ref(), data2.clone(), PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(3, test_block_ref(), data3.clone(), PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.data.len(), 600);
    assert!(segment.data[..100].iter().all(|&x| x == 0xAA));
    assert!(segment.data[100..300].iter().all(|&x| x == 0xBB));
    assert!(segment.data[300..600].iter().all(|&x| x == 0xCC));
}

#[test]
fn test_stor_seg_sec_first_entry_offset_zero() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![1u8; 500], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.entries[0].data_offset, 0);
}

#[test]
fn test_stor_seg_sec_second_entry_offset_correct() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![1u8; 100], PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(2, test_block_ref(), vec![2u8; 200], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.entries[0].data_offset, 0);
    assert_eq!(segment.entries[0].data_len, 100);
    assert_eq!(segment.entries[1].data_offset, 100);
}

#[test]
fn test_stor_seg_sec_three_entries_contiguous_offsets() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![1u8; 100], PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(2, test_block_ref(), vec![2u8; 200], PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(3, test_block_ref(), vec![3u8; 300], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.entries[0].data_offset, 0);
    assert_eq!(segment.entries[1].data_offset, 100);
    assert_eq!(segment.entries[2].data_offset, 300);
    assert_eq!(segment.entries[0].data_len, 100);
    assert_eq!(segment.entries[1].data_len, 200);
    assert_eq!(segment.entries[2].data_len, 300);
}

#[test]
fn test_stor_seg_sec_data_len_matches_actual_size() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let sizes = [50usize, 123, 456, 789];
    for (i, &size) in sizes.iter().enumerate() {
        packer
            .add_entry(
                i as u64,
                test_block_ref(),
                vec![i as u8; size],
                PlacementHint::Journal,
            )
            .unwrap();
    }
    let segment = packer.seal().unwrap().unwrap();
    for (i, &size) in sizes.iter().enumerate() {
        assert_eq!(segment.entries[i].data_len as usize, size);
    }
}

#[test]
fn test_stor_seg_sec_checksum_verifies_correctly() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let data = b"test data for checksum".to_vec();
    packer
        .add_entry(1, test_block_ref(), data.clone(), PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert!(verify(&segment.header.checksum, &segment.data));
}

#[test]
fn test_stor_seg_sec_checksum_changes_with_data() {
    let config = SegmentPackerConfig {
        target_size: SEGMENT_SIZE,
        checksum_algorithm: ChecksumAlgorithm::Crc32c,
    };
    let packer1 = SegmentPacker::new(config.clone());
    let packer2 = SegmentPacker::new(config);
    packer1
        .add_entry(1, test_block_ref(), vec![1u8; 100], PlacementHint::Journal)
        .unwrap();
    packer2
        .add_entry(1, test_block_ref(), vec![2u8; 100], PlacementHint::Journal)
        .unwrap();
    let seg1 = packer1.seal().unwrap().unwrap();
    let seg2 = packer2.seal().unwrap().unwrap();
    assert_ne!(seg1.header.checksum.value, seg2.header.checksum.value);
}

#[test]
fn test_stor_seg_sec_crc32c_is_default_algorithm() {
    let config = SegmentPackerConfig::default();
    assert_eq!(config.checksum_algorithm, ChecksumAlgorithm::Crc32c);
    let packer = SegmentPacker::new(config);
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    let segment = packer.seal().unwrap().unwrap();
    assert_eq!(segment.header.checksum.algorithm, ChecksumAlgorithm::Crc32c);
}

#[test]
fn test_stor_seg_sec_stats_initially_zero() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    let stats = packer.stats();
    assert_eq!(stats.segments_sealed, 0);
    assert_eq!(stats.entries_packed, 0);
    assert_eq!(stats.bytes_packed, 0);
    assert_eq!(stats.pending_entries, 0);
    assert_eq!(stats.pending_bytes, 0);
}

#[test]
fn test_stor_seg_sec_stats_segments_sealed_increments() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.segments_sealed, 1);
    packer
        .add_entry(2, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.segments_sealed, 2);
    packer
        .add_entry(3, test_block_ref(), vec![0u8; 100], PlacementHint::Journal)
        .unwrap();
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.segments_sealed, 3);
}

#[test]
fn test_stor_seg_sec_stats_entries_packed_accumulates() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    for i in 1..=5 {
        packer
            .add_entry(
                i,
                test_block_ref(),
                vec![i as u8; 50],
                PlacementHint::Journal,
            )
            .unwrap();
    }
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.entries_packed, 5);
    for i in 6..=8 {
        packer
            .add_entry(
                i,
                test_block_ref(),
                vec![i as u8; 50],
                PlacementHint::Journal,
            )
            .unwrap();
    }
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.entries_packed, 8);
}

#[test]
fn test_stor_seg_sec_stats_bytes_packed_accumulates() {
    let packer = SegmentPacker::new(SegmentPackerConfig::default());
    packer
        .add_entry(1, test_block_ref(), vec![0u8; 1000], PlacementHint::Journal)
        .unwrap();
    packer
        .add_entry(2, test_block_ref(), vec![0u8; 2000], PlacementHint::Journal)
        .unwrap();
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.bytes_packed, 3000);
    packer
        .add_entry(3, test_block_ref(), vec![0u8; 500], PlacementHint::Journal)
        .unwrap();
    packer.seal().unwrap();
    let stats = packer.stats();
    assert_eq!(stats.bytes_packed, 3500);
}
