# Task: Implement `block_verifier.rs` for claudefs-storage

## Working directory
/home/cfs/claudefs

## Output file
Write to: `crates/claudefs-storage/src/block_verifier.rs`

## Purpose
End-to-end block integrity verification. Given a block's data and its stored checksum,
verifies integrity and reports corruption. Used by the scrub engine and recovery path.

## Code Conventions from existing crate
- File header: `//! <purpose description>`
- Error handling: `thiserror`
- Serialization: `serde` with `Serialize, Deserialize` derives
- Logging: `tracing` crate (use `debug!`, `info!`, `warn!` macros)
- All public items: `///` doc comments (avoid `#[warn(missing_docs)]`)
- 20+ unit tests per module inside `#[cfg(test)] mod tests { ... }`
- NO `#[allow(dead_code)]` or suppression attributes
- NO async — all synchronous

## Available imports from within the crate
```rust
use crate::block::BlockRef;
```

## Data structures to implement

```rust
/// Result of verifying a single block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub block_ref: BlockRef,
    pub passed: bool,
    pub computed_checksum: u32,  // CRC32c of the data
    pub stored_checksum: u32,    // checksum from the block header
    pub data_len: usize,
}

/// A block's data + stored checksum for verification.
#[derive(Debug, Clone)]
pub struct BlockToVerify {
    pub block_ref: BlockRef,
    pub data: Vec<u8>,
    pub stored_checksum: u32,
}

/// Aggregate statistics from a verification run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VerifierStats {
    pub blocks_checked: u64,
    pub blocks_passed: u64,
    pub blocks_failed: u64,
    pub total_bytes_verified: u64,
    pub corruptions_found: u64,
}

/// Configuration for the block verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    pub algorithm: VerifierAlgorithm,
    pub fail_fast: bool,  // stop on first corruption (default: false)
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            algorithm: VerifierAlgorithm::Crc32c,
            fail_fast: false,
        }
    }
}

/// Checksum algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifierAlgorithm { Crc32c, Blake3 }
```

## BlockVerifier struct and methods

```rust
pub struct BlockVerifier { 
    config: VerifierConfig, 
    stats: VerifierStats 
}

impl BlockVerifier {
    /// Creates a new BlockVerifier with the given configuration.
    pub fn new(config: VerifierConfig) -> Self
    
    /// Verifies a single block against its stored checksum.
    /// Returns VerificationResult with passed=true if checksums match.
    /// Updates stats accordingly.
    pub fn verify_block(&mut self, block: &BlockToVerify) -> VerificationResult
    
    /// Verifies multiple blocks. If fail_fast is true, stops after first failure.
    /// Returns results for all verified blocks.
    pub fn verify_batch(&mut self, blocks: &[BlockToVerify]) -> Vec<VerificationResult>
    
    /// Returns current statistics.
    pub fn stats(&self) -> VerifierStats
    
    /// Resets all statistics to zero.
    pub fn reset_stats(&mut self)
    
    /// Computes checksum for the given data using the configured algorithm.
    pub fn compute_checksum(&self, data: &[u8]) -> u32
}
```

## Checksum Implementation Notes

**CRC32c implementation**: Use the CRC32C polynomial `0x82F63B78`.
You can use a table-based implementation similar to the existing checksum.rs module.
Reference from checksum.rs:

```rust
const fn make_crc32c_table() -> [u32; 256] {
    const POLY: u32 = 0x82F63B78;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

fn crc32c(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = make_crc32c_table();
    let mut crc: u32 = !0;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    !crc
}
```

**BLAKE3**: Since we can't use external crates, implement a simple BLAKE3-like hash:
- For simplicity, use a simplified approach: compute a 32-byte hash using a basic mixing function
- Take first 4 bytes as u32 (little-endian)
- Or alternatively, just use a different hash approach like xxHash64 and truncate

For simplicity, you may implement Blake3 as:
- A simple hash that produces different results from CRC32C
- Use the xxhash64 implementation from checksum.rs and take lower 32 bits

## Required Tests (22 tests)
1. Verify block with correct CRC32c passes
2. Verify block with wrong checksum fails
3. Empty data verifies if checksum matches
4. 1-byte data
5. 64KB data verifies correctly
6. stats.blocks_checked increments
7. stats.blocks_failed increments on failure
8. stats.total_bytes_verified accumulates
9. reset_stats clears all counters
10. verify_batch with all passing returns all passed
11. verify_batch with one failing marks it
12. verify_batch with fail_fast=true stops early
13. verify_batch empty input returns empty
14. compute_checksum is deterministic
15. Different data gives different checksums
16. All-zeros data has valid checksum
17. All-0xFF data has valid checksum
18. VerifierAlgorithm::Crc32c and Blake3 differ for same data
19. corruptions_found increments on failed verification
20. blocks_passed + blocks_failed == blocks_checked
21. VerifierConfig::fail_fast=false processes all blocks
22. BlockToVerify with max-size block reference

## Final output
Write the complete Rust file with all structures, implementations, and tests.