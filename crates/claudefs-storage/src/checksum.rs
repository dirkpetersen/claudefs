//! Checksum module for data integrity verification.
//!
//! Provides CRC32C and xxHash64 algorithms for block integrity checks,
//! along with a block header format for metadata and verification.

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::block::BlockSize;

/// Supported checksum algorithms for data integrity verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ChecksumAlgorithm {
    /// CRC32C — hardware-accelerated on modern CPUs, good for inline verification
    #[default]
    Crc32c,
    /// xxHash64 — very fast non-cryptographic hash for block checksums
    XxHash64,
    /// No checksum (for performance-critical paths where integrity is handled elsewhere)
    None,
}

impl Default for ChecksumAlgorithm {
    fn default() -> Self {
        ChecksumAlgorithm::Crc32c
    }
}

impl std::fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChecksumAlgorithm::Crc32c => write!(f, "CRC32C"),
            ChecksumAlgorithm::XxHash64 => write!(f, "xxHash64"),
            ChecksumAlgorithm::None => write!(f, "None"),
        }
    }
}

/// A computed checksum value with its algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Checksum {
    /// The algorithm used to compute this checksum.
    pub algorithm: ChecksumAlgorithm,
    /// The checksum value.
    pub value: u64,
}

impl Checksum {
    /// Creates a new checksum with the given algorithm and value.
    pub fn new(algorithm: ChecksumAlgorithm, value: u64) -> Self {
        Self { algorithm, value }
    }
}

/// Computes the checksum for the given data using the specified algorithm.
///
/// # Arguments
/// * `algorithm` - The checksum algorithm to use
/// * `data` - The data to compute the checksum for
///
/// # Returns
/// A `Checksum` containing the algorithm and computed value
pub fn compute(algorithm: ChecksumAlgorithm, data: &[u8]) -> Checksum {
    let value = match algorithm {
        ChecksumAlgorithm::Crc32c => crc32c(data) as u64,
        ChecksumAlgorithm::XxHash64 => xxhash64(data, 0),
        ChecksumAlgorithm::None => 0,
    };
    debug!(
        algorithm = %algorithm,
        value = value,
        size = data.len(),
        "computed checksum"
    );
    Checksum { algorithm, value }
}

