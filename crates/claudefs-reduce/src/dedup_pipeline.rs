//! Integrated deduplication pipeline that ties together CDC chunking, CAS lookup,
//! and fingerprint store for the full write path.

use fastcdc::v2020::FastCDC;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Result of deduplication for a single chunk.
#[derive(Debug, Clone)]
pub enum DedupResult {
    /// Chunk was deduplicated against an existing hash.
    Deduplicated {
        /// Hash of the existing chunk.
        existing_hash: [u8; 32],
    },
    /// Chunk is new and needs to be stored.
    New {
        /// Hash of the new chunk.
        hash: [u8; 32],
        /// Data for the new chunk.
        data: Vec<u8>,
    },
}

/// Configuration for the dedup pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupPipelineConfig {
    /// Minimum chunk size for CDC.
    pub min_chunk_size: u32,
    /// Maximum chunk size for CDC.
    pub max_chunk_size: u32,
    /// Average (target) chunk size for CDC.
    pub avg_chunk_size: u32,
    /// Enable similarity detection for better dedup.
    pub enable_similarity: bool,
}

impl Default for DedupPipelineConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 4096,
            max_chunk_size: 65536,
            avg_chunk_size: 16384,
            enable_similarity: true,
        }
    }
}

/// Statistics for the dedup pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DedupStats {
    /// Total chunks processed.
    pub chunks_total: u64,
    /// Chunks that were deduplicated.
    pub chunks_deduped: u64,
    /// Total bytes input.
    pub bytes_in: u64,
    /// Bytes saved by deduplication.
    pub bytes_deduped: u64,
}

impl DedupStats {
    /// Deduplication ratio: bytes_deduped / bytes_in.
    pub fn dedup_ratio(&self) -> f64 {
        if self.bytes_in == 0 {
            return 0.0;
        }
        self.bytes_deduped as f64 / self.bytes_in as f64
    }

    /// Unique ratio: (bytes_in - bytes_deduped) / bytes_in.
    pub fn unique_ratio(&self) -> f64 {
        if self.bytes_in == 0 {
            return 1.0;
        }
        (self.bytes_in - self.bytes_deduped) as f64 / self.bytes_in as f64
    }
}

/// Deduplication pipeline combining CDC chunking with CAS lookup.
pub struct DedupPipeline {
    config: DedupPipelineConfig,
    seen: HashSet<[u8; 32]>,
    stats: DedupStats,
}

impl DedupPipeline {
    /// Create a new dedup pipeline with the given configuration.
    pub fn new(config: DedupPipelineConfig) -> Self {
        Self {
            config,
            seen: HashSet::new(),
            stats: DedupStats::default(),
        }
    }

    /// Process a single chunk of data.
    ///
    /// Hashes the data and returns Deduplicated if seen before, New otherwise.
    pub fn process_chunk(&mut self, data: &[u8]) -> DedupResult {
        let hash = *blake3::hash(data).as_bytes();

        self.stats.chunks_total += 1;
        self.stats.bytes_in += data.len() as u64;

        if self.seen.contains(&hash) {
            self.stats.chunks_deduped += 1;
            self.stats.bytes_deduped += data.len() as u64;
            DedupResult::Deduplicated {
                existing_hash: hash,
            }
        } else {
            self.seen.insert(hash);
            DedupResult::New {
                hash,
                data: data.to_vec(),
            }
        }
    }

    /// Process a block of data by chunking it with FastCDC and deduplicating each chunk.
    ///
    /// Returns a Vec of DedupResult for each chunk.
    pub fn process_data(&mut self, data: &[u8]) -> Vec<DedupResult> {
        if data.is_empty() {
            return Vec::new();
        }

        let min_size = self.config.min_chunk_size;
        let avg_size = self.config.avg_chunk_size;
        let max_size = self.config.max_chunk_size;

        let chunker = FastCDC::new(data, min_size, avg_size, max_size);
        let chunks: Vec<_> = chunker.collect();

        chunks
            .into_iter()
            .map(|chunk| {
                let start = chunk.offset;
                let end = start + chunk.length;
                let chunk_data = &data[start..end];
                self.process_chunk(chunk_data)
            })
            .collect()
    }

    /// Get current statistics.
    pub fn stats(&self) -> &DedupStats {
        &self.stats
    }

