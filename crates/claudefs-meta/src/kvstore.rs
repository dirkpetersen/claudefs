//! Embedded key-value store for metadata persistence.
//!
//! Phase 1 implementation uses an in-memory BTreeMap. Future phases will
//! use A1's NVMe block store with atomic writes for crash consistency (D10).

use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::{Arc, RwLock};

use crate::types::MetaError;

/// Key type for the KV store.
pub type Key = Vec<u8>;
/// Value type for the KV store.
pub type Value = Vec<u8>;
/// A key-value pair.
pub type KvPair = (Key, Value);

/// Key-value store trait for metadata persistence.
///
/// This trait abstracts over the storage backend, allowing the metadata
/// service to use an in-memory store for testing and a NVMe-backed store
/// for production.
pub trait KvStore: Send + Sync {
    /// Get a value by key. Returns None if the key doesn't exist.
    fn get(&self, key: &[u8]) -> Result<Option<Value>, MetaError>;

    /// Put a key-value pair. Overwrites any existing value.
    fn put(&self, key: Key, value: Value) -> Result<(), MetaError>;

    /// Delete a key. Returns Ok(()) even if the key didn't exist.
    fn delete(&self, key: &[u8]) -> Result<(), MetaError>;

    /// Scan all keys with the given prefix, returning (key, value) pairs in sorted order.
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KvPair>, MetaError>;

    /// Scan a range of keys [start, end), returning (key, value) pairs in sorted order.
    fn scan_range(&self, start: &[u8], end: &[u8]) -> Result<Vec<KvPair>, MetaError>;

    /// Returns true if the key exists.
    fn contains_key(&self, key: &[u8]) -> Result<bool, MetaError>;

    /// Atomically write a batch of operations (puts and deletes).
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError>;
}

/// A single operation in a write batch.
pub enum BatchOp {
    /// Put a key-value pair.
    Put {
        /// The key to insert or update.
        key: Vec<u8>,
        /// The value to store.
        value: Vec<u8>,
    },
    /// Delete a key.
    Delete {
        /// The key to delete.
        key: Vec<u8>,
    },
}

/// In-memory KV store backed by a BTreeMap. Thread-safe via RwLock.
///
/// This is the Phase 1 implementation. It does not persist data across restarts.
/// Production will use A1's NVMe block store with atomic writes (D10).
pub struct MemoryKvStore {
    data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryKvStore {
    /// Creates a new empty in-memory KV store.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

impl Default for MemoryKvStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KvStore for MemoryKvStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(data.get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MetaError> {
        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        data.insert(key, value);
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<(), MetaError> {
        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        data.remove(key);
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut result = Vec::new();
        for (k, v) in data.range::<Vec<u8>, _>(prefix.to_vec()..) {
            if !k.starts_with(prefix) {
                break;
            }
            result.push((k.clone(), v.clone()));
        }
        Ok(result)
    }

    fn scan_range(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let result: Vec<_> = data
            .range::<Vec<u8>, _>((
                Bound::Included(start.to_vec()),
                Bound::Excluded(end.to_vec()),
            ))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(result)
    }

    fn contains_key(&self, key: &[u8]) -> Result<bool, MetaError> {
        let data = self
            .data
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(data.contains_key(key))
    }

    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), MetaError> {
        let mut data = self
            .data
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        for op in ops {
            match op {
                BatchOp::Put { key, value } => {
                    data.insert(key, value);
                }
                BatchOp::Delete { key } => {
                    data.remove(&key);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_get() {
        let store = MemoryKvStore::new();
        store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(b"key2").unwrap(), None);
    }

    #[test]
    fn test_delete() {
        let store = MemoryKvStore::new();
        store.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.delete(b"key1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_scan_prefix() {
        let store = MemoryKvStore::new();
        store.put(b"dir/a".to_vec(), b"1".to_vec()).unwrap();
        store.put(b"dir/b".to_vec(), b"2".to_vec()).unwrap();
        store.put(b"dir/c".to_vec(), b"3".to_vec()).unwrap();
        store.put(b"other/x".to_vec(), b"4".to_vec()).unwrap();

        let result = store.scan_prefix(b"dir/").unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, b"dir/a");
        assert_eq!(result[1].0, b"dir/b");
        assert_eq!(result[2].0, b"dir/c");
    }

    #[test]
    fn test_scan_range() {
        let store = MemoryKvStore::new();
        store.put(b"a".to_vec(), b"1".to_vec()).unwrap();
        store.put(b"b".to_vec(), b"2".to_vec()).unwrap();
        store.put(b"c".to_vec(), b"3".to_vec()).unwrap();
        store.put(b"d".to_vec(), b"4".to_vec()).unwrap();

        let result = store.scan_range(b"b", b"d").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, b"b");
        assert_eq!(result[1].0, b"c");
    }

    #[test]
    fn test_write_batch() {
        let store = MemoryKvStore::new();
        store.put(b"existing".to_vec(), b"old".to_vec()).unwrap();

        store
            .write_batch(vec![
                BatchOp::Put {
                    key: b"new1".to_vec(),
                    value: b"v1".to_vec(),
                },
                BatchOp::Put {
                    key: b"new2".to_vec(),
                    value: b"v2".to_vec(),
                },
                BatchOp::Delete {
                    key: b"existing".to_vec(),
                },
            ])
            .unwrap();

        assert_eq!(store.get(b"new1").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(store.get(b"new2").unwrap(), Some(b"v2".to_vec()));
        assert_eq!(store.get(b"existing").unwrap(), None);
    }

    #[test]
    fn test_contains_key() {
        let store = MemoryKvStore::new();
        assert!(!store.contains_key(b"key").unwrap());
        store.put(b"key".to_vec(), b"value".to_vec()).unwrap();
        assert!(store.contains_key(b"key").unwrap());
    }

    #[test]
    fn test_overwrite() {
        let store = MemoryKvStore::new();
        store.put(b"key".to_vec(), b"v1".to_vec()).unwrap();
        store.put(b"key".to_vec(), b"v2".to_vec()).unwrap();
        assert_eq!(store.get(b"key").unwrap(), Some(b"v2".to_vec()));
    }
}
