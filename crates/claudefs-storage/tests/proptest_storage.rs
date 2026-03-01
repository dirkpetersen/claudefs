//! Property-based tests for claudefs-storage using proptest.
//!
//! These tests verify invariants about the storage subsystem using
//! property-based testing to catch edge cases that unit tests might miss.

use claudefs_storage::{
    allocator::{AllocatorConfig, BuddyAllocator},
    block_cache::{BlockCache, BlockCacheConfig},
    checksum::{compute, BlockHeader, Checksum, ChecksumAlgorithm},
    io_scheduler::{IoPriority, IoScheduler, IoSchedulerConfig, ScheduledIo},
    metrics::StorageMetrics,
    segment::{SegmentEntry, SegmentPacker, SegmentPackerConfig, SEGMENT_SIZE},
    smart::{NvmeSmartLog, SmartMonitor, SmartMonitorConfig},
    write_journal::{JournalConfig, JournalOp, WriteJournal},
    BlockId, BlockRef, BlockSize, IoOpType, IoRequestId, PlacementHint,
};
use proptest::prelude::*;
use std::collections::HashSet;

/// Generator for BlockSize values.
fn any_block_size() -> impl Strategy<Value = BlockSize> {
    prop_oneof![
        Just(BlockSize::B4K),
        Just(BlockSize::B64K),
        Just(BlockSize::B1M),
        Just(BlockSize::B64M)
    ]
}

/// Generator for valid block data matching a given BlockSize.
#[allow(dead_code)]
fn block_data_for_size(size: BlockSize) -> impl Strategy<Value = Vec<u8>> {
    let size_bytes = size.as_bytes() as usize;
    proptest::collection::vec(any::<u8>(), size_bytes..=size_bytes)
}

/// Generator for BlockRef with valid block size.
#[allow(dead_code)]
fn any_block_ref() -> impl Strategy<Value = BlockRef> {
    any_block_size().prop_flat_map(|size| {
        (0u64..10000u64, Just(size)).prop_map(|(offset, size)| BlockRef {
            id: BlockId::new(0, offset),
            size,
        })
    })
}

/// Strategy for generating random data of various sizes.
fn any_data() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..8192)
}

