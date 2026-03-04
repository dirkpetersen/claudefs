# A3 Phase 10: Cache Coherency, Stripe Coordinator, Read Planner

You are implementing Phase 10 of the A3 (Data Reduction) agent for ClaudeFS, a distributed
scale-out POSIX file system in Rust. You will directly READ and WRITE files in the codebase.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
676 tests passing across 40 modules. Phase 10 goal: reach ~770 tests by:
1. Adding 3 new modules
2. Expanding test coverage in undertested modules

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/cache_coherency.rs`

Implement cache coherency tracking for multi-level caching in ClaudeFS.

The FUSE client (A5) maintains a read cache of decrypted chunks. When data is updated
(write or truncate), stale cache entries must be invalidated. This module tracks which
cache entries are valid and handles invalidation events.

Requirements:
- `CacheKey` struct: `inode_id: u64`, `chunk_index: u64` — identifies a cached chunk
  - Derive Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize
- `CacheVersion` struct: `version: u64` — monotonically increasing version per cache key
  - Derive Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize
  - `fn new() -> Self` → version 0
  - `fn increment(&self) -> Self` → version + 1
- `CacheEntry` struct: `key: CacheKey`, `version: CacheVersion`, `valid: bool`, `size_bytes: u64`
  - Derive Debug, Clone
- `InvalidationEvent` enum:
  - `ChunkInvalidated { key: CacheKey }` — specific chunk
  - `InodeInvalidated { inode_id: u64 }` — all chunks for an inode
  - `AllInvalidated` — full cache flush
  - Derive Debug, Clone
- `CoherencyTracker` struct:
  - `fn new() -> Self`
  - `fn register(&mut self, key: CacheKey, version: CacheVersion, size_bytes: u64)` — register a cache entry
  - `fn invalidate(&mut self, event: &InvalidationEvent) -> Vec<CacheKey>` — apply event, return invalidated keys
  - `fn is_valid(&self, key: &CacheKey, version: &CacheVersion) -> bool` — true if entry exists and version matches current
  - `fn get_version(&self, key: &CacheKey) -> Option<CacheVersion>` — current version for key
  - `fn bump_version(&mut self, key: &CacheKey) -> CacheVersion` — increment version for key
  - `fn valid_entry_count(&self) -> usize` — count of valid entries
  - `fn total_valid_bytes(&self) -> u64` — sum of valid entry sizes
  - `fn clear(&mut self)` — remove all entries

Write at least **16 tests**:
1. cache_key_equality
2. cache_version_ordering
3. cache_version_increment
4. register_entry
5. is_valid_true — correct version
6. is_valid_false_wrong_version — stale version
7. is_valid_false_missing_key — unregistered key
8. invalidate_chunk — specific chunk invalidated
9. invalidate_inode — all chunks for inode invalidated
10. invalidate_all — all entries cleared
11. invalidate_returns_affected_keys
12. bump_version_increments
13. bump_version_creates_if_missing
14. valid_entry_count_after_register
15. valid_entry_count_after_invalidate
16. total_valid_bytes
17. clear_removes_all

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/stripe_coordinator.rs`

Implement EC stripe coordination for distributing data chunks across nodes per D1/D8.

D1: EC 4+2 stripe. D8: EC stripes distributed across 6 different nodes via consistent hash.
The stripe coordinator maps segment IDs to node placement decisions.

Requirements:
- `NodeId` newtype: `pub struct NodeId(pub u64)` — Derive Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize
- `EcConfig` struct: `data_shards: u8` (default 4), `parity_shards: u8` (default 2)
  - `fn total_shards(&self) -> u8` → data_shards + parity_shards
  - `fn min_surviving_shards(&self) -> u8` → data_shards (can reconstruct from data_shards)
  - Derive Debug, Clone, Serialize, Deserialize
- `ShardPlacement` struct: `shard_index: u8`, `node_id: NodeId`, `is_parity: bool`
  - Derive Debug, Clone, Serialize, Deserialize
