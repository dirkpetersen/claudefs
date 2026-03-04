//! End-to-end block integrity verification.
//!
//! Given a block's data and its stored checksum, verifies integrity and reports
//! corruption. Used by the scrub engine and recovery path.

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::block::BlockRef;

/// Result of verifying a single block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Reference to the verified block.
    pub block_ref: BlockRef,
    /// Whether verification passed (checksums match).
    pub passed: bool,
    /// CRC32c of the data computed during verification.
    pub computed_checksum: u32,
    /// Checksum from the block header.
    pub stored_checksum: u32,
    /// Length of the data verified.
    pub data_len: usize,
}

/// A block's data + stored checksum for verification.
#[derive(Debug, Clone)]
pub struct BlockToVerify {
    /// Reference to the block being verified.
    pub block_ref: BlockRef,
    /// The block's data.
    pub data: Vec<u8>,
    /// The stored checksum from the block header.
    pub stored_checksum: u32,
}

/// Aggregate statistics from a verification run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VerifierStats {
    /// Total blocks checked.
    pub blocks_checked: u64,
    /// Blocks that passed verification.
    pub blocks_passed: u64,
    /// Blocks that failed verification.
    pub blocks_failed: u64,
    /// Total bytes verified.
    pub total_bytes_verified: u64,
    /// Number of corruptions found.
    pub corruptions_found: u64,
}

/// Configuration for the block verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// Checksum algorithm to use.
    pub algorithm: VerifierAlgorithm,
    /// Stop on first corruption if true.
    pub fail_fast: bool,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            algorithm: VerifierAlgorithm::Crc32c,
            fail_fast: false,
        }
    }
}

/// Checksum algorithm to use for verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifierAlgorithm {
    /// CRC32C - hardware accelerated on modern CPUs.
    Crc32c,
    /// BLAKE3-like hash (xxHash64-based for this implementation).
    Blake3,
}

/// Generates the CRC32C lookup table at compile time.
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

/// CRC32C implementation using the standard Castagnoli polynomial.
fn crc32c(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = make_crc32c_table();
    let mut crc: u32 = !0;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    !crc
}

const XXH_PRIME1: u64 = 0x9E3779B185EBCA87;
const XXH_PRIME2: u64 = 0xC2B2AE3D27D4EB4F;
const XXH_PRIME3: u64 = 0x165667B19E3779F9;
const XXH_PRIME4: u64 = 0x85EBCA77C2B2AE63;
const XXH_PRIME5: u64 = 0x27D4EB2F165667C5;

fn xxh64_round(acc: u64, input: u64) -> u64 {
    let input = input
        .wrapping_mul(XXH_PRIME2)
        .rotate_left(31)
        .wrapping_mul(XXH_PRIME1);
    acc ^ input
}

fn xxh64_merge_round(acc: u64, val: u64) -> u64 {
    let val = val
        .wrapping_mul(XXH_PRIME2)
        .rotate_left(31)
        .wrapping_mul(XXH_PRIME1);
    let acc = acc ^ val;
    acc.wrapping_mul(XXH_PRIME1).wrapping_add(XXH_PRIME4)
}