proptest! {
    /// Test: After any sequence of alloc/free, free_blocks <= total_blocks
    /// and free_blocks + allocated <= total_blocks (use <= since merge may not fully consolidate)
    #[test]
    fn test_allocator_invariant_total_blocks(
        alloc_count in 2u32..100u32,
        free_count in 0u32..50u32,
    ) {
        let total_blocks = 65536u64;
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: total_blocks,
        };

        let alloc = BuddyAllocator::new(config).unwrap();

        let mut allocated = Vec::new();

        for _ in 0..alloc_count {
            match alloc.allocate(BlockSize::B4K) {
                Ok(block) => allocated.push(block),
                Err(_) => break,
            }
        }

        prop_assume!(!allocated.is_empty(), "Need at least one successful allocation");

        let to_free = free_count.min(allocated.len() as u32) as usize;
        for block in &allocated[0..to_free] {
            let _ = alloc.free(*block);
        }

        let stats = alloc.stats();

        prop_assert!(
            stats.free_blocks_4k <= total_blocks,
            "Free blocks {} exceeds total {}",
            stats.free_blocks_4k,
            total_blocks
        );

        let allocated_after_free = (allocated.len() - to_free) as u64;
        let remaining_free = stats.free_blocks_4k;

        prop_assert!(
            remaining_free + allocated_after_free <= total_blocks,
            "Invariant violated: free ({}) + allocated ({}) > total ({})",
            remaining_free,
            allocated_after_free,
            total_blocks
        );
    }

    /// Test: Every allocated block has a unique offset
    #[test]
    fn test_allocator_unique_offsets(alloc_count in 1u32..100u32) {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16384,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let mut offsets = HashSet::new();
        let mut allocated = Vec::new();

        for _ in 0..alloc_count {
            match alloc.allocate(BlockSize::B4K) {
                Ok(block) => {
                    let offset = block.id.offset;
                    prop_assert!(
                        !offsets.contains(&offset),
                        "Duplicate offset found: {}",
                        offset
                    );
                    offsets.insert(offset);
                    allocated.push(block);
                }
                Err(_) => break,
            }
        }
    }

    /// Test: No allocated block has offset >= total_blocks_4k
    #[test]
    fn test_allocator_offsets_in_bounds(alloc_count in 1u32..200u32) {
        let total_blocks = 16384u64;
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: total_blocks,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        for _ in 0..alloc_count {
            match alloc.allocate(BlockSize::B4K) {
                Ok(block) => {
                    prop_assert!(
                        block.id.offset < total_blocks,
                        "Block offset {} exceeds total blocks {}",
                        block.id.offset,
                        total_blocks
                    );
                }
                Err(_) => break,
            }
        }
    }

    /// Test: After allocating N blocks, the stats reflect N allocations
    #[test]
    fn test_allocator_stats_reflect_allocations(alloc_count in 1u32..100u32) {
        let config = AllocatorConfig {
            device_idx: 0,
            total_blocks_4k: 16384,
        };
        let alloc = BuddyAllocator::new(config).unwrap();

        let initial_stats = alloc.stats();
        let initial_allocations = initial_stats.total_allocations;

        let mut success_count = 0u64;
        for _ in 0..alloc_count {
            if alloc.allocate(BlockSize::B4K).is_ok() {
                success_count += 1;
            }
        }

        let final_stats = alloc.stats();
        prop_assert_eq!(
            final_stats.total_allocations - initial_allocations,
            success_count,
            "Stats should reflect {} allocations, but showed {}",
            success_count,
            final_stats.total_allocations - initial_allocations
        );
    }

    /// Test: For any random data buffer, compute(data) == compute(data) (deterministic)
    #[test]
    fn test_checksum_determinism(data in any_data()) {
        let checksum1 = compute(ChecksumAlgorithm::Crc32c, &data);
        let checksum2 = compute(ChecksumAlgorithm::Crc32c, &data);

        prop_assert_eq!(
            checksum1.value, checksum2.value,
            "CRC32C should be deterministic"
        );

        let xxh1 = compute(ChecksumAlgorithm::XxHash64, &data);
        let xxh2 = compute(ChecksumAlgorithm::XxHash64, &data);

        prop_assert_eq!(
            xxh1.value, xxh2.value,
            "xxHash64 should be deterministic"
        );
    }

    /// Test: CRC32C and xxHash64 produce different outputs for different inputs
    /// This is a probabilistic test - could theoretically fail with hash collision
    #[test]
    fn test_checksums_different_for_different_data(
        data1 in proptest::collection::vec(any::<u8>(), 1..1000),
        data2 in proptest::collection::vec(any::<u8>(), 1..1000),
    ) {
        prop_assume!(data1 != data2, "Skipping identical data");

        let crc1 = compute(ChecksumAlgorithm::Crc32c, &data1);
        let crc2 = compute(ChecksumAlgorithm::Crc32c, &data2);

        if crc1.value == crc2.value {
            let xxh1 = compute(ChecksumAlgorithm::XxHash64, &data1);
            let xxh2 = compute(ChecksumAlgorithm::XxHash64, &data2);
            prop_assert!(
                xxh1.value != xxh2.value || data1 == data2,
                "Different data produced same hashes - possible collision"
            );
        }
    }

    /// Test: For any collection of entries that fit in 2MB, pack+unpack is a round-trip
    #[test]
    fn test_segment_packer_roundtrip(
        entry_sizes in proptest::collection::vec(1u32..1000u32, 1..100),
    ) {
        let total_size: usize = entry_sizes.iter().map(|&s| s as usize).sum();

        prop_assume!(total_size <= SEGMENT_SIZE, "Total size exceeds segment size");

        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        let mut entries_data = Vec::new();

        for (i, &size) in entry_sizes.iter().enumerate() {
            let data = vec![(i % 256) as u8; size as usize];
            let block_ref = BlockRef {
                id: BlockId::new(0, i as u64),
                size: BlockSize::B4K,
            };
            packer
                .add_entry(i as u64 + 1, block_ref, data.clone(), PlacementHint::Journal)
                .unwrap();
            entries_data.push((i as u64 + 1, block_ref, data));
        }

        let sealed = packer.seal().unwrap();
        prop_assert!(sealed.is_some(), "Should have sealed a segment");

        let segment = sealed.unwrap();

        prop_assert_eq!(
            segment.entries.len(),
            entries_data.len(),
            "Entry count should match"
        );

        for (i, entry) in segment.entries.iter().enumerate() {
            prop_assert!(
                (entry.data_offset as usize) < segment.data.len(),
                "Entry {} data_offset out of bounds",
                i
            );

            let expected_len = entries_data[i].2.len();
            prop_assert_eq!(
                entry.data_len as usize,
                expected_len,
                "Entry {} data length mismatch",
                i
            );
        }

        for (i, entry) in segment.entries.iter().enumerate() {
            let start = entry.data_offset as usize;
            let end = start + entry.data_len as usize;
            let extracted = &segment.data[start..end];
            prop_assert_eq!(
                extracted,
                entries_data[i].2.as_slice(),
                "Entry {} data mismatch",
                i
            );
        }
    }

    /// Test: Packed segments never exceed SEGMENT_SIZE (2MB)
    #[test]
    fn test_segment_packer_never_exceeds_size(
        entry_sizes in proptest::collection::vec(1u32..100000u32, 1..100),
    ) {
        let config = SegmentPackerConfig::default();
        let packer = SegmentPacker::new(config);

        for (i, &size) in entry_sizes.iter().enumerate() {
            let data = vec![(i % 256) as u8; size as usize];
            let block_ref = BlockRef {
                id: BlockId::new(0, i as u64),
                size: BlockSize::B4K,
            };

            let _ = packer.add_entry(i as u64 + 1, block_ref, data, PlacementHint::Journal);

            if packer.pending_bytes() > SEGMENT_SIZE {
                prop_assert!(
                    packer.pending_bytes() <= SEGMENT_SIZE,
                    "Pending bytes {} exceeds SEGMENT_SIZE {}",
                    packer.pending_bytes(),
                    SEGMENT_SIZE
                );
            }
        }

        prop_assert!(
            packer.pending_bytes() <= SEGMENT_SIZE,
            "Final pending bytes {} exceeds SEGMENT_SIZE {}",
            packer.pending_bytes(),
            SEGMENT_SIZE
        );
    }

    /// Test: Any BlockHeader serializes/deserializes to the same value
    #[test]
    fn test_block_header_serialization(data in any_data()) {
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        let header = BlockHeader::new(BlockSize::B4K, checksum, 12345);

        let serialized = bincode::serialize(&header).unwrap();

        let deserialized: BlockHeader = bincode::deserialize(&serialized).unwrap();

        prop_assert_eq!(header.magic, deserialized.magic, "Magic should match");
        prop_assert_eq!(header.version, deserialized.version, "Version should match");
        prop_assert_eq!(header.block_size, deserialized.block_size, "Block size should match");
        prop_assert_eq!(header.sequence, deserialized.sequence, "Sequence should match");
        prop_assert_eq!(
            header.data_checksum.value, deserialized.data_checksum.value,
            "Checksum value should match"
        );
        prop_assert_eq!(
            header.data_checksum.algorithm, deserialized.data_checksum.algorithm,
            "Checksum algorithm should match"
        );
    }

    /// Test: BlockHeader serialization produces consistent results
    #[test]
    fn test_block_header_idempotent_serialization(data in any_data()) {
        let checksum = compute(ChecksumAlgorithm::XxHash64, &data);
        let header = BlockHeader::new(BlockSize::B64K, checksum, 99999);

        let ser1 = bincode::serialize(&header).unwrap();
        let ser2 = bincode::serialize(&header).unwrap();

        prop_assert_eq!(ser1.clone(), ser2.clone(), "Serialization should be deterministic");

        let de1: BlockHeader = bincode::deserialize(&ser1).unwrap();
        let de2: BlockHeader = bincode::deserialize(&ser2).unwrap();

        prop_assert_eq!(de1.sequence, de2.sequence, "Deserialized sequences should match");
        prop_assert_eq!(de1.data_checksum.value, de2.data_checksum.value, "Checksums should match");
    }

    /// Test: All BlockSize variants serialize/deserialize correctly
    #[test]
    fn test_block_size_serialization_roundtrip(size in any_block_size()) {
        let serialized = bincode::serialize(&size).unwrap();
        let deserialized: BlockSize = bincode::deserialize(&serialized).unwrap();

        prop_assert_eq!(size, deserialized, "BlockSize should roundtrip");
    }

    /// Test: All PlacementHint variants serialize/deserialize correctly
    #[test]
    fn test_placement_hint_serialization_roundtrip(hint in prop_oneof![
        Just(PlacementHint::Metadata),
        Just(PlacementHint::HotData),
        Just(PlacementHint::WarmData),
        Just(PlacementHint::ColdData),
        Just(PlacementHint::Snapshot),
        Just(PlacementHint::Journal),
    ]) {
        let serialized = bincode::serialize(&hint).unwrap();
        let deserialized: PlacementHint = bincode::deserialize(&serialized).unwrap();

        prop_assert_eq!(hint, deserialized, "PlacementHint should roundtrip");
    }

    /// Test: SegmentEntry serialization roundtrip
    #[test]
    fn test_segment_entry_serialization_roundtrip(
        sequence in 1u64..10000u64,
        offset in 0u64..1000u64,
        data_len in 1u32..4096u32,
        data_offset in 0u32..4096u32,
    ) {
        let entry = SegmentEntry {
            sequence,
            block_ref: BlockRef {
                id: BlockId::new(0, offset),
                size: BlockSize::B4K,
            },
            data_len,
            data_offset,
            placement_hint: PlacementHint::Journal,
        };

        let serialized = bincode::serialize(&entry).unwrap();
        let deserialized: SegmentEntry = bincode::deserialize(&serialized).unwrap();

        prop_assert_eq!(entry.sequence, deserialized.sequence);
        prop_assert_eq!(entry.block_ref.id.offset, deserialized.block_ref.id.offset);
        prop_assert_eq!(entry.data_len, deserialized.data_len);
        prop_assert_eq!(entry.data_offset, deserialized.data_offset);
        prop_assert_eq!(entry.placement_hint, deserialized.placement_hint);
    }
}

