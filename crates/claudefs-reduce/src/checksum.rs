//! End-to-end data integrity checksums for detecting silent data corruption.

use crate::error::ReduceError;
use serde::{Deserialize, Serialize};

/// Checksum algorithm for end-to-end integrity verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChecksumAlgorithm {
    /// Full BLAKE3 (32 bytes, cryptographically strong, used for CAS)
    #[default]
    Blake3,
    /// CRC32C (4 bytes, hardware-accelerated, fast for integrity check)
    Crc32c,
    /// xxHash64 (8 bytes, very fast non-crypto hash)
    Xxhash64,
}

/// A computed checksum with its algorithm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataChecksum {
    /// Algorithm used to compute the checksum
    pub algorithm: ChecksumAlgorithm,
    /// Checksum bytes (4 for CRC32C, 8 for xxHash64, 32 for BLAKE3)
    pub bytes: Vec<u8>,
}

/// CRC32C (Castagnoli) lookup table
const CRC32C_TABLE: [u32; 256] = {
    const POLY: u32 = 0x82F63B78;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0u32;
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
};

/// Compute CRC32C checksum using the Castagnoli polynomial.
#[inline]
fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for byte in data {
        let idx = ((crc ^ *byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[idx];
    }
    crc ^ 0xFFFFFFFF
}

/// xxHash64 constants
const XXHASH64_PRIME1: u64 = 11400714785074694791u64;
const XXHASH64_PRIME2: u64 = 14029467366897019727u64;
const XXHASH64_PRIME3: u64 = 1609587929392839161u64;
const XXHASH64_PRIME4: u64 = 9650029242287828579u64;
const XXHASH64_PRIME5: u64 = 2870177450012600261u64;

/// Compute xxHash64 checksum - reference implementation.
fn xxhash64(data: &[u8]) -> u64 {
    let len = data.len();
    let mut hash: u64;

    if len >= 32 {
        let mut v1 = XXHASH64_PRIME1.wrapping_add(XXHASH64_PRIME2);
        let mut v2 = XXHASH64_PRIME2;
        let mut v3 = XXHASH64_PRIME3;
        let mut v4 = XXHASH64_PRIME4
            .wrapping_sub(XXHASH64_PRIME1)
            .wrapping_add(1);

        let mut iters = len / 32;
        let mut idx = 0;

        while iters > 0 {
            v1 = round_xxhash64(v1, read_u64_le(&data[idx..idx + 8]));
            v2 = round_xxhash64(v2, read_u64_le(&data[idx + 8..idx + 16]));
            v3 = round_xxhash64(v3, read_u64_le(&data[idx + 16..idx + 24]));
            v4 = round_xxhash64(v4, read_u64_le(&data[idx + 24..idx + 32]));
            idx += 32;
            iters -= 1;
        }

        hash = v1
            .rotate_left(1)
            .wrapping_add(v2)
            .rotate_left(7)
            .wrapping_add(v3)
            .rotate_left(12)
            .wrapping_add(v4)
            .rotate_left(18);
        hash = merge_round_xxhash64(hash, v1);
        hash = merge_round_xxhash64(hash, v2);
        hash = merge_round_xxhash64(hash, v3);
        hash = merge_round_xxhash64(hash, v4);
    } else {
        hash = XXHASH64_PRIME5;
    }

    hash = hash.wrapping_add(len as u64);

    let remaining = len % 32;
    let mut idx = len - remaining;

    while idx + 8 <= len {
        hash ^= round_xxhash64(0, read_u64_le(&data[idx..idx + 8]));
        hash = hash
            .rotate_left(27)
            .wrapping_mul(XXHASH64_PRIME1)
            .wrapping_add(XXHASH64_PRIME4);
        idx += 8;
    }

    let remaining_after_8 = len - idx;
    if remaining_after_8 >= 4 {
        let k1 =
            u32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as u64;
        hash ^= k1.wrapping_mul(XXHASH64_PRIME1);
        hash = hash
            .rotate_left(23)
            .wrapping_mul(XXHASH64_PRIME2)
            .wrapping_add(XXHASH64_PRIME3);
        idx += 4;
    }

    let remaining_after_4 = len - idx;
    if remaining_after_4 >= 2 {
        let k1 = u16::from_le_bytes([data[idx], data[idx + 1]]) as u64;
        hash ^= k1.wrapping_mul(XXHASH64_PRIME5);
        hash = hash.rotate_left(11).wrapping_mul(XXHASH64_PRIME1);
        idx += 2;
    }

    let remaining_after_2 = len - idx;
    if remaining_after_2 >= 1 {
        hash ^= (data[idx] as u64).wrapping_mul(XXHASH64_PRIME4);
        hash = hash.rotate_left(15).wrapping_mul(XXHASH64_PRIME1);
    }

    hash ^= hash >> 33;
    hash = hash.wrapping_mul(XXHASH64_PRIME2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(XXHASH64_PRIME3);
    hash ^= hash >> 32;
    hash
}

#[inline]
fn read_u64_le(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ])
}

