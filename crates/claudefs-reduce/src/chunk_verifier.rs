//! Background data integrity verifier for detecting bitrot and corruption.
//!
//! Verifies chunk integrity by recomputing BLAKE3 hashes and comparing against
//! stored expected hashes. Used for background scrubbing and data integrity checks.

use crate::fingerprint::{blake3_hash, ChunkHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Priority level for verification scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VerificationPriority {
    /// Low priority - run during idle time
    Low,
    /// Normal priority - standard verification
    #[default]
    Normal,
    /// High priority - urgent verification
    High,
}

/// Configuration for the chunk verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkVerifierConfig {
    /// Number of chunks to verify per batch
    pub batch_size: usize,
    /// Interval between verification runs in seconds
    pub interval_secs: u64,
    /// Priority level for verification
    pub priority: VerificationPriority,
}

impl Default for ChunkVerifierConfig {
    fn default() -> Self {
        Self {
            batch_size: 64,
            interval_secs: 3600,
            priority: VerificationPriority::Normal,
        }
    }
}

/// Result of verifying a single chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// Chunk hash matches expected hash
    Ok,
    /// Chunk data is corrupted - hash mismatch
    Corrupted {
        /// Hash computed from actual data
        hash: ChunkHash,
        /// Expected hash from metadata
        expected: ChunkHash,
    },
    /// Chunk data is missing
    Missing {
        /// Expected hash that was not found
        hash: ChunkHash,
    },
}

/// Statistics from verification operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationStats {
    /// Total chunks verified
    pub chunks_verified: usize,
    /// Chunks that passed verification
    pub chunks_ok: usize,
    /// Chunks that failed with corruption
    pub chunks_corrupted: usize,
    /// Chunks that were missing
    pub chunks_missing: usize,
    /// Total bytes verified
    pub bytes_verified: u64,
}

impl VerificationStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate results from a single verification
    pub fn record(&mut self, result: &VerificationResult, bytes: u64) {
        self.chunks_verified += 1;
        self.bytes_verified += bytes;

        match result {
            VerificationResult::Ok => self.chunks_ok += 1,
            VerificationResult::Corrupted { .. } => self.chunks_corrupted += 1,
            VerificationResult::Missing { .. } => self.chunks_missing += 1,
        }
    }

    /// Error rate (corrupted + missing) / total
    pub fn error_rate(&self) -> f64 {
        if self.chunks_verified == 0 {
            return 0.0;
        }
        (self.chunks_corrupted + self.chunks_missing) as f64 / self.chunks_verified as f64
    }
}

/// Schedule of chunks to verify, ordered by priority and last verified time.
#[derive(Debug, Clone, Default)]
pub struct VerificationSchedule {
    /// Ordered list of hashes to verify
    hashes: Vec<ChunkHash>,
    /// Timestamps of last verification per hash
    last_verified: HashMap<ChunkHash, u64>,
}

impl VerificationSchedule {
    /// Create a new empty schedule
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of pending verifications
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// Check if schedule is empty
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Get the next batch of hashes to verify
    pub fn next_batch(&mut self, n: usize) -> Vec<ChunkHash> {
        let count = n.min(self.hashes.len());
        self.hashes.drain(..count).collect()
    }

    /// Mark a hash as verified at the given timestamp
    pub fn mark_verified(&mut self, hash: &ChunkHash, timestamp: u64) {
        self.last_verified.insert(*hash, timestamp);
    }

    /// Check if a hash was ever verified
    pub fn was_verified(&self, hash: &ChunkHash) -> bool {
        self.last_verified.contains_key(hash)
    }

    /// Get the last verified timestamp for a hash
    pub fn last_verified_at(&self, hash: &ChunkHash) -> Option<u64> {
        self.last_verified.get(hash).copied()
    }
}

/// Create a verification schedule from a list of hashes
pub fn schedule_verification(hashes: Vec<ChunkHash>) -> VerificationSchedule {
    VerificationSchedule {
        hashes,
        last_verified: HashMap::new(),
    }
}

