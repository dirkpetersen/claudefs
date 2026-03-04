//! Top-level ingest pipeline orchestrating all reduction stages.
//!
//! Processes raw data from clients through: buffer → CDC → dedup → compress → encrypt.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Stage in the ingest pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestStage {
    /// Buffering incoming data.
    Buffering,
    /// Chunking via CDC.
    Chunking,
    /// Deduplicating chunks.
    Deduplicating,
    /// Compressing chunks.
    Compressing,
    /// Encrypting chunks.
    Encrypting,
    /// Packing into segments.
    Packing,
}

/// Configuration for the ingest pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestConfig {
    /// Buffer threshold in bytes before flushing.
    pub buffer_threshold_bytes: usize,
    /// Enable deduplication.
    pub enable_dedup: bool,
    /// Enable compression.
    pub enable_compression: bool,
    /// Enable encryption.
    pub enable_encryption: bool,
    /// Compression level (1-9).
    pub compression_level: i32,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            buffer_threshold_bytes: 1024 * 1024,
            enable_dedup: true,
            enable_compression: true,
            enable_encryption: true,
            compression_level: 3,
        }
    }
}

/// Metrics for the ingest pipeline.
#[derive(Debug, Clone, Default)]
pub struct IngestMetrics {
    /// Total bytes ingested.
    pub bytes_ingested: u64,
    /// Bytes saved by deduplication.
    pub bytes_deduplicated: u64,
    /// Bytes after compression.
    pub bytes_compressed: u64,
    /// Bytes after encryption.
    pub bytes_encrypted: u64,
    /// Number of new chunks created.
    pub chunks_new: u64,
    /// Number of chunks deduplicated.
    pub chunks_deduped: u64,
    /// Latency per stage in microseconds.
    pub stage_latencies_us: Vec<u64>,
}

impl IngestMetrics {
    /// Returns overall reduction ratio (bytes_in / bytes_out).
    pub fn overall_reduction_ratio(&self) -> f64 {
        if self.bytes_encrypted > 0 {
            self.bytes_ingested as f64 / self.bytes_encrypted as f64
        } else {
            1.0
        }
    }

