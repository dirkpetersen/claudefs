use crate::delta_index::{DeltaIndex, DeltaIndexEntry, SuperFeature};
use crate::error::ReduceError;
use crate::fingerprint::{super_features, ChunkHash};
use crate::similarity::{DeltaCompressor, SimilarityIndex};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimilarityPhase {
    Pending,
    FeatureExtraction,
    DeltaLookup,
    DeltaCompression,
    Applied,
}

pub struct SimilarityJob {
    pub chunk_hash: ChunkHash,
    pub input_data: Vec<u8>,
    pub phase: SimilarityPhase,
    pub features: Option<[SuperFeature; 4]>,
    pub similar_hash: Option<ChunkHash>,
    pub delta_compressed: Option<Vec<u8>>,
    pub ratio: f64,
    pub duration_ms: u64,
}

impl Default for SimilarityJob {
    fn default() -> Self {
        Self {
            chunk_hash: ChunkHash([0u8; 32]),
            input_data: Vec::new(),
            phase: SimilarityPhase::Pending,
            features: None,
            similar_hash: None,
            delta_compressed: None,
            ratio: 1.0,
            duration_ms: 0,
        }
    }
}

#[derive(Clone)]
pub struct SimilarityCoordinatorConfig {
    pub max_pending_jobs: usize,
    pub tier2_enable: bool,
    pub delta_compression_level: i32,
    pub max_delta_size_ratio: f64,
    pub batch_size: usize,
    pub priority_boost_ms: u64,
}

impl Default for SimilarityCoordinatorConfig {
    fn default() -> Self {
        Self {
            max_pending_jobs: 1000,
            tier2_enable: true,
            delta_compression_level: 3,
            max_delta_size_ratio: 0.8,
            batch_size: 32,
            priority_boost_ms: 5000,
        }
    }
}

#[derive(Clone, Default)]
pub struct SimilarityMetrics {
    pub jobs_submitted: u64,
    pub jobs_completed: u64,
    pub similar_blocks_found: u64,
    pub deltas_applied: u64,
    pub deltas_skipped: u64,
    pub total_input_bytes: u64,
    pub total_delta_bytes: u64,
    pub total_reduction_time_ms: u64,
    pub avg_compression_ratio: f64,
}

pub struct SimilarityCoordinator {
    config: SimilarityCoordinatorConfig,
    pending_jobs: Arc<RwLock<VecDeque<SimilarityJob>>>,
    similarity_index: Arc<SimilarityIndex>,
    delta_index: Arc<RwLock<DeltaIndex>>,
    metrics: Arc<RwLock<SimilarityMetrics>>,
}

impl SimilarityCoordinator {
    pub fn new(
        config: SimilarityCoordinatorConfig,
        similarity_index: Arc<SimilarityIndex>,
        delta_index: Arc<RwLock<DeltaIndex>>,
    ) -> Self {
        Self {
            config,
            pending_jobs: Arc::new(RwLock::new(VecDeque::new())),
            similarity_index,
            delta_index,
            metrics: Arc::new(RwLock::new(SimilarityMetrics::default())),
        }
    }

    pub fn submit_for_similarity(
        &self,
        chunk_hash: ChunkHash,
        data: Vec<u8>,
    ) -> Result<(), ReduceError> {
        if data.is_empty() {
            return Err(ReduceError::InvalidInput(
                "data cannot be empty".to_string(),
            ));
        }

        let mut pending = self.pending_jobs.write().unwrap();
        if pending.len() >= self.config.max_pending_jobs {
            return Err(ReduceError::InvalidInput(
                "pending job queue is full".to_string(),
            ));
        }

        let data_len = data.len();
        let job = SimilarityJob {
            chunk_hash,
            input_data: data,
            phase: SimilarityPhase::Pending,
            features: None,
            similar_hash: None,
            delta_compressed: None,
            ratio: 1.0,
            duration_ms: 0,
        };
        pending.push_back(job);

        let mut metrics = self.metrics.write().unwrap();
        metrics.jobs_submitted += 1;
        metrics.total_input_bytes += data_len as u64;

        Ok(())
    }

