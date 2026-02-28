//! CAS fingerprint index for content-addressable deduplication.
//!
//! This module stores BLAKE3 content hashes and maps them to block locations
//! for the A3 data reduction pipeline. When a new chunk is written, A3 checks
//! this index to determine if the content already exists (deduplication).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// An entry in the fingerprint index storing hash metadata and location.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FingerprintEntry {
    /// BLAKE3 content hash (32 bytes).
    pub hash: [u8; 32],
    /// Number of references to this content.
    pub ref_count: u64,
    /// Block location on storage device.
    pub block_location: u64,
    /// Size of the content in bytes.
    pub size: u64,
    /// Timestamp when entry was created.
    pub created_at: Timestamp,
}

/// Manages the CAS fingerprint index for content-addressable storage.
pub struct FingerprintIndex {
    /// Hash map from content hash to fingerprint entry.
    entries: RwLock<HashMap<[u8; 32], FingerprintEntry>>,
}

impl FingerprintIndex {
    /// Creates a new empty fingerprint index.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Looks up a fingerprint entry by hash.
    ///
    /// # Arguments
    /// * `hash` - The content hash to look up
    ///
    /// # Returns
    /// The fingerprint entry if found
    pub fn lookup(&self, hash: &[u8; 32]) -> Option<FingerprintEntry> {
        let entries = self.entries.read().unwrap();
        entries.get(hash).cloned()
    }

    /// Inserts a new fingerprint entry or increments ref count if exists.
    ///
    /// # Arguments
    /// * `hash` - The content hash
    /// * `block_location` - The block location on storage
    /// * `size` - The size of the content
    ///
    /// # Returns
    /// Ok(true) if new entry created, Ok(false) if already existed
    pub fn insert(
        &self,
        hash: [u8; 32],
        block_location: u64,
        size: u64,
    ) -> Result<bool, MetaError> {
        let mut entries = self.entries.write().unwrap();

        if let Some(entry) = entries.get_mut(&hash) {
            entry.ref_count = entry.ref_count.saturating_add(1);
            Ok(false)
        } else {
            let entry = FingerprintEntry {
                hash,
                ref_count: 1,
                block_location,
                size,
                created_at: Timestamp::now(),
            };
            entries.insert(hash, entry);
            Ok(true)
        }
    }

    /// Increments the reference count for an existing hash.
    ///
    /// # Arguments
    /// * `hash` - The content hash
    ///
    /// # Returns
    /// Ok(()) on success, Err if not found
    pub fn increment_ref(&self, hash: &[u8; 32]) -> Result<(), MetaError> {
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .get_mut(hash)
            .ok_or_else(|| MetaError::KvError(format!("hash not found: {:?}", &hash[..8])))?;

        entry.ref_count = entry.ref_count.saturating_add(1);
        Ok(())
    }

    /// Decrements the reference count for a hash.
    ///
    /// # Arguments
    /// * `hash` - The content hash
    ///
    /// # Returns
    /// Ok(new_ref_count) after decrement, Err if not found
    pub fn decrement_ref(&self, hash: &[u8; 32]) -> Result<u64, MetaError> {
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .get_mut(hash)
            .ok_or_else(|| MetaError::KvError(format!("hash not found: {:?}", &hash[..8])))?;

        entry.ref_count = entry.ref_count.saturating_sub(1);
        let new_count = entry.ref_count;

        if new_count == 0 {
            entries.remove(hash);
        }

        Ok(new_count)
    }

    /// Checks if a hash exists in the index.
    ///
    /// # Arguments
    /// * `hash` - The content hash to check
    ///
    /// # Returns
    /// True if the hash exists
    pub fn contains(&self, hash: &[u8; 32]) -> bool {
        let entries = self.entries.read().unwrap();
        entries.contains_key(hash)
    }

    /// Returns the number of entries in the index.
    pub fn entry_count(&self) -> usize {
        let entries = self.entries.read().unwrap();
        entries.len()
    }

    /// Returns total bytes saved through deduplication.
    ///
    /// This sums size * (ref_count - 1) for entries with ref_count > 1.
    pub fn total_deduplicated_bytes(&self) -> u64 {
        let entries = self.entries.read().unwrap();
        entries
            .values()
            .filter(|e| e.ref_count > 1)
            .map(|e| e.size * (e.ref_count - 1))
            .sum()
    }

    /// Removes all entries with zero reference count.
    ///
    /// # Returns
    /// Number of entries removed
    pub fn garbage_collect(&self) -> usize {
        let mut entries = self.entries.write().unwrap();
        let before = entries.len();
        entries.retain(|_, v| v.ref_count > 0);
        let after = entries.len();
        before - after
    }
}

