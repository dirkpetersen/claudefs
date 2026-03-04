//! Object store bridge for S3-compatible tiered storage operations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Object key identifying a bucket and object path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectKey {
    /// S3 bucket name.
    pub bucket: String,
    /// Object key within the bucket.
    pub key: String,
}

impl ObjectKey {
    /// Creates a new ObjectKey from bucket and key strings.
    pub fn new(bucket: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            key: key.into(),
        }
    }

    /// Returns the full path as "bucket/key".
    pub fn full_path(&self) -> String {
        format!("{}/{}", self.bucket, self.key)
    }
}

/// Metadata for a stored object.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectMetadata {
    /// The object's key.
    pub key: ObjectKey,
    /// Size in bytes.
    pub size_bytes: u64,
    /// ETag hash of the object content.
    pub etag: String,
    /// Timestamp of upload in milliseconds since epoch.
    pub uploaded_at_ms: u64,
}

/// Result of a store operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreResult {
    /// Object was successfully uploaded.
    Uploaded,
    /// Object already exists (upload skipped).
    AlreadyExists,
    /// Object was successfully deleted.
    Deleted,
    /// Object was not found.
    NotFound,
}

/// Statistics for the object store.
#[derive(Debug, Clone, Default)]
pub struct ObjectStoreStats {
    /// Total number of uploads.
    pub uploads: u64,
    /// Total number of downloads.
    pub downloads: u64,
    /// Total number of deletes.
    pub deletes: u64,
    /// Total bytes uploaded.
    pub bytes_uploaded: u64,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
}

/// In-memory object store implementation for testing.
pub struct MemoryObjectStore {
    data: HashMap<ObjectKey, (Vec<u8>, ObjectMetadata)>,
    stats: ObjectStoreStats,
}

impl MemoryObjectStore {
    /// Creates a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            stats: ObjectStoreStats::default(),
        }
    }

    /// Stores an object with the given key and data.
    pub fn put(&mut self, key: ObjectKey, data: Vec<u8>, now_ms: u64) -> StoreResult {
        let is_new = !self.data.contains_key(&key);
        let size = data.len() as u64;
        let etag = format!("{:x}", blake3_hash(&data));

        let metadata = ObjectMetadata {
            key: key.clone(),
            size_bytes: size,
            etag,
            uploaded_at_ms: now_ms,
        };

        if is_new {
            self.data.insert(key, (data, metadata));
            self.stats.uploads += 1;
            self.stats.bytes_uploaded += size;
            StoreResult::Uploaded
        } else {
            StoreResult::AlreadyExists
        }
    }

    /// Retrieves an object by key.
    pub fn get(&mut self, key: &ObjectKey) -> Option<Vec<u8>> {
        if let Some((data, _)) = self.data.get(key) {
            self.stats.downloads += 1;
            self.stats.bytes_downloaded += data.len() as u64;
            Some(data.clone())
        } else {
            None
        }
    }

    /// Deletes an object by key.
    pub fn delete(&mut self, key: &ObjectKey) -> StoreResult {
        if self.data.remove(key).is_some() {
            self.stats.deletes += 1;
            StoreResult::Deleted
        } else {
            StoreResult::NotFound
        }
    }

    /// Returns metadata for an object without retrieving its content.
    pub fn head(&self, key: &ObjectKey) -> Option<ObjectMetadata> {
        self.data.get(key).map(|(_, meta)| meta.clone())
    }

    /// Lists all objects in a bucket with the given prefix.
    pub fn list_prefix(&self, bucket: &str, prefix: &str) -> Vec<ObjectMetadata> {
        self.data
            .iter()
            .filter(|(k, _)| k.bucket == bucket && k.key.starts_with(prefix))
            .map(|(_, (_, meta))| meta.clone())
            .collect()
    }

    /// Returns the store statistics.
    pub fn stats(&self) -> &ObjectStoreStats {
        &self.stats
    }

    /// Returns the number of objects in the store.
    pub fn object_count(&self) -> usize {
        self.data.len()
    }

    /// Returns the total bytes stored across all objects.
    pub fn total_bytes(&self) -> u64 {
        self.data.values().map(|(d, _)| d.len() as u64).sum()
    }
}

