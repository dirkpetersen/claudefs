# A3: Data Reduction — Phase 27 Implementation Prompt

**Agent:** A3 | **Phase:** 27 | **Target:** 2000+ tests (current: 1927, +75-100 tests)
**Modules:** 4 new + 1 refactor | **Architecture:** Tier 2 Similarity Enhancement + Adaptive Tiering

---

## Overview: Tier 2 Similarity Pipeline Enhancement

Phase 27 completes Tier 2 (async similarity + delta compression) with production-grade orchestration, monitoring, and recovery. The pipeline currently has:

- **Tier 1 (Exact-match):** BLAKE3 hash → CAS lookup (inline, always-on)
- **Tier 2 Foundation:** `similarity.rs`, `delta_index.rs`, `chunk_pipeline.rs`

Phase 27 adds:
1. **similarity_coordinator.rs** — Orchestrate Tier 2 across chunk_pipeline (35 tests)
2. **adaptive_classifier.rs** — AI-assisted workload classification (25 tests)
3. **recovery_enhancer.rs** — Cross-shard crash recovery (20 tests)
4. **similarity_tier_stats.rs** — Detailed monitoring + effectiveness metrics (15-20 tests)

---

## Module 1: similarity_coordinator.rs (~35 tests)

**Purpose:** Orchestrate Tier 2 similarity detection across the chunk_pipeline. Coordinates feature extraction, similarity lookups, delta compression scheduling, and result caching.

### Core Types

```rust
/// Tier 2 similarity detection coordinator.
/// Manages feature extraction, lookup, and delta compression scheduling.
pub struct SimilarityCoordinator {
    similarity_index: Arc<SimilarityIndex>,
    delta_index: Arc<RwLock<DeltaIndex>>,
    feature_extractor: FeatureExtractor,
    delta_compressor: DeltaCompressor,
    cache: Arc<RwLock<LruCache<ChunkHash, SimilarityResult>>>,
    stats: Arc<RwLock<CoordinatorStats>>,
    config: SimilarityConfig,
}

/// Similarity detection result (caching + async tracking).
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    pub query_hash: ChunkHash,
    pub similar_hash: Option<ChunkHash>,
    pub delta_bytes: Option<usize>,
    pub compression_ratio: Option<f64>,
    pub detected_at: Instant,
}

/// Configuration for similarity detection (thresholds, caching, scheduling).
#[derive(Debug, Clone)]
pub struct SimilarityConfig {
    pub enable_similarity: bool,
    pub feature_extraction_threshold: usize,  // Min chunk size for feature extraction
    pub cache_size: usize,  // LRU cache entries for recent results
    pub batch_delay_ms: u64,  // Max delay before processing batch
    pub max_batch_size: usize,  // Max chunks per feature extraction batch
    pub delta_compression_enabled: bool,
    pub result_ttl_secs: u64,  // Cache entry TTL
}

/// Coordinator statistics (per-workload tracking).
#[derive(Debug, Clone, Default)]
pub struct CoordinatorStats {
    pub chunks_processed: u64,
    pub similarity_lookups: u64,
    pub similarity_hits: u64,
    pub similarity_misses: u64,
    pub delta_compressions_scheduled: u64,
    pub delta_compressions_completed: u64,
    pub total_delta_bytes_saved: u64,
    pub feature_extraction_time_ms: u64,
    pub similarity_lookup_time_ms: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}
```

### Key Methods

