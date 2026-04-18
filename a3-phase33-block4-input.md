# Phase 33 Block 4: Feature Extraction Optimization
## OpenCode Implementation Prompt

**Target:** Tile caching and batch processing for similarity detection
**Output:** ~700 LOC (source) + 16 tests

## Context
Optimize feature extraction with L3-resident tile cache, vectorized batch processing, and bloom filter pre-filtering for sub-100μs per-chunk latency.

## Key Components

### 1. FeatureTileCache (250 LOC)
```rust
pub struct FeatureTileCache {
    tiles: Arc<RwLock<LRUCache<ChunkHash, SuperFeatures>>>,
    hit_stats: Arc<Atomic<CacheStats>>,
    max_size_mb: usize,
}
```

### 2. BatchFeatureExtractor (250 LOC)
- Process 128-chunk batches with SIMD
- Vectorized Rabin fingerprint computation
- 2-4x speedup over serial extraction

### 3. BloomFilterIndex (200 LOC)
- Pre-filter for feature lookups
- Dynamic updates, lock-free reads
- <1% false positive rate

## Test Categories (16 tests)

1. **Tile caching** (4 tests)
   - test_tile_cache_hit_rate
   - test_tile_cache_eviction_policy
   - test_tile_cache_memory_bounded
   - test_tile_cache_invalidation

2. **Batch processing** (4 tests)
   - test_batch_feature_extraction_correctness
   - test_batch_feature_extraction_speedup
   - test_batch_feature_extraction_simd
   - test_batch_feature_extraction_heterogeneous

3. **Bloom filter optimization** (4 tests)
   - test_bloom_filter_false_positive_rate
   - test_bloom_filter_reduces_lookups
   - test_bloom_filter_update_correctness
   - test_bloom_filter_concurrent_reads

4. **Performance validation** (4 tests)
   - test_feature_extraction_latency_p50
   - test_feature_extraction_latency_p99
   - test_similarity_index_lookup_l3_cache
   - test_feature_extraction_throughput_scale

## Generate
- `crates/claudefs-reduce/src/tile_cache.rs`
- `crates/claudefs-reduce/src/batch_extractor.rs`
- `crates/claudefs-reduce/src/bloom_index.rs`
- `crates/claudefs-reduce/tests/cluster_feature_optimization.rs` — 16 tests

All tests marked #[ignore].
