# A3 Phase 13: Encryption Key Store, Bandwidth Throttle, Chunk Dedup Stats

You are implementing Phase 13 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
916 tests across 49 modules. Phase 13 goal: ~1010 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/key_store.rs`

Implement a persistent-style encryption key store for managing data encryption keys by version.

In ClaudeFS, chunk data is encrypted per D7. Keys are versioned; old keys must be retained
to decrypt existing data. New keys can be generated for new data.

Requirements:
- `KeyStoreConfig` struct: `max_versions: usize` (default 100), `rotation_interval_ms: u64` (default 30 days = 30*24*3600*1000)
  - Derive Debug, Clone, Serialize, Deserialize
- `StoredKey` struct: `version: u32`, `key_bytes: [u8; 32]`, `created_at_ms: u64`, `deprecated_at_ms: Option<u64>`, `retired_at_ms: Option<u64>`
  - `fn is_active(&self) -> bool` → deprecated_at_ms.is_none() && retired_at_ms.is_none()
  - `fn is_deprecated(&self) -> bool` → deprecated_at_ms.is_some() && retired_at_ms.is_none()
  - `fn is_retired(&self) -> bool` → retired_at_ms.is_some()
  - Derive Debug, Clone, Serialize, Deserialize
- `KeyStoreStats` struct: `active_keys: usize`, `deprecated_keys: usize`, `retired_keys: usize`, `total_versions: usize`
  - Derive Debug, Clone, Default
- `KeyStore` struct:
  - `fn new(config: KeyStoreConfig) -> Self`
  - `fn generate_key(&mut self, version: u32, now_ms: u64) -> &StoredKey` — create new active key with random bytes (use `rand::thread_rng` and `rand::RngCore::fill_bytes`)
  - `fn get(&self, version: u32) -> Option<&StoredKey>`
  - `fn current_version(&self) -> Option<u32>` — highest active version
  - `fn deprecate(&mut self, version: u32, now_ms: u64) -> bool` — mark deprecated
  - `fn retire(&mut self, version: u32, now_ms: u64) -> bool` — mark retired (can no longer decrypt)
  - `fn list_active(&self) -> Vec<&StoredKey>` — sorted by version ascending
  - `fn needs_rotation(&self, last_rotation_ms: u64, now_ms: u64) -> bool` → (now_ms - last_rotation_ms) >= rotation_interval_ms
  - `fn stats(&self) -> KeyStoreStats`
  - `fn prune_retired(&mut self, keep_last_n: usize)` — remove all but the last keep_last_n retired keys

Write at least **16 tests**:
1. key_store_config_default
2. generate_key_creates_entry
3. get_existing_key
4. get_missing_key
5. current_version_empty
6. current_version_with_key
7. deprecate_key
8. deprecate_unknown_returns_false
9. retire_key
10. is_active_true
11. is_deprecated_true
12. is_retired_true
13. list_active_sorted
14. needs_rotation_false
15. needs_rotation_true
16. stats_counts_correctly
17. prune_retired_keeps_last_n

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/bandwidth_throttle.rs`

Implement bandwidth throttling for background data reduction operations.

Background operations (compaction, tier migration, GC) must not saturate disk I/O
and impact foreground user I/O. This implements a token bucket throttler.

Requirements:
- `ThrottleConfig` struct: `rate_bytes_per_sec: u64` (default 100MB = 100*1024*1024), `burst_bytes: u64` (default 4MB = 4*1024*1024)
  - Derive Debug, Clone, Serialize, Deserialize
- `TokenBucket` struct:
  - `fn new(rate_bytes_per_sec: u64, burst_bytes: u64) -> Self`
  - `fn try_consume(&mut self, bytes: u64, now_ms: u64) -> bool` — add tokens based on elapsed time (tokens = rate * elapsed_secs), cap at burst_bytes; consume bytes if available; return true if consumed, false if insufficient tokens
  - `fn available_tokens(&self) -> u64` — current token count
  - `fn refill(&mut self, now_ms: u64)` — add tokens based on elapsed time since last refill
- `BandwidthThrottle` struct:
  - `fn new(config: ThrottleConfig) -> Self`
  - `fn request(&mut self, bytes: u64, now_ms: u64) -> ThrottleDecision`
  - `fn stats(&self) -> ThrottleStats`
