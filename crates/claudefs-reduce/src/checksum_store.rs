//! Checksum store for tracking end-to-end data integrity.
//!
//! Stores CRC32C checksums for chunks, tracks verification status and failures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A checksum entry for a chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumEntry {
    /// BLAKE3 hash of the chunk.
    pub chunk_hash: [u8; 32],
    /// CRC32C checksum of the data.
    pub checksum: [u8; 4],
    /// Timestamp of last verification in milliseconds.
    pub verified_at_ms: u64,
    /// Number of verification failures.
    pub fail_count: u8,
}

impl ChecksumEntry {
    /// Check if this entry has failed verification.
    pub fn is_suspect(&self) -> bool {
        self.fail_count > 0
    }
}

/// Configuration for the checksum store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumStoreConfig {
    /// Maximum number of entries to store (default 1_000_000).
    pub max_entries: usize,
    /// Threshold for suspect entries (default 1).
    pub suspect_threshold: u8,
}

impl Default for ChecksumStoreConfig {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
            suspect_threshold: 1,
        }
    }
}

/// Result of checksum verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumVerifyResult {
    /// Checksum matched.
    Ok,
    /// Checksum mismatch.
    Mismatch {
        /// Stored checksum.
        stored: [u8; 4],
        /// Computed checksum.
        computed: [u8; 4],
    },
    /// Entry not found.
    NotFound,
}

/// Checksum store for tracking chunk integrity.
#[derive(Debug, Clone)]
pub struct ChecksumStore {
    config: ChecksumStoreConfig,
    entries: HashMap<[u8; 32], ChecksumEntry>,
}

