# Task: Implement block_verifier.rs for claudefs-storage

## Location
Create file: `crates/claudefs-storage/src/block_verifier.rs`

## Purpose
End-to-end block integrity verifier. Given block data + stored checksum, verifies integrity and reports corruption. Used by scrub engine.

## Conventions
- thiserror for errors, serde Serialize+Deserialize, tracing for logging
- Full doc comments (///), no #[allow(dead_code)]
- 22+ tests in #[cfg(test)] mod tests

## Available in this crate
```rust
use crate::block::{BlockRef, BlockSize, BlockId, PlacementHint};
use crate::error::{StorageError, StorageResult};
```

## Implementation

### Types

```rust
use serde::{Deserialize, Serialize};

/// Checksum algorithm for block verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifierAlgorithm {
    Crc32c,
    Blake3,
}

/// Configuration for the block verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// Checksum algorithm to use.
    pub algorithm: VerifierAlgorithm,
    /// Stop processing on first corruption.
    pub fail_fast: bool,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self { algorithm: VerifierAlgorithm::Crc32c, fail_fast: false }
    }
}

/// A block's data and stored checksum for verification.
#[derive(Debug, Clone)]
pub struct BlockToVerify {
    pub block_ref: BlockRef,
    pub data: Vec<u8>,
    pub stored_checksum: u32,
}

/// Result of verifying a single block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub block_ref: BlockRef,
    pub passed: bool,
    pub computed_checksum: u32,
    pub stored_checksum: u32,
    pub data_len: usize,
}

/// Aggregate verifier statistics.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VerifierStats {
    pub blocks_checked: u64,
    pub blocks_passed: u64,
    pub blocks_failed: u64,
    pub total_bytes_verified: u64,
    pub corruptions_found: u64,
}

/// Block integrity verifier.
#[derive(Debug)]
pub struct BlockVerifier {
    config: VerifierConfig,
    stats: VerifierStats,
}
```

### CRC32c implementation (table-based, no external crate)
```rust
fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[index];
    }
    crc ^ 0xFFFF_FFFF
}

static CRC32C_TABLE: [u32; 256] = generate_crc32c_table();

const fn generate_crc32c_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0usize;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}
```

### Blake3 implementation (simple, no external crate)
For Blake3, use a simple FNV-1a 64-bit hash (acceptable substitute):
```rust
fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}
fn blake3_checksum(data: &[u8]) -> u32 {
    let h = fnv1a_64(data);
    (h ^ (h >> 32)) as u32
}
```

### Methods on BlockVerifier
```rust
impl BlockVerifier {
    pub fn new(config: VerifierConfig) -> Self
    pub fn compute_checksum(&self, data: &[u8]) -> u32  // uses config.algorithm
    pub fn verify_block(&mut self, block: &BlockToVerify) -> VerificationResult
    pub fn verify_batch(&mut self, blocks: &[BlockToVerify]) -> Vec<VerificationResult>
        // respects fail_fast: if true, stop on first failure
    pub fn stats(&self) -> VerifierStats
    pub fn reset_stats(&mut self)
}
```

### Tests (22 tests)

1. test_verify_crc32c_passes — correct checksum passes
2. test_verify_crc32c_fails — wrong checksum fails
3. test_empty_data — empty data with matching checksum passes
4. test_single_byte — 1-byte data
5. test_64kb_data — 65536-byte data
6. test_stats_blocks_checked — counter increments
7. test_stats_blocks_failed — failure counter
8. test_stats_bytes_verified — accumulates
9. test_reset_stats — all counters to zero
10. test_verify_batch_all_pass — batch all pass
11. test_verify_batch_one_fail — batch with one failure
12. test_verify_batch_fail_fast — fail_fast=true stops at first failure
13. test_verify_batch_empty — empty input returns empty
14. test_compute_deterministic — same data, same checksum
15. test_different_data_checksum — different data different checksum
16. test_all_zeros_data — all-zero data passes
17. test_all_ff_data — 0xFF data passes
18. test_crc32c_vs_blake3_differ — same data, different algorithms, different checksums
19. test_corruptions_found — increments on failed verify
20. test_passed_plus_failed_equals_checked — invariant
21. test_fail_fast_false_processes_all — processes all when not fail_fast
22. test_large_block_ref — block_ref with large BlockId

Use `BlockRef { id: BlockId::new(0, 100), size: BlockSize::B4K }` for tests.
`BlockId::new(device_idx: u16, offset_4k: u64)`.

## Output
Write the complete Rust file. Then run:
```
cd /home/cfs/claudefs && cargo test -p claudefs-storage block_verifier 2>&1 | tail -3
```

Show the test result.
