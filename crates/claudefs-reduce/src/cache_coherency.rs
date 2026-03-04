//! Cache coherency tracking for multi-level caching in ClaudeFS.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Identifies a cached chunk by inode and chunk index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// Inode ID of the file
    pub inode_id: u64,
    /// Index of the chunk within the file
    pub chunk_index: u64,
}

/// Monotonically increasing version per cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheVersion {
    /// Version number
    pub version: u64,
}

impl CacheVersion {
    /// Create a new version starting at 0.
    pub fn new() -> Self {
        Self { version: 0 }
    }

    /// Increment the version number.
    pub fn increment(&self) -> Self {
        Self {
            version: self.version + 1,
        }
    }
}

impl Default for CacheVersion {
    fn default() -> Self {
        Self::new()
    }
}

/// A cache entry with key, version, validity, and size.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Cache key identifying this entry
    pub key: CacheKey,
    /// Current version of the entry
    pub version: CacheVersion,
    /// Whether the entry is valid
    pub valid: bool,
    /// Size of the cached data in bytes
    pub size_bytes: u64,
}

/// Invalidation event types.
#[derive(Debug, Clone)]
pub enum InvalidationEvent {
    /// A specific chunk has been invalidated
    ChunkInvalidated {
        /// Key of the invalidated chunk
        key: CacheKey,
    },
    /// All chunks for an inode have been invalidated
    InodeInvalidated {
        /// Inode ID
        inode_id: u64,
    },
    /// All entries have been invalidated (full cache flush)
    AllInvalidated,
}

/// Tracks cache coherency for multi-level caching.
pub struct CoherencyTracker {
    entries: HashMap<CacheKey, CacheEntry>,
}

impl CoherencyTracker {
    /// Create a new empty coherency tracker.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a new cache entry or update an existing one.
    pub fn register(&mut self, key: CacheKey, version: CacheVersion, size_bytes: u64) {
        self.entries.insert(
            key,
            CacheEntry {
                key,
                version,
                valid: true,
                size_bytes,
            },
        );
    }

    /// Apply an invalidation event and return the keys of invalidated entries.
    pub fn invalidate(&mut self, event: &InvalidationEvent) -> Vec<CacheKey> {
        let invalidated: Vec<CacheKey> = match event {
            InvalidationEvent::ChunkInvalidated { key } => {
                if let Some(entry) = self.entries.get_mut(key) {
                    entry.valid = false;
                    vec![*key]
                } else {
                    vec![]
                }
            }
            InvalidationEvent::InodeInvalidated { inode_id } => self
                .entries
                .iter_mut()
                .filter(|(k, e)| k.inode_id == *inode_id && e.valid)
                .map(|(k, e)| {
                    e.valid = false;
                    *k
                })
                .collect(),
            InvalidationEvent::AllInvalidated => {
                let keys: Vec<CacheKey> = self
                    .entries
                    .iter()
                    .filter(|(_, e)| e.valid)
                    .map(|(k, _)| *k)
                    .collect();
                for entry in self.entries.values_mut() {
                    entry.valid = false;
                }
                keys
            }
        };
        invalidated
    }

    /// Check if an entry is valid with the given version.
    pub fn is_valid(&self, key: &CacheKey, version: &CacheVersion) -> bool {
        self.entries
            .get(key)
            .map(|e| e.valid && e.version == *version)
            .unwrap_or(false)
    }

    /// Get the current version for a key, if it exists.
    pub fn get_version(&self, key: &CacheKey) -> Option<CacheVersion> {
        self.entries.get(key).map(|e| e.version)
    }

    /// Increment the version for a key, creating it if missing.
    pub fn bump_version(&mut self, key: &CacheKey) -> CacheVersion {
        let entry = self.entries.entry(*key).or_insert(CacheEntry {
            key: *key,
            version: CacheVersion::new(),
            valid: false,
            size_bytes: 0,
        });
        entry.version = entry.version.increment();
        entry.version
    }

    /// Count valid entries.
    pub fn valid_entry_count(&self) -> usize {
        self.entries.values().filter(|e| e.valid).count()
    }

    /// Sum of valid entry sizes.
    pub fn total_valid_bytes(&self) -> u64 {
        self.entries
            .values()
            .filter(|e| e.valid)
            .map(|e| e.size_bytes)
            .sum()
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for CoherencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_equality() {
        let k1 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let k2 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let k3 = CacheKey {
            inode_id: 2,
            chunk_index: 0,
        };
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn cache_version_ordering() {
        let v0 = CacheVersion::new();
        let v1 = v0.increment();
        let v2 = v1.increment();
        assert!(v0 < v1);
        assert!(v1 < v2);
        assert!(v0 < v2);
    }

    #[test]
    fn cache_version_increment() {
        let v0 = CacheVersion::new();
        assert_eq!(v0.version, 0);
        let v1 = v0.increment();
        assert_eq!(v1.version, 1);
        let v2 = v1.increment();
        assert_eq!(v2.version, 2);
    }

    #[test]
    fn register_entry() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key, version, 4096);
        assert!(tracker.get_version(&key).is_some());
    }

