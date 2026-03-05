//! AI-assisted data classification for workload-specific tiering.
//!
//! Adaptive classifier learns from reduction stats and makes recommendations
//! for compression levels, dedup strength, and S3 tiering policies.

use crate::chunk_pipeline::ChunkPipelineStats;
use crate::error::ReduceError;
use crate::similarity_coordinator::CoordinatorStats;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Access pattern classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessPatternType {
    /// Sequential access pattern (logs, archives).
    Sequential,
    /// Random access pattern (databases, VMs).
    Random,
    /// Temporal locality (recently accessed likely accessed again).
    Temporal,
    /// Highly compressible data.
    Compressible,
    /// High similarity hit rate workload.
    SimilarityHigh,
    /// Low similarity hit rate workload.
    SimilarityLow,
    /// Unknown pattern (insufficient data).
    Unknown,
}

impl Default for AccessPatternType {
    fn default() -> Self {
        AccessPatternType::Unknown
    }
}

/// Compression level recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// No compression.
    None,
    /// Fast compression (LZ4).
    Fast,
    /// Default compression (Zstd level 3).
    Default,
    /// Best compression (Zstd level 19).
    Best,
}

impl Default for CompressionLevel {
    fn default() -> Self {
        CompressionLevel::Default
    }
}

/// Deduplication strength recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupStrength {
    /// Inline dedup (fast path).
    Inline,
    /// Async dedup (batch processing).
    Async,
    /// No dedup.
    None,
}

impl Default for DedupStrength {
    fn default() -> Self {
        DedupStrength::Inline
    }
}

/// S3 tiering policy recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3TieringPolicy {
    /// Flash tier (keep in NVMe).
    Flash,
    /// Warm tier (S3 Standard).
    Warm,
    /// Cold tier (S3 Infrequent Access).
    Cold,
    /// Archive tier (S3 Glacier).
    Archive,
}

impl Default for S3TieringPolicy {
    fn default() -> Self {
        S3TieringPolicy::Flash
    }
}

/// Workload fingerprint: learned access pattern characteristics.
#[derive(Debug, Clone)]
pub struct WorkloadFingerprint {
    /// Access pattern type.
    pub pattern_type: AccessPatternType,
    /// Similarity hit rate (0.0 to 1.0).
    pub similarity_hit_rate: f64,
    /// Compression effectiveness ratio.
    pub compression_effectiveness: f64,
    /// Average chunk size in bytes.
    pub chunk_size_avg: usize,
    /// Write frequency (operations per second).
    pub write_frequency: f64,
    /// Age of fingerprint in seconds.
    pub age_secs: u64,
}

impl Default for WorkloadFingerprint {
    fn default() -> Self {
        Self {
            pattern_type: AccessPatternType::Unknown,
            similarity_hit_rate: 0.0,
            compression_effectiveness: 0.0,
            chunk_size_avg: 0,
            write_frequency: 0.0,
            age_secs: 0,
        }
    }
}

/// Adaptive tiering recommendation.
#[derive(Debug, Clone)]
pub struct TieringRecommendation {
    /// Detected access pattern.
    pub pattern: AccessPatternType,
    /// Recommended compression level.
    pub compression_level: CompressionLevel,
    /// Recommended dedup strength.
    pub dedup_strength: DedupStrength,
    /// Recommended S3 tiering policy.
    pub s3_tiering_policy: S3TieringPolicy,
    /// Confidence in recommendation (0.0 to 1.0).
    pub confidence: f64,
}

impl Default for TieringRecommendation {
    fn default() -> Self {
        Self {
            pattern: AccessPatternType::Unknown,
            compression_level: CompressionLevel::Default,
            dedup_strength: DedupStrength::Inline,
            s3_tiering_policy: S3TieringPolicy::Flash,
            confidence: 0.0,
        }
    }
}