- `ThrottleDecision` enum: `Allowed`, `Throttled { retry_after_ms: u64 }` — retry_after_ms = (needed_tokens / rate_bytes_per_ms) where rate_bytes_per_ms = rate/1000
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `ThrottleStats` struct: `requests_allowed: u64`, `requests_throttled: u64`, `bytes_allowed: u64`
  - Derive Debug, Clone, Default

Write at least **15 tests**:
1. throttle_config_default
2. token_bucket_initial_tokens_equals_burst
3. token_bucket_consume_within_burst
4. token_bucket_consume_exceeds_available
5. token_bucket_refill_over_time
6. token_bucket_capped_at_burst
7. bandwidth_throttle_allows_small_request
8. bandwidth_throttle_throttles_large_request
9. bandwidth_throttle_allows_after_refill
10. throttle_decision_allowed_variant
11. throttle_decision_throttled_has_retry_time
12. throttle_stats_counts_allowed
13. throttle_stats_counts_throttled
14. throttle_stats_bytes_allowed
15. zero_elapsed_time_no_refill

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/dedup_analytics.rs`

Implement dedup analytics for capacity planning and reporting.

Requirements:
- `DedupSample` struct: `timestamp_ms: u64`, `total_logical_bytes: u64`, `total_physical_bytes: u64`, `unique_chunks: u64`, `dedup_ratio: f64`
  - Derive Debug, Clone, Serialize, Deserialize
- `DedupTrend` enum: `Improving`, `Stable`, `Degrading`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `DedupAnalytics` struct:
  - `fn new(window_size: usize) -> Self` — rolling window of samples
  - `fn record_sample(&mut self, sample: DedupSample)` — add to rolling window (drop oldest if full)
  - `fn current_ratio(&self) -> Option<f64>` — last sample's dedup_ratio
  - `fn average_ratio(&self) -> Option<f64>` — mean of all samples' dedup_ratios
  - `fn trend(&self) -> DedupTrend` — compare first half vs second half average; Improving if second > first+0.05, Degrading if second < first-0.05, else Stable
  - `fn peak_ratio(&self) -> Option<f64>` — maximum dedup_ratio seen
  - `fn savings_bytes(&self) -> Option<u64>` — last sample: logical - physical
  - `fn sample_count(&self) -> usize`
  - `fn estimate_future_capacity(&self, months_ahead: u32) -> Option<u64>` — simple linear extrapolation of physical bytes based on last 2 samples

Write at least **14 tests**:
1. new_analytics_empty
2. record_sample_adds_entry
3. record_sample_rolling_window
4. current_ratio_empty
5. current_ratio_with_sample
6. average_ratio_single_sample
7. average_ratio_multiple
8. trend_empty_returns_stable
9. trend_improving
10. trend_degrading
11. trend_stable
12. peak_ratio
13. savings_bytes
14. estimate_future_capacity_empty
15. sample_count

---

## EXPAND TESTS in existing modules

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/worm_reducer.rs`
Read the file first (36 tests). Add 8 more tests.

New tests covering edge cases: WORM policy enforcement under various retention periods,
WORM violation detection, retention policy expiry.

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/audit_log.rs`
Read the file first (27 tests). Add 8 more tests.

New tests covering: audit event filtering, log rotation, event timestamps,
audit log config options.

### Expand `/home/cfs/claudefs/crates/claudefs-reduce/src/key_rotation_scheduler.rs`
Read the file first (20 tests). Add 7 more tests covering rotation scheduling edge cases.

---

## Implementation instructions

1. READ each existing file before editing it
2. For new files: complete, compilable Rust with doc comments
3. For test expansions: append to existing `mod tests` blocks
4. Import `rand` for key generation in key_store.rs: `use rand::RngCore; rand::thread_rng().fill_bytes(&mut key_bytes);`
5. No async in new modules
6. Do NOT modify Cargo.toml

## Also update lib.rs

Add:
- `pub mod key_store;`
- `pub mod bandwidth_throttle;`
- `pub mod dedup_analytics;`
- Re-exports for key types

## Goal
- Build: 0 errors, 0 warnings
- Tests: ~1010+ passing
- Clippy: 0 warnings