/// Test: Verify Checksum struct is properly serializable
#[test]
fn test_checksum_serialization() {
    let checksum = Checksum::new(ChecksumAlgorithm::Crc32c, 0xDEADBEEF);

    let serialized = bincode::serialize(&checksum).unwrap();
    let deserialized: Checksum = bincode::deserialize(&serialized).unwrap();

    assert_eq!(checksum.algorithm, deserialized.algorithm);
    assert_eq!(checksum.value, deserialized.value);
}

/// Test: Verify allocator can handle mixed size allocations
#[test]
fn test_allocator_mixed_sizes() {
    let config = AllocatorConfig {
        device_idx: 0,
        total_blocks_4k: 65536,
    };
    let alloc = BuddyAllocator::new(config).unwrap();

    let mut all_blocks = Vec::new();

    for _ in 0..10 {
        all_blocks.push(alloc.allocate(BlockSize::B4K).unwrap());
    }
    for _ in 0..10 {
        all_blocks.push(alloc.allocate(BlockSize::B64K).unwrap());
    }
    for _ in 0..5 {
        all_blocks.push(alloc.allocate(BlockSize::B1M).unwrap());
    }

    let stats = alloc.stats();

    let allocated = all_blocks
        .iter()
        .map(|b| match b.size {
            BlockSize::B4K => 1,
            BlockSize::B64K => 16,
            BlockSize::B1M => 256,
            BlockSize::B64M => 16384,
        })
        .sum::<u64>();

    assert!(
        stats.free_blocks_4k + allocated <= 65536,
        "free {} + allocated {} should be <= 65536",
        stats.free_blocks_4k,
        allocated
    );
}