/// Classifier configuration.
#[derive(Debug, Clone)]
pub struct ClassifierConfig {
    /// Learning window in seconds.
    pub learning_window_secs: u64,
    /// Minimum samples before classification.
    pub min_samples: usize,
    /// Similarity threshold for high hit rate classification.
    pub similarity_threshold_for_high: f64,
    /// Compression threshold for high effectiveness classification.
    pub compression_threshold_for_high: f64,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            learning_window_secs: 3600,
            min_samples: 10,
            similarity_threshold_for_high: 0.5,
            compression_threshold_for_high: 0.5,
        }
    }
}

/// Classifier statistics for historical tracking.
#[derive(Debug, Clone)]
pub struct ClassifierStats {
    /// Workload identifier.
    pub workload: String,
    /// Unix timestamp.
    pub timestamp: u64,
    /// Similarity hit rate.
    pub similarity_hit_rate: f64,
    /// Compression ratio achieved.
    pub compression_ratio: f64,
    /// Observed access pattern.
    pub pattern_observed: AccessPatternType,
}

/// Adaptive classifier for workload-aware tiering.
pub struct AdaptiveClassifier {
    fingerprints: Arc<RwLock<HashMap<String, WorkloadFingerprint>>>,
    stats_history: Arc<RwLock<Vec<ClassifierStats>>>,
    config: ClassifierConfig,
    last_update: Arc<RwLock<HashMap<String, Instant>>>,
}