fn xxhash64(data: &[u8], seed: u64) -> u64 {
    let len = data.len();
    let mut hash: u64;

    if len >= 32 {
        let mut v1 = seed.wrapping_add(XXH_PRIME1).wrapping_add(XXH_PRIME2);
        let mut v2 = seed.wrapping_add(XXH_PRIME2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(XXH_PRIME1);

        let mut remaining = len;
        let mut ptr = 0;

        while remaining >= 32 {
            let mut read: u64;
            read = data[ptr] as u64;
            read |= (data[ptr + 1] as u64) << 8;
            read |= (data[ptr + 2] as u64) << 16;
            read |= (data[ptr + 3] as u64) << 24;
            read |= (data[ptr + 4] as u64) << 32;
            read |= (data[ptr + 5] as u64) << 40;
            read |= (data[ptr + 6] as u64) << 48;
            read |= (data[ptr + 7] as u64) << 56;
            v1 = xxh64_round(v1, read);

            read = data[ptr + 8] as u64;
            read |= (data[ptr + 9] as u64) << 8;
            read |= (data[ptr + 10] as u64) << 16;
            read |= (data[ptr + 11] as u64) << 24;
            read |= (data[ptr + 12] as u64) << 32;
            read |= (data[ptr + 13] as u64) << 40;
            read |= (data[ptr + 14] as u64) << 48;
            read |= (data[ptr + 15] as u64) << 56;
            v2 = xxh64_round(v2, read);

            read = data[ptr + 16] as u64;
            read |= (data[ptr + 17] as u64) << 8;
            read |= (data[ptr + 18] as u64) << 16;
            read |= (data[ptr + 19] as u64) << 24;
            read |= (data[ptr + 20] as u64) << 32;
            read |= (data[ptr + 21] as u64) << 40;
            read |= (data[ptr + 22] as u64) << 48;
            read |= (data[ptr + 23] as u64) << 56;
            v3 = xxh64_round(v3, read);

            read = data[ptr + 24] as u64;
            read |= (data[ptr + 25] as u64) << 8;
            read |= (data[ptr + 26] as u64) << 16;
            read |= (data[ptr + 27] as u64) << 24;
            read |= (data[ptr + 28] as u64) << 32;
            read |= (data[ptr + 29] as u64) << 40;
            read |= (data[ptr + 30] as u64) << 48;
            read |= (data[ptr + 31] as u64) << 56;
            v4 = xxh64_round(v4, read);

            ptr += 32;
            remaining -= 32;
        }

        hash = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));

        hash = xxh64_merge_round(hash, v1);
        hash = xxh64_merge_round(hash, v2);
        hash = xxh64_merge_round(hash, v3);
        hash = xxh64_merge_round(hash, v4);
    } else {
        hash = seed.wrapping_add(XXH_PRIME5);
    }

    hash = hash.wrapping_add(len as u64);
    let mut ptr = 0;
    while ptr + 8 <= len {
        let mut k1: u64;
        k1 = data[ptr] as u64;
        k1 |= (data[ptr + 1] as u64) << 8;
        k1 |= (data[ptr + 2] as u64) << 16;
        k1 |= (data[ptr + 3] as u64) << 24;
        k1 |= (data[ptr + 4] as u64) << 32;
        k1 |= (data[ptr + 5] as u64) << 40;
        k1 |= (data[ptr + 6] as u64) << 48;
        k1 |= (data[ptr + 7] as u64) << 56;
        k1 = k1
            .wrapping_mul(XXH_PRIME2)
            .rotate_left(31)
            .wrapping_mul(XXH_PRIME1);
        hash ^= k1;
        hash = hash
            .rotate_left(27)
            .wrapping_mul(XXH_PRIME1)
            .wrapping_add(XXH_PRIME4);
        ptr += 8;
    }

    if ptr + 4 <= len {
        let mut k1: u64 = data[ptr] as u64;
        k1 |= (data[ptr + 1] as u64) << 8;
        k1 |= (data[ptr + 2] as u64) << 16;
        k1 |= (data[ptr + 3] as u64) << 24;
        hash ^= k1.wrapping_mul(XXH_PRIME1);
        hash = hash
            .rotate_left(23)
            .wrapping_mul(XXH_PRIME2)
            .wrapping_add(XXH_PRIME3);
        ptr += 4;
    }

    while ptr < len {
        hash ^= (data[ptr] as u64).wrapping_mul(XXH_PRIME5);
        hash = hash.rotate_left(11).wrapping_mul(XXH_PRIME1);
        ptr += 1;
    }

    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51AFD7ED558CCD);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xC4CEB9FE1A85EC53);
    hash ^= hash >> 33;

    hash
}

/// Block integrity verifier.
#[derive(Debug, Clone)]
pub struct BlockVerifier {
    config: VerifierConfig,
    stats: VerifierStats,
}

impl BlockVerifier {
    /// Creates a new BlockVerifier with the given configuration.
    pub fn new(config: VerifierConfig) -> Self {
        debug!(algorithm = ?config.algorithm, fail_fast = config.fail_fast, "creating block verifier");
        Self {
            config,
            stats: VerifierStats::default(),
        }
    }