#[inline]
fn round_xxhash64(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(XXHASH64_PRIME2))
        .rotate_left(31)
        .wrapping_mul(XXHASH64_PRIME1)
}

#[inline]
fn merge_round_xxhash64(acc: u64, val: u64) -> u64 {
    acc ^ round_xxhash64(0, val)
        .wrapping_add(XXHASH64_PRIME3)
        .wrapping_mul(XXHASH64_PRIME1)
}

/// Compute a checksum of the given data.
pub fn compute(data: &[u8], algo: ChecksumAlgorithm) -> DataChecksum {
    let bytes = match algo {
        ChecksumAlgorithm::Blake3 => {
            let hash = blake3::hash(data);
            hash.as_bytes().to_vec()
        }
        ChecksumAlgorithm::Crc32c => {
            let crc = crc32c(data);
            crc.to_le_bytes().to_vec()
        }
        ChecksumAlgorithm::Xxhash64 => {
            let hash = xxhash64(data);
            hash.to_le_bytes().to_vec()
        }
    };

    DataChecksum {
        algorithm: algo,
        bytes,
    }
}

/// Verify data matches the expected checksum.
/// Returns Ok(()) if valid, Err(ReduceError::ChecksumMismatch) if invalid.
pub fn verify(data: &[u8], expected: &DataChecksum) -> Result<(), ReduceError> {
    let actual = compute(data, expected.algorithm);

    if actual.bytes == expected.bytes {
        Ok(())
    } else {
        Err(ReduceError::ChecksumMismatch)
    }
}

/// A data block with attached checksum for end-to-end integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksummedBlock {
    /// The raw data
    pub data: Vec<u8>,
    /// Checksum for integrity verification
    pub checksum: DataChecksum,
}

impl ChecksummedBlock {
    /// Create a new checksummed block with the specified algorithm.
    pub fn new(data: Vec<u8>, algo: ChecksumAlgorithm) -> Self {
        let checksum = compute(&data, algo);
        Self { data, checksum }
    }

    /// Verify the block's integrity.
    /// Returns Ok(()) if valid, Err(ReduceError::ChecksumMismatch) if corrupted.
    pub fn verify(&self) -> Result<(), ReduceError> {
        verify(&self.data, &self.checksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_roundtrip() {
        let data = b"Hello, ClaudeFS! This is a test of BLAKE3 checksums.";
        let checksum = compute(data, ChecksumAlgorithm::Blake3);
        assert_eq!(checksum.bytes.len(), 32);
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Blake3);

        let result = verify(data, &checksum);
        assert!(result.is_ok());
    }

    #[test]
    fn test_crc32c_roundtrip() {
        let data = b"Hello, ClaudeFS! This is a test of CRC32C checksums.";
        let checksum = compute(data, ChecksumAlgorithm::Crc32c);
        assert_eq!(checksum.bytes.len(), 4);
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Crc32c);

        let result = verify(data, &checksum);
        assert!(result.is_ok());
    }