impl AdaptiveClassifier {
    /// Create new adaptive classifier.
    pub fn new(config: ClassifierConfig) -> Self {
        Self {
            fingerprints: Arc::new(RwLock::new(HashMap::new())),
            stats_history: Arc::new(RwLock::new(Vec::new())),
            config,
            last_update: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Update fingerprint with new observation.
    pub fn update_fingerprint(
        &self,
        workload: &str,
        pipeline_stats: &ChunkPipelineStats,
        coordinator_stats: &CoordinatorStats,
    ) -> Result<(), ReduceError> {
        if workload.is_empty() {
            return Err(ReduceError::InvalidInput(
                "workload name cannot be empty".to_string(),
            ));
        }

        let current_time = Instant::now();
        let similarity_hit_rate = coordinator_stats.similarity_hit_rate();
        let compression_ratio = if pipeline_stats.total_input_bytes > 0 {
            pipeline_stats.total_output_bytes as f64 / pipeline_stats.total_input_bytes as f64
        } else {
            1.0
        };
        let chunk_size_avg = if pipeline_stats.chunks_processed > 0 {
            (pipeline_stats.total_input_bytes / pipeline_stats.chunks_processed) as usize
        } else {
            0
        };

        let pattern = Self::infer_pattern(similarity_hit_rate, compression_ratio);

        let mut fingerprints = self.fingerprints.write().unwrap();
        let entry = fingerprints
            .entry(workload.to_string())
            .or_insert_with(WorkloadFingerprint::default);

        let samples = self
            .stats_history
            .read()
            .unwrap()
            .iter()
            .filter(|s| s.workload == workload)
            .count()
            + 1;

        let smoothing = 1.0 / (samples as f64).min(10.0);
        entry.similarity_hit_rate =
            entry.similarity_hit_rate * (1.0 - smoothing) + similarity_hit_rate * smoothing;
        entry.compression_effectiveness = entry.compression_effectiveness * (1.0 - smoothing)
            + (1.0 - compression_ratio).min(1.0) * smoothing;

        if entry.chunk_size_avg == 0 {
            entry.chunk_size_avg = chunk_size_avg;
        } else {
            entry.chunk_size_avg = (entry.chunk_size_avg * (1.0 - smoothing)
                + chunk_size_avg as f64 * smoothing) as usize;
        }

        entry.pattern_type = if samples >= self.config.min_samples {
            pattern
        } else {
            AccessPatternType::Unknown
        };

        let mut last_update = self.last_update.write().unwrap();
        if let Some(last) = last_update.get(workload) {
            let elapsed = current_time.duration_since(*last).as_secs();
            entry.age_secs += elapsed;
        }
        last_update.insert(workload.to_string(), current_time);

        let mut history = self.stats_history.write().unwrap();
        history.push(ClassifierStats {
            workload: workload.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            similarity_hit_rate,
            compression_ratio,
            pattern_observed: pattern,
        });

        while history.len() > 1000 {
            history.remove(0);
        }

        Ok(())
    }

    fn infer_pattern(similarity_hit_rate: f64, compression_ratio: f64) -> AccessPatternType {
        if similarity_hit_rate >= 0.5 {
            AccessPatternType::SimilarityHigh
        } else if similarity_hit_rate < 0.1 {
            AccessPatternType::SimilarityLow
        } else if compression_ratio < 0.5 {
            AccessPatternType::Compressible
        } else {
            AccessPatternType::Random
        }
    }

    /// Classify a workload based on learned fingerprint.
    pub fn classify_pattern(&self, workload: &str) -> Result<AccessPatternType, ReduceError> {
        let fingerprints = self.fingerprints.read().unwrap();

        if let Some(fp) = fingerprints.get(workload) {
            Ok(fp.pattern_type)
        } else {
            Ok(AccessPatternType::Unknown)
        }
    }

    /// Get tiering recommendation for a workload.
    pub fn recommend_tiering(&self, workload: &str) -> Result<TieringRecommendation, ReduceError> {
        let fingerprints = self.fingerprints.read().unwrap();

        let fp = match fingerprints.get(workload) {
            Some(f) => f,
            None => {
                return Err(ReduceError::NotFound(format!(
                    "workload {} not found",
                    workload
                )))
            }
        };

        let compression_level = self.recommend_compression_level_internal(fp);
        let dedup_strength = self.recommend_dedup_strength_internal(fp);
        let s3_tiering_policy = self.recommend_s3_policy_internal(fp);

        let confidence = if fp.pattern_type == AccessPatternType::Unknown {
            0.0
        } else {
            ((fp.similarity_hit_rate + fp.compression_effectiveness) / 2.0).min(1.0)
        };

        Ok(TieringRecommendation {
            pattern: fp.pattern_type,
            compression_level,
            dedup_strength,
            s3_tiering_policy,
            confidence,
        })
    }

    fn recommend_compression_level_internal(fp: &WorkloadFingerprint) -> CompressionLevel {
        if fp.compression_effectiveness > 0.7 {
            CompressionLevel::Best
        } else if fp.compression_effectiveness > 0.4 {
            CompressionLevel::Default
        } else if fp.compression_effectiveness > 0.2 {
            CompressionLevel::Fast
        } else {
            CompressionLevel::None
        }
    }

    fn recommend_dedup_strength_internal(fp: &WorkloadFingerprint) -> DedupStrength {
        if fp.similarity_hit_rate > 0.6 {
            DedupStrength::Async
        } else if fp.similarity_hit_rate > 0.3 {
            DedupStrength::Inline
        } else {
            DedupStrength::None
        }
    }

    fn recommend_s3_policy_internal(fp: &WorkloadFingerprint) -> S3TieringPolicy {
        if fp.write_frequency > 10.0 {
            S3TieringPolicy::Flash
        } else if fp.pattern_type == AccessPatternType::Temporal {
            S3TieringPolicy::Warm
        } else if fp.compression_effectiveness > 0.6 {
            S3TieringPolicy::Cold
        } else {
            S3TieringPolicy::Archive
        }
    }

    /// Get compression level recommendation.
    pub fn recommend_compression_level(
        &self,
        workload: &str,
    ) -> Result<CompressionLevel, ReduceError> {
        let fingerprints = self.fingerprints.read().unwrap();

        match fingerprints.get(workload) {
            Some(fp) => Ok(Self::recommend_compression_level_internal(fp)),
            None => Ok(CompressionLevel::Default),
        }
    }

    /// Get dedup strength recommendation.
    pub fn recommend_dedup_strength(&self, workload: &str) -> Result<DedupStrength, ReduceError> {
        let fingerprints = self.fingerprints.read().unwrap();

        match fingerprints.get(workload) {
            Some(fp) => Ok(Self::recommend_dedup_strength_internal(fp)),
            None => Ok(DedupStrength::Inline),
        }
    }

    /// Get S3 tiering policy recommendation.
    pub fn recommend_s3_policy(&self, workload: &str) -> Result<S3TieringPolicy, ReduceError> {
        let fingerprints = self.fingerprints.read().unwrap();

        match fingerprints.get(workload) {
            Some(fp) => Ok(Self::recommend_s3_policy_internal(fp)),
            None => Ok(S3TieringPolicy::Flash),
        }
    }

    /// Get workload fingerprint (read-only).
    pub fn get_fingerprint(&self, workload: &str) -> Option<WorkloadFingerprint> {
        let fingerprints = self.fingerprints.read().unwrap();
        fingerprints.get(workload).cloned()
    }

    /// Reset learning for a workload.
    pub fn reset_workload(&self, workload: &str) {
        let mut fingerprints = self.fingerprints.write().unwrap();
        fingerprints.remove(workload);

        let mut last_update = self.last_update.write().unwrap();
        last_update.remove(workload);

        let mut history = self.stats_history.write().unwrap();
        history.retain(|s| s.workload != workload);
    }

    /// Get all known workloads.
    pub fn list_workloads(&self) -> Vec<String> {
        let fingerprints = self.fingerprints.read().unwrap();
        fingerprints.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pipeline_stats(chunks: u64, input: u64, output: u64) -> ChunkPipelineStats {
        ChunkPipelineStats {
            chunks_processed: chunks,
            chunks_deduped: 0,
            chunks_compressed: chunks,
            chunks_encrypted: 0,
            total_input_bytes: input,
            total_output_bytes: output,
        }
    }

    fn make_coordinator_stats(lookups: u64, hits: u64) -> CoordinatorStats {
        CoordinatorStats {
            similarity_lookups: lookups,
            similarity_hits: hits,
            ..Default::default()
        }
    }

    mod initialization {
        use super::*;

        #[test]
        fn test_new_classifier() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig::default());
            let workloads = classifier.list_workloads();
            assert!(workloads.is_empty());
        }

        #[test]
        fn test_custom_config() {
            let config = ClassifierConfig {
                learning_window_secs: 7200,
                min_samples: 20,
                similarity_threshold_for_high: 0.7,
                compression_threshold_for_high: 0.6,
            };
            let classifier = AdaptiveClassifier::new(config);
            assert!(classifier.list_workloads().is_empty());
        }
    }

    mod fingerprint_learning {
        use super::*;

        #[test]
        fn test_update_fingerprint_sequential() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let pipeline = make_pipeline_stats(100, 100000, 30000);
            let coordinator = make_coordinator_stats(100, 10);

            let result = classifier.update_fingerprint("workload1", &pipeline, &coordinator);
            assert!(result.is_ok());

            let fp = classifier.get_fingerprint("workload1");
            assert!(fp.is_some());
        }

        #[test]
        fn test_update_fingerprint_random() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let pipeline = make_pipeline_stats(100, 100000, 90000);
            let coordinator = make_coordinator_stats(100, 5);

            let result = classifier.update_fingerprint("workload2", &pipeline, &coordinator);
            assert!(result.is_ok());

            let fp = classifier.get_fingerprint("workload2");
            assert!(fp.is_some());
        }