```rust
impl SimilarityCoordinator {
    /// Create new coordinator with default or custom config.
    pub fn new(config: SimilarityConfig) -> Result<Self, ReduceError>;

    /// Process a chunk through Tier 2 similarity detection.
    /// 1. Check cache for recent result
    /// 2. Extract features (MinHash Super-Features)
    /// 3. Query similarity index for similar blocks
    /// 4. If found, schedule delta compression
    /// 5. Return result and update stats
    pub async fn process_chunk(
        &self,
        hash: ChunkHash,
        data: &[u8],
    ) -> Result<SimilarityResult, ReduceError>;

    /// Batch feature extraction and similarity lookup (background task).
    /// Called periodically or when batch reaches max_batch_size.
    /// Coordinates with chunk_pipeline post-dedup scheduling.
    pub async fn process_batch(
        &self,
        chunks: Vec<(ChunkHash, Vec<u8>)>,
    ) -> Result<Vec<SimilarityResult>, ReduceError>;

    /// Schedule delta compression for a similar chunk pair.
    /// Computes Zstd dictionary delta and updates delta_index.
    /// Returns delta_bytes saved and compression_ratio.
    pub async fn schedule_delta_compression(
        &self,
        query_hash: ChunkHash,
        query_data: &[u8],
        similar_hash: ChunkHash,
        similar_data: &[u8],
    ) -> Result<(usize, f64), ReduceError>;

    /// Invalidate cache entry (used on chunk eviction or GC).
    pub fn invalidate_cache(&self, hash: ChunkHash);

    /// Get current stats (read-only snapshot).
    pub fn stats(&self) -> CoordinatorStats;

    /// Reset stats (for testing or per-interval reset).
    pub fn reset_stats(&self);

    /// Coordinate integration with chunk_pipeline:
    /// - Called after Tier 1 dedup (exact-match) completes
    /// - Schedules Tier 2 (similarity) asynchronously
    /// - Returns immediately (non-blocking)
    pub fn schedule_for_similarity(
        &self,
        hash: ChunkHash,
        data: Vec<u8>,
    ) -> Result<(), ReduceError>;
}
```

### Tests (~35 tests)

1. **Initialization & Configuration** (3-4 tests)
   - `test_new_with_default_config()` — create with defaults
   - `test_new_with_custom_config()` — set custom thresholds
   - `test_invalid_config_rejected()` — reject bad config

2. **Feature Extraction & Cache** (5-6 tests)
   - `test_process_chunk_small_skipped()` — chunks below threshold skipped
   - `test_process_chunk_cache_hit()` — recent result cached
   - `test_process_chunk_cache_miss_recalculates()` — expired entries recalculated
   - `test_cache_invalidation()` — explicit invalidation works
   - `test_cache_ttl_expiration()` — entries expire after TTL

3. **Similarity Lookup** (6-7 tests)
   - `test_similarity_lookup_hit()` — find similar chunk
   - `test_similarity_lookup_miss()` — no similar chunk found
   - `test_similarity_lookup_multiple_candidates()` — pick best match
   - `test_similarity_threshold_respected()` — threshold filtering
   - `test_similarity_with_empty_index()` — graceful empty index

4. **Delta Compression Scheduling** (6-7 tests)
   - `test_schedule_delta_compression_success()` — compute delta correctly
   - `test_schedule_delta_compression_ratio_tracked()` — record compression ratio
   - `test_schedule_delta_compression_updates_delta_index()` — delta_index updated
   - `test_delta_compression_partial_match()` — partial similarities handled
   - `test_delta_compression_failed_gracefully()` — errors don't crash coordinator

5. **Batch Processing** (4-5 tests)
   - `test_batch_process_empty()` — empty batch OK
   - `test_batch_process_multiple()` — process multiple chunks
   - `test_batch_delay_timer()` — delay before batch submission
   - `test_batch_max_size_trigger()` — full batch triggers submission

6. **Stats & Telemetry** (3-4 tests)
   - `test_stats_tracking()` — all metrics incremented correctly
   - `test_stats_ratios_calculated()` — hit rate, compression ratio computed
   - `test_stats_reset()` — reset clears counters

7. **Integration with chunk_pipeline** (3-4 tests)
   - `test_schedule_for_similarity_non_blocking()` — returns immediately
   - `test_schedule_for_similarity_batches()` — batches scheduled chunks
   - `test_coordinate_with_chunk_pipeline()` — end-to-end with pipeline

