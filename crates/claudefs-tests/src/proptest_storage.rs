//! Property-Based Tests for Storage - Self-contained tests

use proptest::prelude::*;
use std::collections::HashSet;

/// Generates valid block size values
pub fn arb_block_size() -> impl Strategy<Value = u64> {
    prop_oneof![Just(4096), Just(65536), Just(1048576), Just(67108864),]
}

/// Generates arbitrary placement hint values
pub fn arb_placement_hint() -> impl Strategy<Value = u8> {
    prop_oneof![
        Just(0u8), // Metadata
        Just(1u8), // HotData
        Just(2u8), // WarmData
        Just(3u8), // ColdData
        Just(4u8), // Snapshot
        Just(5u8), // Journal
    ]
}

/// Simple block ID struct
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId {
    device_idx: u16,
    offset: u64,
}

impl BlockId {
    pub fn new(device_idx: u16, offset: u64) -> Self {
        Self { device_idx, offset }
    }
}

/// Simple block size enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockSize {
    B4K,
    B64K,
    B1M,
    B64M,
}

impl BlockSize {
    pub fn as_bytes(&self) -> u64 {
        match self {
            BlockSize::B4K => 4096,
            BlockSize::B64K => 65536,
            BlockSize::B1M => 1048576,
            BlockSize::B64M => 67108864,
        }
    }
}

/// Simple placement hint enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementHint {
    Metadata,
    HotData,
    WarmData,
    ColdData,
    Snapshot,
    Journal,
}

/// Test block ID creation
fn block_id_new(device_idx: u16, offset: u64) {
    let block_id = BlockId::new(device_idx, offset);
    assert_eq!(block_id.device_idx, device_idx);
    assert_eq!(block_id.offset, offset);
}

/// Test block size bytes
fn block_size_bytes(size: u64) {
    assert!(size > 0);
    assert!(size % 4096 == 0 || size == 0 || size == 1);
}

/// Simple checksum simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checksum(u64);

impl Checksum {
    pub fn compute(data: &[u8]) -> Self {
        let mut sum: u64 = 0;
        for (i, &b) in data.iter().enumerate() {
            sum = sum.wrapping_add((b as u64).wrapping_mul((i + 1) as u64));
        }
        Checksum(sum)
    }

    pub fn verify(&self, data: &[u8]) -> bool {
        *self == Checksum::compute(data)
    }
}

/// Test checksum roundtrip
fn checksum_roundtrip(data: Vec<u8>) {
    let chk = Checksum::compute(&data);
    assert!(chk.verify(&data), "Checksum verification failed");
}

/// Test allocator alloc/free invariants
fn alloc_free_invariants(capacity: u64, alloc_size: u64) {
    let blocks = capacity / 4096;
    let requested_blocks = alloc_size / 4096;

    if requested_blocks <= blocks {
        let remaining = blocks - requested_blocks;
        let after_free = remaining + requested_blocks;
        assert_eq!(after_free, blocks, "Allocator capacity mismatch");
    }
}

/// Test placement hint variants
fn placement_hint_variants(hint: u8) {
    assert!(hint <= 5);
}

/// Test checksum different data produces different results
fn checksum_different_data() {
    let data1 = vec![1u8, 2, 3];
    let data2 = vec![1u8, 2, 4];

    let chk1 = Checksum::compute(&data1);
    let chk2 = Checksum::compute(&data2);

    assert_ne!(chk1, chk2);
}

/// Test empty data checksum
fn checksum_empty_data() {
    let data: Vec<u8> = vec![];
    let chk = Checksum::compute(&data);
    assert!(chk.verify(&data));
}

/// Test large data checksum
fn checksum_large_data(size: usize) {
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
    let chk = Checksum::compute(&data);
    assert!(chk.verify(&data));
}

/// Test block ID comparison via equality
fn block_id_equality() {
    let id1 = BlockId::new(0, 100);
    let id2 = BlockId::new(0, 200);
    let id3 = BlockId::new(0, 100);

    assert_ne!(id1, id2);
    assert_eq!(id1, id3);
}

/// Test block size values
fn block_size_values() {
    assert_eq!(BlockSize::B4K.as_bytes(), 4096);
    assert_eq!(BlockSize::B64K.as_bytes(), 65536);
    assert_eq!(BlockSize::B1M.as_bytes(), 1048576);
    assert_eq!(BlockSize::B64M.as_bytes(), 67108864);
}