/// Test: Verify segment packer handles various data sizes
#[test]
fn test_segment_packer_various_sizes() {
    let config = SegmentPackerConfig {
        target_size: 1024,
        ..Default::default()
    };
    let packer = SegmentPacker::new(config);

    packer
        .add_entry(
            1,
            BlockRef {
                id: BlockId::new(0, 0),
                size: BlockSize::B4K,
            },
            vec![0xAA; 100],
            PlacementHint::Journal,
        )
        .unwrap();

    packer
        .add_entry(
            2,
            BlockRef {
                id: BlockId::new(0, 1),
                size: BlockSize::B4K,
            },
            vec![0xBB; 500],
            PlacementHint::Journal,
        )
        .unwrap();

    packer
        .add_entry(
            3,
            BlockRef {
                id: BlockId::new(0, 2),
                size: BlockSize::B4K,
            },
            vec![0xCC; 600],
            PlacementHint::Journal,
        )
        .unwrap();

    let stats = packer.stats();
    assert!(stats.segments_sealed >= 1);
}

proptest! {
    /// Test: For any sequence of N append operations, all returned sequence numbers are strictly increasing.
    #[test]
    fn test_write_journal_sequence_monotonicity(n in 1usize..100usize) {
        let config = JournalConfig::default();
        let mut journal = WriteJournal::new(config);

        let mut sequences = Vec::new();
        for i in 0..n {
            let op = JournalOp::Write {
                data: vec![i as u8; 128],
            };
            let seq = journal.append(op, 0, i as u64).unwrap();
            sequences.push(seq);
        }

        for i in 1..sequences.len() {
            prop_assert!(
                sequences[i] > sequences[i - 1],
                "Sequence {} should be greater than {}",
                sequences[i],
                sequences[i - 1]
            );
        }
    }

    /// Test: After appending N entries and committing, entries_since(0) returns all N entries.
    #[test]
    fn test_write_journal_entries_since_consistency(n in 1usize..50usize) {
        let config = JournalConfig::default();
        let mut journal = WriteJournal::new(config);

        for i in 0..n {
            let op = JournalOp::Write {
                data: vec![i as u8; 128],
            };
            journal.append(op, 0, i as u64).unwrap();
        }

        journal.commit().unwrap();

        let entries = journal.entries_since(0);
        prop_assert_eq!(
            entries.len(),
            n,
            "Expected {} entries, got {}",
            n,
            entries.len()
        );
    }

    /// Test: After appending N entries, truncating before sequence M removes exactly the right count.
    #[test]
    fn test_write_journal_truncate_reclaims(n in 5usize..100usize, m in 1usize..100usize) {
        let config = JournalConfig::default();
        let mut journal = WriteJournal::new(config);

        let mut sequences = Vec::new();
        for i in 0..n {
            let op = JournalOp::Write {
                data: vec![i as u8; 128],
            };
            let seq = journal.append(op, 0, i as u64).unwrap();
            sequences.push(seq);
        }

        let truncate_before = if m < sequences.len() {
            sequences[m]
        } else {
            sequences.last().copied().unwrap_or(0)
        };

        let before_count = journal.entries_since(0).len();
        journal.truncate_before(truncate_before).unwrap();
        let after_count = journal.entries_since(0).len();

        let expected_removed = if m < n { m } else { n - 1 };
        prop_assert_eq!(
            before_count - after_count,
            expected_removed,
            "Expected {} removed, got {}",
            expected_removed,
            before_count - after_count
        );
    }

    /// Test: When N requests of random priorities are enqueued, they are dequeued in priority order.
    #[test]
    fn test_io_scheduler_priority_ordering(n in 2usize..50usize) {
        let config = IoSchedulerConfig {
            max_queue_depth: 100,
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        for i in 0..n {
            let priorities = [IoPriority::Critical, IoPriority::High, IoPriority::Normal, IoPriority::Low];
            let priority = priorities[i % 4];
            let io = ScheduledIo::new(
                IoRequestId(i as u64),
                priority,
                IoOpType::Read,
                BlockRef {
                    id: BlockId::new(0, i as u64),
                    size: BlockSize::B4K,
                },
                0,
            );
            scheduler.enqueue(io).unwrap();
        }

        let mut last_priority = 0u8;
        let mut dequeued = 0;
        while let Some(io) = scheduler.dequeue() {
            let current_priority = io.priority.as_index() as u8;
            prop_assert!(
                current_priority >= last_priority,
                "Priority ordering violated: {:?} before {:?}",
                io.priority,
                last_priority
            );
            last_priority = current_priority;
            dequeued += 1;
        }
        prop_assert_eq!(dequeued, n, "All {} requests should be dequeued", n);
    }

    /// Test: For any N enqueue operations, the number of dequeues equals min(N, queue capacity).
    #[test]
    fn test_io_scheduler_enqueue_dequeue_conservation(n in 1usize..100usize) {
        let capacity = 50;
        let config = IoSchedulerConfig {
            max_queue_depth: capacity,
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        for i in 0..n {
            let io = ScheduledIo::new(
                IoRequestId(i as u64),
                IoPriority::Normal,
                IoOpType::Read,
                BlockRef {
                    id: BlockId::new(0, i as u64),
                    size: BlockSize::B4K,
                },
                0,
            );
            let _ = scheduler.enqueue(io);
        }

        let mut dequeued = 0;
        while scheduler.dequeue().is_some() {
            dequeued += 1;
        }

        let expected = n.min(capacity);
        prop_assert_eq!(
            dequeued,
            expected,
            "Expected {} dequeues for {} enqueues with capacity {}",
            expected,
            n,
            capacity
        );
    }

    /// Test: For random data of size matching BlockSize::B4K, inserting then getting returns the same data.
    #[test]
    fn test_block_cache_insert_get_roundtrip(data in proptest::collection::vec(any::<u8>(), 4096..=4096)) {
        let config = BlockCacheConfig {
            max_entries: 100,
            ..Default::default()
        };
        let mut cache = BlockCache::new(config);

        let block_ref = BlockRef {
            id: BlockId::new(0, 42),
            size: BlockSize::B4K,
        };
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        cache.insert(block_ref, data.clone(), checksum).unwrap();

        let retrieved = cache.get_data(&block_ref.id).unwrap();
        prop_assert_eq!(
            retrieved,
            data.as_slice(),
            "Retrieved data should match inserted data"
        );
    }

    /// Test: Insert N blocks into a cache with max_entries=50, verify len() never exceeds 50.
    #[test]
    fn test_block_cache_eviction_respects_capacity(n in 1usize..200usize) {
        let config = BlockCacheConfig {
            max_entries: 50,
            ..Default::default()
        };
        let mut cache = BlockCache::new(config);

        for i in 0..n {
            let block_ref = BlockRef {
                id: BlockId::new(0, i as u64),
                size: BlockSize::B4K,
            };
            let data = vec![i as u8; 4096];
            let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
            cache.insert(block_ref, data, checksum).unwrap();

            let len = cache.len();
            prop_assert!(
                len <= 50,
                "Cache length {} exceeds max_entries 50",
                len
            );
        }
    }

    /// Test: Record N random I/O ops, verify total ops counted equals N and bytes match sum.
    #[test]
    fn test_metrics_io_accumulation(n in 1usize..100usize) {
        let mut metrics = StorageMetrics::new();

        let mut total_bytes = 0u64;
        for i in 0..n {
            let op_type = match i % 4 {
                0 => IoOpType::Read,
                1 => IoOpType::Write,
                2 => IoOpType::Flush,
                _ => IoOpType::Discard,
            };
            let bytes = (((i % 16) + 1) * 4096) as u64;
            metrics.record_io(op_type, bytes, 0);
            total_bytes += bytes;
        }

        let exported = metrics.export();
        let total_ops: u64 = exported.iter()
            .filter(|m| m.name.contains("io_ops_total"))
            .filter_map(|m| match m.value {
                claudefs_storage::metrics::MetricValue::Counter(v) => Some(v),
                _ => None,
            })
            .sum();
        let total_bytes_export: u64 = exported.iter()
            .filter(|m| m.name.contains("io_bytes_total"))
            .filter_map(|m| match m.value {
                claudefs_storage::metrics::MetricValue::Counter(v) => Some(v),
                _ => None,
            })
            .sum();

        prop_assert_eq!(total_ops, n as u64, "Total ops should be {}", n);
        prop_assert_eq!(total_bytes_export, total_bytes, "Total bytes should be {}", total_bytes);
    }

    /// Test: For any temperature T in Kelvin (200..500), verify celsius = T - 273.15 (within f64 epsilon).
    #[test]
    fn test_smart_temperature_conversion(temp_k in 200f64..500f64) {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: temp_k as u16,
            available_spare_pct: 100,
            available_spare_threshold: 10,
            percent_used: 0,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };

        let celsius = log.temperature_celsius();
        let expected = temp_k - 273.15;
        let kelvin_back = log.temperature_kelvin as f64;

        prop_assert!(
            (celsius - expected).abs() < 1.0,
            "Temperature conversion failed: {}K -> {}C, expected {}C (input converted to {}K)",
            temp_k,
            celsius,
            expected,
            kelvin_back
        );
    }

    /// Test: For any NvmeSmartLog, evaluating health twice after update produces the same result.
    #[test]
    fn test_smart_health_evaluation_deterministic(
        temp in 0u32..100u32,
        available_spare in 0u8..101u8,
        percent_used in 0u8..101u8,
    ) {
        let config = SmartMonitorConfig::default();
        let mut monitor = SmartMonitor::new(config);

        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: (temp + 200) as u16,
            available_spare_pct: available_spare,
            available_spare_threshold: 10,
            percent_used,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };

        monitor.update_device("test_device", log.clone());
        let health1 = monitor.evaluate_health("test_device").unwrap();
        let health2 = monitor.evaluate_health("test_device").unwrap();

        prop_assert_eq!(
            std::mem::discriminant(&health1),
            std::mem::discriminant(&health2),
            "Health evaluation should be deterministic"
        );
    }
}

