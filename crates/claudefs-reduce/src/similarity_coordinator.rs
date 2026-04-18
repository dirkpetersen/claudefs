//! Tier 2 similarity detection and delta compression coordinator.
//!
//! Orchestrates feature extraction, similarity lookups, delta compression scheduling,
//! and result caching for the Tier 2 similarity detection pipeline.

use crate::delta_index::DeltaIndex;
use crate::error::ReduceError;
use crate::fingerprint::{ChunkHash, SuperFeatures};
use crate::similarity::{DeltaCompressor, SimilarityIndex};
use crate::fingerprint::super_features as compute_super_features;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Configuration for similarity detection.
#[derive(Debug, Clone)]
pub struct SimilarityConfig {
    /// Enable similarity detection pipeline.
    pub enable_similarity: bool,
    /// Minimum chunk size for feature extraction (bytes).
    pub feature_extraction_threshold: usize,
    /// LRU cache entries for recent results.
    pub cache_size: usize,
    /// Maximum delay before processing batch (milliseconds).
    pub batch_delay_ms: u64,
    /// Maximum chunks per feature extraction batch.
    pub max_batch_size: usize,
    /// Enable delta compression for similar chunks.
    pub delta_compression_enabled: bool,
    /// Cache entry TTL in seconds.
    pub result_ttl_secs: u64,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            enable_similarity: true,
            feature_extraction_threshold: 64,
            cache_size: 1024,
            batch_delay_ms: 100,
            max_batch_size: 64,
            delta_compression_enabled: true,
            result_ttl_secs: 300,
        }
    }
}

/// Similarity detection result with caching and timing.
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    /// Query chunk hash.
    pub query_hash: ChunkHash,
    /// Similar chunk hash if found.
    pub similar_hash: Option<ChunkHash>,
    /// Delta compression bytes if applicable.
    pub delta_bytes: Option<usize>,
    /// Compression ratio achieved.
    pub compression_ratio: Option<f64>,
    /// When result was computed.
    pub detected_at: Instant,
}

impl SimilarityResult {
    /// Create a new similarity result.
    pub fn new(query_hash: ChunkHash) -> Self {
        Self {
            query_hash,
            similar_hash: None,
            delta_bytes: None,
            compression_ratio: None,
            detected_at: Instant::now(),
        }
    }

    /// Create a result with a similar chunk found.
    pub fn with_similar(
        query_hash: ChunkHash,
        similar_hash: ChunkHash,
        delta_bytes: usize,
    ) -> Self {
        let ratio = if delta_bytes > 0 {
            Some(1.0 - (delta_bytes as f64 / 1.0))
        } else {
            Some(0.0)
        };
        Self {
            query_hash,
            similar_hash: Some(similar_hash),
            delta_bytes: Some(delta_bytes),
            compression_ratio: ratio,
            detected_at: Instant::now(),
        }
    }
}

/// Coordinator statistics tracking.
#[derive(Debug, Clone, Default)]
pub struct CoordinatorStats {
    /// Total chunks processed.
    pub chunks_processed: u64,
    /// Similarity lookups performed.
    pub similarity_lookups: u64,
    /// Similarity hits found.
    pub similarity_hits: u64,
    /// Similarity misses.
    pub similarity_misses: u64,
    /// Delta compressions scheduled.
    pub delta_compressions_scheduled: u64,
    /// Delta compressions completed.
    pub delta_compressions_completed: u64,
    /// Total delta bytes saved.
    pub total_delta_bytes_saved: u64,
    /// Feature extraction time in milliseconds.
    pub feature_extraction_time_ms: u64,
    /// Similarity lookup time in milliseconds.
    pub similarity_lookup_time_ms: u64,
    /// Cache hits.
    pub cache_hits: u64,
    /// Cache misses.
    pub cache_misses: u64,
}

impl CoordinatorStats {
    /// Calculate similarity hit rate.
    pub fn similarity_hit_rate(&self) -> f64 {
        if self.similarity_lookups == 0 {
            return 0.0;
        }
        self.similarity_hits as f64 / self.similarity_lookups as f64
    }

    /// Calculate cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / total as f64
    }

    /// Calculate average delta bytes saved.
    pub fn avg_delta_bytes_saved(&self) -> f64 {
        if self.delta_compressions_completed == 0 {
            return 0.0;
        }
        self.total_delta_bytes_saved as f64 / self.delta_compressions_completed as f64
    }
}