---

## Module 2: adaptive_classifier.rs (~25 tests)

**Purpose:** Workload-aware adaptive classification for Tier 2 scheduling and compression level hints. Learns access patterns and recommends tiering policies.

### Core Types

```rust
/// Workload fingerprint: learned access pattern characteristics.
#[derive(Debug, Clone)]
pub struct WorkloadFingerprint {
    pub pattern_type: AccessPatternType,  // Sequential, Random, Temporal, etc.
    pub similarity_hit_rate: f64,  // Observed Tier 2 hit rate
    pub compression_effectiveness: f64,  // Avg compression ratio
    pub chunk_size_avg: usize,
    pub write_frequency: f64,  // Writes per second
    pub age_secs: u64,
}

/// Access pattern classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessPatternType {
    Sequential,  // High locality (sequential writes)
    Random,  // Low locality (random writes)
    Temporal,  // Temporal locality (recent chunks accessed again)
    Compressible,  // High compression ratio observed
    SimilarityHigh,  // High similarity hit rate
    SimilarityLow,  // Low similarity hit rate
    Unknown,  // Insufficient data
}

/// Adaptive tiering recommendation (output of classifier).
#[derive(Debug, Clone)]
pub struct TieringRecommendation {
    pub pattern: AccessPatternType,
    pub compression_level: CompressionLevel,  // None, Fast, Default, Best
    pub dedup_strength: DedupStrength,  // Inline, Async, None
    pub s3_tiering_policy: S3TieringPolicy,  // Flash, Warm, Cold, Archive
    pub confidence: f64,  // 0.0-1.0
}

#[derive(Debug, Clone, Copy)]
pub enum CompressionLevel {
    None,
    Fast,
    Default,
    Best,
}

#[derive(Debug, Clone, Copy)]
pub enum DedupStrength {
    Inline,  // Tier 1 exact-match only
    Async,  // Tier 1 + Tier 2 background
    None,  // Disabled
}

#[derive(Debug, Clone, Copy)]
pub enum S3TieringPolicy {
    Flash,  // Stay in flash tier
    Warm,  // Transition to S3 after N days
    Cold,  // Immediate S3 transition
    Archive,  // S3 + glacier after N days
}

/// Adaptive classifier learns from reduction stats and makes recommendations.
pub struct AdaptiveClassifier {
    fingerprints: Arc<RwLock<HashMap<String, WorkloadFingerprint>>>,  // workload_name -> fingerprint
    stats_history: Arc<RwLock<Vec<ClassifierStats>>>,  // Rolling window of stats
    config: ClassifierConfig,
    last_update: Arc<Mutex<Instant>>,
}

#[derive(Debug, Clone)]
pub struct ClassifierConfig {
    pub learning_window_secs: u64,  // How long to observe before recommending
    pub min_samples: usize,  // Min observations before confident recommendation
    pub similarity_threshold_for_high: f64,  // Hit rate > this = "high"
    pub compression_threshold_for_high: f64,  // Ratio > this = "high"
}

#[derive(Debug, Clone)]
pub struct ClassifierStats {
    pub workload: String,
    pub timestamp: u64,
    pub similarity_hit_rate: f64,
    pub compression_ratio: f64,
    pub pattern_observed: AccessPatternType,
}
```

### Key Methods