    #[test]
    fn is_valid_true() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key, version, 4096);
        assert!(tracker.is_valid(&key, &version));
    }

    #[test]
    fn is_valid_false_wrong_version() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key, version, 4096);
        let stale_version = CacheVersion { version: 100 };
        assert!(!tracker.is_valid(&key, &stale_version));
    }

    #[test]
    fn is_valid_false_missing_key() {
        let tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        assert!(!tracker.is_valid(&key, &version));
    }

    #[test]
    fn invalidate_chunk() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key, version, 4096);
        let invalidated = tracker.invalidate(&InvalidationEvent::ChunkInvalidated { key });
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0], key);
        assert_eq!(tracker.valid_entry_count(), 0);
    }

    #[test]
    fn invalidate_inode() {
        let mut tracker = CoherencyTracker::new();
        let key1 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let key2 = CacheKey {
            inode_id: 1,
            chunk_index: 1,
        };
        let key3 = CacheKey {
            inode_id: 2,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key1, version, 4096);
        tracker.register(key2, version, 4096);
        tracker.register(key3, version, 4096);
        let invalidated = tracker.invalidate(&InvalidationEvent::InodeInvalidated { inode_id: 1 });
        assert_eq!(invalidated.len(), 2);
        assert_eq!(tracker.valid_entry_count(), 1);
    }

    #[test]
    fn invalidate_all() {
        let mut tracker = CoherencyTracker::new();
        let key1 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let key2 = CacheKey {
            inode_id: 2,
            chunk_index: 0,
        };
        let version = CacheVersion::new();
        tracker.register(key1, version, 4096);
        tracker.register(key2, version, 4096);
        let invalidated = tracker.invalidate(&InvalidationEvent::AllInvalidated);
        assert_eq!(invalidated.len(), 2);
        assert_eq!(tracker.valid_entry_count(), 0);
    }

    #[test]
    fn invalidate_returns_affected_keys() {
        let mut tracker = CoherencyTracker::new();
        let key1 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let key2 = CacheKey {
            inode_id: 1,
            chunk_index: 1,
        };
        let version = CacheVersion::new();
        tracker.register(key1, version, 4096);
        tracker.register(key2, version, 4096);
        let invalidated = tracker.invalidate(&InvalidationEvent::ChunkInvalidated { key: key1 });
        assert_eq!(invalidated, vec![key1]);
    }

    #[test]
    fn bump_version_increments() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        tracker.register(key, CacheVersion::new(), 4096);
        let new_version = tracker.bump_version(&key);
        assert_eq!(new_version.version, 1);
    }

    #[test]
    fn bump_version_creates_if_missing() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let version = tracker.bump_version(&key);
        assert_eq!(version.version, 1);
        assert!(tracker.get_version(&key).is_some());
    }

    #[test]
    fn valid_entry_count_after_register() {
        let mut tracker = CoherencyTracker::new();
        assert_eq!(tracker.valid_entry_count(), 0);
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        tracker.register(key, CacheVersion::new(), 4096);
        assert_eq!(tracker.valid_entry_count(), 1);
    }

    #[test]
    fn valid_entry_count_after_invalidate() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        tracker.register(key, CacheVersion::new(), 4096);
        assert_eq!(tracker.valid_entry_count(), 1);
        tracker.invalidate(&InvalidationEvent::ChunkInvalidated { key });
        assert_eq!(tracker.valid_entry_count(), 0);
    }

    #[test]
    fn total_valid_bytes() {
        let mut tracker = CoherencyTracker::new();
        let key1 = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        let key2 = CacheKey {
            inode_id: 1,
            chunk_index: 1,
        };
        tracker.register(key1, CacheVersion::new(), 4096);
        tracker.register(key2, CacheVersion::new(), 8192);
        assert_eq!(tracker.total_valid_bytes(), 12288);
    }

    #[test]
    fn clear_removes_all() {
        let mut tracker = CoherencyTracker::new();
        let key = CacheKey {
            inode_id: 1,
            chunk_index: 0,
        };
        tracker.register(key, CacheVersion::new(), 4096);
        tracker.clear();
        assert_eq!(tracker.valid_entry_count(), 0);
        assert_eq!(tracker.total_valid_bytes(), 0);
    }
}