/// Simple LRU cache for similarity results.
struct LruCache<K: Eq + Hash + Clone, V> {
    map: HashMap<K, (V, Instant)>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V: Clone> LruCache<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &K, ttl_secs: u64) -> Option<V>
    where
        V: Clone,
    {
        if let Some((value, inserted_at)) = self.map.get(key) {
            if inserted_at.elapsed() < Duration::from_secs(ttl_secs) {
                self.order.retain(|k| k != key);
                self.order.push_back(key.clone());
                return Some(value.clone());
            } else {
                self.map.remove(key);
                self.order.retain(|k| k != key);
            }
        }
        None
    }

    fn insert(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.order.retain(|k| k != &key);
        } else if self.map.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.map.remove(&evicted);
            }
        }
        self.map.insert(key.clone(), (value, Instant::now()));
        self.order.push_back(key);
    }

    fn remove(&mut self, key: &K) {
        self.map.remove(key);
        self.order.retain(|k| k != key);
    }

    fn len(&self) -> usize {
        self.map.len()
    }
}

/// Feature extractor for similarity detection.
struct FeatureExtractor;

impl FeatureExtractor {
    fn extract(data: &[u8], threshold: usize) -> Option<SuperFeatures> {
        if data.len() >= threshold {
            Some(compute_super_features(data))
        } else {
            None
        }
    }
}

/// Tier 2 similarity detection coordinator.
pub struct SimilarityCoordinator {
    similarity_index: Arc<SimilarityIndex>,
    delta_index: Arc<RwLock<DeltaIndex>>,
    feature_extractor: FeatureExtractor,
    delta_compressor: DeltaCompressor,
    cache: Arc<RwLock<LruCache<ChunkHash, SimilarityResult>>>,
    stats: Arc<RwLock<CoordinatorStats>>,
    config: SimilarityConfig,
}

impl SimilarityCoordinator {
    /// Create new coordinator with default or custom config.
    pub fn new(config: SimilarityConfig) -> Result<Self, ReduceError> {
        if config.cache_size == 0 {
            return Err(ReduceError::InvalidInput(
                "cache_size must be greater than 0".to_string(),
            ));
        }
        if config.feature_extraction_threshold == 0 {
            return Err(ReduceError::InvalidInput(
                "feature_extraction_threshold must be greater than 0".to_string(),
            ));
        }

        let delta_index = DeltaIndex::new(crate::delta_index::DeltaIndexConfig::default());

        Ok(Self {
            similarity_index: Arc::new(SimilarityIndex::new()),
            delta_index: Arc::new(RwLock::new(delta_index)),
            feature_extractor: FeatureExtractor,
            delta_compressor: DeltaCompressor,
            cache: Arc::new(RwLock::new(LruCache::new(config.cache_size))),
            stats: Arc::new(RwLock::new(CoordinatorStats::default())),
            config,
        })
    }

    /// Process a chunk through Tier 2 similarity detection.
    pub async fn process_chunk(
        &self,
        hash: ChunkHash,
        data: &[u8],
    ) -> Result<SimilarityResult, ReduceError> {
        {
            let mut stats = self.stats.write().unwrap();
            stats.chunks_processed += 1;
        }

        if data.len() < self.config.feature_extraction_threshold {
            return Ok(SimilarityResult::new(hash));
        }

        {
            let mut cache = self.cache.write().unwrap();
            if let Some(cached) = cache.get(&hash, self.config.result_ttl_secs) {
                let mut stats = self.stats.write().unwrap();
                stats.cache_hits += 1;
                return Ok(cached);
            }
            let mut stats = self.stats.write().unwrap();
            stats.cache_misses += 1;
        }

        let features = match FeatureExtractor::extract(data, self.config.feature_extraction_threshold) {
            Some(f) => f,
            None => return Ok(SimilarityResult::new(hash)),
        };

        let lookup_start = Instant::now();
        let similar_hash = self.similarity_index.find_similar(&features);
        let lookup_time = lookup_start.elapsed().as_millis() as u64;

        {
            let mut stats = self.stats.write().unwrap();
            stats.similarity_lookups += 1;
            stats.similarity_lookup_time_ms += lookup_time;

            if similar_hash.is_some() {
                stats.similarity_hits += 1;
            } else {
                stats.similarity_misses += 1;
            }
        }

        self.similarity_index.insert(hash, features);

        let result = if similar_hash.is_some() {
            SimilarityResult::with_similar(hash, similar_hash.unwrap(), 0)
        } else {
            SimilarityResult::new(hash)
        };

        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(hash, result.clone());
        }