/// Verify a single chunk's integrity.
///
/// Recomputes BLAKE3 hash and compares against expected hash.
pub fn verify_chunk(data: &[u8], expected_hash: &ChunkHash) -> VerificationResult {
    let computed = blake3_hash(data);
    if computed == *expected_hash {
        VerificationResult::Ok
    } else {
        VerificationResult::Corrupted {
            hash: computed,
            expected: *expected_hash,
        }
    }
}

/// Verify multiple chunks in batch.
///
/// Returns a result for each input chunk in order.
pub fn verify_batch(chunks: &[(ChunkHash, Vec<u8>)]) -> Vec<VerificationResult> {
    chunks
        .iter()
        .map(|(expected_hash, data)| verify_chunk(data, expected_hash))
        .collect()
}

/// Background chunk verifier for data integrity.
pub struct ChunkVerifier {
    #[allow(dead_code)]
    config: ChunkVerifierConfig,
    stats: VerificationStats,
}

impl ChunkVerifier {
    /// Create a new verifier with the given configuration
    pub fn new(config: ChunkVerifierConfig) -> Self {
        Self {
            config,
            stats: VerificationStats::default(),
        }
    }

    /// Verify a single chunk and update stats
    pub fn verify(&mut self, hash: ChunkHash, data: &[u8]) -> VerificationResult {
        let result = verify_chunk(data, &hash);
        self.stats.record(&result, data.len() as u64);
        result
    }

    /// Verify a batch of chunks
    pub fn verify_batch(&mut self, chunks: &[(ChunkHash, Vec<u8>)]) -> Vec<VerificationResult> {
        chunks
            .iter()
            .map(|(hash, data)| {
                let result = verify_chunk(data, hash);
                self.stats.record(&result, data.len() as u64);
                result
            })
            .collect()
    }

    /// Get current verification statistics
    pub fn stats(&self) -> &VerificationStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = VerificationStats::default();
    }
}