    /// Reset the pipeline, clearing seen hashes and stats.
    pub fn reset(&mut self) {
        self.seen.clear();
        self.stats = DedupStats::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_pipeline_config_default() {
        let config = DedupPipelineConfig::default();
        assert_eq!(config.min_chunk_size, 4096);
        assert_eq!(config.max_chunk_size, 65536);
        assert_eq!(config.avg_chunk_size, 16384);
        assert!(config.enable_similarity);
    }

    #[test]
    fn dedup_stats_default() {
        let stats = DedupStats::default();
        assert_eq!(stats.chunks_total, 0);
        assert_eq!(stats.chunks_deduped, 0);
        assert_eq!(stats.bytes_in, 0);
        assert_eq!(stats.bytes_deduped, 0);
    }

    #[test]
    fn dedup_ratio_zero_bytes_in() {
        let stats = DedupStats::default();
        assert_eq!(stats.dedup_ratio(), 0.0);
    }

    #[test]
    fn dedup_ratio_all_deduped() {
        let stats = DedupStats {
            chunks_total: 10,
            chunks_deduped: 10,
            bytes_in: 1000,
            bytes_deduped: 1000,
        };
        assert_eq!(stats.dedup_ratio(), 1.0);
    }

    #[test]
    fn unique_ratio_no_dedup() {
        let stats = DedupStats {
            chunks_total: 10,
            chunks_deduped: 0,
            bytes_in: 1000,
            bytes_deduped: 0,
        };
        assert_eq!(stats.unique_ratio(), 1.0);
    }

    #[test]
    fn process_chunk_new() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let data = b"hello world";
        let result = pipeline.process_chunk(data);

        match result {
            DedupResult::New { hash, data: d } => {
                let expected = *blake3::hash(b"hello world").as_bytes();
                assert_eq!(hash, expected);
                assert_eq!(d, b"hello world");
            }
            DedupResult::Deduplicated { .. } => panic!("Expected New"),
        }
    }

    #[test]
    fn process_chunk_dedup() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let data = b"hello world";

        let _first = pipeline.process_chunk(data);
        let second = pipeline.process_chunk(data);

        match second {
            DedupResult::Deduplicated { existing_hash } => {
                let expected = *blake3::hash(b"hello world").as_bytes();
                assert_eq!(existing_hash, expected);
            }
            DedupResult::New { .. } => panic!("Expected Deduplicated"),
        }
    }

    #[test]
    fn process_chunk_different_data() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let data1 = b"hello world";
        let data2 = b"goodbye world";

        let result1 = pipeline.process_chunk(data1);
        let result2 = pipeline.process_chunk(data2);

        assert!(matches!(result1, DedupResult::New { .. }));
        assert!(matches!(result2, DedupResult::New { .. }));
    }

    #[test]
    fn process_data_single_chunk_below_min() {
        let config = DedupPipelineConfig {
            min_chunk_size: 4096,
            avg_chunk_size: 8192,
            max_chunk_size: 16384,
            enable_similarity: true,
        };
        let mut pipeline = DedupPipeline::new(config);
        let data = b"small";

        let results = pipeline.process_data(data);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn process_data_splits_large_data() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let data: Vec<u8> = (0u8..=255u8).cycle().take(200 * 1024).collect();

        let results = pipeline.process_data(&data);
        assert!(results.len() > 1, "Should split into multiple chunks");
    }

    #[test]
    fn process_data_deduplicates_repeated_block() {
        let config = DedupPipelineConfig {
            min_chunk_size: 1024,
            avg_chunk_size: 2048,
            max_chunk_size: 4096,
            enable_similarity: true,
        };
        let mut pipeline = DedupPipeline::new(config);

        let block: Vec<u8> = (0u8..=255u8).cycle().take(4 * 1024).collect();
        let mut data = Vec::new();
        data.extend_from_slice(&block);
        data.extend_from_slice(&block);

        let results = pipeline.process_data(&data);

        let mut dedup_count = 0;
        for result in &results {
            match result {
                DedupResult::New { .. } => {}
                DedupResult::Deduplicated { .. } => dedup_count += 1,
            }
        }
        assert!(dedup_count > 0, "Should have deduplicated some chunks");
    }

    #[test]
    fn stats_tracks_chunks_total() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        pipeline.process_chunk(b"data1");
        pipeline.process_chunk(b"data2");
        assert_eq!(pipeline.stats().chunks_total, 2);
    }

    #[test]
    fn stats_tracks_bytes_in() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        pipeline.process_chunk(b"data1");
        pipeline.process_chunk(b"data2");
        assert_eq!(pipeline.stats().bytes_in, 10);
    }

    #[test]
    fn reset_clears_seen_hashes() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let data = b"hello world";

        let _first = pipeline.process_chunk(data);
        pipeline.reset();
        let second = pipeline.process_chunk(data);

        assert!(matches!(second, DedupResult::New { .. }));
    }

    #[test]
    fn reset_clears_stats() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        pipeline.process_chunk(b"data");
        pipeline.reset();
        assert_eq!(pipeline.stats().chunks_total, 0);
        assert_eq!(pipeline.stats().bytes_in, 0);
    }

    #[test]
    fn process_data_empty_returns_empty() {
        let mut pipeline = DedupPipeline::new(DedupPipelineConfig::default());
        let results = pipeline.process_data(&[]);
        assert!(results.is_empty());
    }
}