- `StripePlan` struct: `segment_id: u64`, `placements: Vec<ShardPlacement>`
  - `fn data_nodes(&self) -> Vec<NodeId>` — nodes holding data shards
  - `fn parity_nodes(&self) -> Vec<NodeId>` — nodes holding parity shards
  - `fn node_for_shard(&self, shard_index: u8) -> Option<NodeId>`
  - Derive Debug, Clone
- `StripeCoordinator` struct with `config: EcConfig`, `nodes: Vec<NodeId>`
  - `fn new(config: EcConfig, nodes: Vec<NodeId>) -> Self`
  - `fn plan_stripe(&self, segment_id: u64) -> StripePlan` — consistent hash to assign shards to nodes; uses `(segment_id * prime + shard_index) % nodes.len()` as simple placement hash; ensure same segment always gets same placement
  - `fn all_nodes_distinct(&self, plan: &StripePlan) -> bool` — returns true if all shard placements are on different nodes
  - `fn can_tolerate_failures(&self, plan: &StripePlan, failed_nodes: &[NodeId]) -> bool` — true if the number of failed nodes doesn't exceed parity_shards
- `StripeStats` struct: `segments_planned: u64`, `avg_nodes_per_stripe: f64`

Write at least **15 tests**:
1. ec_config_default
2. ec_config_total_shards — 4+2=6
3. ec_config_min_surviving — equals data_shards
4. plan_stripe_creates_correct_shard_count — 6 placements for 4+2 config
5. plan_stripe_deterministic — same segment_id → same plan
6. plan_stripe_different_segments_different_placements
7. data_nodes_count — equals data_shards
8. parity_nodes_count — equals parity_shards
9. node_for_shard_found
10. node_for_shard_not_found — shard_index out of range
11. can_tolerate_one_failure — with 4+2, one failure tolerable
12. can_tolerate_two_failures — with 4+2, two failures tolerable
13. cannot_tolerate_three_failures — with 4+2, three failures → false
14. plan_stripe_wraps_nodes — with 3 nodes and 6 shards, nodes reused
15. stripe_plan_parity_is_marked — is_parity field set correctly for parity shards

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/read_planner.rs`

Implement read planning: given a file read request, determine which chunks to fetch and from where.

On a file read, the FUSE client needs to:
1. Consult the block map to find which chunks cover the requested range
2. Check the read cache for already-cached chunks
3. Plan fetches for uncached chunks (from storage nodes)
4. Handle EC reconstruction if a node is unavailable

Requirements:
- `ReadRequest` struct: `inode_id: u64`, `offset: u64`, `length: u64`
  - Derive Debug, Clone, Copy
- `ChunkFetchPlan` struct: `chunk_hash: [u8; 32]`, `node_id: u64`, `segment_id: u64`, `from_cache: bool`
  - Derive Debug, Clone
- `ReadPlan` struct: `request: ReadRequest`, `fetches: Vec<ChunkFetchPlan>`, `cache_hits: usize`, `cache_misses: usize`
  - `fn total_chunks(&self) -> usize` → fetches.len()
  - `fn cache_hit_rate(&self) -> f64` → cache_hits as f64 / (cache_hits + cache_misses) as f64, or 0.0
  - Derive Debug
- `CachedChunkInfo` struct: `chunk_hash: [u8; 32]`, `cached: bool`
  - Derive Debug, Clone
- `ReadPlanner` struct:
  - `fn new() -> Self`
  - `fn plan(&self, request: ReadRequest, available_chunks: &[(CachedChunkInfo, u64)]) -> ReadPlan`
    - `available_chunks`: list of (chunk_info, node_id) — each chunk available at a node
    - Returns ReadPlan with fetches for all chunks; `from_cache=true` when cached
  - `fn estimate_latency_us(&self, plan: &ReadPlan, cache_latency_us: u64, network_latency_us: u64) -> u64`
    - Returns `cache_hits * cache_latency_us + cache_misses * network_latency_us`

Write at least **14 tests**:
1. read_plan_total_chunks
2. read_plan_cache_hit_rate_zero — no hits
3. read_plan_cache_hit_rate_one — all hits
4. read_plan_cache_hit_rate_partial
5. plan_no_chunks — empty available list → empty fetches
6. plan_all_cached — all chunks cached → all from_cache=true
7. plan_all_uncached — no chunks cached → from_cache=false
8. plan_mixed_cached
9. estimate_latency_all_cache — only cache latency
10. estimate_latency_all_network
11. estimate_latency_mixed
12. read_request_fields
13. chunk_fetch_plan_from_cache_field
14. read_planner_deterministic — same input → same output

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/async_meta_bridge.rs`
Read the file first (10 tokio tests). Add 10 more tokio tests.