impl Default for ChunkVerifier {
    fn default() -> Self {
        Self::new(ChunkVerifierConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_hash(data: &[u8]) -> ChunkHash {
        blake3_hash(data)
    }

    #[test]
    fn test_verify_chunk_correct_data() {
        let data = b"hello world";
        let hash = make_test_hash(data);
        let result = verify_chunk(data, &hash);
        assert!(matches!(result, VerificationResult::Ok));
    }

    #[test]
    fn test_verify_chunk_corrupted_data() {
        let original = b"hello world";
        let hash = make_test_hash(original);
        let corrupted = b"hello world!"; // Different data
        let result = verify_chunk(corrupted, &hash);
        match result {
            VerificationResult::Corrupted { expected, .. } => {
                assert_eq!(expected, hash);
            }
            _ => panic!("Expected Corrupted result"),
        }
    }

    #[test]
    fn test_verify_chunk_empty_data() {
        let data = b"";
        let hash = make_test_hash(data);
        let result = verify_chunk(data, &hash);
        assert!(matches!(result, VerificationResult::Ok));
    }

    #[test]
    fn test_verify_batch_mixed_results() {
        let data1 = b"chunk one";
        let data2 = b"chunk two";
        let data3 = b"chunk three";

        let hash1 = make_test_hash(data1);
        let hash2 = make_test_hash(data2);
        let hash3 = make_test_hash(data3);

        // Use wrong data for hash2 to cause corruption
        let chunks = vec![
            (hash1, data1.to_vec()),
            (hash2, b"wrong data".to_vec()),
            (hash3, data3.to_vec()),
        ];

        let results = verify_batch(&chunks);

        assert!(matches!(results[0], VerificationResult::Ok));
        assert!(matches!(results[1], VerificationResult::Corrupted { .. }));
        assert!(matches!(results[2], VerificationResult::Ok));
    }

    #[test]
    fn test_verify_batch_all_ok() {
        let data1 = b"chunk one";
        let data2 = b"chunk two";
        let data3 = b"chunk three";

        let hash1 = make_test_hash(data1);
        let hash2 = make_test_hash(data2);
        let hash3 = make_test_hash(data3);

        let chunks = vec![
            (hash1, data1.to_vec()),
            (hash2, data2.to_vec()),
            (hash3, data3.to_vec()),
        ];

        let results = verify_batch(&chunks);

        assert!(results.iter().all(|r| matches!(r, VerificationResult::Ok)));
    }

    #[test]
    fn test_verify_batch_all_corrupted() {
        let data1 = b"chunk one";
        let data2 = b"chunk two";
        let data3 = b"chunk three";

        let hash1 = make_test_hash(data1);
        let hash2 = make_test_hash(data2);
        let hash3 = make_test_hash(data3);

        // All with wrong data
        let chunks = vec![
            (hash1, b"wrong".to_vec()),
            (hash2, b"wrong".to_vec()),
            (hash3, b"wrong".to_vec()),
        ];

        let results = verify_batch(&chunks);

        assert!(results
            .iter()
            .all(|r| matches!(r, VerificationResult::Corrupted { .. })));
    }

    #[test]
    fn test_schedule_verification_creates_schedule() {
        let hashes = vec![
            make_test_hash(b"one"),
            make_test_hash(b"two"),
            make_test_hash(b"three"),
        ];

        let schedule = schedule_verification(hashes.clone());

        assert_eq!(schedule.len(), 3);
        assert!(!schedule.is_empty());
    }

    #[test]
    fn test_next_batch_returns_up_to_n_items() {
        let hashes: Vec<ChunkHash> = (0..10)
            .map(|i| make_test_hash(format!("chunk {}", i).as_bytes()))
            .collect();

        let mut schedule = schedule_verification(hashes);

        let batch = schedule.next_batch(5);
        assert_eq!(batch.len(), 5);
        assert_eq!(schedule.len(), 5); // 10 - 5 remaining
    }

    #[test]
    fn test_next_batch_empty_schedule() {
        let mut schedule = VerificationSchedule::new();
        let batch = schedule.next_batch(5);
        assert!(batch.is_empty());
        assert!(schedule.is_empty());
    }

    #[test]
    fn test_verification_stats_default_values() {
        let stats = VerificationStats::default();
        assert_eq!(stats.chunks_verified, 0);
        assert_eq!(stats.chunks_ok, 0);
        assert_eq!(stats.chunks_corrupted, 0);
        assert_eq!(stats.chunks_missing, 0);
        assert_eq!(stats.bytes_verified, 0);
    }

    #[test]
    fn test_stats_accumulation() {
        let mut stats = VerificationStats::new();

        stats.record(&VerificationResult::Ok, 100);
        stats.record(
            &VerificationResult::Corrupted {
                hash: ChunkHash([0; 32]),
                expected: ChunkHash([1; 32]),
            },
            200,
        );
        stats.record(&VerificationResult::Ok, 300);

        assert_eq!(stats.chunks_verified, 3);
        assert_eq!(stats.chunks_ok, 2);
        assert_eq!(stats.chunks_corrupted, 1);
        assert_eq!(stats.bytes_verified, 600);
    }

    #[test]
    fn test_chunk_verifier_config_default() {
        let config = ChunkVerifierConfig::default();
        assert_eq!(config.batch_size, 64);
        assert_eq!(config.interval_secs, 3600);
        assert_eq!(config.priority, VerificationPriority::Normal);
    }

    #[test]
    fn test_verification_result_debug_formatting() {
        let ok_result = VerificationResult::Ok;
        let debug_str = format!("{:?}", ok_result);
        assert!(debug_str.contains("Ok"));

        let corrupted = VerificationResult::Corrupted {
            hash: ChunkHash([0; 32]),
            expected: ChunkHash([1; 32]),
        };
        let debug_str = format!("{:?}", corrupted);
        assert!(debug_str.contains("Corrupted"));
    }

    #[test]
    fn test_large_batch_verification() {
        let chunks: Vec<(ChunkHash, Vec<u8>)> = (0..150)
            .map(|i| {
                let data = vec![i as u8; 100];
                let hash = make_test_hash(&data);
                (hash, data)
            })
            .collect();

        let results = verify_batch(&chunks);
        assert_eq!(results.len(), 150);
        assert!(results.iter().all(|r| matches!(r, VerificationResult::Ok)));
    }

    #[test]
    fn test_duplicate_hash_in_schedule() {
        let hash = make_test_hash(b"duplicate");
        let hashes = vec![hash, hash, hash];

        let schedule = schedule_verification(hashes);
        assert_eq!(schedule.len(), 3); // Duplicates allowed
    }
}