```rust
impl AdaptiveClassifier {
    /// Create new adaptive classifier.
    pub fn new(config: ClassifierConfig) -> Self;

    /// Update fingerprint with new observation from chunk_pipeline stats.
    /// Called periodically (e.g., every 10 seconds) with rolling stats.
    pub fn update_fingerprint(
        &self,
        workload: &str,
        stats: &ChunkPipelineStats,
        coordinator_stats: &CoordinatorStats,
    ) -> Result<(), ReduceError>;

    /// Classify a workload based on learned fingerprint.
    /// Returns AccessPatternType or Unknown if insufficient data.
    pub fn classify_pattern(&self, workload: &str) -> Result<AccessPatternType, ReduceError>;

    /// Get tiering recommendation for a workload.
    /// Combines pattern classification, hit rates, compression effectiveness.
    pub fn recommend_tiering(
        &self,
        workload: &str,
    ) -> Result<TieringRecommendation, ReduceError>;

    /// Get compression level recommendation based on workload.
    pub fn recommend_compression_level(
        &self,
        workload: &str,
    ) -> Result<CompressionLevel, ReduceError>;

    /// Get dedup strength recommendation (inline vs async vs none).
    pub fn recommend_dedup_strength(
        &self,
        workload: &str,
    ) -> Result<DedupStrength, ReduceError>;

    /// Get S3 tiering policy recommendation.
    pub fn recommend_s3_policy(
        &self,
        workload: &str,
    ) -> Result<S3TieringPolicy, ReduceError>;

    /// Get workload fingerprint (read-only).
    pub fn get_fingerprint(&self, workload: &str) -> Option<WorkloadFingerprint>;

    /// Reset learning for a workload (fresh start).
    pub fn reset_workload(&self, workload: &str);

    /// Get all known workloads.
    pub fn list_workloads(&self) -> Vec<String>;
}
```

### Tests (~25 tests)

1. **Initialization** (2 tests)
   - `test_new_classifier()` — create with defaults
   - `test_custom_config()` — set thresholds

2. **Fingerprint Learning** (5-6 tests)
   - `test_update_fingerprint_sequential()` — detect sequential pattern
   - `test_update_fingerprint_random()` — detect random pattern
   - `test_update_fingerprint_temporal()` — detect temporal locality
   - `test_update_fingerprint_insufficient_data()` — Unknown until min_samples
   - `test_fingerprint_converges()` — pattern stabilizes

3. **Pattern Classification** (4-5 tests)
   - `test_classify_pattern_sequential()` — classify as Sequential
   - `test_classify_pattern_unknown()` — Unknown when no data
   - `test_classify_multiple_workloads()` — separate fingerprints per workload
   - `test_classify_workload_not_found()` — error handling

4. **Compression Level Recommendation** (3-4 tests)
   - `test_recommend_compression_high_ratio()` — Best for high compression
   - `test_recommend_compression_low_ratio()` — Fast for low compression
   - `test_recommend_compression_unknown_pattern()` — Default for unknown

5. **Dedup Strength Recommendation** (3-4 tests)
   - `test_recommend_dedup_high_similarity()` — Async for high hit rate
   - `test_recommend_dedup_low_similarity()` — Inline for low hit rate
   - `test_recommend_dedup_none_option()` — Disable dedup if ineffective

6. **S3 Tiering Recommendation** (3-4 tests)
   - `test_recommend_s3_frequent_access()` — Flash for active workloads
   - `test_recommend_s3_cold_access()` — Archive for cold workloads
   - `test_recommend_s3_temporal()` — Warm for temporal patterns

7. **Integration & Management** (2-3 tests)
   - `test_reset_workload()` — clear fingerprint
   - `test_list_workloads()` — enumerate all workloads

---

## Module 3: recovery_enhancer.rs (~20 tests)

**Purpose:** Crash recovery and cross-shard consistency verification for Tier 2. Detects incomplete similarity detection operations from crashes and resumes them.

### Core Types