    /// Verifies a single block against its stored checksum.
    ///
    /// Returns a `VerificationResult` with `passed=true` if checksums match.
    /// Updates statistics accordingly.
    pub fn verify_block(&mut self, block: &BlockToVerify) -> VerificationResult {
        let computed = self.compute_checksum(&block.data);
        let passed = computed == block.stored_checksum;

        self.stats.blocks_checked += 1;
        self.stats.total_bytes_verified += block.data.len() as u64;

        if passed {
            self.stats.blocks_passed += 1;
        } else {
            self.stats.blocks_failed += 1;
            self.stats.corruptions_found += 1;
            debug!(
                block_ref = ?block.block_ref,
                computed = computed,
                stored = block.stored_checksum,
                "block verification failed"
            );
        }

        VerificationResult {
            block_ref: block.block_ref,
            passed,
            computed_checksum: computed,
            stored_checksum: block.stored_checksum,
            data_len: block.data.len(),
        }
    }

    /// Verifies multiple blocks.
    ///
    /// If `fail_fast` is true in config, stops after first failure.
    /// Returns results for all verified blocks.
    pub fn verify_batch(&mut self, blocks: &[BlockToVerify]) -> Vec<VerificationResult> {
        let mut results = Vec::with_capacity(blocks.len());

        for block in blocks {
            let result = self.verify_block(block);
            let failed = !result.passed;
            results.push(result);

            if failed && self.config.fail_fast {
                break;
            }
        }

        results
    }

    /// Returns current statistics.
    pub fn stats(&self) -> VerifierStats {
        self.stats.clone()
    }

    /// Resets all statistics to zero.
    pub fn reset_stats(&mut self) {
        self.stats = VerifierStats::default();
    }

    /// Computes checksum for the given data using the configured algorithm.
    pub fn compute_checksum(&self, data: &[u8]) -> u32 {
        match self.config.algorithm {
            VerifierAlgorithm::Crc32c => crc32c(data),
            VerifierAlgorithm::Blake3 => {
                let hash = xxhash64(data, 0x424C4B33);
                hash as u32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};

    fn make_block_ref(device_idx: u16, offset: u64) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size: BlockSize::B4K,
        }
    }

    fn make_block_to_verify(data: &[u8], checksum: u32) -> BlockToVerify {
        BlockToVerify {
            block_ref: make_block_ref(0, 0),
            data: data.to_vec(),
            stored_checksum: checksum,
        }
    }

    #[test]
    fn verify_block_with_correct_crc32c_passes() {
        let mut verifier = BlockVerifier::new(VerifierConfig {
            algorithm: VerifierAlgorithm::Crc32c,
            fail_fast: false,
        });
        let data = b"hello world";
        let checksum = verifier.compute_checksum(data);
        let block = make_block_to_verify(data, checksum);
        let result = verifier.verify_block(&block);
        assert!(result.passed);
    }

