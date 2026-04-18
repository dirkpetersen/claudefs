# Phase 33 Block 5: Similarity Search Scaling (LSH)
## OpenCode Implementation Prompt

**Target:** Locality-Sensitive Hashing for approximate nearest-neighbor matching
**Output:** ~650 LOC (source) + 14 tests

## Context
Implement LSH for sub-10ms approximate similarity matching at 1PB scale with 95%+ recall.

## Key Components

### 1. LSHIndex (350 LOC)
```rust
pub struct LSHIndex {
    hash_tables: Vec<HashMap<u64, Vec<BlockId>>>,
    band_count: usize,
    row_count: usize,
}

impl LSHIndex {
    pub fn insert(&mut self, features: &[u8], block_id: BlockId);
    pub fn query(&self, features: &[u8]) -> Vec<BlockId>;
}
```

### 2. ApproximateMatcher (200 LOC)
- Coarse filtering + fine ranking
- Candidate aggregation from multiple shards
- Configurable recall vs latency

### 3. RecallTuner (100 LOC)
- Adaptive LSH parameters
- Adjust for target recall/latency

## Test Categories (14 tests)

1. **LSH correctness** (3 tests)
   - test_lsh_exact_matches_always_found
   - test_lsh_similar_within_threshold
   - test_lsh_dissimilar_rejected

2. **Distributed search** (3 tests)
   - test_lsh_query_single_shard
   - test_lsh_query_multi_shard
   - test_lsh_query_shard_failure

3. **Hierarchical search** (3 tests)
   - test_hierarchical_coarse_filtering
   - test_hierarchical_fine_ranking
   - test_hierarchical_recall_tuning

4. **Scalability** (2 tests)
   - test_search_latency_1pb_index
   - test_search_latency_scale_horizontal

5. **Recall validation** (3 tests)
   - test_recall_95_percent
   - test_recall_similarity_threshold
   - test_recall_index_rebuild

## Generate
- `crates/claudefs-reduce/src/lsh_index.rs`
- `crates/claudefs-reduce/src/approximate_matcher.rs`
- `crates/claudefs-reduce/src/recall_tuner.rs`
- `crates/claudefs-reduce/tests/cluster_lsh_scaling.rs` — 14 tests

All tests marked #[ignore].