```rust
/// Recovery tracker for incomplete Tier 2 operations.
pub struct RecoveryEnhancer {
    checkpoint_store: Arc<CheckpointStore>,
    similarity_coordinator: Arc<SimilarityCoordinator>,
    config: RecoveryConfig,
    stats: Arc<RwLock<RecoveryStats>>,
}

/// Checkpoint for incomplete similarity operation (crash recovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityCheckpoint {
    pub checkpoint_id: u64,
    pub workload: String,
    pub chunks_processed: u64,
    pub chunks_remaining: u64,
    pub delta_compressions_completed: u64,
    pub delta_compressions_pending: u64,
    pub last_chunk_hash: Option<ChunkHash>,
    pub created_at: u64,
    pub last_updated: u64,
    pub progress_percent: f64,
}

/// Recovery configuration (retry limits, batch sizes, timeouts).
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    pub checkpoint_retention_days: u64,
    pub max_retry_attempts: usize,
    pub retry_delay_ms: u64,
    pub verification_batch_size: usize,  // Chunks per verify batch
}

/// Recovery statistics (tracking resume/recovery operations).
#[derive(Debug, Clone, Default)]
pub struct RecoveryStats {
    pub checkpoints_created: u64,
    pub checkpoints_resumed: u64,
    pub chunks_recovered: u64,
    pub delta_compressions_resumed: u64,
    pub inconsistencies_detected: u64,
    pub inconsistencies_fixed: u64,
    pub recovery_failures: u64,
}
```

### Key Methods

```rust
impl RecoveryEnhancer {
    /// Create new recovery enhancer.
    pub fn new(
        checkpoint_store: Arc<CheckpointStore>,
        coordinator: Arc<SimilarityCoordinator>,
        config: RecoveryConfig,
    ) -> Self;

    /// Create checkpoint for in-progress similarity detection batch.
    /// Called periodically during long-running batch similarity operations.
    pub async fn create_checkpoint(
        &self,
        workload: &str,
        chunks_processed: u64,
        chunks_remaining: u64,
        last_chunk_hash: ChunkHash,
    ) -> Result<u64, ReduceError>;

    /// Resume incomplete similarity detection from checkpoint.
    /// Called on startup if checkpoint found for incomplete operation.
    /// Returns count of chunks resumed.
    pub async fn resume_from_checkpoint(
        &self,
        checkpoint: SimilarityCheckpoint,
    ) -> Result<u64, ReduceError>;

    /// Detect incomplete operations and resume them.
    /// Scans checkpoint store for abandoned operations (older than TTL).
    /// Called on service startup.
    pub async fn detect_and_resume_incomplete(
        &self,
    ) -> Result<(usize, u64), ReduceError>;  // (count, total_chunks)

    /// Verify cross-shard consistency of delta_index.
    /// Ensures all similarity pairs are consistent across shards.
    /// Returns list of inconsistencies found.
    pub async fn verify_cross_shard_consistency(
        &self,
    ) -> Result<Vec<InconsistencyRecord>, ReduceError>;

    /// Fix detected inconsistency (rebuild delta_index entry).
    pub async fn fix_inconsistency(
        &self,
        record: &InconsistencyRecord,
    ) -> Result<(), ReduceError>;

    /// Get recovery stats.
    pub fn stats(&self) -> RecoveryStats;

    /// Cleanup old checkpoints (by TTL).
    pub async fn cleanup_old_checkpoints(&self) -> Result<usize, ReduceError>;
}

#[derive(Debug, Clone)]
pub struct InconsistencyRecord {
    pub shard_id: u64,
    pub chunk_hash: ChunkHash,
    pub expected_delta_index_entry: Option<DeltaIndexEntry>,
    pub actual_delta_index_entry: Option<DeltaIndexEntry>,
    pub timestamp: u64,
}
```

### Tests (~20 tests)

1. **Checkpoint Creation & Persistence** (4-5 tests)
   - `test_create_checkpoint()` — create and persist
   - `test_checkpoint_fields_correct()` — all fields recorded
   - `test_checkpoint_crc32_validation()` — checkpoint integrity verified
   - `test_checkpoint_retrieval()` — load from store

2. **Recovery from Checkpoint** (4-5 tests)
   - `test_resume_from_checkpoint_success()` — resume successfully
   - `test_resume_updates_coordinator_state()` — coordinator state updated
   - `test_resume_partial_delta_compressions()` — pending deltas resumed
   - `test_resume_retry_on_transient_failure()` — retry logic works

