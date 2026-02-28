//! S3 tiering backend for segment storage.
//!
//! Per D5: Cache mode (default) - every segment asynchronously written to S3.
//! Per D6: Tiered mode - only aged-out segments go to S3.
//!
//! This module provides the S3 tiering interface with a mock implementation for testing.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{StorageError, StorageResult};

/// Boxed future type for async trait methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for object store operations (S3, Azure Blob, GCS, etc.).
pub trait ObjectStoreBackend: Send + Sync {
    /// Put a segment into the object store.
    fn put_segment(&self, segment_id: u64, data: Vec<u8>) -> BoxFuture<'_, StorageResult<()>>;
    /// Get a segment from the object store.
    fn get_segment(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<Vec<u8>>>;
    /// Delete a segment from the object store.
    fn delete_segment(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<()>>;
    /// Check if a segment exists in the object store.
    fn exists(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<bool>>;
    /// List all segment IDs in the object store.
    fn list_segments(&self) -> BoxFuture<'_, StorageResult<Vec<u64>>>;
}

/// Statistics for the mock object store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockObjectStoreStats {
    /// Number of put operations.
    pub puts: u64,
    /// Number of get operations.
    pub gets: u64,
    /// Number of delete operations.
    pub deletes: u64,
    /// Number of exists checks.
    pub exists_checks: u64,
    /// Number of list operations.
    pub list_calls: u64,
    /// Total bytes stored.
    pub total_bytes_stored: u64,
}

/// In-memory mock object store for testing.
pub struct MockObjectStore {
    store: Mutex<HashMap<u64, Vec<u8>>>,
    stats: Mutex<MockObjectStoreStats>,
}

impl MockObjectStore {
    /// Create a new mock object store.
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
            stats: Mutex::new(MockObjectStoreStats {
                puts: 0,
                gets: 0,
                deletes: 0,
                exists_checks: 0,
                list_calls: 0,
                total_bytes_stored: 0,
            }),
        }
    }

    /// Get statistics about store operations.
    pub fn stats(&self) -> MockObjectStoreStats {
        self.stats.lock().unwrap().clone()
    }

    /// Get the number of stored segments.
    pub fn stored_count(&self) -> usize {
        self.store.lock().unwrap().len()
    }
}

impl Default for MockObjectStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectStoreBackend for MockObjectStore {
    fn put_segment(&self, segment_id: u64, data: Vec<u8>) -> BoxFuture<'_, StorageResult<()>> {
        let mut store = self.store.lock().unwrap();
        let bytes = data.len() as u64;
        store.insert(segment_id, data);

        let mut stats = self.stats.lock().unwrap();
        stats.puts += 1;
        stats.total_bytes_stored = stats.total_bytes_stored.saturating_add(bytes);

        Box::pin(async move {
            debug!("Mock put_segment: id={}", segment_id);
            Ok(())
        })
    }

    fn get_segment(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<Vec<u8>>> {
        let store = self.store.lock().unwrap();
        let result = store.get(&segment_id).cloned();
        drop(store);

        let mut stats = self.stats.lock().unwrap();
        stats.gets += 1;

        Box::pin(async move {
            debug!("Mock get_segment: id={}", segment_id);
            result.ok_or_else(|| StorageError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Segment {} not found in mock store", segment_id),
            )))
        })
    }

    fn delete_segment(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<()>> {
        let mut store = self.store.lock().unwrap();
        let removed = store.remove(&segment_id);
        let bytes_removed = removed.as_ref().map(|d| d.len() as u64).unwrap_or(0);

        let mut stats = self.stats.lock().unwrap();
        stats.deletes += 1;
        stats.total_bytes_stored = stats.total_bytes_stored.saturating_sub(bytes_removed);

        Box::pin(async move {
            debug!("Mock delete_segment: id={}", segment_id);
            Ok(())
        })
    }

    fn exists(&self, segment_id: u64) -> BoxFuture<'_, StorageResult<bool>> {
        let store = self.store.lock().unwrap();
        let exists = store.contains_key(&segment_id);
        drop(store);

        let mut stats = self.stats.lock().unwrap();
        stats.exists_checks += 1;

        Box::pin(async move {
            debug!("Mock exists: id={}, result={}", segment_id, exists);
            Ok(exists)
        })
    }

    fn list_segments(&self) -> BoxFuture<'_, StorageResult<Vec<u64>>> {
        let store = self.store.lock().unwrap();
        let mut ids: Vec<u64> = store.keys().copied().collect();
        ids.sort();
        drop(store);

        let mut stats = self.stats.lock().unwrap();
        stats.list_calls += 1;

        Box::pin(async move {
            debug!("Mock list_segments: count={}", ids.len());
            Ok(ids)
        })
    }
}