        #[test]
        fn test_update_fingerprint_temporal() {
            let classifier = AdaptiveClassifier::new(Default::default());

            for i in 0..15 {
                let pipeline = make_pipeline_stats(10, 10000, 5000);
                let coordinator = make_coordinator_stats(10, 8);
                let _ = classifier.update_fingerprint("temporal_workload", &pipeline, &coordinator);
            }

            let fp = classifier.get_fingerprint("temporal_workload");
            assert!(fp.is_some());
        }

        #[test]
        fn test_update_fingerprint_insufficient_data() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 20,
                ..Default::default()
            });

            let pipeline = make_pipeline_stats(5, 5000, 2500);
            let coordinator = make_coordinator_stats(5, 4);
            let _ = classifier.update_fingerprint("new_workload", &pipeline, &coordinator);

            let pattern = classifier.classify_pattern("new_workload");
            assert!(pattern.is_ok());
            assert_eq!(pattern.unwrap(), AccessPatternType::Unknown);
        }

        #[test]
        fn test_fingerprint_converges() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            let initial_stats = make_coordinator_stats(10, 1);
            let pipeline1 = make_pipeline_stats(10, 10000, 9000);
            let _ = classifier.update_fingerprint("converge_test", &pipeline1, &initial_stats);

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 3000);
                let coordinator = make_coordinator_stats(10, 8);
                let _ = classifier.update_fingerprint("converge_test", &pipeline, &coordinator);
            }

            let fp = classifier.get_fingerprint("converge_test");
            assert!(fp.is_some());
            assert!(fp.unwrap().similarity_hit_rate > 0.0);
        }

        #[test]
        fn test_fingerprint_age_tracked() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let pipeline = make_pipeline_stats(10, 10000, 5000);
            let coordinator = make_coordinator_stats(10, 5);

            let _ = classifier.update_fingerprint("age_test", &pipeline, &coordinator);

            std::thread::sleep(Duration::from_millis(50));

            let _ = classifier.update_fingerprint("age_test", &pipeline, &coordinator);

            let fp = classifier.get_fingerprint("age_test");
            assert!(fp.is_some());
            assert!(fp.unwrap().age_secs >= 0);
        }
    }

    mod pattern_classification {
        use super::*;

        #[test]
        fn test_classify_pattern_sequential() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 20000);
                let coordinator = make_coordinator_stats(100, 5);
                let _ = classifier.update_fingerprint("seq_workload", &pipeline, &coordinator);
            }

            let pattern = classifier.classify_pattern("seq_workload");
            assert!(pattern.is_ok());
        }

        #[test]
        fn test_classify_pattern_unknown() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let pattern = classifier.classify_pattern("nonexistent");
            assert!(pattern.is_ok());
            assert_eq!(pattern.unwrap(), AccessPatternType::Unknown);
        }

        #[test]
        fn test_classify_multiple_workloads() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for i in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 5000);
                let coordinator = make_coordinator_stats(10, 8);
                let _ = classifier.update_fingerprint(
                    &format!("workload_{}", i),
                    &pipeline,
                    &coordinator,
                );
            }

            let workloads = classifier.list_workloads();
            assert_eq!(workloads.len(), 10);
        }

        #[test]
        fn test_classify_workload_not_found() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let pattern = classifier.classify_pattern("missing");
            assert!(pattern.is_ok());
            assert_eq!(pattern.unwrap(), AccessPatternType::Unknown);
        }

        #[test]
        fn test_classify_pattern_caching() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 3000);
                let coordinator = make_coordinator_stats(10, 7);
                let _ = classifier.update_fingerprint("cache_test", &pipeline, &coordinator);
            }

            let pattern1 = classifier.classify_pattern("cache_test");
            let pattern2 = classifier.classify_pattern("cache_test");
            assert_eq!(pattern1.unwrap(), pattern2.unwrap());
        }
    }

    mod compression_level_recommendation {
        use super::*;

        #[test]
        fn test_recommend_compression_high_ratio() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 10000);
                let coordinator = make_coordinator_stats(100, 50);
                let _ = classifier.update_fingerprint("high_comp", &pipeline, &coordinator);
            }

            let level = classifier.recommend_compression_level("high_comp");
            assert!(level.is_ok());
        }

        #[test]
        fn test_recommend_compression_low_ratio() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 90000);
                let coordinator = make_coordinator_stats(100, 5);
                let _ = classifier.update_fingerprint("low_comp", &pipeline, &coordinator);
            }

            let level = classifier.recommend_compression_level("low_comp");
            assert!(level.is_ok());
        }

        #[test]
        fn test_recommend_compression_unknown_pattern() {
            let classifier = AdaptiveClassifier::new(Default::default());
            let level = classifier.recommend_compression_level("unknown_workload");
            assert!(level.is_ok());
            assert_eq!(level.unwrap(), CompressionLevel::Default);
        }

        #[test]
        fn test_recommend_compression_confidence_tracked() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 3000);
                let coordinator = make_coordinator_stats(10, 8);
                let _ = classifier.update_fingerprint("conf_test", &pipeline, &coordinator);
            }

            let recommendation = classifier.recommend_tiering("conf_test");
            assert!(recommendation.is_ok());
        }
    }

    mod dedup_strength_recommendation {
        use super::*;

        #[test]
        fn test_recommend_dedup_high_similarity() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 50000);
                let coordinator = make_coordinator_stats(100, 80);
                let _ = classifier.update_fingerprint("high_sim", &pipeline, &coordinator);
            }

            let strength = classifier.recommend_dedup_strength("high_sim");
            assert!(strength.is_ok());
        }

        #[test]
        fn test_recommend_dedup_low_similarity() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 90000);
                let coordinator = make_coordinator_stats(100, 5);
                let _ = classifier.update_fingerprint("low_sim", &pipeline, &coordinator);
            }

            let strength = classifier.recommend_dedup_strength("low_sim");
            assert!(strength.is_ok());
        }

        #[test]
        fn test_recommend_dedup_none_option() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 95000);
                let coordinator = make_coordinator_stats(100, 2);
                let _ = classifier.update_fingerprint("no_dedup", &pipeline, &coordinator);
            }

            let strength = classifier.recommend_dedup_strength("no_dedup");
            assert!(strength.is_ok());
        }

        #[test]
        fn test_recommend_dedup_temporal_pattern() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 5000);
                let coordinator = make_coordinator_stats(10, 7);
                let _ = classifier.update_fingerprint("temporal_dedup", &pipeline, &coordinator);
            }

            let strength = classifier.recommend_dedup_strength("temporal_dedup");
            assert!(strength.is_ok());
        }
    }

    mod s3_tiering_recommendation {
        use super::*;

        #[test]
        fn test_recommend_s3_frequent_access() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(1000, 1000000, 500000);
                let coordinator = make_coordinator_stats(1000, 500);
                let _ = classifier.update_fingerprint("frequent", &pipeline, &coordinator);
            }

            let policy = classifier.recommend_s3_policy("frequent");
            assert!(policy.is_ok());
        }

        #[test]
        fn test_recommend_s3_cold_access() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(10, 10000, 2000);
                let coordinator = make_coordinator_stats(10, 1);
                let _ = classifier.update_fingerprint("cold", &pipeline, &coordinator);
            }

            let policy = classifier.recommend_s3_policy("cold");
            assert!(policy.is_ok());
        }

        #[test]
        fn test_recommend_s3_temporal() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(50, 50000, 25000);
                let coordinator = make_coordinator_stats(50, 35);
                let _ = classifier.update_fingerprint("temporal_s3", &pipeline, &coordinator);
            }

            let policy = classifier.recommend_s3_policy("temporal_s3");
            assert!(policy.is_ok());
        }

        #[test]
        fn test_recommend_s3_compressible() {
            let classifier = AdaptiveClassifier::new(ClassifierConfig {
                min_samples: 5,
                ..Default::default()
            });

            for _ in 0..10 {
                let pipeline = make_pipeline_stats(100, 100000, 10000);
                let coordinator = make_coordinator_stats(100, 10);
                let _ = classifier.update_fingerprint("compressible_s3", &pipeline, &coordinator);
            }

            let policy = classifier.recommend_s3_policy("compressible_s3");
            assert!(policy.is_ok());
        }
    }
}