3. **Detect & Resume Incomplete Operations** (3-4 tests)
   - `test_detect_incomplete_on_startup()` — find abandoned operations
   - `test_resume_multiple_incomplete()` — handle multiple simultaneously
   - `test_detect_skips_recent_checkpoints()` — don't resume active operations

4. **Cross-Shard Consistency Verification** (3-4 tests)
   - `test_verify_consistency_all_consistent()` — no inconsistencies
   - `test_verify_consistency_detects_missing_entry()` — detect missing
   - `test_verify_consistency_detects_mismatch()` — detect value mismatch
   - `test_verify_consistency_cross_shard()` — verify across shard boundaries

5. **Inconsistency Fixing** (2-3 tests)
   - `test_fix_inconsistency_missing_entry()` — rebuild missing entry
   - `test_fix_inconsistency_mismatch()` — correct mismatched value

6. **Cleanup & Management** (2-3 tests)
   - `test_cleanup_old_checkpoints()` — remove expired checkpoints
   - `test_stats_tracking()` — recovery stats incremented correctly

---

## Module 4: similarity_tier_stats.rs (~15-20 tests)

**Purpose:** Detailed Tier 2 monitoring and effectiveness metrics. Tracks feature extraction latency, delta compression ratios, hit rates per workload, and effectiveness for tuning.

### Core Types

```rust
/// Per-workload Tier 2 performance metrics.
#[derive(Debug, Clone)]
pub struct TierStats {
    pub workload: String,
    pub feature_extraction_samples: u64,
    pub feature_extraction_latency_us: Vec<u64>,  // Sample latencies for percentile calc
    pub similarity_lookups: u64,
    pub similarity_hits: u64,
    pub delta_compressions_scheduled: u64,
    pub delta_compressions_completed: u64,
    pub delta_bytes_before: u64,
    pub delta_bytes_after: u64,
    pub delta_compression_latency_us: Vec<u64>,
    pub timestamp_secs: u64,
}

/// Effectiveness metrics for Tier 2 tuning.
#[derive(Debug, Clone)]
pub struct EffectivenessMetrics {
    pub workload: String,
    pub feature_extraction_throughput_gb_per_sec: f64,
    pub similarity_hit_rate: f64,
    pub delta_compression_ratio: f64,
    pub total_bytes_saved: u64,
    pub cpu_cost_percent: f64,  // Estimated CPU % used for Tier 2
    pub overall_effectiveness_score: f64,  // 0.0-1.0 composite score
}

/// Collector for Tier 2 statistics and metrics.
pub struct SimilarityTierStats {
    stats_by_workload: Arc<RwLock<HashMap<String, TierStats>>>,
    effectiveness: Arc<RwLock<HashMap<String, EffectivenessMetrics>>>,
    config: StatsConfig,
}

#[derive(Debug, Clone)]
pub struct StatsConfig {
    pub sample_window_size: usize,  // Max samples in latency vectors
    pub effectiveness_update_interval_secs: u64,
}
```

### Key Methods

```rust
impl SimilarityTierStats {
    /// Create new stats collector.
    pub fn new(config: StatsConfig) -> Self;

    /// Record feature extraction sample (latency in microseconds).
    pub fn record_feature_extraction(
        &self,
        workload: &str,
        latency_us: u64,
        bytes_processed: u64,
    );

    /// Record similarity lookup (hit or miss).
    pub fn record_similarity_lookup(
        &self,
        workload: &str,
        hit: bool,
    );

    /// Record delta compression operation.
    pub fn record_delta_compression(
        &self,
        workload: &str,
        before_bytes: u64,
        after_bytes: u64,
        latency_us: u64,
    );

    /// Calculate effectiveness metrics for workload.
    /// Returns throughput, hit rate, compression ratio, efficiency score.
    pub fn calculate_effectiveness(
        &self,
        workload: &str,
    ) -> Result<EffectivenessMetrics, ReduceError>;

    /// Get percentile latencies (p50, p95, p99).
    pub fn get_latency_percentiles(
        &self,
        workload: &str,
    ) -> Result<(u64, u64, u64), ReduceError>;  // (p50, p95, p99)

    /// Get workload stats snapshot (read-only).
    pub fn get_stats(&self, workload: &str) -> Option<TierStats>;

    /// List all monitored workloads.
    pub fn list_workloads(&self) -> Vec<String>;

    /// Export stats to Prometheus metrics format.
    pub fn export_prometheus(&self) -> String;

    /// Reset stats for workload (for testing or per-interval reset).
    pub fn reset_workload(&self, workload: &str);
}
```