/// Tiering mode for segment storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TieringMode {
    /// Cache mode (default): every segment asynchronously written to S3.
    Cache,
    /// Tiered mode: only aged-out segments go to S3.
    Tiered,
    /// Disabled: no S3 tiering.
    Disabled,
}

impl Default for TieringMode {
    fn default() -> Self {
        TieringMode::Cache
    }
}

/// Configuration for S3 tiering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringConfig {
    /// Tiering mode.
    pub mode: TieringMode,
    /// S3 bucket name.
    pub bucket_name: String,
    /// S3 key prefix.
    pub key_prefix: String,
    /// Maximum concurrent uploads.
    pub max_concurrent_uploads: usize,
    /// Maximum concurrent downloads.
    pub max_concurrent_downloads: usize,
    /// Upload timeout in seconds.
    pub upload_timeout_secs: u64,
    /// Whether to verify uploads by downloading and comparing.
    pub verify_after_upload: bool,
}

impl Default for TieringConfig {
    fn default() -> Self {
        Self {
            mode: TieringMode::Cache,
            bucket_name: String::new(),
            key_prefix: "segments/".to_string(),
            max_concurrent_uploads: 4,
            max_concurrent_downloads: 4,
            upload_timeout_secs: 300,
            verify_after_upload: true,
        }
    }
}

/// Statistics for the tiering engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TieringStats {
    /// Number of segments uploaded to S3.
    pub segments_uploaded: u64,
    /// Number of segments downloaded from S3.
    pub segments_downloaded: u64,
    /// Number of segments deleted from S3.
    pub segments_deleted: u64,
    /// Total bytes uploaded.
    pub bytes_uploaded: u64,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
    /// Number of upload errors.
    pub upload_errors: u64,
    /// Number of download errors.
    pub download_errors: u64,
}

/// Engine for managing S3 tiering operations.
pub struct TieringEngine<B: ObjectStoreBackend> {
    config: TieringConfig,
    backend: B,
    stats: Mutex<TieringStats>,
}

impl<B: ObjectStoreBackend> TieringEngine<B> {
    /// Create a new tiering engine.
    pub fn new(config: TieringConfig, backend: B) -> Self {
        info!("TieringEngine created: mode={:?}, bucket={}", config.mode, config.bucket_name);
        Self {
            config,
            backend,
            stats: Mutex::new(TieringStats::default()),
        }
    }

    /// Upload a segment to S3.
    pub async fn upload_segment(&self, segment_id: u64, data: Vec<u8>) -> StorageResult<bool> {
        let bytes = data.len() as u64;

        self.backend.put_segment(segment_id, data).await?;

        let verify = self.config.verify_after_upload;
        if verify {
            let downloaded = self.backend.get_segment(segment_id).await?;
            if downloaded.len() != bytes as usize {
                warn!("Verification failed for segment {}: size mismatch", segment_id);
                return Ok(false);
            }
        }

        let mut stats = self.stats.lock().unwrap();
        stats.segments_uploaded += 1;
        stats.bytes_uploaded += bytes;

        debug!("Uploaded segment {} ({} bytes)", segment_id, bytes);
        Ok(true)
    }

    /// Download a segment from S3.
    pub async fn download_segment(&self, segment_id: u64) -> StorageResult<Vec<u8>> {
        let data = self.backend.get_segment(segment_id).await?;
        let bytes = data.len() as u64;

        let mut stats = self.stats.lock().unwrap();
        stats.segments_downloaded += 1;
        stats.bytes_downloaded += bytes;

        debug!("Downloaded segment {} ({} bytes)", segment_id, bytes);
        Ok(data)
    }