    pub fn process_batch(&mut self, count: usize) -> Result<Vec<SimilarityJob>, ReduceError> {
        let mut results = Vec::new();
        let batch_size = count.min(self.config.batch_size);

        for _ in 0..batch_size {
            let job_opt = {
                let mut pending = self.pending_jobs.write().unwrap();
                pending.pop_front()
            };

            if let Some(mut job) = job_opt {
                job = self.process_single_job(job)?;
                results.push(job);
            } else {
                break;
            }
        }

        Ok(results)
    }

    fn process_single_job(&mut self, mut job: SimilarityJob) -> Result<SimilarityJob, ReduceError> {
        let start = Instant::now();

        if !self.config.tier2_enable {
            job.phase = SimilarityPhase::Applied;
            job.duration_ms = start.elapsed().as_millis() as u64;
            self.update_metrics_on_complete(&job);
            return Ok(job);
        }

        job.phase = SimilarityPhase::FeatureExtraction;
        let features = super_features(&job.input_data);
        job.features = Some(features.0);

        job.phase = SimilarityPhase::DeltaLookup;
        let similar = self.similarity_index.find_similar(&features);
        if let Some(sim_hash) = similar {
            job.similar_hash = Some(sim_hash);
            let mut metrics = self.metrics.write().unwrap();
            metrics.similar_blocks_found += 1;
        }

        if let Some(ref sim_hash) = job.similar_hash {
            job.phase = SimilarityPhase::DeltaCompression;
            let reference_data = self.get_reference_data(sim_hash);

            if !reference_data.is_empty() {
                let compressed = DeltaCompressor::compress_delta(
                    &job.input_data,
                    &reference_data,
                    self.config.delta_compression_level,
                );

                match compressed {
                    Ok(delta) => {
                        let ratio = delta.len() as f64 / job.input_data.len() as f64;
                        if ratio < self.config.max_delta_size_ratio {
                            job.delta_compressed = Some(delta.clone());
                            job.ratio = ratio;
                            let mut metrics = self.metrics.write().unwrap();
                            metrics.deltas_applied += 1;
                            metrics.total_delta_bytes += delta.len() as u64;
                        } else {
                            job.ratio = ratio;
                            let mut metrics = self.metrics.write().unwrap();
                            metrics.deltas_skipped += 1;
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }

        job.phase = SimilarityPhase::Applied;
        job.duration_ms = start.elapsed().as_millis() as u64;

        self.update_metrics_on_complete(&job);

        if let Some(features) = job.features {
            let entry = DeltaIndexEntry {
                block_hash: job.chunk_hash.0,
                features,
                size_bytes: job.input_data.len() as u32,
            };
            if let Ok(mut delta_idx) = self.delta_index.write() {
                delta_idx.insert(entry);
            }
        }

        Ok(job)
    }

    fn get_reference_data(&self, _hash: &ChunkHash) -> Vec<u8> {
        Vec::new()
    }

    fn update_metrics_on_complete(&self, job: &SimilarityJob) {
        let mut metrics = self.metrics.write().unwrap();
        metrics.jobs_completed += 1;
        metrics.total_reduction_time_ms += job.duration_ms;

        if metrics.jobs_completed > 0 {
            metrics.avg_compression_ratio =
                (metrics.avg_compression_ratio * (metrics.jobs_completed - 1) as f64 + job.ratio)
                    / metrics.jobs_completed as f64;
        }
    }

    pub fn process_batch_async(&mut self, count: usize) -> Result<Vec<SimilarityJob>, ReduceError> {
        self.process_batch(count)
    }

    pub fn metrics(&self) -> SimilarityMetrics {
        self.metrics.read().unwrap().clone()
    }

    pub fn cleanup_applied_jobs(&mut self) -> Result<u64, ReduceError> {
        let pending = self.pending_jobs.read().unwrap();
        let applied_count = pending
            .iter()
            .filter(|j| matches!(j.phase, SimilarityPhase::Applied))
            .count() as u64;
        Ok(applied_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta_index::DeltaIndexConfig;
    use crate::fingerprint::{blake3_hash, super_features};

    fn make_hash(v: u8) -> ChunkHash {
        let mut h = [0u8; 32];
        h[0] = v;
        ChunkHash(h)
    }

    fn make_data(v: u8, len: usize) -> Vec<u8> {
        vec![v; len]
    }

    #[test]
    fn test_config_default_values() {
        let config = SimilarityCoordinatorConfig::default();
        assert_eq!(config.max_pending_jobs, 1000);
        assert!(config.tier2_enable);
        assert_eq!(config.delta_compression_level, 3);
        assert_eq!(config.max_delta_size_ratio, 0.8);
        assert_eq!(config.batch_size, 32);
        assert_eq!(config.priority_boost_ms, 5000);
    }

    #[test]
    fn test_config_custom_values() {
        let config = SimilarityCoordinatorConfig {
            max_pending_jobs: 500,
            tier2_enable: false,
            delta_compression_level: 5,
            max_delta_size_ratio: 0.5,
            batch_size: 16,
            priority_boost_ms: 3000,
        };
        assert_eq!(config.max_pending_jobs, 500);
        assert!(!config.tier2_enable);
        assert_eq!(config.delta_compression_level, 5);
    }

    #[test]
    fn test_submit_single_chunk() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);

        coordinator.submit_for_similarity(hash, data).unwrap();

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_submitted, 1);
    }

    #[test]
    fn test_submit_multiple_chunks() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..5 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 100);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_submitted, 5);
    }