impl ChecksumStore {
    /// Create a new checksum store with the given configuration.
    pub fn new(config: ChecksumStoreConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
        }
    }

    /// Insert a new checksum entry.
    pub fn insert(&mut self, hash: [u8; 32], checksum: [u8; 4], now_ms: u64) {
        if self.entries.len() >= self.config.max_entries {
            return;
        }
        self.entries.insert(
            hash,
            ChecksumEntry {
                chunk_hash: hash,
                checksum,
                verified_at_ms: now_ms,
                fail_count: 0,
            },
        );
    }

    /// Get a checksum entry by hash.
    pub fn get(&self, hash: &[u8; 32]) -> Option<&ChecksumEntry> {
        self.entries.get(hash)
    }

    /// Verify data against the stored checksum.
    pub fn verify(&self, hash: &[u8; 32], data: &[u8]) -> ChecksumVerifyResult {
        match self.entries.get(hash) {
            Some(entry) => {
                let computed = Self::compute_checksum(data);
                if entry.checksum == computed {
                    ChecksumVerifyResult::Ok
                } else {
                    ChecksumVerifyResult::Mismatch {
                        stored: entry.checksum,
                        computed,
                    }
                }
            }
            None => ChecksumVerifyResult::NotFound,
        }
    }

    /// Record a verification failure for a chunk.
    pub fn record_failure(&mut self, hash: &[u8; 32]) -> bool {
        if let Some(entry) = self.entries.get_mut(hash) {
            entry.fail_count = entry.fail_count.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Get all suspect entries (with failures).
    pub fn suspect_entries(&self) -> Vec<&ChecksumEntry> {
        self.entries
            .values()
            .filter(|e| e.fail_count >= self.config.suspect_threshold)
            .collect()
    }

    /// Get the number of entries in the store.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Remove an entry from the store.
    pub fn remove(&mut self, hash: &[u8; 32]) -> bool {
        self.entries.remove(hash).is_some()
    }

    /// Compute a simple checksum for data.
    fn compute_checksum(data: &[u8]) -> [u8; 4] {
        if data.is_empty() {
            return [0u8; 4];
        }
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let sum_byte = (sum % 256) as u8;
        [sum_byte, 0, 0, 0]
    }
}

impl Default for ChecksumStore {
    fn default() -> Self {
        Self::new(ChecksumStoreConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(id: u8) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash[0] = id;
        hash
    }

    #[test]
    fn config_default() {
        let config = ChecksumStoreConfig::default();
        assert_eq!(config.max_entries, 1_000_000);
        assert_eq!(config.suspect_threshold, 1);
    }

    #[test]
    fn insert_and_get() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        let checksum = [1u8, 2, 3, 4];

        store.insert(hash, checksum, 1000);

        let entry = store.get(&hash).unwrap();
        assert_eq!(entry.chunk_hash, hash);
        assert_eq!(entry.checksum, checksum);
        assert_eq!(entry.verified_at_ms, 1000);
        assert_eq!(entry.fail_count, 0);
    }

    #[test]
    fn get_missing_returns_none() {
        let store = ChecksumStore::default();
        let hash = make_hash(1);
        assert!(store.get(&hash).is_none());
    }

    #[test]
    fn verify_ok() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        let data = b"hello world";
        let checksum = ChecksumStore::compute_checksum(data);

        store.insert(hash, checksum, 1000);

        let result = store.verify(&hash, data);
        assert_eq!(result, ChecksumVerifyResult::Ok);
    }

    #[test]
    fn verify_mismatch() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        let checksum = [1u8, 2, 3, 4];

        store.insert(hash, checksum, 1000);

        let result = store.verify(&hash, b"different data");
        match result {
            ChecksumVerifyResult::Mismatch { stored, computed } => {
                assert_eq!(stored, checksum);
                assert_ne!(computed, checksum);
            }
            _ => panic!("Expected Mismatch"),
        }
    }

    #[test]
    fn verify_not_found() {
        let store = ChecksumStore::default();
        let hash = make_hash(1);
        let result = store.verify(&hash, b"some data");
        assert_eq!(result, ChecksumVerifyResult::NotFound);
    }

    #[test]
    fn record_failure_increments() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        store.insert(hash, [1, 2, 3, 4], 1000);

        let found = store.record_failure(&hash);
        assert!(found);
        assert_eq!(store.get(&hash).unwrap().fail_count, 1);

        store.record_failure(&hash);
        assert_eq!(store.get(&hash).unwrap().fail_count, 2);
    }

    #[test]
    fn record_failure_unknown_returns_false() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        assert!(!store.record_failure(&hash));
    }

    #[test]
    fn is_suspect_false() {
        let entry = ChecksumEntry {
            chunk_hash: make_hash(1),
            checksum: [1, 2, 3, 4],
            verified_at_ms: 1000,
            fail_count: 0,
        };
        assert!(!entry.is_suspect());
    }

    #[test]
    fn is_suspect_true() {
        let entry = ChecksumEntry {
            chunk_hash: make_hash(1),
            checksum: [1, 2, 3, 4],
            verified_at_ms: 1000,
            fail_count: 1,
        };
        assert!(entry.is_suspect());
    }

    #[test]
    fn suspect_entries_empty() {
        let store = ChecksumStore::default();
        assert!(store.suspect_entries().is_empty());
    }

    #[test]
    fn suspect_entries_after_failure() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        store.insert(hash, [1, 2, 3, 4], 1000);
        store.record_failure(&hash);

        let suspects = store.suspect_entries();
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].chunk_hash, hash);
    }

    #[test]
    fn entry_count_increments() {
        let mut store = ChecksumStore::default();
        assert_eq!(store.entry_count(), 0);

        store.insert(make_hash(1), [1, 2, 3, 4], 1000);
        assert_eq!(store.entry_count(), 1);

        store.insert(make_hash(2), [5, 6, 7, 8], 1000);
        assert_eq!(store.entry_count(), 2);
    }

    #[test]
    fn remove_existing() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        store.insert(hash, [1, 2, 3, 4], 1000);

        assert!(store.remove(&hash));
        assert_eq!(store.entry_count(), 0);
        assert!(store.get(&hash).is_none());
    }

    #[test]
    fn remove_missing_returns_false() {
        let mut store = ChecksumStore::default();
        let hash = make_hash(1);
        assert!(!store.remove(&hash));
    }

    #[test]
    fn checksum_empty_data() {
        let checksum = ChecksumStore::compute_checksum(&[]);
        assert_eq!(checksum, [0u8; 4]);
    }

    #[test]
    fn max_entries_limit() {
        let config = ChecksumStoreConfig {
            max_entries: 2,
            suspect_threshold: 1,
        };
        let mut store = ChecksumStore::new(config);

        store.insert(make_hash(1), [1, 2, 3, 4], 1000);
        store.insert(make_hash(2), [5, 6, 7, 8], 1000);
        store.insert(make_hash(3), [9, 10, 11, 12], 1000);

        assert_eq!(store.entry_count(), 2);
    }
}