    #[test]
    fn verify_block_with_wrong_checksum_fails() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let block = make_block_to_verify(b"hello world", 0xDEADBEEF);
        let result = verifier.verify_block(&block);
        assert!(!result.passed);
    }

    #[test]
    fn empty_data_verifies_if_checksum_matches() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let checksum = verifier.compute_checksum(b"");
        let block = make_block_to_verify(b"", checksum);
        let result = verifier.verify_block(&block);
        assert!(result.passed);
        assert_eq!(result.data_len, 0);
    }

    #[test]
    fn one_byte_data_verifies_correctly() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let data = b"x";
        let checksum = verifier.compute_checksum(data);
        let block = make_block_to_verify(data, checksum);
        let result = verifier.verify_block(&block);
        assert!(result.passed);
        assert_eq!(result.data_len, 1);
    }

    #[test]
    fn sixty_four_kb_data_verifies_correctly() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let data: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
        let checksum = verifier.compute_checksum(&data);
        let block = make_block_to_verify(&data, checksum);
        let result = verifier.verify_block(&block);
        assert!(result.passed);
        assert_eq!(result.data_len, 65536);
    }

    #[test]
    fn stats_blocks_checked_increments() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let block = make_block_to_verify(b"data", verifier.compute_checksum(b"data"));
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().blocks_checked, 1);
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().blocks_checked, 2);
    }

    #[test]
    fn stats_blocks_failed_increments_on_failure() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let block = make_block_to_verify(b"data", 0xBAD);
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().blocks_failed, 1);
    }

    #[test]
    fn stats_total_bytes_verified_accumulates() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let checksum = verifier.compute_checksum(b"hello");
        let block = make_block_to_verify(b"hello", checksum);
        verifier.verify_block(&block);
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().total_bytes_verified, 10);
    }

    #[test]
    fn reset_stats_clears_all_counters() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let block = make_block_to_verify(b"data", verifier.compute_checksum(b"data"));
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().blocks_checked, 1);
        verifier.reset_stats();
        let stats = verifier.stats();
        assert_eq!(stats.blocks_checked, 0);
        assert_eq!(stats.blocks_passed, 0);
        assert_eq!(stats.blocks_failed, 0);
        assert_eq!(stats.total_bytes_verified, 0);
        assert_eq!(stats.corruptions_found, 0);
    }

    #[test]
    fn verify_batch_with_all_passing_returns_all_passed() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let blocks: Vec<BlockToVerify> = (0..3)
            .map(|i| {
                let data = format!("data{}", i).into_bytes();
                BlockToVerify {
                    block_ref: make_block_ref(0, i as u64),
                    data: data.clone(),
                    stored_checksum: verifier.compute_checksum(&data),
                }
            })
            .collect();
        let results = verifier.verify_batch(&blocks);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.passed));
    }

    #[test]
    fn verify_batch_with_one_failing_marks_it() {
        let mut verifier = BlockVerifier::new(VerifierConfig {
            fail_fast: false,
            ..Default::default()
        });
        let checksum = verifier.compute_checksum(b"good");
        let blocks = vec![
            BlockToVerify {
                block_ref: make_block_ref(0, 0),
                data: b"good".to_vec(),
                stored_checksum: checksum,
            },
            BlockToVerify {
                block_ref: make_block_ref(0, 1),
                data: b"bad".to_vec(),
                stored_checksum: 0xBAD,
            },
            BlockToVerify {
                block_ref: make_block_ref(0, 2),
                data: b"good".to_vec(),
                stored_checksum: checksum,
            },
        ];
        let results = verifier.verify_batch(&blocks);
        assert_eq!(results.len(), 3);
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[2].passed);
    }

    #[test]
    fn verify_batch_with_fail_fast_stops_early() {
        let mut verifier = BlockVerifier::new(VerifierConfig {
            fail_fast: true,
            ..Default::default()
        });
        let checksum = verifier.compute_checksum(b"good");
        let blocks = vec![
            BlockToVerify {
                block_ref: make_block_ref(0, 0),
                data: b"good".to_vec(),
                stored_checksum: checksum,
            },
            BlockToVerify {
                block_ref: make_block_ref(0, 1),
                data: b"bad".to_vec(),
                stored_checksum: 0xBAD,
            },
            BlockToVerify {
                block_ref: make_block_ref(0, 2),
                data: b"good".to_vec(),
                stored_checksum: checksum,
            },
        ];
        let results = verifier.verify_batch(&blocks);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(!results[1].passed);
    }

    #[test]
    fn verify_batch_empty_input_returns_empty() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let results = verifier.verify_batch(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn compute_checksum_is_deterministic() {
        let verifier = BlockVerifier::new(VerifierConfig::default());
        let data = b"test data";
        let c1 = verifier.compute_checksum(data);
        let c2 = verifier.compute_checksum(data);
        assert_eq!(c1, c2);
    }

    #[test]
    fn different_data_gives_different_checksums() {
        let verifier = BlockVerifier::new(VerifierConfig::default());
        let c1 = verifier.compute_checksum(b"hello");
        let c2 = verifier.compute_checksum(b"world");
        assert_ne!(c1, c2);
    }

    #[test]
    fn all_zeros_data_has_valid_checksum() {
        let verifier = BlockVerifier::new(VerifierConfig::default());
        let data = vec![0u8; 1000];
        let checksum = verifier.compute_checksum(&data);
        let block = make_block_to_verify(&data, checksum);
        let mut v2 = BlockVerifier::new(VerifierConfig::default());
        let result = v2.verify_block(&block);
        assert!(result.passed);
    }

    #[test]
    fn all_0xff_data_has_valid_checksum() {
        let verifier = BlockVerifier::new(VerifierConfig::default());
        let data = vec![0xFFu8; 1000];
        let checksum = verifier.compute_checksum(&data);
        let block = make_block_to_verify(&data, checksum);
        let mut v2 = BlockVerifier::new(VerifierConfig::default());
        let result = v2.verify_block(&block);
        assert!(result.passed);
    }

    #[test]
    fn crc32c_and_blake3_differ_for_same_data() {
        let crc_verifier = BlockVerifier::new(VerifierConfig {
            algorithm: VerifierAlgorithm::Crc32c,
            ..Default::default()
        });
        let blake_verifier = BlockVerifier::new(VerifierConfig {
            algorithm: VerifierAlgorithm::Blake3,
            ..Default::default()
        });
        let data = b"test data for algorithm comparison";
        let crc = crc_verifier.compute_checksum(data);
        let blake = blake_verifier.compute_checksum(data);
        assert_ne!(crc, blake);
    }

    #[test]
    fn corruptions_found_increments_on_failed_verification() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let block = make_block_to_verify(b"data", 0xBAD);
        verifier.verify_block(&block);
        assert_eq!(verifier.stats().corruptions_found, 1);
    }

    #[test]
    fn blocks_passed_plus_failed_equals_checked() {
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let checksum = verifier.compute_checksum(b"good");
        let good_block = make_block_to_verify(b"good", checksum);
        let bad_block = make_block_to_verify(b"bad", 0xBAD);

        verifier.verify_block(&good_block);
        verifier.verify_block(&bad_block);

        let stats = verifier.stats();
        assert_eq!(
            stats.blocks_passed + stats.blocks_failed,
            stats.blocks_checked
        );
    }

    #[test]
    fn fail_fast_false_processes_all_blocks() {
        let mut verifier = BlockVerifier::new(VerifierConfig {
            fail_fast: false,
            ..Default::default()
        });
        let blocks: Vec<BlockToVerify> = (0..5)
            .map(|i| {
                let data = format!("data{}", i).into_bytes();
                BlockToVerify {
                    block_ref: make_block_ref(0, i as u64),
                    data: data.clone(),
                    stored_checksum: if i == 2 {
                        0xBAD
                    } else {
                        verifier.compute_checksum(&data)
                    },
                }
            })
            .collect();
        let results = verifier.verify_batch(&blocks);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn block_to_verify_with_max_size_block_reference() {
        let block_ref = BlockRef {
            id: BlockId::new(u16::MAX, u64::MAX),
            size: BlockSize::B64M,
        };
        let data = b"test data";
        let block = BlockToVerify {
            block_ref,
            data: data.to_vec(),
            stored_checksum: 0x12345678,
        };
        let mut verifier = BlockVerifier::new(VerifierConfig::default());
        let result = verifier.verify_block(&block);
        assert_eq!(result.block_ref.id.device_idx, u16::MAX);
        assert_eq!(result.block_ref.id.offset, u64::MAX);
    }

    #[test]
    fn verifier_config_default() {
        let config = VerifierConfig::default();
        assert_eq!(config.algorithm, VerifierAlgorithm::Crc32c);
        assert!(!config.fail_fast);
    }

    #[test]
    fn verification_result_serialization() {
        let block_ref = make_block_ref(1, 100);
        let result = VerificationResult {
            block_ref,
            passed: true,
            computed_checksum: 0x12345678,
            stored_checksum: 0x12345678,
            data_len: 42,
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: VerificationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.passed, decoded.passed);
        assert_eq!(result.computed_checksum, decoded.computed_checksum);
    }

    #[test]
    fn verifier_stats_serialization() {
        let stats = VerifierStats {
            blocks_checked: 100,
            blocks_passed: 95,
            blocks_failed: 5,
            total_bytes_verified: 409600,
            corruptions_found: 5,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let decoded: VerifierStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats.blocks_checked, decoded.blocks_checked);
    }
}