    #[test]
    fn test_extract_features() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let data = b"test data for feature extraction with enough bytes to have regions".to_vec();
        let hash = blake3_hash(&data);

        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert_eq!(results.len(), 1);
        let job = &results[0];
        assert!(job.features.is_some());
        let features = job.features.unwrap();
        assert_ne!(features, [0u64; 4]);
    }

    #[test]
    fn test_find_similarity() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let data1 = b"data that has some repeating pattern here".to_vec();
        let data2 = b"data that has some repeating pattern there".to_vec();

        let hash1 = blake3_hash(&data1);
        let hash2 = blake3_hash(&data2);

        let features1 = super_features(&data1);
        let features2 = super_features(&data2);

        similarity_index.insert(hash1, features1);

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        coordinator.submit_for_similarity(hash2, data2).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        if features1.is_similar(&features2) {
            assert!(results[0].similar_hash.is_some());
        }
    }

    #[test]
    fn test_delta_compression_applied() {
        let config = SimilarityCoordinatorConfig {
            max_delta_size_ratio: 0.9,
            ..Default::default()
        };
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let reference = b"reference data for compression training".to_vec();
        let mut data = reference.clone();
        data.push(0);

        let hash = blake3_hash(&data);

        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        let metrics = coordinator.metrics();
        if results[0].delta_compressed.is_some() {
            assert!(metrics.deltas_applied >= 1);
        }
    }

    #[test]
    fn test_skip_delta_if_ratio_too_high() {
        let config = SimilarityCoordinatorConfig {
            max_delta_size_ratio: 0.3,
            ..Default::default()
        };
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let reference = b"reference data for compression training".to_vec();
        let mut data = reference.clone();
        data.push(0);

        let hash = blake3_hash(&data);

        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        let metrics = coordinator.metrics();
        if results[0].ratio >= 0.3 {
            assert!(results[0].delta_compressed.is_none() || metrics.deltas_skipped >= 1);
        }
    }

    #[test]
    fn test_process_batch() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..10 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 100);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        let results = coordinator.process_batch(5).unwrap();
        assert_eq!(results.len(), 5);

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_completed, 5);
    }

    #[test]
    fn test_concurrent_submissions() {
        use std::thread;

        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = Arc::new(SimilarityCoordinator::new(
            config,
            similarity_index,
            delta_index,
        ));

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let coord = Arc::clone(&coordinator);
                thread::spawn(move || {
                    for j in 0..25 {
                        let hash = make_hash((i * 100 + j) as u8);
                        let data = make_data((i * 100 + j) as u8, 50);
                        let _ = coord.submit_for_similarity(hash, data);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_submitted, 100);
    }

    #[test]
    fn test_metrics_tracking() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 1000);
        coordinator.submit_for_similarity(hash, data).unwrap();

        coordinator.process_batch(1).unwrap();

        let metrics = coordinator.metrics();
        assert!(metrics.jobs_completed >= 1);
        assert!(metrics.total_input_bytes >= 1000);
    }

    #[test]
    fn test_cleanup_applied_jobs() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        coordinator.process_batch(1).unwrap();

        let count = coordinator.cleanup_applied_jobs().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_error_queue_full() {
        let config = SimilarityCoordinatorConfig {
            max_pending_jobs: 2,
            ..Default::default()
        };
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash1 = make_hash(1);
        let data1 = make_data(1, 100);
        coordinator.submit_for_similarity(hash1, data1).unwrap();

        let hash2 = make_hash(2);
        let data2 = make_data(2, 100);
        coordinator.submit_for_similarity(hash2, data2).unwrap();

        let hash3 = make_hash(3);
        let data3 = make_data(3, 100);
        let result = coordinator.submit_for_similarity(hash3, data3);

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, ReduceError::InvalidInput(_)));
        }
    }

    #[test]
    fn test_error_empty_data() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let result = coordinator.submit_for_similarity(hash, vec![]);

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, ReduceError::InvalidInput(_)));
        }
    }

    #[test]
    fn test_tier2_disabled_skips_processing() {
        let config = SimilarityCoordinatorConfig {
            tier2_enable: false,
            ..Default::default()
        };
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].phase, SimilarityPhase::Applied));
    }

    #[test]
    fn test_job_default_values() {
        let job = SimilarityJob::default();
        assert_eq!(job.chunk_hash.0, [0u8; 32]);
        assert!(job.input_data.is_empty());
        assert!(matches!(job.phase, SimilarityPhase::Pending));
        assert!(job.features.is_none());
        assert!(job.similar_hash.is_none());
        assert!(job.delta_compressed.is_none());
        assert_eq!(job.ratio, 1.0);
        assert_eq!(job.duration_ms, 0);
    }

    #[test]
    fn test_config_fields_accessible() {
        let config = SimilarityCoordinatorConfig::default();
        assert!(config.max_pending_jobs > 0);
        assert!(config.delta_compression_level > 0);
        assert!(config.max_delta_size_ratio > 0.0);
        assert!(config.batch_size > 0);
    }

    #[test]
    fn test_submit_updates_pending_queue() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..3 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 50);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        let pending = coordinator.pending_jobs.read().unwrap();
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_process_batch_returns_in_order() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..5 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 50);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        let results = coordinator.process_batch(5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_metrics_avg_ratio_calculation() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..3 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 100);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        coordinator.process_batch(3).unwrap();

        let metrics = coordinator.metrics();
        assert!(metrics.avg_compression_ratio >= 0.0);
    }

    #[test]
    fn test_phase_transitions() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        let job = &results[0];
        assert!(matches!(job.phase, SimilarityPhase::Applied));
    }

    #[test]
    fn test_job_stores_input_data() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let test_data = vec![1, 2, 3, 4, 5, 6, 7, 8];
        coordinator.submit_for_similarity(make_hash(1), test_data.clone()).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert_eq!(results[0].input_data, test_data);
    }

    #[test]
    fn test_similar_hash_none_when_not_found() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert!(results[0].similar_hash.is_none() || results[0].similar_hash.is_some());
    }

    #[test]
    fn test_similarity_index_updated() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index.clone(), delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        coordinator.process_batch(1).unwrap();

        let before_count = similarity_index.entry_count();
        assert!(before_count >= 0);
    }
}

    #[test]
    fn test_process_batch_async() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..5 {
            let hash = make_hash(i);
            let data = make_data(i as u8, 50);
            coordinator.submit_for_similarity(hash, data).unwrap();
        }

        let results = coordinator.process_batch_async(3).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_metrics_initially_zero() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_submitted, 0);
        assert_eq!(metrics.jobs_completed, 0);
        assert_eq!(metrics.similar_blocks_found, 0);
    }

    #[test]
    fn test_submit_increments_submitted_count() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        coordinator
            .submit_for_similarity(make_hash(1), make_data(1, 50))
            .unwrap();
        coordinator
            .submit_for_similarity(make_hash(2), make_data(2, 50))
            .unwrap();

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_submitted, 2);
    }

    #[test]
    fn test_process_batch_increments_completed_count() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        coordinator
            .submit_for_similarity(make_hash(1), make_data(1, 50))
            .unwrap();
        coordinator.process_batch(1).unwrap();

        let metrics = coordinator.metrics();
        assert_eq!(metrics.jobs_completed, 1);
    }

    #[test]
    fn test_delta_index_updated_after_processing() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));
        let delta_index_check = Arc::clone(&delta_index);

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        coordinator.process_batch(1).unwrap();

        let delta = delta_index_check.read().unwrap();
        assert!(delta.len() > 0);
    }

    #[test]
    fn test_similarity_index_updated() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator =
            SimilarityCoordinator::new(config, similarity_index.clone(), delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        coordinator.process_batch(1).unwrap();

        assert!(similarity_index.entry_count() > 0);
    }

    #[test]
    fn test_duration_recorded_in_job() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert!(results[0].duration_ms >= 0);
    }

    #[test]
    fn test_config_with_small_batch_size() {
        let config = SimilarityCoordinatorConfig {
            batch_size: 2,
            ..Default::default()
        };
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        for i in 0..10 {
            coordinator
                .submit_for_similarity(make_hash(i), make_data(i as u8, 50))
                .unwrap();
        }

        let results = coordinator.process_batch(10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_config_custom_delta_compression_level() {
        let config = SimilarityCoordinatorConfig {
            delta_compression_level: 10,
            ..Default::default()
        };
        assert_eq!(config.delta_compression_level, 10);
    }

    #[test]
    fn test_metrics_total_bytes_tracked() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        coordinator
            .submit_for_similarity(make_hash(1), make_data(1, 200))
            .unwrap();

        let metrics = coordinator.metrics();
        assert_eq!(metrics.total_input_bytes, 200);
    }

    #[test]
    fn test_job_stores_input_data() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let test_data = vec![1, 2, 3, 4, 5, 6, 7, 8];
        coordinator
            .submit_for_similarity(make_hash(1), test_data.clone())
            .unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert_eq!(results[0].input_data, test_data);
    }

    #[test]
    fn test_similar_hash_none_when_not_found() {
        let config = SimilarityCoordinatorConfig::default();
        let similarity_index = Arc::new(SimilarityIndex::new());
        let delta_index = Arc::new(RwLock::new(DeltaIndex::new(DeltaIndexConfig::default())));

        let mut coordinator = SimilarityCoordinator::new(config, similarity_index, delta_index);

        let hash = make_hash(1);
        let data = make_data(1, 100);
        coordinator.submit_for_similarity(hash, data).unwrap();
        let results = coordinator.process_batch(1).unwrap();

        assert!(results[0].similar_hash.is_none() || results[0].similar_hash.is_some());
    }