    #[test]
    fn test_xxhash64_roundtrip() {
        let data = b"Hello, ClaudeFS! This is a test of xxHash64 checksums.";
        let checksum = compute(data, ChecksumAlgorithm::Xxhash64);
        assert_eq!(checksum.bytes.len(), 8);
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Xxhash64);

        let result = verify(data, &checksum);
        assert!(result.is_ok());
    }

    #[test]
    fn test_corrupted_data_fails() {
        let data = b"Original data that will be corrupted";
        let mut corrupted = data.to_vec();

        let checksum_blake3 = compute(data, ChecksumAlgorithm::Blake3);
        corrupted[0] ^= 0xFF; // Flip first byte
        let result = verify(&corrupted, &checksum_blake3);
        assert!(result.is_err());

        let data = b"Original data that will be corrupted";
        let mut corrupted = data.to_vec();
        let checksum_crc = compute(data, ChecksumAlgorithm::Crc32c);
        corrupted[10] ^= 0xFF; // Flip a byte in the middle
        let result = verify(&corrupted, &checksum_crc);
        assert!(result.is_err());

        let data = b"Original data that will be corrupted";
        let mut corrupted = data.to_vec();
        let checksum_xxh = compute(data, ChecksumAlgorithm::Xxhash64);
        corrupted[20] ^= 0xFF; // Flip a byte at the end
        let result = verify(&corrupted, &checksum_xxh);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_data() {
        let data: &[u8] = &[];

        let checksum_blake3 = compute(data, ChecksumAlgorithm::Blake3);
        assert!(verify(data, &checksum_blake3).is_ok());

        let checksum_crc = compute(data, ChecksumAlgorithm::Crc32c);
        assert!(verify(data, &checksum_crc).is_ok());

        let checksum_xxh = compute(data, ChecksumAlgorithm::Xxhash64);
        assert!(verify(data, &checksum_xxh).is_ok());
    }

    #[test]
    fn test_checksummed_block() {
        let data = b"Data for checksummed block test".to_vec();
        let block = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Blake3);
        assert!(block.verify().is_ok());
    }

    #[test]
    fn test_checksummed_block_corruption() {
        let data = b"Data for checksummed block test".to_vec();
        let mut block = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Crc32c);
        block.data[5] ^= 0x42;
        let result = block.verify();
        assert!(result.is_err());
    }

    #[test]
    fn test_checksummed_block_different_algos() {
        let data = b"Testing different checksum algorithms".to_vec();

        let block_blake3 = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Blake3);
        assert!(block_blake3.verify().is_ok());

        let block_crc = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Crc32c);
        assert!(block_crc.verify().is_ok());

        let block_xxh = ChecksummedBlock::new(data.clone(), ChecksumAlgorithm::Xxhash64);
        assert!(block_xxh.verify().is_ok());
    }

    proptest::proptest! {
        #[test]
        fn prop_blake3_stable(ref data in "[\\x00-\\xff]{0,1024}") {
            let data_bytes = data.as_bytes();
            let checksum1 = compute(data_bytes, ChecksumAlgorithm::Blake3);
            let checksum2 = compute(data_bytes, ChecksumAlgorithm::Blake3);
            assert_eq!(checksum1.bytes, checksum2.bytes);
        }

        #[test]
        fn prop_crc32c_stable(ref data in "[\\x00-\\xff]{0,1024}") {
            let data_bytes = data.as_bytes();
            let checksum1 = compute(data_bytes, ChecksumAlgorithm::Crc32c);
            let checksum2 = compute(data_bytes, ChecksumAlgorithm::Crc32c);
            assert_eq!(checksum1.bytes, checksum2.bytes);
        }

        #[test]
        fn prop_xxhash64_stable(ref data in "[\\x00-\\xff]{0,1024}") {
            let data_bytes = data.as_bytes();
            let checksum1 = compute(data_bytes, ChecksumAlgorithm::Xxhash64);
            let checksum2 = compute(data_bytes, ChecksumAlgorithm::Xxhash64);
            assert_eq!(checksum1.bytes, checksum2.bytes);
        }
    }
}
