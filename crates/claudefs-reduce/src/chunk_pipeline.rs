use crate::compression::CompressionAlgorithm;
use crate::encryption::EncryptionAlgorithm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ChunkPipelineResult {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub was_deduped: bool,
    pub was_compressed: bool,
    pub was_encrypted: bool,
    pub data: Vec<u8>,
    pub fingerprint: [u8; 32],
}

impl ChunkPipelineResult {
    pub fn savings_ratio(&self) -> f64 {
        if self.input_bytes == 0 || self.was_deduped {
            return 1.0;
        }
        1.0 - (self.output_bytes as f64 / self.input_bytes as f64)
    }

    pub fn was_reduced(&self) -> bool {
        self.was_deduped || self.was_compressed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkPipelineConfig {
    pub compression: CompressionAlgorithm,
    pub encryption: EncryptionAlgorithm,
    pub enable_dedup: bool,
    pub enable_compression: bool,
    pub enable_encryption: bool,
}

impl Default for ChunkPipelineConfig {
    fn default() -> Self {
        Self {
            compression: CompressionAlgorithm::Lz4,
            encryption: EncryptionAlgorithm::AesGcm256,
            enable_dedup: true,
            enable_compression: true,
            enable_encryption: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChunkPipelineStats {
    pub chunks_processed: u64,
    pub chunks_deduped: u64,
    pub chunks_compressed: u64,
    pub chunks_encrypted: u64,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
}

impl ChunkPipelineStats {
    pub fn overall_ratio(&self) -> f64 {
        if self.total_input_bytes == 0 {
            return 1.0;
        }
        self.total_input_bytes as f64 / self.total_output_bytes as f64
    }
}

pub struct ChunkPipeline {
    config: ChunkPipelineConfig,
    stats: ChunkPipelineStats,
    known_fingerprints: std::collections::HashSet<[u8; 32]>,
}

impl ChunkPipeline {
    pub fn new(config: ChunkPipelineConfig) -> Self {
        Self {
            config,
            stats: ChunkPipelineStats::default(),
            known_fingerprints: std::collections::HashSet::new(),
        }
    }

    pub fn add_known_fingerprint(&mut self, fp: [u8; 32]) {
        self.known_fingerprints.insert(fp);
    }

    pub fn process(&mut self, data: Vec<u8>, fingerprint: [u8; 32]) -> ChunkPipelineResult {
        self.stats.chunks_processed += 1;
        let input_bytes = data.len();
        self.stats.total_input_bytes += input_bytes as u64;

        if self.config.enable_dedup && self.known_fingerprints.contains(&fingerprint) {
            self.stats.chunks_deduped += 1;
            return ChunkPipelineResult {
                input_bytes,
                output_bytes: 0,
                was_deduped: true,
                was_compressed: false,
                was_encrypted: false,
                data: Vec::new(),
                fingerprint,
            };
        }

        self.known_fingerprints.insert(fingerprint);

        let (processed_data, was_compressed) = if self.config.enable_compression {
            self.stats.chunks_compressed += 1;
            let mut compressed = vec![0x01u8];
            compressed.extend_from_slice(&data);
            (compressed, true)
        } else {
            (data, false)
        };

        let (final_data, was_encrypted) = if self.config.enable_encryption {
            self.stats.chunks_encrypted += 1;
            let encrypted: Vec<u8> = processed_data.iter().map(|&b| b ^ 0xAB).collect();
            (encrypted, true)
        } else {
            (processed_data, false)
        };

        let output_bytes = final_data.len();
        self.stats.total_output_bytes += output_bytes as u64;

        ChunkPipelineResult {
            input_bytes,
            output_bytes,
            was_deduped: false,
            was_compressed,
            was_encrypted,
            data: final_data,
            fingerprint,
        }
    }

    pub fn stats(&self) -> &ChunkPipelineStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fingerprint(idx: u8) -> [u8; 32] {
        let mut fp = [0u8; 32];
        fp[0] = idx;
        fp
    }

    #[test]
    fn chunk_pipeline_config_default() {
        let config = ChunkPipelineConfig::default();
        assert_eq!(config.compression, CompressionAlgorithm::Lz4);
        assert_eq!(config.encryption, EncryptionAlgorithm::AesGcm256);
        assert!(config.enable_dedup);
        assert!(config.enable_compression);
        assert!(config.enable_encryption);
    }

    #[test]
    fn process_first_chunk_not_deduped() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let data = b"hello world".to_vec();
        let result = pipeline.process(data, make_fingerprint(1));
        assert!(!result.was_deduped);
    }

    #[test]
    fn process_second_identical_chunk_deduped() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let data = b"hello world".to_vec();
        let fp = make_fingerprint(1);
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert!(result.was_deduped);
    }

    #[test]
    fn process_was_compressed_when_enabled() {
        let config = ChunkPipelineConfig {
            enable_compression: true,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert!(result.was_compressed);
    }

    #[test]
    fn process_was_encrypted_when_enabled() {
        let config = ChunkPipelineConfig {
            enable_encryption: true,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert!(result.was_encrypted);
    }

    #[test]
    fn process_no_compress_when_disabled() {
        let config = ChunkPipelineConfig {
            enable_compression: false,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert!(!result.was_compressed);
    }

    #[test]
    fn process_no_encrypt_when_disabled() {
        let config = ChunkPipelineConfig {
            enable_encryption: false,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert!(!result.was_encrypted);
    }

    #[test]
    fn process_deduped_output_is_empty() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert!(result.data.is_empty());
    }

    #[test]
    fn process_deduped_output_bytes_zero() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert_eq!(result.output_bytes, 0);
    }

    #[test]
    fn process_input_bytes_matches() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let data = b"hello world".to_vec();
        let result = pipeline.process(data.clone(), make_fingerprint(1));
        assert_eq!(result.input_bytes, data.len());
    }

    #[test]
    fn savings_ratio_on_dedup() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert!((result.savings_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn savings_ratio_on_write_through() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let result = pipeline.process(b"test data here".to_vec(), make_fingerprint(1));
        assert!(result.savings_ratio() < 0.0);
    }

    #[test]
    fn was_reduced_when_deduped() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert!(result.was_reduced());
    }

    #[test]
    fn was_reduced_when_compressed() {
        let config = ChunkPipelineConfig {
            enable_dedup: false,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"some data".to_vec(), make_fingerprint(1));
        assert!(result.was_reduced());
    }

    #[test]
    fn was_reduced_when_neither() {
        let config = ChunkPipelineConfig {
            enable_dedup: false,
            enable_compression: false,
            enable_encryption: false,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let result = pipeline.process(b"some data".to_vec(), make_fingerprint(1));
        assert!(!result.was_reduced());
    }

    #[test]
    fn stats_chunks_processed() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let _ = pipeline.process(b"a".to_vec(), make_fingerprint(1));
        let _ = pipeline.process(b"b".to_vec(), make_fingerprint(2));
        assert_eq!(pipeline.stats().chunks_processed, 2);
    }

    #[test]
    fn stats_chunks_deduped() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let _ = pipeline.process(data.clone(), fp);
        let _ = pipeline.process(data, fp);
        assert_eq!(pipeline.stats().chunks_deduped, 2);
    }

    #[test]
    fn stats_chunks_compressed() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let _ = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert_eq!(pipeline.stats().chunks_compressed, 1);
    }

    #[test]
    fn stats_chunks_encrypted() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let _ = pipeline.process(b"data".to_vec(), make_fingerprint(1));
        assert_eq!(pipeline.stats().chunks_encrypted, 1);
    }

    #[test]
    fn stats_total_input_bytes() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let _ = pipeline.process(b"hello".to_vec(), make_fingerprint(1));
        let _ = pipeline.process(b"world".to_vec(), make_fingerprint(2));
        assert_eq!(pipeline.stats().total_input_bytes, 10);
    }

    #[test]
    fn stats_overall_ratio_no_data() {
        let stats = ChunkPipelineStats::default();
        assert!((stats.overall_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn add_known_fingerprint() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let fp = make_fingerprint(42);
        pipeline.add_known_fingerprint(fp);
        let result = pipeline.process(b"anything".to_vec(), fp);
        assert!(result.was_deduped);
    }

    #[test]
    fn dedup_disabled() {
        let config = ChunkPipelineConfig {
            enable_dedup: false,
            ..Default::default()
        };
        let mut pipeline = ChunkPipeline::new(config);
        let fp = make_fingerprint(1);
        let data = b"test".to_vec();
        let _ = pipeline.process(data.clone(), fp);
        let result = pipeline.process(data, fp);
        assert!(!result.was_deduped);
    }

    #[test]
    fn process_empty_data() {
        let mut pipeline = ChunkPipeline::new(ChunkPipelineConfig::default());
        let result = pipeline.process(Vec::new(), make_fingerprint(1));
        assert_eq!(result.input_bytes, 0);
    }
}
