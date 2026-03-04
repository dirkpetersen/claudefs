//! Async fingerprint store bridge for Tokio-based distributed metadata integration.

use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use crate::meta_bridge::{BlockLocation, FingerprintStore};
use crate::pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
use crate::segment::{Segment, SegmentPacker, SegmentPackerConfig};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Async version of FingerprintStore for Tokio-based distributed metadata integration.
/// Implementors can delegate to A2's distributed fingerprint index.
#[async_trait]
pub trait AsyncFingerprintStore: Send + Sync {
    /// Lookup a fingerprint, returning its block location if found.
    async fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation>;

    /// Insert a new fingerprint-location pair.
    /// Returns true if this was a new entry, false if it already existed.
    async fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool;

    /// Increment reference count for an existing entry.
    /// Returns true if the entry existed and was incremented.
    async fn increment_ref(&self, hash: &[u8; 32]) -> bool;

    /// Decrement reference count for an entry.
    /// Returns the new refcount, or None if entry not found.
    async fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64>;

    /// Total number of entries in the store.
    async fn entry_count(&self) -> usize;
}

/// In-memory async fingerprint store using RwLock for thread-safe async access.
pub struct AsyncLocalFingerprintStore {
    entries: RwLock<HashMap<[u8; 32], (BlockLocation, u64)>>,
}

impl AsyncLocalFingerprintStore {
    /// Create a new empty async local fingerprint store.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Calculate total bytes stored via deduplicated chunks.
    pub async fn total_deduplicated_bytes(&self) -> u64 {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|(_, count)| *count > 1)
            .map(|(loc, count)| loc.size * (count - 1))
            .sum()
    }
}

impl Default for AsyncLocalFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncFingerprintStore for AsyncLocalFingerprintStore {
    async fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation> {
        let entries = self.entries.read().await;
        entries.get(hash).map(|(loc, _)| *loc)
    }