The file has `AsyncFingerprintStore` trait, `AsyncLocalFingerprintStore`, `AsyncNullFingerprintStore`,
and `AsyncIntegratedWritePath`. New tests:
1. `test_async_store_empty_initially`
2. `test_async_store_insert_and_lookup`
3. `test_async_store_lookup_missing`
4. `test_async_store_increment_ref`
5. `test_async_store_decrement_ref`
6. `test_async_store_decrement_to_zero`
7. `test_async_null_store_lookup_none`
8. `test_async_null_store_insert_true`
9. `test_async_store_entry_count`
10. `test_async_store_total_deduplicated_bytes`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/checksum.rs`
Read the file first (11 tests). Add 9 more tests.

New tests covering `ChecksumAlgorithm`, `ChecksummedBlock`, `DataChecksum`:
1. `test_checksum_algorithm_variants`
2. `test_data_checksum_blake3`
3. `test_data_checksum_crc32c`
4. `test_checksummed_block_verify_ok`
5. `test_checksummed_block_verify_corrupted`
6. `test_checksum_deterministic`
7. `test_checksummed_block_roundtrip`
8. `test_different_data_different_checksum`
9. `test_checksum_empty_data`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/pipeline.rs`
Read the file first (13 tests). Add 9 more tests.

New tests covering `PipelineConfig`, `ReducedChunk`, `ReductionPipeline`, `ReductionStats`:
1. `test_pipeline_config_default`
2. `test_pipeline_stats_default`
3. `test_pipeline_process_tiny_data` — < 256 bytes
4. `test_pipeline_process_exactly_min_chunk`
5. `test_pipeline_reduction_ratio`
6. `test_pipeline_multiple_identical_chunks` — same data twice, dedupe kicks in
7. `test_reduced_chunk_fields`
8. `test_pipeline_stats_accumulate`
9. `test_pipeline_with_disabled_compression`

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/compression.rs`
Read the file first (13 tests). Add 7 more tests.

New tests:
1. `test_compress_lz4_empty` — empty data compresses/decompresses
2. `test_compress_zstd_level_1` — fastest zstd
3. `test_compress_zstd_level_19` — best zstd (verify roundtrip, not timing)
4. `test_compression_algorithm_display` — if Display is implemented, verify strings
5. `test_compress_binary_data` — compress random-ish binary
6. `test_decompress_invalid_data_returns_error`
7. `test_lz4_vs_zstd_same_data` — both algorithms decompress to same original

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: complete, compilable Rust with doc comments on all public items
3. For test expansions: append to existing `mod tests` blocks
4. Use `serde::{Serialize, Deserialize}` for data structs
5. No async in new modules
6. Do NOT modify Cargo.toml

## Also update lib.rs

Add:
- `pub mod cache_coherency;`
- `pub mod stripe_coordinator;`
- `pub mod read_planner;`
- Re-exports for key types

## Goal
- `cargo build -p claudefs-reduce` compiles with 0 errors, 0 warnings
- `cargo test -p claudefs-reduce` shows ~770+ tests passing
- `cargo clippy -p claudefs-reduce -- -D warnings` passes cleanly