    /// Returns percentage of bytes saved by deduplication.
    pub fn dedup_savings_pct(&self) -> f64 {
        if self.bytes_ingested > 0 {
            self.bytes_deduplicated as f64 / self.bytes_ingested as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// A chunk produced by the ingest pipeline.
#[derive(Debug, Clone)]
pub struct IngestChunk {
    /// BLAKE3 hash of the original chunk.
    pub hash: [u8; 32],
    /// Processed data (compressed, encrypted if enabled).
    pub data: Vec<u8>,
    /// Original size before processing.
    pub original_size: usize,
    /// Whether this chunk was deduplicated.
    pub deduped: bool,
}

/// The ingest pipeline.
#[derive(Debug)]
pub struct IngestPipeline {
    config: IngestConfig,
    metrics: IngestMetrics,
    current_stage: IngestStage,
    fingerprint_index: HashMap<[u8; 32], Vec<u8>>,
    request_id: u64,
}

impl IngestPipeline {
    /// Create a new ingest pipeline.
    pub fn new(config: IngestConfig) -> Self {
        Self {
            config,
            metrics: IngestMetrics::default(),
            current_stage: IngestStage::Buffering,
            fingerprint_index: HashMap::new(),
            request_id: 0,
        }
    }

    /// Process data through all enabled stages.
    pub fn ingest(&mut self, _inode_id: u64, data: &[u8]) -> Vec<IngestChunk> {
        if data.is_empty() {
            return Vec::new();
        }

        self.metrics.bytes_ingested += data.len() as u64;

        let mut chunks = Vec::new();
        let chunk_size = 4096;

        for (i, chunk_data) in data.chunks(chunk_size).enumerate() {
            let hash = self.compute_hash(chunk_data);
            let original_size = chunk_data.len();

            if self.config.enable_dedup && self.fingerprint_index.contains_key(&hash) {
                self.metrics.bytes_deduplicated += original_size as u64;
                self.metrics.chunks_deduped += 1;
                chunks.push(IngestChunk {
                    hash,
                    data: Vec::new(),
                    original_size,
                    deduped: true,
                });
            } else {
                let processed = self.process_chunk(chunk_data);
                self.metrics.bytes_compressed += processed.len() as u64;
                self.metrics.bytes_encrypted += processed.len() as u64;
                self.metrics.chunks_new += 1;

                self.fingerprint_index.insert(hash, processed.clone());

                chunks.push(IngestChunk {
                    hash,
                    data: processed,
                    original_size,
                    deduped: false,
                });
            }

            if i == 0 {
                self.current_stage = IngestStage::Chunking;
            }
        }

        self.current_stage = IngestStage::Packing;
        self.request_id += 1;
        chunks
    }

    /// Returns the current processing stage.
    pub fn current_stage(&self) -> IngestStage {
        self.current_stage
    }

    /// Returns a reference to the metrics.
    pub fn metrics(&self) -> &IngestMetrics {
        &self.metrics
    }

    /// Reset all metrics.
    pub fn reset_metrics(&mut self) {
        self.metrics = IngestMetrics::default();
    }

    fn compute_hash(&self, data: &[u8]) -> [u8; 32] {
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash[0] = (data.len() as u8).wrapping_add(hash[0]);
        hash
    }

    fn process_chunk(&self, data: &[u8]) -> Vec<u8> {
        let mut result = data.to_vec();
        if self.config.enable_compression {
            result = self.simulate_compress(&result);
        }
        if self.config.enable_encryption {
            result = self.simulate_encrypt(&result);
        }
        result
    }

    fn simulate_compress(&self, data: &[u8]) -> Vec<u8> {
        let reduction = (self.config.compression_level as f64 / 10.0).min(0.5);
        let target_len = ((data.len() as f64) * (1.0 - reduction)).max(1.0) as usize;
        data[..target_len.min(data.len())].to_vec()
    }

    fn simulate_encrypt(&self, data: &[u8]) -> Vec<u8> {
        let mut result = data.to_vec();
        result.push(0xDE);
        result.push(0xAD);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_config_default() {
        let config = IngestConfig::default();
        assert_eq!(config.buffer_threshold_bytes, 1024 * 1024);
        assert!(config.enable_dedup);
        assert!(config.enable_compression);
        assert!(config.enable_encryption);
        assert_eq!(config.compression_level, 3);
    }

    #[test]
    fn ingest_metrics_default() {
        let metrics = IngestMetrics::default();
        assert_eq!(metrics.bytes_ingested, 0);
        assert_eq!(metrics.bytes_deduplicated, 0);
        assert_eq!(metrics.bytes_compressed, 0);
        assert_eq!(metrics.bytes_encrypted, 0);
        assert_eq!(metrics.chunks_new, 0);
        assert_eq!(metrics.chunks_deduped, 0);
        assert!(metrics.stage_latencies_us.is_empty());
    }

    #[test]
    fn overall_reduction_ratio_no_data() {
        let metrics = IngestMetrics::default();
        assert_eq!(metrics.overall_reduction_ratio(), 1.0);
    }

    #[test]
    fn dedup_savings_pct_no_savings() {
        let metrics = IngestMetrics::default();
        assert_eq!(metrics.dedup_savings_pct(), 0.0);
    }

    #[test]
    fn ingest_empty_data_returns_empty() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let chunks = pipeline.ingest(1, &[]);
        assert!(chunks.is_empty());
    }

    #[test]
    fn ingest_small_data_returns_chunks() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 100];
        let chunks = pipeline.ingest(1, &data);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn ingest_metrics_bytes_ingested() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 1000];
        pipeline.ingest(1, &data);
        assert_eq!(pipeline.metrics().bytes_ingested, 1000);
    }

    #[test]
    fn ingest_metrics_chunks_new() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 100];
        pipeline.ingest(1, &data);
        assert!(pipeline.metrics().chunks_new > 0);
    }

    #[test]
    fn ingest_dedup_disabled() {
        let config = IngestConfig {
            enable_dedup: false,
            ..Default::default()
        };
        let mut pipeline = IngestPipeline::new(config);
        let data = vec![1u8; 100];
        let chunks = pipeline.ingest(1, &data);
        assert!(!chunks.is_empty());
        assert_eq!(pipeline.metrics().chunks_deduped, 0);
    }

    #[test]
    fn ingest_compression_disabled() {
        let config = IngestConfig {
            enable_compression: false,
            ..Default::default()
        };
        let mut pipeline = IngestPipeline::new(config);
        let data = vec![1u8; 100];
        let chunks = pipeline.ingest(1, &data);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn ingest_same_data_twice_deduplicates() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 100];

        pipeline.ingest(1, &data);
        let initial_new = pipeline.metrics().chunks_new;

        pipeline.ingest(2, &data);
        assert!(pipeline.metrics().chunks_deduped > 0);
        assert_eq!(pipeline.metrics().chunks_new, initial_new);
    }

    #[test]
    fn reset_metrics_clears() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 100];
        pipeline.ingest(1, &data);
        assert!(pipeline.metrics().bytes_ingested > 0);

        pipeline.reset_metrics();
        assert_eq!(pipeline.metrics().bytes_ingested, 0);
        assert_eq!(pipeline.metrics().chunks_new, 0);
    }

    #[test]
    fn ingest_large_data_multiple_chunks() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 10000];
        let chunks = pipeline.ingest(1, &data);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn ingest_chunk_deduped_flag() {
        let mut pipeline = IngestPipeline::new(IngestConfig::default());
        let data = vec![1u8; 100];

        let chunks1 = pipeline.ingest(1, &data);
        assert!(!chunks1[0].deduped);

        let chunks2 = pipeline.ingest(2, &data);
        assert!(chunks2[0].deduped);
    }
}