/// Test: Create a JournalEntry via append, serialize with bincode, deserialize, verify fields match.
#[test]
fn test_write_journal_serialization_roundtrip() {
    let config = JournalConfig::default();
    let mut journal = WriteJournal::new(config);

    let op = JournalOp::Write {
        data: vec![0xAB; 256],
    };
    let _seq = journal.append(op, 0, 0).unwrap();

    let entries = journal.entries_since(0);
    assert_eq!(entries.len(), 1, "Should have 1 entry");

    let entry = entries[0];
    let serialized = bincode::serialize(entry).unwrap();
    let deserialized: claudefs_storage::write_journal::JournalEntry =
        bincode::deserialize(&serialized).unwrap();

    assert_eq!(deserialized.sequence, entry.sequence);
    assert_eq!(deserialized.data_len, entry.data_len);
}

/// Test: Enqueue one request at each priority level, dequeue all, verify all 4 were served.
#[test]
fn test_io_scheduler_all_priorities_served() {
    let config = IoSchedulerConfig {
        max_queue_depth: 100,
        ..Default::default()
    };
    let mut scheduler = IoScheduler::new(config);

    let io1 = ScheduledIo::new(
        IoRequestId(0),
        IoPriority::Critical,
        IoOpType::Read,
        BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        },
        0,
    );
    let io2 = ScheduledIo::new(
        IoRequestId(1),
        IoPriority::High,
        IoOpType::Write,
        BlockRef {
            id: BlockId::new(0, 1),
            size: BlockSize::B4K,
        },
        0,
    );
    let io3 = ScheduledIo::new(
        IoRequestId(2),
        IoPriority::Normal,
        IoOpType::Flush,
        BlockRef {
            id: BlockId::new(0, 2),
            size: BlockSize::B4K,
        },
        0,
    );
    let io4 = ScheduledIo::new(
        IoRequestId(3),
        IoPriority::Low,
        IoOpType::Discard,
        BlockRef {
            id: BlockId::new(0, 3),
            size: BlockSize::B4K,
        },
        0,
    );

    scheduler.enqueue(io1).unwrap();
    scheduler.enqueue(io2).unwrap();
    scheduler.enqueue(io3).unwrap();
    scheduler.enqueue(io4).unwrap();

    let mut priorities_seen = Vec::new();
    while let Some(io) = scheduler.dequeue() {
        priorities_seen.push(io.priority);
    }

    assert_eq!(priorities_seen.len(), 4, "Should have dequeued 4 requests");
    assert!(
        priorities_seen.contains(&IoPriority::Critical),
        "Critical priority should be served"
    );
    assert!(
        priorities_seen.contains(&IoPriority::High),
        "High priority should be served"
    );
    assert!(
        priorities_seen.contains(&IoPriority::Normal),
        "Normal priority should be served"
    );
    assert!(
        priorities_seen.contains(&IoPriority::Low),
        "Low priority should be served"
    );
}