    /// Delete a segment from S3.
    pub async fn delete_segment(&self, segment_id: u64) -> StorageResult<()> {
        self.backend.delete_segment(segment_id).await?;

        let mut stats = self.stats.lock().unwrap();
        stats.segments_deleted += 1;

        debug!("Deleted segment {}", segment_id);
        Ok(())
    }

    /// Check if a segment exists in S3.
    pub async fn is_segment_stored(&self, segment_id: u64) -> StorageResult<bool> {
        self.backend.exists(segment_id).await
    }

    /// Process a batch of segments for eviction.
    pub async fn process_eviction_batch(
        &self,
        segment_ids: &[u64],
        segment_data: &HashMap<u64, Vec<u8>>,
    ) -> Vec<u64> {
        let mut successful = Vec::new();

        for &segment_id in segment_ids {
            if let Some(data) = segment_data.get(&segment_id) {
                match self.upload_segment(segment_id, data.clone()).await {
                    Ok(true) => {
                        successful.push(segment_id);
                    }
                    Ok(false) => {
                        warn!("Upload verification failed for segment {}", segment_id);
                    }
                    Err(e) => {
                        warn!("Failed to upload segment {}: {:?}", segment_id, e);
                        let mut stats = self.stats.lock().unwrap();
                        stats.upload_errors += 1;
                    }
                }
            } else {
                warn!("Segment {} not found in provided data", segment_id);
            }
        }

        debug!(
            "Eviction batch processed: {}/{} successful",
            successful.len(),
            segment_ids.len()
        );
        successful
    }

    pub fn stats(&self) -> TieringStats {
        self.stats.lock().unwrap().clone()
    }
}

pub struct S3KeyBuilder {
    prefix: String,
}