impl Default for FingerprintIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(i: u8) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash[0] = i;
        hash
    }

    #[test]
    fn test_insert_new_hash() {
        let index = FingerprintIndex::new();
        let hash = make_hash(1);

        let result = index.insert(hash, 1000, 4096);
        assert!(result.expect("insert failed"));
        assert!(index.contains(&hash));
        assert_eq!(index.entry_count(), 1);
    }

    #[test]
    fn test_insert_duplicate_increments_ref() {
        let index = FingerprintIndex::new();
        let hash = make_hash(2);

        index.insert(hash, 1000, 4096).expect("insert failed");
        let result = index.insert(hash, 2000, 4096).expect("insert failed");

        assert!(!result);

        let entry = index.lookup(&hash).expect("lookup failed");
        assert_eq!(entry.ref_count, 2);
        assert_eq!(entry.block_location, 1000);
    }

    #[test]
    fn test_lookup() {
        let index = FingerprintIndex::new();
        let hash = make_hash(3);

        index.insert(hash, 5000, 8192).expect("insert failed");

        let entry = index.lookup(&hash).expect("entry not found");
        assert_eq!(entry.block_location, 5000);
        assert_eq!(entry.size, 8192);
        assert_eq!(entry.ref_count, 1);
    }

    #[test]
    fn test_lookup_nonexistent() {
        let index = FingerprintIndex::new();
        let hash = make_hash(4);

        let result = index.lookup(&hash);
        assert!(result.is_none());
    }

    #[test]
    fn test_increment_ref() {
        let index = FingerprintIndex::new();
        let hash = make_hash(5);

        index.insert(hash, 1000, 4096).expect("insert failed");
        index.increment_ref(&hash).expect("increment failed");

        let entry = index.lookup(&hash).expect("lookup failed");
        assert_eq!(entry.ref_count, 2);
    }

    #[test]
    fn test_increment_ref_not_found() {
        let index = FingerprintIndex::new();
        let hash = make_hash(6);

        let result = index.increment_ref(&hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrement_ref() {
        let index = FingerprintIndex::new();
        let hash = make_hash(7);

        index.insert(hash, 1000, 4096).expect("insert failed");
        index.increment_ref(&hash).expect("increment failed");

        let new_count = index.decrement_ref(&hash).expect("decrement failed");
        assert_eq!(new_count, 1);

        let entry = index.lookup(&hash).expect("lookup failed");
        assert_eq!(entry.ref_count, 1);
    }

    #[test]
    fn test_decrement_ref_removes_at_zero() {
        let index = FingerprintIndex::new();
        let hash = make_hash(8);

        index.insert(hash, 1000, 4096).expect("insert failed");

        let new_count = index.decrement_ref(&hash).expect("decrement failed");
        assert_eq!(new_count, 0);
        assert!(!index.contains(&hash));
    }

    #[test]
    fn test_decrement_ref_not_found() {
        let index = FingerprintIndex::new();
        let hash = make_hash(9);

        let result = index.decrement_ref(&hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_total_deduplicated_bytes() {
        let index = FingerprintIndex::new();

        index
            .insert(make_hash(1), 1000, 4096)
            .expect("insert failed");
        index
            .insert(make_hash(1), 1000, 4096)
            .expect("insert failed");

        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");
        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");
        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");

        index
            .insert(make_hash(3), 3000, 1024)
            .expect("insert failed");

        let deduped = index.total_deduplicated_bytes();
        assert_eq!(deduped, 4096 + 16384);
    }

    #[test]
    fn test_garbage_collect() {
        let index = FingerprintIndex::new();

        let hash1 = make_hash(1);
        let hash2 = make_hash(2);

        index.insert(hash1, 1000, 4096).expect("insert failed");
        index.insert(hash2, 2000, 4096).expect("insert failed");
        index.insert(hash2, 2000, 4096).expect("insert failed");

        index.decrement_ref(&hash2).expect("decrement failed");

        let removed = index.garbage_collect();
        assert_eq!(removed, 0);
        assert_eq!(index.entry_count(), 2);
    }

    #[test]
    fn test_multiple_hashes() {
        let index = FingerprintIndex::new();

        for i in 0..100u8 {
            let hash = make_hash(i);
            index
                .insert(hash, (i as u64) * 1000, 4096)
                .expect("insert failed");
        }

        assert_eq!(index.entry_count(), 100);
    }

    #[test]
    fn test_contains() {
        let index = FingerprintIndex::new();
        let hash = make_hash(10);

        assert!(!index.contains(&hash));

        index.insert(hash, 1000, 4096).expect("insert failed");

        assert!(index.contains(&hash));
    }
}
