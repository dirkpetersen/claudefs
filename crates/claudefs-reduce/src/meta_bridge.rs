//! Distributed fingerprint index bridge for A2 metadata service.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Location of a block in the distributed storage system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockLocation {
    /// Node ID where the block is stored
    pub node_id: u64,
    /// Byte offset on the block device
    pub block_offset: u64,
    /// Size of the block in bytes
    pub size: u64,
}

/// Trait for fingerprint index used by distributed metadata.
/// Object-safe and sync-capable for cross-component integration.
pub trait FingerprintStore: Send + Sync {
    /// Lookup a fingerprint, returning its block location if found.
    fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation>;

    /// Insert a new fingerprint-location pair.
    /// Returns true if this was a new entry, false if it already existed.
    fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool;

    /// Increment reference count for an existing entry.
    /// Returns true if the entry existed and was incremented.
    fn increment_ref(&self, hash: &[u8; 32]) -> bool;

    /// Decrement reference count for an entry.
    /// Returns the new refcount, or None if entry not found.
    fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64>;

    /// Total number of entries in the store.
    fn entry_count(&self) -> usize;
}

/// In-memory fingerprint store using RwLock for thread-safe access.
pub struct LocalFingerprintStore {
    entries: RwLock<HashMap<[u8; 32], (BlockLocation, u64)>>,
}

impl LocalFingerprintStore {
    /// Create a new empty local fingerprint store.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Calculate total bytes stored via deduplicated chunks.
    pub async fn total_deduplicated_bytes(&self) -> u64 {
        let entries = self.entries.read().await;
        entries.values()
            .filter(|(_, count)| *count > 1)
            .map(|(loc, count)| loc.size * (count - 1))
            .sum()
    }

    /// Synchronous version for use cases that don't need async.
    pub fn total_deduplicated_bytes_sync(&self) -> u64 {
        let entries = self.entries.blocking_read();
        entries.values()
            .filter(|(_, count)| *count > 1)
            .map(|(loc, count)| loc.size * (count - 1))
            .sum()
    }
}

impl Default for LocalFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FingerprintStore for LocalFingerprintStore {
    fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation> {
        let entries = self.entries.blocking_read();
        entries.get(hash).map(|(loc, _)| *loc)
    }

    fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool {
        let mut entries = self.entries.blocking_write();
        if entries.contains_key(&hash) {
            if let Some((_, refs)) = entries.get_mut(&hash) {
                *refs += 1;
            }
            false
        } else {
            entries.insert(hash, (location, 1));
            debug!(node_id = location.node_id, offset = location.block_offset, "Inserted new fingerprint");
            true
        }
    }

    fn increment_ref(&self, hash: &[u8; 32]) -> bool {
        let mut entries = self.entries.blocking_write();
        if let Some((_, refs)) = entries.get_mut(hash) {
            *refs += 1;
            debug!(hash = ?hash, refs = *refs, "Incremented refcount");
            true
        } else {
            false
        }
    }

    fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64> {
        let mut entries = self.entries.blocking_write();
        if let Some((_, refs)) = entries.get_mut(hash) {
            if *refs > 0 {
                *refs -= 1;
                debug!(hash = ?hash, refs = *refs, "Decremented refcount");
                Some(*refs)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn entry_count(&self) -> usize {
        self.entries.blocking_read().len()
    }
}

/// No-op fingerprint store for testing or when distributed dedup is disabled.
pub struct NullFingerprintStore;

impl NullFingerprintStore {
    /// Create a new null fingerprint store.
    pub fn new() -> Self {
        Self
    }
}

impl Default for NullFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FingerprintStore for NullFingerprintStore {
    fn lookup(&self, _hash: &[u8; 32]) -> Option<BlockLocation> {
        None
    }

    fn insert(&self, _hash: [u8; 32], _location: BlockLocation) -> bool {
        true
    }

    fn increment_ref(&self, _hash: &[u8; 32]) -> bool {
        false
    }

    fn decrement_ref(&self, _hash: &[u8; 32]) -> Option<u64> {
        None
    }

    fn entry_count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_location_equality() {
        let loc1 = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        let loc2 = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        let loc3 = BlockLocation { node_id: 2, block_offset: 100, size: 4096 };
        assert_eq!(loc1, loc2);
        assert_ne!(loc1, loc3);
    }

    #[test]
    fn test_local_store_lookup_insert() {
        let store = LocalFingerprintStore::new();
        let hash = [0u8; 32];
        let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        assert!(store.lookup(&hash).is_none());
        
        let is_new = store.insert(hash, location);
        assert!(is_new);
        
        assert_eq!(store.lookup(&hash), Some(location));
    }

    #[test]
    fn test_local_store_insert_existing() {
        let store = LocalFingerprintStore::new();
        let hash = [0u8; 32];
        let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        store.insert(hash, location);
        let is_new = store.insert(hash, location);
        assert!(!is_new); // Already existed
    }

    #[test]
    fn test_local_store_ref_counts() {
        let store = LocalFingerprintStore::new();
        let hash = [0u8; 32];
        let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        store.insert(hash, location);
        
        assert!(store.increment_ref(&hash));
        assert!(store.increment_ref(&hash));
        
        assert_eq!(store.decrement_ref(&hash), Some(2));
        assert_eq!(store.decrement_ref(&hash), Some(1));
        assert_eq!(store.decrement_ref(&hash), Some(0));
        
        // Decrementing at 0 returns None since entry no longer exists
        assert_eq!(store.decrement_ref(&hash), None);
    }

    #[test]
    fn test_local_store_entry_count() {
        let store = LocalFingerprintStore::new();
        let loc = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        assert_eq!(store.entry_count(), 0);
        
        store.insert([1u8; 32], loc);
        assert_eq!(store.entry_count(), 1);
        
        store.insert([2u8; 32], loc);
        assert_eq!(store.entry_count(), 2);
        
        // Inserting existing doesn't increase count
        store.insert([1u8; 32], loc);
        assert_eq!(store.entry_count(), 2);
    }

    #[test]
    fn test_local_store_total_deduplicated_bytes() {
        let store = LocalFingerprintStore::new();
        let loc1 = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        let loc2 = BlockLocation { node_id: 1, block_offset: 200, size: 8192 };
        
        store.insert([1u8; 32], loc1);
        store.insert([1u8; 32], loc1);  // refcount now 2, saves 1 * 4096 = 4096 bytes
        
        store.insert([2u8; 32], loc2);
        store.insert([2u8; 32], loc2);
        store.insert([2u8; 32], loc2);  // refcount now 3, saves 2 * 8192 = 16384 bytes
        
        assert_eq!(store.total_deduplicated_bytes_sync(), 4096 + 16384);  // 20480
    }

    #[test]
    fn test_null_store_always_returns_none() {
        let store = NullFingerprintStore::new();
        let hash = [0u8; 32];
        let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        assert!(store.lookup(&hash).is_none());
        assert!(store.lookup(&[1u8; 32]).is_none());
    }

    #[test]
    fn test_null_store_always_new() {
        let store = NullFingerprintStore::new();
        let hash = [0u8; 32];
        let location = BlockLocation { node_id: 1, block_offset: 100, size: 4096 };
        
        // Null store always claims to be new (for dedup purposes)
        assert!(store.insert(hash, location));
        assert!(store.insert(hash, location));
    }
}