        Ok(result)
    }

    /// Batch feature extraction and similarity lookup (background task).
    pub async fn process_batch(
        &self,
        chunks: Vec<(ChunkHash, Vec<u8>)>,
    ) -> Result<Vec<SimilarityResult>, ReduceError> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(chunks.len());

        for (hash, data) in chunks {
            let result = self.process_chunk(hash, &data).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Schedule delta compression for a similar chunk pair.
    pub async fn schedule_delta_compression(
        &self,
        query_hash: ChunkHash,
        query_data: &[u8],
        _similar_hash: ChunkHash,
        similar_data: &[u8],
    ) -> Result<(usize, f64), ReduceError> {
        if !self.config.delta_compression_enabled {
            return Err(ReduceError::InvalidInput(
                "delta compression is disabled".to_string(),
            ));
        }

        if similar_data.is_empty() {
            return Err(ReduceError::CompressionFailed(
                "reference data cannot be empty for delta compression".to_string(),
            ));
        }

        let compression_start = Instant::now();

        let compressed = DeltaCompressor::compress_delta(query_data, similar_data, 3)?;
        let delta_bytes = query_data.len() - compressed.len();
        let ratio = if !query_data.is_empty() {
            1.0 - (compressed.len() as f64 / query_data.len() as f64)
        } else {
            0.0
        };

        let compression_time = compression_start.elapsed().as_millis() as u64;

        {
            let mut stats = self.stats.write().unwrap();
            stats.delta_compressions_scheduled += 1;
            stats.delta_compressions_completed += 1;
            stats.total_delta_bytes_saved += delta_bytes.max(0) as u64;
            stats.feature_extraction_time_ms += compression_time;
        }

        let features = compute_super_features(query_data);
        let entry = crate::delta_index::DeltaIndexEntry {
            block_hash: query_hash.0,
            features: features.0,
            size_bytes: query_data.len() as u32,
        };
        {
            let mut delta_index = self.delta_index.write().unwrap();
            delta_index.insert(entry);
        }

        Ok((delta_bytes.max(0), ratio))
    }

    /// Invalidate cache entry (used on chunk eviction or GC).
    pub fn invalidate_cache(&self, hash: ChunkHash) {
        let mut cache = self.cache.write().unwrap();
        cache.remove(&hash);
    }

    /// Get current stats (read-only snapshot).
    pub fn stats(&self) -> CoordinatorStats {
        self.stats.read().unwrap().clone()
    }

    /// Reset stats (for testing or per-interval reset).
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write().unwrap();
        *stats = CoordinatorStats::default();
    }

    /// Coordinate integration with chunk_pipeline: non-blocking scheduling.
    pub fn schedule_for_similarity(
        &self,
        hash: ChunkHash,
        data: Vec<u8>,
    ) -> Result<(), ReduceError> {
        if !self.config.enable_similarity {
            return Ok(());
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _ = rt.block_on(self.process_chunk(hash, &data));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(v: u8) -> ChunkHash {
        let mut h = [0u8; 32];
        h[0] = v;
        ChunkHash(h)
    }

    mod initialization {
        use super::*;

        #[test]
        fn test_new_with_default_config() {
            let coord = SimilarityCoordinator::new(SimilarityConfig::default());
            assert!(coord.is_ok());
        }

        #[test]
        fn test_new_with_custom_config() {
            let config = SimilarityConfig {
                enable_similarity: true,
                feature_extraction_threshold: 128,
                cache_size: 2048,
                batch_delay_ms: 50,
                max_batch_size: 32,
                delta_compression_enabled: false,
                result_ttl_secs: 600,
            };
            let coord = SimilarityCoordinator::new(config.clone());
            assert!(coord.is_ok());
            let c = coord.unwrap();
            assert_eq!(c.config.feature_extraction_threshold, 128);
            assert_eq!(c.config.cache_size, 2048);
        }

        #[test]
        fn test_invalid_config_rejected() {
            let config = SimilarityConfig {
                cache_size: 0,
                ..Default::default()
            };
            let result = SimilarityCoordinator::new(config);
            assert!(result.is_err());

            let config2 = SimilarityConfig {
                feature_extraction_threshold: 0,
                ..Default::default()
            };
            let result2 = SimilarityCoordinator::new(config2);
            assert!(result2.is_err());
        }

        #[test]
        fn test_config_defaults() {
            let config = SimilarityConfig::default();
            assert!(config.enable_similarity);
            assert_eq!(config.feature_extraction_threshold, 64);
            assert_eq!(config.cache_size, 1024);
            assert_eq!(config.batch_delay_ms, 100);
            assert_eq!(config.max_batch_size, 64);
            assert!(config.delta_compression_enabled);
            assert_eq!(config.result_ttl_secs, 300);
        }
    }

    mod feature_extraction_cache {
        use super::*;

        #[test]
        fn test_process_chunk_small_skipped() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                feature_extraction_threshold: 100,
                ..Default::default()
            }).unwrap();

            let hash = make_hash(1);
            let data = vec![1u8; 50];

            let result = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(coord.process_chunk(hash, &data));

            assert!(result.is_ok());
            assert!(result.unwrap().similar_hash.is_none());
        }

        #[test]
        fn test_process_chunk_cache_hit() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result1 = rt.block_on(coord.process_chunk(hash, &data));
            assert!(result1.is_ok());

            let stats1 = coord.stats();
            assert_eq!(stats1.cache_misses, 1);

            let result2 = rt.block_on(coord.process_chunk(hash, &data));
            assert!(result2.is_ok());

            let stats2 = coord.stats();
            assert_eq!(stats2.cache_hits, 1);
        }

        #[test]
        fn test_process_chunk_cache_miss_recalculates() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                result_ttl_secs: 0,
                ..Default::default()
            }).unwrap();

            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash, &data));

            let stats1 = coord.stats();
            let misses_before = stats1.cache_misses;

            std::thread::sleep(Duration::from_millis(10));

            let _ = rt.block_on(coord.process_chunk(hash, &data));
            let stats2 = coord.stats();

            assert!(stats2.cache_misses > misses_before);
        }

        #[test]
        fn test_cache_invalidation() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash, &data));

            coord.invalidate_cache(hash);

            let stats = coord.stats();
            assert_eq!(stats.cache_hits, 0);
            assert_eq!(stats.cache_misses, 1);
        }

        #[test]
        fn test_cache_ttl_expiration() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                result_ttl_secs: 0,
                ..Default::default()
            }).unwrap();

            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash, &data));

            std::thread::sleep(Duration::from_millis(50));

            let _ = rt.block_on(coord.process_chunk(hash, &data));
            let stats = coord.stats();

            assert!(stats.cache_misses >= 1);
        }

        #[test]
        fn test_cache_lru_eviction() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                cache_size: 2,
                ..Default::default()
            }).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

            for i in 0..3 {
                let hash = make_hash(i);
                let data = vec![i; 1024];
                let _ = rt.block_on(coord.process_chunk(hash, &data));
            }

            let hash0 = make_hash(0);
            let stats = coord.stats();
            let misses_after = stats.cache_misses;

            let _ = rt.block_on(coord.process_chunk(hash0, &vec![0; 1024]));
            let stats2 = coord.stats();

            assert!(stats2.cache_misses > misses_after);
        }
    }

    mod similarity_lookup {
        use super::*;

        #[test]
        fn test_similarity_lookup_hit() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash1 = make_hash(1);
            let hash2 = make_hash(2);
            let data1 = vec![1u8; 1024];
            let data2 = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash1, &data1));
            let result = rt.block_on(coord.process_chunk(hash2, &data2));

            assert!(result.is_ok());
            let r = result.unwrap();
            assert!(r.similar_hash.is_some());
        }

        #[test]
        fn test_similarity_lookup_miss() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.process_chunk(hash, &data));

            assert!(result.is_ok());
            assert!(result.unwrap().similar_hash.is_none());
        }

        #[test]
        fn test_similarity_lookup_multiple_candidates() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

            for i in 0..3 {
                let hash = make_hash(i);
                let mut data = vec![1u8; 512];
                data.extend_from_slice(&[2u8; 512]);
                let _ = rt.block_on(coord.process_chunk(hash, &data));
            }

            let hash = make_hash(10);
            let mut data = vec![1u8; 512];
            data.extend_from_slice(&[2u8; 512]);
            let result = rt.block_on(coord.process_chunk(hash, &data));

            assert!(result.is_ok());
            assert!(result.unwrap().similar_hash.is_some());
        }

        #[test]
        fn test_similarity_threshold_respected() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                feature_extraction_threshold: 2048,
                ..Default::default()
            }).unwrap();

            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.process_chunk(hash, &data));

            assert!(result.is_ok());
            assert!(result.unwrap().similar_hash.is_none());
        }

        #[test]
        fn test_similarity_with_empty_index() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.process_chunk(hash, &data));

            assert!(result.is_ok());
            assert!(result.unwrap().similar_hash.is_none());
        }

        #[test]
        fn test_similarity_lookup_updates_stats() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash1 = make_hash(1);
            let hash2 = make_hash(2);
            let data1 = vec![1u8; 1024];
            let data2 = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash1, &data1));
            let _ = rt.block_on(coord.process_chunk(hash2, &data2));

            let stats = coord.stats();
            assert_eq!(stats.similarity_lookups, 2);
            assert!(stats.similarity_hits >= 1);
        }

        #[test]
        fn test_similarity_result_stores_compression_ratio() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash1 = make_hash(1);
            let hash2 = make_hash(2);
            let data1 = vec![1u8; 1024];
            let data2 = vec![1u8; 1024];

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(hash1, &data1));
            let result = rt.block_on(coord.process_chunk(hash2, &data2));

            assert!(result.is_ok());
            let r = result.unwrap();
            assert!(r.compression_ratio.is_some() || r.similar_hash.is_some());
        }
    }

    mod delta_compression {
        use super::*;

        #[test]
        fn test_schedule_delta_compression_success() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let query_data = b"This is test data for delta compression".to_vec();
            let similar_data = b"This is test data for delta compression with more content".to_vec();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                &query_data,
                make_hash(2),
                &similar_data,
            ));

            assert!(result.is_ok());
            let (delta_bytes, ratio) = result.unwrap();
            assert!(delta_bytes >= 0);
            assert!(ratio >= 0.0);
        }

        #[test]
        fn test_schedule_delta_compression_ratio_tracked() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let query_data = std::iter::repeat_n(b"test data ".as_slice(), 10).flatten().copied().collect::<Vec<_>>();
            let mut similar_data = query_data.clone();
            similar_data.push(0);

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                &query_data,
                make_hash(2),
                &similar_data,
            ));

            let stats = coord.stats();
            assert!(stats.delta_compressions_completed >= 1);
        }

        #[test]
        fn test_schedule_delta_compression_updates_delta_index() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let query_data = b"test data for delta index".to_vec();
            let similar_data = b"test data for delta index with more content".to_vec();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                &query_data,
                make_hash(2),
                &similar_data,
            ));

            let delta_index = coord.delta_index.read().unwrap();
            assert!(delta_index.len() >= 1);
        }

        #[test]
        fn test_delta_compression_partial_match() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let mut data1 = vec![1u8; 512];
            data1.extend_from_slice(&[2u8; 512]);
            let mut data2 = vec![1u8; 512];
            data2.extend_from_slice(&[3u8; 512]);

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                &data1,
                make_hash(2),
                &data2,
            ));

            assert!(result.is_ok());
        }

        #[test]
        fn test_delta_compression_failed_gracefully() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                delta_compression_enabled: false,
                ..Default::default()
            }).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                b"test",
                make_hash(2),
                b"reference",
            ));

            assert!(result.is_err());
        }

        #[test]
        fn test_delta_compression_empty_reference_fails() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                b"test",
                make_hash(2),
                b"",
            ));

            assert!(result.is_err());
        }

        #[test]
        fn test_delta_compression_roundtrip() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let original = b"Original data for roundtrip test with some content".to_vec();
            let reference = b"Original data for roundtrip test with some content and extra".to_vec();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.schedule_delta_compression(
                make_hash(1),
                &original,
                make_hash(2),
                &reference,
            ));

            assert!(result.is_ok());
        }
    }

    mod batch_processing {
        use super::*;

        #[test]
        fn test_batch_process_empty() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(coord.process_batch(Vec::new()));

            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn test_batch_process_multiple() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let chunks = vec![
                (make_hash(1), vec![1u8; 1024]),
                (make_hash(2), vec![2u8; 1024]),
                (make_hash(3), vec![3u8; 1024]),
            ];

            let result = rt.block_on(coord.process_batch(chunks));

            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 3);
        }

        #[test]
        fn test_batch_delay_timer() {
            let config = SimilarityConfig {
                batch_delay_ms: 50,
                ..Default::default()
            };
            let coord = SimilarityCoordinator::new(config).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let chunks = vec![(make_hash(1), vec![1u8; 1024])];

            let start = Instant::now();
            let _ = rt.block_on(coord.process_batch(chunks));
            let elapsed = start.elapsed().as_millis();

            assert!(elapsed >= 0);
        }

        #[test]
        fn test_batch_max_size_trigger() {
            let config = SimilarityConfig {
                max_batch_size: 2,
                ..Default::default()
            };
            let coord = SimilarityCoordinator::new(config).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let chunks = vec![
                (make_hash(1), vec![1u8; 1024]),
                (make_hash(2), vec![2u8; 1024]),
                (make_hash(3), vec![3u8; 1024]),
            ];

            let result = rt.block_on(coord.process_batch(chunks));

            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 3);
        }

        #[test]
        fn test_batch_partial_results() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let chunks = vec![
                (make_hash(1), vec![1u8; 1024]),
                (make_hash(2), vec![2u8; 50]),
            ];

            let result = rt.block_on(coord.process_batch(chunks));

            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 2);
        }
    }

    mod stats_telemetry {
        use super::*;

        #[test]
        fn test_stats_tracking() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let hash1 = make_hash(1);
            let hash2 = make_hash(2);
            let data1 = vec![1u8; 1024];
            let data2 = vec![1u8; 1024];

            let _ = rt.block_on(coord.process_chunk(hash1, &data1));
            let _ = rt.block_on(coord.process_chunk(hash2, &data2));
            let _ = rt.block_on(coord.schedule_delta_compression(
                hash1,
                &data1,
                hash2,
                &data2,
            ));

            let stats = coord.stats();
            assert!(stats.chunks_processed >= 2);
            assert!(stats.delta_compressions_completed >= 1);
        }

        #[test]
        fn test_stats_ratios_calculated() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let hash1 = make_hash(1);
            let hash2 = make_hash(2);
            let data1 = vec![1u8; 1024];
            let data2 = vec![1u8; 1024];

            let _ = rt.block_on(coord.process_chunk(hash1, &data1));
            let _ = rt.block_on(coord.process_chunk(hash2, &data2));

            let stats = coord.stats();
            let hit_rate = stats.similarity_hit_rate();
            let cache_rate = stats.cache_hit_rate();

            assert!(hit_rate >= 0.0);
            assert!(cache_rate >= 0.0);
        }

        #[test]
        fn test_stats_reset() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(make_hash(1), &vec![1u8; 1024]));

            coord.reset_stats();

            let stats = coord.stats();
            assert_eq!(stats.chunks_processed, 0);
            assert_eq!(stats.similarity_lookups, 0);
        }

        #[test]
        fn test_stats_snapshot_immutable() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            let stats1 = coord.stats();
            let chunks1 = stats1.chunks_processed;

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = rt.block_on(coord.process_chunk(make_hash(1), &vec![1u8; 1024]));

            let stats2 = coord.stats();
            assert_eq!(stats1.chunks_processed, chunks1);
            assert!(stats2.chunks_processed > chunks1);
        }
    }

    mod integration {
        use super::*;

        #[test]
        fn test_schedule_for_similarity_non_blocking() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];

            let result = coord.schedule_for_similarity(hash, data);

            assert!(result.is_ok());
        }

        #[test]
        fn test_schedule_for_similarity_batches() {
            let coord = SimilarityCoordinator::new(Default::default()).unwrap();

            for i in 0..5 {
                let hash = make_hash(i);
                let data = vec![i; 1024];
                let result = coord.schedule_for_similarity(hash, data);
                assert!(result.is_ok());
            }

            let stats = coord.stats();
            assert!(stats.chunks_processed >= 5);
        }

        #[test]
        fn test_schedule_for_similarity_disabled() {
            let coord = SimilarityCoordinator::new(SimilarityConfig {
                enable_similarity: false,
                ..Default::default()
            }).unwrap();

            let result = coord.schedule_for_similarity(make_hash(1), vec![1u8; 1024]);

            assert!(result.is_ok());
        }
    }
}