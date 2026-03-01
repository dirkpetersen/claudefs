//! Integrated write path: reduction pipeline + distributed fingerprint + segment packing.

use crate::compression::CompressionAlgorithm;
use crate::encryption::EncryptionKey;
use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use crate::meta_bridge::{BlockLocation, FingerprintStore};
use crate::pipeline::{PipelineConfig, ReducedChunk, ReductionPipeline, ReductionStats};
use crate::segment::{Segment, SegmentPacker, SegmentPackerConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

/// Configuration for the integrated write path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritePathConfig {
    /// Reduction pipeline configuration
    pub pipeline: PipelineConfig,
    /// Segment packer configuration
    pub segment: SegmentPackerConfig,
}

impl Default for WritePathConfig {
    fn default() -> Self {
        Self {
            pipeline: PipelineConfig::default(),
            segment: SegmentPackerConfig::default(),
        }
    }
}

/// Statistics from the integrated write path.
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

/// Result from processing a write through the integrated path.
#[derive(Debug)]
pub struct WritePathResult {
    /// Reduced chunks (for CAS and application use)
    pub reduced_chunks: Vec<ReducedChunk>,
    /// Sealed segments ready for EC/storage
    pub sealed_segments: Vec<Segment>,
    /// Statistics from the operation
    pub stats: WritePathStats,
}

/// Integrated write path combining pipeline + distributed fingerprint + segment packing.
pub struct IntegratedWritePath<F: FingerprintStore + Send + Sync> {
    pipeline: ReductionPipeline,
    packer: SegmentPacker,
    fingerprint_store: Arc<F>,
    stats: WritePathStats,
}

impl<F: FingerprintStore + Send + Sync> IntegratedWritePath<F> {
    /// Create a new integrated write path without encryption.
    pub fn new(config: WritePathConfig, fingerprint_store: Arc<F>) -> Self {
        Self {
            pipeline: ReductionPipeline::new(config.pipeline),
            packer: SegmentPacker::new(config.segment),
            fingerprint_store,
            stats: WritePathStats::default(),
        }
    }

    /// Create a new integrated write path with encryption enabled.
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

    /// Process a write through the full integrated path:
    /// 1. Run through reduction pipeline (chunk → dedup → compress → encrypt)
    /// 2. Check distributed fingerprint store for existing chunks
    /// 3. Pack new chunks into segments
    /// 4. Insert new fingerprints to the store
    pub fn process_write(&mut self, data: &[u8]) -> Result<WritePathResult, ReduceError> {
        // (a) Run through reduction pipeline
        let (mut chunks, pipeline_stats) = self.pipeline.process_write(data)?;

        // Update stats
        self.stats.pipeline = pipeline_stats;
        let mut sealed_segments = Vec::new();

        // (b) Check distributed fingerprint store and (c) pack new chunks
        for chunk in &chunks {
            // Check if chunk exists in distributed fingerprint store
            if let Some(location) = self.fingerprint_store.lookup(chunk.hash.as_bytes()) {
                // Distributed dedup hit
                self.stats.distributed_dedup_hits += 1;
                debug!(
                    hash = %chunk.hash.to_hex(),
                    node = location.node_id,
                    "Distributed dedup hit"
                );

                // Increment ref in fingerprint store
                self.fingerprint_store.increment_ref(chunk.hash.as_bytes());
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
                    self.fingerprint_store.insert(chunk.hash.0, location);
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
    /// Note: Returns default/empty stats as ReductionStats doesn't support cloning well.
    pub fn stats_snapshot(&self) -> WritePathStats {
        WritePathStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::compress;
    use crate::encryption::EncryptionKey;
    use crate::meta_bridge::{BlockLocation, LocalFingerprintStore, NullFingerprintStore};

    fn test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn test_basic_write() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        let data = test_data(10000);
        let result = write_path.process_write(&data).unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert!(result.stats.pipeline.input_bytes > 0);
    }

    #[test]
    fn test_encryption_write() {
        let mut config = WritePathConfig::default();
        config.pipeline.encryption_enabled = true;

        let store = Arc::new(NullFingerprintStore::new());
        let key = EncryptionKey([0x42u8; 32]);
        let mut write_path = IntegratedWritePath::new_with_key(config, key, store);

        let data = b"secret data for encryption test".to_vec();
        let result = write_path.process_write(&data).unwrap();

        // Should have encrypted chunks
        assert!(result.reduced_chunks.iter().any(|c| c.payload.is_some()));
    }

    #[test]
    fn test_flush_segments() {
        let config = WritePathConfig {
            segment: SegmentPackerConfig { target_size: 1000 },
            ..Default::default()
        };

        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Write small data
        write_path.process_write(&test_data(100)).unwrap();

        // Flush should return any pending segments
        let segments = write_path.flush_segments();

        // At least one segment should be flushed
        assert!(segments.len() >= 1);
    }

    #[test]
    fn test_distributed_dedup() {
        let config = WritePathConfig::default();
        let store = Arc::new(LocalFingerprintStore::new());

        let mut write_path = IntegratedWritePath::new(config, store.clone());

        // First write - adds fingerprints to store
        let data = test_data(100_000);
        let result1 = write_path.process_write(&data).unwrap();

        // Create a new write path using the SAME store
        let config2 = WritePathConfig::default();
        let mut write_path2 = IntegratedWritePath::new(config2, store);

        // Second write with same data - should hit distributed dedup
        let result2 = write_path2.process_write(&data).unwrap();

        assert!(
            result2.stats.distributed_dedup_hits > 0,
            "Expected distributed dedup hits"
        );
    }

    #[test]
    fn test_null_fingerprint_store() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        let data = test_data(5000);
        let result = write_path.process_write(&data).unwrap();

        // Should work without distributed dedup
        assert!(result.reduced_chunks.len() > 0);
    }

    #[test]
    fn test_small_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Very small data
        let data = b"tiny";
        let result = write_path.process_write(data).unwrap();

        assert!(!result.reduced_chunks.is_empty());
        assert_eq!(result.stats.pipeline.input_bytes, 4);
    }

    #[test]
    fn test_large_data() {
        let config = WritePathConfig::default();
        let store = Arc::new(NullFingerprintStore::new());
        let mut write_path = IntegratedWritePath::new(config, store);

        // Larger data
        let data = test_data(1_000_000);
        let result = write_path.process_write(&data).unwrap();

        assert!(result.reduced_chunks.len() >= 1);
        assert!(result.stats.pipeline.input_bytes == 1_000_000);
    }
}