### Tests (~15-20 tests)

1. **Feature Extraction Tracking** (3-4 tests)
   - `test_record_feature_extraction()` — latency recorded
   - `test_feature_extraction_latency_percentiles()` — p50, p95, p99 calculated
   - `test_feature_extraction_throughput()` — GB/s calculated

2. **Similarity Lookup Tracking** (2-3 tests)
   - `test_record_similarity_hit()` — hit recorded
   - `test_record_similarity_miss()` — miss recorded
   - `test_similarity_hit_rate_calculated()` — hit rate = hits / total

3. **Delta Compression Tracking** (3-4 tests)
   - `test_record_delta_compression()` — compression data recorded
   - `test_delta_compression_ratio()` — ratio calculated
   - `test_delta_bytes_saved()` — total savings calculated
   - `test_delta_compression_latency_percentiles()` — latency metrics

4. **Effectiveness Metrics** (3-4 tests)
   - `test_effectiveness_metrics_calculated()` — all metrics computed
   - `test_effectiveness_score_formula()` — composite score reasonable
   - `test_effectiveness_workload_comparison()` — multiple workloads tracked

5. **Prometheus Export** (2 tests)
   - `test_export_prometheus_format()` — valid Prometheus format
   - `test_export_includes_all_metrics()` — all metrics present

6. **Workload Management** (2-3 tests)
   - `test_list_workloads()` — enumerate tracked workloads
   - `test_reset_workload()` — clear stats
   - `test_separate_workload_stats()` — no cross-contamination

---

## Integration & Architecture

### Data Flow

```
Input: ChunkPipelineResult (after Tier 1 exact-match dedup)
  ↓
SimilarityCoordinator.process_chunk()
  ├→ FeatureExtractor: extract MinHash Super-Features
  ├→ SimilarityIndex.find_similar(): lookup matching chunks
  ├→ If found: schedule_delta_compression()
  │   ├→ DeltaCompressor: compute Zstd delta
  │   └→ DeltaIndex.insert(): add to index
  └→ SimilarityTierStats.record_*(): track metrics
  ↓
AdaptiveClassifier.update_fingerprint(): learn workload patterns
  ├→ Recommend CompressionLevel
  ├→ Recommend DedupStrength
  └→ Recommend S3TieringPolicy
  ↓
RecoveryEnhancer (background):
  ├→ Periodically create_checkpoint() for long batches
  ├→ On startup: detect_and_resume_incomplete()
  └→ Periodically verify_cross_shard_consistency()
```

### Module Dependencies

- **similarity_coordinator.rs** depends on:
  - `similarity.rs` (SimilarityIndex)
  - `delta_index.rs` (DeltaIndex)
  - `fingerprint.rs` (ChunkHash, SuperFeatures)
  - `compression.rs` (for Zstd delta compression)

- **adaptive_classifier.rs** depends on:
  - `chunk_pipeline.rs` (ChunkPipelineStats)
  - `similarity_coordinator.rs` (CoordinatorStats)
  - Minimal—mostly logic, no external dependencies