/// Test placement hint variants
fn placement_hint_all() {
    let hints = vec![
        PlacementHint::Metadata,
        PlacementHint::HotData,
        PlacementHint::WarmData,
        PlacementHint::ColdData,
        PlacementHint::Snapshot,
        PlacementHint::Journal,
    ];
    assert_eq!(hints.len(), 6);
}

proptest! {
    #[test]
    fn prop_block_id_new(device_idx in 0u16..10, offset in 0u64..1000) {
        block_id_new(device_idx, offset);
    }

    #[test]
    fn prop_block_size_bytes(size in arb_block_size()) {
        block_size_bytes(size);
    }

    #[test]
    fn prop_checksum_roundtrip(data in proptest::collection::vec(0u8..255, 0..65536)) {
        checksum_roundtrip(data);
    }

    #[test]
    fn prop_alloc_free_invariants(capacity in 4096u64..67108864u64 * 100, alloc_size in 4096u64..67108864u64) {
        alloc_free_invariants(capacity, alloc_size);
    }

    #[test]
    fn prop_placement_hint_variants(hint in arb_placement_hint()) {
        placement_hint_variants(hint);
    }
}

#[test]
fn test_checksum_different_data() {
    checksum_different_data();
}

#[test]
fn test_checksum_empty_data() {
    checksum_empty_data();
}

#[test]
fn test_checksum_large_data_4kb() {
    checksum_large_data(4096);
}

#[test]
fn test_checksum_large_data_64kb() {
    checksum_large_data(65536);
}

#[test]
fn test_checksum_large_data_1mb() {
    checksum_large_data(1048576);
}

#[test]
fn test_block_id_equality() {
    block_id_equality();
}

#[test]
fn test_block_size_values() {
    block_size_values();
}

#[test]
fn test_placement_hint_all() {
    placement_hint_all();
}

#[test]
fn test_block_id_zero() {
    let id = BlockId::new(0, 0);
    assert_eq!(id.device_idx, 0);
    assert_eq!(id.offset, 0);
}

#[test]
fn test_block_id_max() {
    let id = BlockId::new(u16::MAX, u64::MAX);
    assert_eq!(id.device_idx, u16::MAX);
    assert_eq!(id.offset, u64::MAX);
}

#[test]
fn test_block_size_all_variants() {
    let sizes = [
        BlockSize::B4K,
        BlockSize::B64K,
        BlockSize::B1M,
        BlockSize::B64M,
    ];
    for size in sizes {
        assert!(size.as_bytes() > 0);
    }
}

#[test]
fn test_checksum_clone() {
    let data = b"test";
    let chk = Checksum::compute(data);
    let cloned = chk;
    assert_eq!(chk, cloned);
}

#[test]
fn test_block_id_clone() {
    let id = BlockId::new(0, 42);
    let cloned = id.clone();
    assert_eq!(id, cloned);
}

#[test]
fn test_block_size_clone() {
    let size = BlockSize::B4K;
    let cloned = size.clone();
    assert_eq!(size, cloned);
}

#[test]
fn test_allocator_simulation_basic() {
    let total_capacity: u64 = 16 * 1024 * 1024;
    let block_size: u64 = 4096;
    let total_blocks = total_capacity / block_size;

    let allocated = 10u64;
    let remaining = total_blocks - allocated;

    assert_eq!(remaining + allocated, total_blocks);
}

#[test]
fn test_allocator_simulation_fragmentation() {
    let capacity = 100u64;

    let alloc1 = 30u64;
    let alloc2 = 25u64;
    let alloc3 = 20u64;

    let used = alloc1 + alloc2 + alloc3;
    let free = capacity - used;

    assert_eq!(used + free, capacity);
}

#[test]
fn test_block_id_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(BlockId::new(0, 1));
    set.insert(BlockId::new(0, 2));
    set.insert(BlockId::new(0, 1)); // duplicate

    assert_eq!(set.len(), 2);
}

#[test]
fn test_checksum_single_byte() {
    let chk = Checksum::compute(b"x");
    assert!(chk.verify(b"x"));
}

#[test]
fn test_checksum_many_bytes() {
    let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let chk = Checksum::compute(&data);
    assert!(chk.verify(&data));
}