impl S3KeyBuilder {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }

    pub fn segment_key(&self, segment_id: u64) -> String {
        format!("{}{}", self.prefix, segment_id)
    }

    pub fn parse_segment_id(&self, key: &str) -> Option<u64> {
        key.strip_prefix(&self.prefix)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_store_put_get() {
        let store = MockObjectStore::new();
        let data = vec![1u8, 2, 3, 4, 5];

        store.put_segment(42, data.clone()).await.unwrap();
        let retrieved = store.get_segment(42).await.unwrap();

        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_mock_store_delete() {
        let store = MockObjectStore::new();
        let data = vec![1u8, 2, 3];

        store.put_segment(100, data).await.unwrap();
        assert!(store.stored_count() == 1);

        store.delete_segment(100).await.unwrap();
        assert!(store.stored_count() == 0);
    }

    #[tokio::test]
    async fn test_mock_store_exists() {
        let store = MockObjectStore::new();

        assert!(!store.exists(1).await.unwrap());

        store.put_segment(1, vec![0u8; 10]).await.unwrap();

        assert!(store.exists(1).await.unwrap());
        assert!(!store.exists(2).await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_store_list() {
        let store = MockObjectStore::new();

        store.put_segment(3, vec![]).await.unwrap();
        store.put_segment(1, vec![]).await.unwrap();
        store.put_segment(2, vec![]).await.unwrap();

        let ids = store.list_segments().await.unwrap();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_mock_store_overwrite() {
        let store = MockObjectStore::new();

        store.put_segment(1, vec![1, 2, 3]).await.unwrap();
        store.put_segment(1, vec![4, 5, 6, 7]).await.unwrap();

        let data = store.get_segment(1).await.unwrap();
        assert_eq!(data, vec![4, 5, 6, 7]);
    }

    #[tokio::test]
    async fn test_mock_store_get_nonexistent() {
        let store = MockObjectStore::new();
        let result = store.get_segment(999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_store_stats() {
        let store = MockObjectStore::new();

        store.put_segment(1, vec![1, 2, 3]).await.unwrap();
        store.put_segment(2, vec![4, 5]).await.unwrap();
        store.get_segment(1).await.unwrap();
        store.exists(1).await.unwrap();
        store.list_segments().await.unwrap();
        store.delete_segment(1).await.unwrap();

        let stats = store.stats();
        assert_eq!(stats.puts, 2);
        assert_eq!(stats.gets, 1);
        assert_eq!(stats.deletes, 1);
        assert_eq!(stats.exists_checks, 1);
        assert_eq!(stats.list_calls, 1);
    }

    #[test]
    fn test_tiering_config_defaults() {
        let config = TieringConfig::default();

        assert_eq!(config.mode, TieringMode::Cache);
        assert_eq!(config.bucket_name, "");
        assert_eq!(config.key_prefix, "segments/");
        assert_eq!(config.max_concurrent_uploads, 4);
        assert_eq!(config.max_concurrent_downloads, 4);
        assert_eq!(config.upload_timeout_secs, 300);
        assert!(config.verify_after_upload);
    }

    #[tokio::test]
    async fn test_tiering_engine_upload() {
        let store = MockObjectStore::new();
        let config = TieringConfig {
            mode: TieringMode::Cache,
            bucket_name: "test-bucket".to_string(),
            key_prefix: "segments/".to_string(),
            max_concurrent_uploads: 4,
            max_concurrent_downloads: 4,
            upload_timeout_secs: 300,
            verify_after_upload: false,
        };
        let engine = TieringEngine::new(config, store);

        let data = vec![0u8; 1024];
        let result = engine.upload_segment(1, data.clone()).await.unwrap();

        assert!(result);
        let stats = engine.stats();
        assert_eq!(stats.segments_uploaded, 1);
        assert_eq!(stats.bytes_uploaded, 1024);
    }

    #[tokio::test]
    async fn test_tiering_engine_download() {
        let store = MockObjectStore::new();
        store.put_segment(5, vec![1, 2, 3, 4, 5]).await.unwrap();

        let config = TieringConfig::default();
        let engine = TieringEngine::new(config, store);

        let data = engine.download_segment(5).await.unwrap();
        assert_eq!(data, vec![1, 2, 3, 4, 5]);

        let stats = engine.stats();
        assert_eq!(stats.segments_downloaded, 1);
        assert_eq!(stats.bytes_downloaded, 5);
    }

    #[tokio::test]
    async fn test_tiering_engine_upload_verify() {
        let store = MockObjectStore::new();
        let config = TieringConfig {
            verify_after_upload: true,
            ..Default::default()
        };
        let engine = TieringEngine::new(config, store);

        let data = vec![9u8; 512];
        let result = engine.upload_segment(100, data.clone()).await.unwrap();

        assert!(result);
    }

    #[tokio::test]
    async fn test_tiering_engine_eviction_batch() {
        let store = MockObjectStore::new();
        let config = TieringConfig::default();
        let engine = TieringEngine::new(config, store);

        let mut segment_data = HashMap::new();
        segment_data.insert(1, vec![1u8; 100]);
        segment_data.insert(2, vec![2u8; 200]);
        segment_data.insert(3, vec![3u8; 300]);

        let segment_ids = vec![1, 2, 3];
        let successful = engine.process_eviction_batch(&segment_ids, &segment_data).await;

        assert_eq!(successful.len(), 3);
        assert!(successful.contains(&1));
        assert!(successful.contains(&2));
        assert!(successful.contains(&3));

        let stats = engine.stats();
        assert_eq!(stats.segments_uploaded, 3);
    }

    #[test]
    fn test_tiering_engine_stats() {
        let store = MockObjectStore::new();
        let config = TieringConfig::default();
        let engine = TieringEngine::new(config, store);

        let stats = engine.stats();
        assert_eq!(stats.segments_uploaded, 0);
        assert_eq!(stats.segments_downloaded, 0);
        assert_eq!(stats.segments_deleted, 0);
    }

    #[test]
    fn test_s3_key_builder() {
        let builder = S3KeyBuilder::new("segments/".to_string());

        let key = builder.segment_key(123);
        assert_eq!(key, "segments/123");

        let parsed = builder.parse_segment_id("segments/456");
        assert_eq!(parsed, Some(456));

        let no_prefix = builder.parse_segment_id("other/456");
        assert_eq!(no_prefix, None);
    }

    #[test]
    fn test_tiering_mode_variants() {
        assert_eq!(TieringMode::Cache, TieringMode::Cache);
        assert_eq!(TieringMode::Tiered, TieringMode::Tiered);
        assert_eq!(TieringMode::Disabled, TieringMode::Disabled);

        assert_ne!(TieringMode::Cache, TieringMode::Tiered);
    }
}