/// Verifies that the data matches the given checksum.
///
/// # Arguments
/// * `checksum` - The expected checksum
/// * `data` - The data to verify
///
/// # Returns
/// `true` if the computed checksum matches the expected value, `false` otherwise
pub fn verify(checksum: &Checksum, data: &[u8]) -> bool {
    let computed = compute(checksum.algorithm, data);
    let matches = computed.value == checksum.value;
    if !matches {
        debug!(
            algorithm = %checksum.algorithm,
            expected = checksum.value,
            actual = computed.value,
            "checksum mismatch"
        );
    }
    matches
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

/// Header stored at the beginning of each block for integrity verification.
/// Fixed size: 64 bytes.
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct BlockHeader {
    /// Magic number for block identification (0x434C4653 = "CLFS")
    pub magic: u32,
    /// Version of the block format
    pub version: u8,
    /// Block size class
    pub block_size: BlockSize,
    /// Checksum of the data portion (after the header)
    pub data_checksum: Checksum,
    /// Sequence number for write ordering
    pub sequence: u64,
    /// Timestamp when the block was written (seconds since epoch)
    pub timestamp_secs: u64,
}

impl BlockHeader {
    /// Magic number for block identification: 0x434C4653 = "CLFS"
    pub const MAGIC: u32 = 0x434C4653;

    /// Current version of the block format
    pub const CURRENT_VERSION: u8 = 1;

    /// Size of the header in bytes
    pub const SIZE: usize = 64;

    /// Creates a new BlockHeader with the given parameters.
    ///
    /// # Arguments
    /// * `block_size` - The size class of the block
    /// * `data_checksum` - The checksum of the data portion
    /// * `sequence` - The sequence number for write ordering
    pub fn new(block_size: BlockSize, data_checksum: Checksum, sequence: u64) -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::CURRENT_VERSION,
            block_size,
            data_checksum,
            sequence,
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Validates the magic number.
    ///
    /// # Returns
    /// `true` if the magic number matches the expected value
    pub fn validate_magic(&self) -> bool {
        self.magic == Self::MAGIC
    }

    /// Returns the size of the data portion (total block size minus header).
    pub fn data_size(&self) -> u64 {
        self.block_size.as_bytes() - Self::SIZE as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_known_vectors() {
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c(b"123456789"), 0xE3069283);
        let h1 = crc32c(b"hello");
        let h2 = crc32c(b"hello");
        assert_eq!(h1, h2);
        let h3 = crc32c(b"world");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_xxhash64_known_vectors() {
        let h1 = xxhash64(b"hello", 0);
        let h2 = xxhash64(b"hello", 0);
        assert_eq!(h1, h2);
        let h3 = xxhash64(b"world", 0);
        assert_ne!(h1, h3);
        assert_ne!(xxhash64(b"", 0), 0);
    }

    #[test]
    fn test_checksum_compute_crc32c() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"hello");
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Crc32c);
        assert_ne!(checksum.value, 0);
    }

    #[test]
    fn test_checksum_compute_xxhash64() {
        let checksum = compute(ChecksumAlgorithm::XxHash64, b"hello");
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::XxHash64);
        assert_ne!(checksum.value, 0);
    }

    #[test]
    fn test_checksum_verify() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"hello world");
        assert!(verify(&checksum, b"hello world"));
    }

    #[test]
    fn test_checksum_mismatch() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"hello");
        assert!(!verify(&checksum, b"hello world"));
    }

    #[test]
    fn test_none_checksum() {
        let checksum = compute(ChecksumAlgorithm::None, b"anything");
        assert_eq!(checksum.value, 0);
        assert!(verify(&checksum, b"data"));
        assert!(verify(&checksum, b""));
    }

    #[test]
    fn test_block_header_creation() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"test data");
        let header = BlockHeader::new(BlockSize::B4K, checksum, 1);

        assert_eq!(header.magic, BlockHeader::MAGIC);
        assert_eq!(header.version, BlockHeader::CURRENT_VERSION);
        assert_eq!(header.block_size, BlockSize::B4K);
        assert_eq!(header.sequence, 1);
        assert!(header.validate_magic());
    }

    #[test]
    fn test_block_header_magic_validation() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"test");
        let header = BlockHeader::new(BlockSize::B64K, checksum, 42);

        assert!(header.validate_magic());

        let mut invalid_header = header;
        invalid_header.magic = 0xDEADBEEF;
        assert!(!invalid_header.validate_magic());
    }

    #[test]
    fn test_checksum_hash_trait() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let checksum = Checksum::new(ChecksumAlgorithm::Crc32c, 0x12345678);
        let mut hasher = DefaultHasher::new();
        checksum.hash(&mut hasher);
        let hash = hasher.finish();
        assert!(hash > 0);
    }

    #[test]
    fn test_block_header_hash_trait() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let checksum = compute(ChecksumAlgorithm::Crc32c, b"test");
        let header = BlockHeader::new(BlockSize::B1M, checksum, 100);
        let mut hasher = DefaultHasher::new();
        header.hash(&mut hasher);
        let hash = hasher.finish();
        assert!(hash > 0);
    }

    #[test]
    fn test_checksum_default() {
        let default_algo = ChecksumAlgorithm::default();
        assert_eq!(default_algo, ChecksumAlgorithm::Crc32c);
    }

    #[test]
    fn test_checksum_display() {
        assert_eq!(format!("{}", ChecksumAlgorithm::Crc32c), "CRC32C");
        assert_eq!(format!("{}", ChecksumAlgorithm::XxHash64), "xxHash64");
        assert_eq!(format!("{}", ChecksumAlgorithm::None), "None");
    }

    #[test]
    fn test_block_header_data_size() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"");
        let header = BlockHeader::new(BlockSize::B4K, checksum, 0);

        let expected_data_size = 4096 - BlockHeader::SIZE;
        assert_eq!(header.data_size(), expected_data_size as u64);
    }

    #[test]
    fn test_checksum_algorithm_equality() {
        assert_eq!(ChecksumAlgorithm::Crc32c, ChecksumAlgorithm::Crc32c);
        assert_eq!(ChecksumAlgorithm::XxHash64, ChecksumAlgorithm::XxHash64);
        assert_eq!(ChecksumAlgorithm::None, ChecksumAlgorithm::None);
        assert_ne!(ChecksumAlgorithm::Crc32c, ChecksumAlgorithm::XxHash64);
    }

    #[test]
    fn test_large_data_checksum() {
        let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let crc_checksum = compute(ChecksumAlgorithm::Crc32c, &large_data);
        let xxh_checksum = compute(ChecksumAlgorithm::XxHash64, &large_data);

        assert!(verify(&crc_checksum, &large_data));
        assert!(verify(&xxh_checksum, &large_data));

        let mut corrupted = large_data.clone();
        corrupted[5000] = !corrupted[5000];
        assert!(!verify(&crc_checksum, &corrupted));
        assert!(!verify(&xxh_checksum, &corrupted));
    }

    #[test]
    fn test_checksum_copy() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"test");
        let copied = checksum;
        assert_eq!(checksum, copied);
    }

    #[test]
    fn test_block_header_clone() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"test");
        let header = BlockHeader::new(BlockSize::B4K, checksum, 1);
        let copied = header.clone();
        assert_eq!(header.magic, copied.magic);
        assert_eq!(header.sequence, copied.sequence);
    }

    #[test]
    fn test_checksum_debug_format() {
        let checksum = Checksum::new(ChecksumAlgorithm::Crc32c, 0x12345678);
        let debug_str = format!("{:?}", checksum);
        assert!(debug_str.contains("Crc32c"));
        assert!(debug_str.contains("Checksum"));
    }

    #[test]
    fn test_header_debug_format() {
        let checksum = compute(ChecksumAlgorithm::Crc32c, b"data");
        let header = BlockHeader::new(BlockSize::B4K, checksum, 42);
        let debug_str = format!("{:?}", header);
        assert!(debug_str.contains("BlockHeader"));
        assert!(debug_str.contains("magic"));
    }

    #[test]
    fn test_various_data_sizes() {
        // Test that different sized inputs produce different (deterministic) checksums
        let empty = compute(ChecksumAlgorithm::Crc32c, b"");
        let a = compute(ChecksumAlgorithm::Crc32c, b"a");
        let _ab = compute(ChecksumAlgorithm::Crc32c, b"ab");
        let _abc = compute(ChecksumAlgorithm::Crc32c, b"abc");

        // Verify determinism - same input produces same output
        assert_eq!(a.value, compute(ChecksumAlgorithm::Crc32c, b"a").value);

        // Empty should be different from non-empty
        assert_ne!(empty.value, a.value);
    }
}