- **recovery_enhancer.rs** depends on:
  - `similarity_coordinator.rs` (SimilarityCoordinator)
  - Checkpoint storage (external, injected)
  - `delta_index.rs` (for consistency verification)

- **similarity_tier_stats.rs** depends on:
  - None—pure stats collection, can be called from any module

### Async/Await & Concurrency

- All public methods marked `async` where they perform I/O or long-running ops
- Use `Arc<RwLock<T>>` for shared mutable state (coordinator, stats, classifier)
- Use `Arc<Mutex<T>>` for last_update timestamp (single writer)
- Feature extraction, delta compression, and cross-shard verification run as background tasks
- No thread spawning—all work on Tokio thread pool

### Error Handling

All methods return `Result<T, ReduceError>`. Key errors:

- `ReduceError::InvalidChunk` — data validation failed
- `ReduceError::CompressionFailed` — delta compression error
- `ReduceError::IndexFull` — similarity index capacity exceeded
- `ReduceError::Checkpointing` — checkpoint store unavailable
- `ReduceError::InconsistencyDetected` — cross-shard verification failed

### Testing Requirements

- **Unit tests:** Each method tested independently with mock/local data
- **Integration tests:** End-to-end coordinator → classifier → recovery → stats flow
- **Concurrency tests:** Multiple threads accessing coordinator simultaneously
- **Recovery tests:** Simulate crash, restart, verify resume works
- **Consistency tests:** Cross-shard verification catches introduced inconsistencies
- **Performance tests:** Measure feature extraction, delta compression latencies

---

## Success Criteria

✅ **Phase 27 Complete** when:
1. All 4 modules compile (`cargo check -p claudefs-reduce`)
2. All ~90-100 new tests pass (`cargo test -p claudefs-reduce --lib`)
3. Total tests: 1927 + 90-100 = **2000+ tests passing**
4. No clippy warnings on new code
5. Integration with existing `chunk_pipeline`, `similarity`, `delta_index` verified
6. Recovery coordinator integrates with checkpoint store (mock OK)
7. Stats exported to Prometheus format (mock OK)
8. All modules added to `lib.rs` public API

**Test target breakdown:**
- similarity_coordinator.rs: 35 tests ✓
- adaptive_classifier.rs: 25 tests ✓
- recovery_enhancer.rs: 20 tests ✓
- similarity_tier_stats.rs: 18 tests ✓
- **Total: 98 tests** (2025 tests passing) ✓

---

## Implementation Notes

### Model Recommendation

Use **minimax-m2p5** (default). If it times out or struggles, fall back to **glm-5**.

### Code Quality

- Follow existing code style in claudefs-reduce (Arc<RwLock>, async methods)
- Minimal panics—return Result<T, ReduceError> instead
- No unwrap() except in tests
- All public types and methods documented with /// comments
- Test names follow pattern: `test_<function>_<scenario>()`

### Performance Targets

- Feature extraction: < 100 µs per chunk (measured in tests)
- Similarity lookup: < 50 µs per chunk
- Delta compression: < 1 ms per chunk (with background execution)
- Cache hit rate: > 80% for repeated chunks (tested)

---

## References

### Existing Modules to Integrate With

- `chunk_pipeline.rs` (line 50) — ChunkPipelineStats, ChunkPipelineResult
- `similarity.rs` (line 1) — SimilarityIndex, find_similar()
- `delta_index.rs` (line 1) — DeltaIndex, DeltaIndexEntry
- `fingerprint.rs` — ChunkHash, SuperFeatures, FeatureExtractor
- `compression.rs` — CompressionAlgorithm, Zstd delta compression
- `error.rs` — ReduceError enum

### Docs Reference

- `docs/reduction.md` — Tier 1/Tier 2 architecture overview
- `docs/decisions.md` (D3) — Data reduction design decisions
- Finesse paper (FAST '14) — Resemblance detection via Super-Features
- VAST architecture — Tier 2 similarity pipeline (reference)

---

END OF SPECIFICATION