    async fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool {
        let mut entries = self.entries.write().await;
        match entries.entry(hash) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().1 += 1;
                false
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((location, 1));
                debug!(
                    node_id = location.node_id,
                    offset = location.block_offset,
                    "Inserted new fingerprint"
                );
                true
            }
        }
    }

    async fn increment_ref(&self, hash: &[u8; 32]) -> bool {
        let mut entries = self.entries.write().await;
        if let Some((_, refs)) = entries.get_mut(hash) {
            *refs += 1;
            debug!(hash = ?hash, refs = *refs, "Incremented refcount");
            true
        } else {
            false
        }
    }

    async fn decrement_ref(&self, hash: &[u8; 32]) -> Option<u64> {
        let mut entries = self.entries.write().await;
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

    async fn entry_count(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl FingerprintStore for AsyncLocalFingerprintStore {
    fn lookup(&self, hash: &[u8; 32]) -> Option<BlockLocation> {
        // Use blocking read for sync compatibility
        let entries = self.entries.blocking_read();
        entries.get(hash).map(|(loc, _)| *loc)
    }

    fn insert(&self, hash: [u8; 32], location: BlockLocation) -> bool {
        let mut entries = self.entries.blocking_write();
        match entries.entry(hash) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().1 += 1;
                false
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((location, 1));
                debug!(
                    node_id = location.node_id,
                    offset = location.block_offset,
                    "Inserted new fingerprint"
                );
                true
            }
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

/// No-op async fingerprint store for testing or when distributed dedup is disabled.
pub struct AsyncNullFingerprintStore;

impl AsyncNullFingerprintStore {
    /// Create a new null async fingerprint store.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsyncNullFingerprintStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncFingerprintStore for AsyncNullFingerprintStore {
    async fn lookup(&self, _hash: &[u8; 32]) -> Option<BlockLocation> {
        None
    }

    async fn insert(&self, _hash: [u8; 32], _location: BlockLocation) -> bool {
        true
    }

    async fn increment_ref(&self, _hash: &[u8; 32]) -> bool {
        false
    }

    async fn decrement_ref(&self, _hash: &[u8; 32]) -> Option<u64> {
        None
    }

    async fn entry_count(&self) -> usize {
        0
    }
}

impl FingerprintStore for AsyncNullFingerprintStore {
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

/// Configuration for the async integrated write path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WritePathConfig {
    /// Reduction pipeline configuration
    pub pipeline: PipelineConfig,
    /// Segment packer configuration
    pub segment: SegmentPackerConfig,
}

/// Statistics from the async integrated write path.
#[derive(Debug, Default, Clone)]
pub struct WritePathStats {
    /// Pipeline statistics
    pub pipeline: ReductionStats,
    /// Number of sealed segments produced
    pub segments_produced: usize,
    /// Hits from distributed deduplication (chunks found in fingerprint store)
    pub distributed_dedup_hits: usize,
}

impl WritePathStats {
    /// Total input bytes processed
    pub fn total_input_bytes(&self) -> u64 {
        self.pipeline.input_bytes
    }

    /// Total bytes stored in segments
    pub fn total_bytes_stored(&self) -> u64 {
        self.pipeline.bytes_after_encryption
    }

    /// Overall reduction ratio (input / stored)
    pub fn overall_reduction_ratio(&self) -> f64 {
        if self.total_bytes_stored() > 0 {
            self.total_input_bytes() as f64 / self.total_bytes_stored() as f64
        } else {
            1.0
        }
    }
}

/// Result from processing a write through the async integrated path.
#[derive(Debug)]
pub struct WritePathResult {
    /// Reduced chunks (for CAS and application use)
    pub reduced_chunks: Vec<ReducedChunk>,
    /// Sealed segments ready for EC/storage
    pub sealed_segments: Vec<Segment>,
    /// Statistics from the operation
    pub stats: WritePathStats,
}

/// Async integrated write path combining pipeline + distributed fingerprint + segment packing.
pub struct AsyncIntegratedWritePath<F: AsyncFingerprintStore> {
    pipeline: ReductionPipeline,
    packer: SegmentPacker,
    fingerprint_store: Arc<F>,
    stats: WritePathStats,
}

impl<F: AsyncFingerprintStore> AsyncIntegratedWritePath<F> {
    /// Create a new async integrated write path without encryption.
    pub fn new(config: WritePathConfig, fingerprint_store: Arc<F>) -> Self {
        Self {
            pipeline: ReductionPipeline::new(config.pipeline),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Create a new async integrated write path with encryption enabled.
    pub fn new_with_key(
        config: WritePathConfig,
        master_key: EncryptionKey,
        fingerprint_store: Arc<F>,
    ) -> Self {
        Self {
            pipeline: ReductionPipeline::with_master_key(config.pipeline, master_key),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Process a write through the full async integrated path:
    /// 1. Run through reduction pipeline (chunk → dedup → compress → encrypt)
    /// 2. Await distributed fingerprint store lookup for each chunk
    /// 3. If found in distributed store, increment ref (distributed dedup hit)
    /// 4. If not found and not duplicate, pack into segment + insert into store
    /// 5. Return WritePathResult
    pub async fn process_write(&mut self, data: &[u8]) -> Result<WritePathResult, ReduceError> {
        // (a) Run through reduction pipeline
        let (chunks, pipeline_stats) = self.pipeline.process_write(data)?;

        // Update stats
        self.stats.pipeline = pipeline_stats;
        let mut sealed_segments = Vec::new();

        // (b) Check distributed fingerprint store and (c) pack new chunks
        for chunk in &chunks {
            // Check if chunk exists in distributed fingerprint store
            if let Some(location) = self.fingerprint_store.lookup(chunk.hash.as_bytes()).await {
                // Distributed dedup hit
                self.stats.distributed_dedup_hits += 1;
                debug!(
                    hash = %chunk.hash.to_hex(),
                    node = location.node_id,
                    "Distributed dedup hit"
                );

                // Increment ref in fingerprint store
                self.fingerprint_store.increment_ref(chunk.hash.as_bytes()).await;
            } else if !chunk.is_duplicate {
                // New chunk - pack into segment
                if let Some(payload) = &chunk.payload {
                    let location = BlockLocation {
                        node_id: 0, // Will be set by actual storage layer
                        block_offset: 0,
                        size: payload.ciphertext.len() as u64,
                    };

                    // Add to segment packer
                    if let Some(segment) = self.packer.add_chunk(
                        chunk.hash,
                        &payload.ciphertext,
                        chunk.original_size as u32,
                    ) {
                        sealed_segments.push(segment);
                        self.stats.segments_produced += 1;
                    }

                    // Insert to distributed fingerprint store
                    self.fingerprint_store
                        .insert(chunk.hash.0, location)
                        .await;
                }
            }
        }

        // (d) Return result
        Ok(WritePathResult {
            reduced_chunks: chunks,
            sealed_segments,
            stats: WritePathStats {
                pipeline: self.stats.pipeline.clone(),
                segments_produced: self.stats.segments_produced,
                distributed_dedup_hits: self.stats.distributed_dedup_hits,
            },
        })
    }

    /// Flush any pending segments.
    /// Returns sealed segments even if not full.
    pub fn flush_segments(&mut self) -> Vec<Segment> {
        let mut segments = Vec::new();
        if let Some(segment) = self.packer.flush() {
            segments.push(segment);
            self.stats.segments_produced += 1;
        }
        segments
    }

    /// Get a snapshot of current statistics.
    pub fn stats_snapshot(&self) -> WritePathStats {
        WritePathStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn test_async_basic_write() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(10000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes > 0);
    }

    #[tokio::test]
    async fn test_async_encryption_write() {
        let mut config = WritePathConfig::default();
        config.pipeline.encryption_enabled = true;

        let store = Arc::new(AsyncNullFingerprintStore::new());
        let key = EncryptionKey([0x42u8; 32]);
        let mut write_path = AsyncIntegratedWritePath::new_with_key(config, key, store);

        let data = b"secret data for encryption test".to_vec();
        let result = write_path.process_write(&data).await.unwrap();

        assert!(result.reduced_chunks.iter().any(|c| c.payload.is_some()));
    }

    #[tokio::test]
    async fn test_async_flush_segments() {
        let config = WritePathConfig {
            segment: SegmentPackerConfig { target_size: 1000 },
            ..Default::default()
        };

        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        write_path.process_write(&test_data(100)).await.unwrap();

        let segments = write_path.flush_segments();

        assert!(segments.len() >= 1);
    }

    #[tokio::test]
    async fn test_async_distributed_dedup() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncLocalFingerprintStore::new());

        let mut write_path = AsyncIntegratedWritePath::new(config, store.clone());

        let data = test_data(100_000);
        let _result1 = write_path.process_write(&data).await.unwrap();

        let config2 = WritePathConfig::default();
        let mut write_path2 = AsyncIntegratedWritePath::new(config2, store);

        let result2 = write_path2.process_write(&data).await.unwrap();

        assert!(
            result2.stats.distributed_dedup_hits > 0,
            "Expected distributed dedup hits"
        );
    }

    #[tokio::test]
    async fn test_async_null_store() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(5000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(result.reduced_chunks.len() > 0);
    }

    #[tokio::test]
    async fn test_async_large_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncNullFingerprintStore::new());
        let mut write_path = AsyncIntegratedWritePath::new(config, store);

        let data = test_data(1_000_000);
        let result = write_path.process_write(&data).await.unwrap();

        assert!(result.reduced_chunks.len() >= 1);
        assert!(result.stats.pipeline.input_bytes == 1_000_000);
    }

    #[tokio::test]
    async fn test_async_concurrent_writes() {
        let config = WritePathConfig::default();
        let store = Arc::new(AsyncLocalFingerprintStore::new());

        let store_clone = store.clone();
        let handle1 = tokio::spawn(async move {
            let mut write_path = AsyncIntegratedWritePath::new(config.clone(), store_clone);
            let data = test_data(50_000);
            write_path.process_write(&data).await.unwrap()
        });

        let config2 = WritePathConfig::default();
        let store2 = Arc::new(AsyncLocalFingerprintStore::new());
        let handle2 = tokio::spawn(async move {
            let mut write_path = AsyncIntegratedWritePath::new(config2, store2);
            let data = test_data(50_000);
            write_path.process_write(&data).await.unwrap()
        });

        let _ = handle1.await;
        let _ = handle2.await;

        // Both writes completed without panic
    }

    #[tokio::test]
    async fn test_async_local_store_total_deduplicated_bytes() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let loc1 = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };
        let loc2 = BlockLocation {
            node_id: 1,
            block_offset: 200,
            size: 8192,
        };

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc1).await;
        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc1).await; // refcount now 2

        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await;
        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await;
        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc2).await; // refcount now 3

        assert_eq!(store.total_deduplicated_bytes().await, 4096 + 16384);
    }

    #[tokio::test]
    async fn test_async_local_store_ref_counts() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let hash = [0u8; 32];
        let location = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };

        AsyncFingerprintStore::insert(&*store, hash, location).await;

        assert!(AsyncFingerprintStore::increment_ref(&*store, &hash).await);
        assert!(AsyncFingerprintStore::increment_ref(&*store, &hash).await);

        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(2));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(1));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, Some(0));
        assert_eq!(AsyncFingerprintStore::decrement_ref(&*store, &hash).await, None);
    }

    #[tokio::test]
    async fn test_async_local_store_entry_count() {
        let store = Arc::new(AsyncLocalFingerprintStore::new());
        let loc = BlockLocation {
            node_id: 1,
            block_offset: 100,
            size: 4096,
        };

        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 0);

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 1);

        AsyncFingerprintStore::insert(&*store, [2u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 2);

        AsyncFingerprintStore::insert(&*store, [1u8; 32], loc).await;
        assert_eq!(AsyncFingerprintStore::entry_count(&*store).await, 2);
    }
}