impl Default for MemoryObjectStore {
    fn default() -> Self {
        Self::new()
    }
}

fn blake3_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_full_path() {
        let key = ObjectKey::new("bucket1", "path/to/object");
        assert_eq!(key.full_path(), "bucket1/path/to/object");
    }

    #[test]
    fn put_new_object() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "obj1");
        let result = store.put(key, b"test data".to_vec(), 1000);
        assert_eq!(result, StoreResult::Uploaded);
        assert_eq!(store.object_count(), 1);
    }

    #[test]
    fn put_existing_returns_already_exists() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "obj1");
        store.put(key.clone(), b"data1".to_vec(), 1000);
        let result = store.put(key, b"data2".to_vec(), 1001);
        assert_eq!(result, StoreResult::AlreadyExists);
    }

    #[test]
    fn get_existing_object() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "obj1");
        store.put(key.clone(), b"hello world".to_vec(), 1000);
        let data = store.get(&key);
        assert_eq!(data, Some(b"hello world".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "nonexistent");
        let data = store.get(&key);
        assert_eq!(data, None);
    }

    #[test]
    fn delete_existing_returns_deleted() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "obj1");
        store.put(key.clone(), b"data".to_vec(), 1000);
        let result = store.delete(&key);
        assert_eq!(result, StoreResult::Deleted);
        assert_eq!(store.object_count(), 0);
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "nonexistent");
        let result = store.delete(&key);
        assert_eq!(result, StoreResult::NotFound);
    }

    #[test]
    fn head_returns_metadata() {
        let mut store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "obj1");
        store.put(key.clone(), b"test content".to_vec(), 12345);
        let meta = store.head(&key);
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.size_bytes, 12);
        assert_eq!(meta.uploaded_at_ms, 12345);
    }

    #[test]
    fn head_missing_returns_none() {
        let store = MemoryObjectStore::new();
        let key = ObjectKey::new("bucket1", "nonexistent");
        let meta = store.head(&key);
        assert_eq!(meta, None);
    }

    #[test]
    fn list_prefix_empty() {
        let store = MemoryObjectStore::new();
        let results = store.list_prefix("bucket1", "prefix/");
        assert!(results.is_empty());
    }

    #[test]
    fn list_prefix_matches() {
        let mut store = MemoryObjectStore::new();
        store.put(
            ObjectKey::new("bucket1", "prefix/file1"),
            b"data".to_vec(),
            1000,
        );
        store.put(
            ObjectKey::new("bucket1", "prefix/file2"),
            b"data".to_vec(),
            1000,
        );
        store.put(
            ObjectKey::new("bucket1", "other/file"),
            b"data".to_vec(),
            1000,
        );
        let results = store.list_prefix("bucket1", "prefix/");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn list_prefix_no_matches() {
        let mut store = MemoryObjectStore::new();
        store.put(
            ObjectKey::new("bucket1", "other/file"),
            b"data".to_vec(),
            1000,
        );
        let results = store.list_prefix("bucket1", "prefix/");
        assert!(results.is_empty());
    }

    #[test]
    fn stats_uploads_count() {
        let mut store = MemoryObjectStore::new();
        store.put(ObjectKey::new("bucket1", "obj1"), b"data".to_vec(), 1000);
        store.put(ObjectKey::new("bucket1", "obj2"), b"data".to_vec(), 1000);
        assert_eq!(store.stats().uploads, 2);
    }

    #[test]
    fn stats_bytes_uploaded() {
        let mut store = MemoryObjectStore::new();
        store.put(ObjectKey::new("bucket1", "obj1"), b"hello".to_vec(), 1000);
        store.put(ObjectKey::new("bucket1", "obj2"), b"world".to_vec(), 1000);
        assert_eq!(store.stats().bytes_uploaded, 10);
    }

    #[test]
    fn object_count_and_total_bytes() {
        let mut store = MemoryObjectStore::new();
        store.put(ObjectKey::new("bucket1", "obj1"), vec![0u8; 100], 1000);
        store.put(ObjectKey::new("bucket1", "obj2"), vec![0u8; 200], 1000);
        assert_eq!(store.object_count(), 2);
        assert_eq!(store.total_bytes(), 300);
    }
}
