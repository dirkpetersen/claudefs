# A6: ClaudeFS Replication — Phase 6: Compression, Backpressure, and Metrics

You are implementing Phase 6 (production readiness) of the `claudefs-repl` crate for ClaudeFS.

## Already Implemented (Phase 1-5 + Security, 371 tests, 17 modules)

Existing modules in `crates/claudefs-repl/src/`:
- `error.rs`, `journal.rs`, `wal.rs`, `topology.rs` — foundation types
- `conduit.rs` — in-process mock conduit (`Conduit`, `EntryBatch`, `ConduitConfig`)
- `sync.rs` — `ConflictDetector` (LWW), `BatchCompactor`, `ReplicationSync`, `Conflict`
- `uidmap.rs` — UID/GID translation (`UidMapper`)
- `engine.rs` — `ReplicationEngine` coordinator
- `checkpoint.rs` — `ReplicationCheckpoint`
- `fanout.rs` — `FanoutSender` for parallel multi-site dispatch
- `health.rs` — `ReplicationHealthMonitor`, `LinkHealth`, `ClusterHealth`
- `report.rs` — `ConflictReport`, `ReplicationStatusReport`
- `throttle.rs` — `TokenBucket`, `SiteThrottle`, `ThrottleManager`, `ThrottleConfig`
- `pipeline.rs` — `ReplicationPipeline`, `PipelineConfig`, `PipelineStats`
- `batch_auth.rs` — `BatchAuthenticator`, `BatchAuthKey` (HMAC-SHA256 batch auth)
- `failover.rs` — `FailoverManager`, `SiteMode` (active-active failover)
- `auth_ratelimit.rs` — `AuthRateLimiter` (sliding-window auth rate limiting)

Key types used in this task:
```rust
// From conduit.rs
pub struct EntryBatch {
    pub source_site_id: u64,
    pub entries: Vec<JournalEntry>,
    pub batch_seq: u64,
}

// From journal.rs
pub struct JournalEntry {
    pub seq: u64,
    pub shard_id: u32,
    pub site_id: u64,
    // ... more fields
}

// From pipeline.rs
pub struct PipelineStats {
    pub entries_tailed: u64,
    pub entries_compacted_away: u64,
    pub batches_dispatched: u64,
    pub total_entries_sent: u64,
    pub total_bytes_sent: u64,
    pub throttle_stalls: u64,
    pub fanout_failures: u64,
}

// From health.rs
pub enum LinkHealth { Healthy, Degraded { lag_entries: u64, lag_ms: Option<u64> }, Disconnected, Critical { lag_entries: u64 } }
pub struct LinkHealthReport { pub site_id: u64, pub site_name: String, pub health: LinkHealth, ... }
pub enum ClusterHealth { Healthy, Degraded, Critical, NotConfigured }
```

Current `src/lib.rs`:
```rust
#![warn(missing_docs)]
//! ClaudeFS replication subsystem

pub mod auth_ratelimit;
pub mod batch_auth;
pub mod checkpoint;
pub mod conduit;
pub mod engine;
pub mod error;
pub mod failover;
pub mod fanout;
pub mod health;
pub mod journal;
pub mod pipeline;
pub mod report;
pub mod sync;
pub mod throttle;
pub mod topology;
pub mod uidmap;
pub mod wal;
```

Current `Cargo.toml` for claudefs-repl:
```toml
[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
bincode.workspace = true
rand.workspace = true
bytes.workspace = true
```

Workspace has `lz4_flex = { version = "0.11", features = ["frame"] }` and `zstd = "0.13"` available.

## Task: Phase 6 — Three New Modules (Production Readiness)

### Module 1: `src/compression.rs` — Journal Batch Compression

Compresses journal entry batches before WAN transmission to reduce bandwidth.

```rust
use serde::{Deserialize, Serialize};

/// Compression algorithm for journal batch wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgo {
    /// No compression.
    None,
    /// LZ4 frame format (low latency, ~2x ratio).
    Lz4,
    /// Zstd (higher ratio, slightly more CPU — good for WAN).
    Zstd,
}

impl Default for CompressionAlgo {
    fn default() -> Self { Self::Lz4 }
}

impl CompressionAlgo {
    /// Returns true if this algo actually compresses data.
    pub fn is_compressed(&self) -> bool { !matches!(self, Self::None) }
}

/// Compression configuration.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Algorithm to use.
    pub algo: CompressionAlgo,
    /// Zstd compression level (1–22; default 3). Ignored for LZ4/None.
    pub zstd_level: i32,
    /// Minimum uncompressed bytes before attempting compression.
    /// Batches smaller than this are sent uncompressed even if algo != None.
    pub min_compress_bytes: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algo: CompressionAlgo::Lz4,
            zstd_level: 3,
            min_compress_bytes: 256,
        }
    }
}

/// A compressed batch ready for wire transmission.
#[derive(Debug, Clone)]
pub struct CompressedBatch {
    /// Batch sequence number (same as EntryBatch.batch_seq).
    pub batch_seq: u64,
    /// Source site ID.
    pub source_site_id: u64,
    /// Original (uncompressed) byte count.
    pub original_bytes: usize,
    /// Compressed byte count (== original_bytes if algo == None).
    pub compressed_bytes: usize,
    /// Algorithm used.
    pub algo: CompressionAlgo,
    /// Compressed payload (bincode-serialized EntryBatch, then compressed).
    pub data: Vec<u8>,
}

impl CompressedBatch {
    /// Returns the compression ratio (original / compressed). >= 1.0 means compression helped.
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_bytes == 0 { return 1.0; }
        self.original_bytes as f64 / self.compressed_bytes as f64
    }

    /// Returns true if compression reduced the size.
    pub fn is_beneficial(&self) -> bool { self.compressed_bytes < self.original_bytes }
}

/// Compresses and decompresses EntryBatch objects.
pub struct BatchCompressor {
    config: CompressionConfig,
}

impl BatchCompressor {
    /// Create a new compressor with the given config.
    pub fn new(config: CompressionConfig) -> Self

    /// Get the current config.
    pub fn config(&self) -> &CompressionConfig

    /// Compress an EntryBatch into a CompressedBatch.
    /// Uses bincode for serialization, then applies the configured compression.
    /// Falls back to CompressionAlgo::None if the batch is below min_compress_bytes.
    pub fn compress(&self, batch: &crate::conduit::EntryBatch) -> Result<CompressedBatch, crate::error::ReplError>

    /// Decompress a CompressedBatch back into an EntryBatch.
    pub fn decompress(&self, compressed: &CompressedBatch) -> Result<crate::conduit::EntryBatch, crate::error::ReplError>

    /// Compress raw bytes with the configured algorithm.
    /// Returns (compressed_bytes, algo_actually_used).
    pub fn compress_bytes(&self, data: &[u8]) -> Result<(Vec<u8>, CompressionAlgo), crate::error::ReplError>

    /// Decompress raw bytes with the specified algorithm.
    pub fn decompress_bytes(&self, data: &[u8], algo: CompressionAlgo) -> Result<Vec<u8>, crate::error::ReplError>
}
```

Implementation notes for compression.rs:
- Use `lz4_flex::compress_prepend_size` / `lz4_flex::decompress_size_prepended` for LZ4
- Use `zstd::encode_all` / `zstd::decode_all` for Zstd
- Use `bincode::serialize` / `bincode::deserialize` for serializing EntryBatch
- Zstd level clamped to 1..=22
- If batch serialized size < min_compress_bytes, use CompressionAlgo::None regardless of config
- ReplError: add variant `Compression(String)` (thiserror) if it doesn't exist, or use `ReplError::Internal(String)` — check error.rs; use whatever variant makes sense for an internal error
- All public types derive `Debug, Clone`

Include **22 tests** in a `#[cfg(test)]` block at the bottom of compression.rs:
1. `compression_algo_default_is_lz4`
2. `compression_algo_is_compressed_none_false`
3. `compression_algo_is_compressed_lz4_true`
4. `compression_config_default`
5. `compress_decompress_roundtrip_none`
6. `compress_decompress_roundtrip_lz4`
7. `compress_decompress_roundtrip_zstd`
8. `compress_small_batch_uses_none_algo` (< min_compress_bytes → algo = None)
9. `compressed_batch_compression_ratio`
10. `compressed_batch_is_beneficial_when_compressed`
11. `compressed_batch_is_beneficial_false_for_none`
12. `compress_bytes_lz4_roundtrip`
13. `compress_bytes_zstd_roundtrip`
14. `compress_bytes_none_passthrough`
15. `decompress_wrong_algo_returns_error`
16. `compression_config_custom_zstd_level`
17. `compress_large_batch_lz4` (batch with 100 entries)
18. `compress_large_batch_zstd` (batch with 100 entries)
19. `batch_compressor_config_accessor`
20. `compressed_batch_seq_preserved`
21. `compressed_batch_site_id_preserved`
22. `empty_entries_batch_compress_decompress`

Tests must create EntryBatch values. Use:
```rust
use crate::conduit::{ConduitConfig, EntryBatch};
use crate::journal::JournalEntry;
```
Create JournalEntry using whatever fields exist. You MUST look at the types carefully and use correct field names. The JournalEntry type has at minimum: `seq: u64`, `shard_id: u32`, `site_id: u64`. Use `Default` or fill in all required fields.

---

### Module 2: `src/backpressure.rs` — Adaptive Backpressure Control

Controls when to slow down the replication sender based on observed lag and error signals.

```rust
/// Level of backpressure to apply to the replication sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackpressureLevel {
    /// No backpressure; send at full speed.
    None,
    /// Mild slowdown: introduce small delay (e.g., 5ms).
    Mild,
    /// Moderate slowdown: delay 50ms per batch.
    Moderate,
    /// Severe: delay 500ms per batch.
    Severe,
    /// Halt: stop sending until explicitly cleared.
    Halt,
}

impl BackpressureLevel {
    /// Returns the suggested delay in milliseconds for this level.
    pub fn suggested_delay_ms(&self) -> u64 {
        match self {
            Self::None => 0,
            Self::Mild => 5,
            Self::Moderate => 50,
            Self::Severe => 500,
            Self::Halt => u64::MAX,  // caller should check is_halted() not sleep forever
        }
    }

    /// Returns true if sending should be halted entirely.
    pub fn is_halted(&self) -> bool { matches!(self, Self::Halt) }

    /// Returns true if any backpressure is applied.
    pub fn is_active(&self) -> bool { !matches!(self, Self::None) }
}

/// Configuration for backpressure thresholds.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Queue depth (pending entries) that triggers Mild backpressure.
    pub mild_queue_depth: u64,
    /// Queue depth for Moderate.
    pub moderate_queue_depth: u64,
    /// Queue depth for Severe.
    pub severe_queue_depth: u64,
    /// Queue depth for Halt.
    pub halt_queue_depth: u64,
    /// Consecutive error count that triggers Moderate backpressure.
    pub error_count_moderate: u32,
    /// Consecutive error count for Severe backpressure.
    pub error_count_severe: u32,
    /// Consecutive error count for Halt.
    pub error_count_halt: u32,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            mild_queue_depth: 1_000,
            moderate_queue_depth: 10_000,
            severe_queue_depth: 100_000,
            halt_queue_depth: 1_000_000,
            error_count_moderate: 3,
            error_count_severe: 10,
            error_count_halt: 20,
        }
    }
}

/// Tracks backpressure state for one remote site.
pub struct BackpressureController {
    config: BackpressureConfig,
    /// Current queue depth (entries waiting to be sent).
    queue_depth: u64,
    /// Consecutive send errors since last success.
    consecutive_errors: u32,
    /// Whether backpressure is manually overridden (forced halt).
    force_halt: bool,
    /// Last computed level (cached for reporting).
    last_level: BackpressureLevel,
}

impl BackpressureController {
    /// Create a new controller with the given config.
    pub fn new(config: BackpressureConfig) -> Self

    /// Update the observed queue depth.
    pub fn set_queue_depth(&mut self, depth: u64)

    /// Record a successful send (resets consecutive_errors).
    pub fn record_success(&mut self)

    /// Record a send error (increments consecutive_errors).
    pub fn record_error(&mut self)

    /// Force halt regardless of other signals (e.g., admin command).
    pub fn force_halt(&mut self)

    /// Clear the force halt.
    pub fn clear_halt(&mut self)

    /// Compute and return the current backpressure level.
    /// Uses the max of queue-depth-based and error-count-based levels.
    pub fn compute_level(&mut self) -> BackpressureLevel

    /// Get the last computed level without recomputing.
    pub fn current_level(&self) -> BackpressureLevel

    /// Get the suggested delay in milliseconds (delegates to level).
    pub fn suggested_delay_ms(&mut self) -> u64

    /// Returns true if sending is halted.
    pub fn is_halted(&mut self) -> bool

    /// Get current queue depth.
    pub fn queue_depth(&self) -> u64

    /// Get consecutive error count.
    pub fn consecutive_errors(&self) -> u32
}

/// Manages backpressure controllers for multiple remote sites.
pub struct BackpressureManager {
    per_site: std::collections::HashMap<u64, BackpressureController>,
    default_config: BackpressureConfig,
}

impl BackpressureManager {
    pub fn new(default_config: BackpressureConfig) -> Self

    /// Register a site with the default config.
    pub fn register_site(&mut self, site_id: u64)

    /// Register a site with a specific config.
    pub fn register_site_with_config(&mut self, site_id: u64, config: BackpressureConfig)

    /// Get the current level for a site (None if site not registered).
    pub fn level(&mut self, site_id: u64) -> Option<BackpressureLevel>

    /// Record a success for a site.
    pub fn record_success(&mut self, site_id: u64)

    /// Record an error for a site.
    pub fn record_error(&mut self, site_id: u64)

    /// Update queue depth for a site.
    pub fn set_queue_depth(&mut self, site_id: u64, depth: u64)

    /// Force halt for a site.
    pub fn force_halt(&mut self, site_id: u64)

    /// Clear halt for a site.
    pub fn clear_halt(&mut self, site_id: u64)

    /// Get all sites that are currently halted.
    pub fn halted_sites(&mut self) -> Vec<u64>

    /// Remove a site.
    pub fn remove_site(&mut self, site_id: u64)
}
```

Include **20 tests** in `#[cfg(test)]`:
1. `backpressure_level_ordering` (None < Mild < Moderate < Severe < Halt)
2. `suggested_delay_ms_values`
3. `is_halted_only_for_halt`
4. `is_active_for_non_none`
5. `controller_default_level_is_none`
6. `controller_set_queue_depth_mild`
7. `controller_set_queue_depth_moderate`
8. `controller_set_queue_depth_severe`
9. `controller_set_queue_depth_halt`
10. `controller_error_count_moderate`
11. `controller_error_count_severe`
12. `controller_error_count_halt`
13. `controller_record_success_resets_errors`
14. `controller_force_halt`
15. `controller_clear_halt`
16. `controller_queue_and_error_max_level`
17. `manager_register_and_level`
18. `manager_record_success_error`
19. `manager_halted_sites`
20. `manager_remove_site`

---

### Module 3: `src/metrics.rs` — Prometheus-Compatible Replication Metrics

Exposes replication pipeline metrics in Prometheus text exposition format.

```rust
/// A single Prometheus metric (counter or gauge).
#[derive(Debug, Clone)]
pub struct Metric {
    /// Metric name (e.g., "claudefs_repl_entries_sent_total").
    pub name: String,
    /// Help text for the metric.
    pub help: String,
    /// Metric type ("counter" or "gauge").
    pub metric_type: String,
    /// Labels as key=value pairs (e.g., vec![("site_id".to_string(), "42".to_string())]).
    pub labels: Vec<(String, String)>,
    /// Current value.
    pub value: f64,
}

impl Metric {
    pub fn counter(name: &str, help: &str, labels: Vec<(String, String)>, value: f64) -> Self
    pub fn gauge(name: &str, help: &str, labels: Vec<(String, String)>, value: f64) -> Self

    /// Format this metric as Prometheus text exposition format.
    /// Example output:
    /// ```
    /// # HELP claudefs_repl_entries_sent_total Total entries sent
    /// # TYPE claudefs_repl_entries_sent_total counter
    /// claudefs_repl_entries_sent_total{site_id="1"} 12345
    /// ```
    pub fn format(&self) -> String
}

/// Snapshot of all replication metrics for one pipeline instance.
#[derive(Debug, Clone, Default)]
pub struct ReplMetrics {
    /// Site ID this pipeline belongs to.
    pub site_id: u64,
    /// Total entries tailed from local journal.
    pub entries_tailed: u64,
    /// Entries removed by compaction.
    pub entries_compacted_away: u64,
    /// Batches dispatched to fanout.
    pub batches_dispatched: u64,
    /// Total entries successfully sent to remote sites.
    pub entries_sent: u64,
    /// Total bytes sent to remote sites.
    pub bytes_sent: u64,
    /// Number of times throttling blocked a send.
    pub throttle_stalls: u64,
    /// Number of fanout failures.
    pub fanout_failures: u64,
    /// Current replication lag in entries (per site, summed).
    pub lag_entries: u64,
    /// Whether the pipeline is currently running (1.0) or not (0.0).
    pub pipeline_running: f64,
}

impl ReplMetrics {
    /// Update metrics from a PipelineStats snapshot.
    pub fn update_from_stats(&mut self, stats: &crate::pipeline::PipelineStats)

    /// Produce the full list of Prometheus metrics.
    pub fn to_metrics(&self) -> Vec<Metric>

    /// Format all metrics as Prometheus text exposition format.
    /// This is the format Prometheus scrapes via HTTP.
    pub fn format_prometheus(&self) -> String

    /// Returns the compaction rate (entries compacted / entries tailed), or 0.0 if no entries.
    pub fn compaction_rate(&self) -> f64

    /// Returns the fanout failure rate (failures / batches_dispatched), or 0.0 if no batches.
    pub fn fanout_failure_rate(&self) -> f64
}

/// Aggregates metrics across multiple pipeline instances (multi-site).
pub struct MetricsAggregator {
    per_site: std::collections::HashMap<u64, ReplMetrics>,
}

impl MetricsAggregator {
    pub fn new() -> Self

    /// Update or insert metrics for a site.
    pub fn update(&mut self, metrics: ReplMetrics)

    /// Remove a site.
    pub fn remove(&mut self, site_id: u64)

    /// Get metrics for a site.
    pub fn get(&self, site_id: u64) -> Option<&ReplMetrics>

    /// Aggregate all sites into a combined format_prometheus string.
    pub fn format_all(&self) -> String

    /// Total entries sent across all sites.
    pub fn total_entries_sent(&self) -> u64

    /// Total bytes sent across all sites.
    pub fn total_bytes_sent(&self) -> u64

    /// Number of registered sites.
    pub fn site_count(&self) -> usize
}

impl Default for MetricsAggregator { fn default() -> Self { Self::new() } }
```

Include **18 tests** in `#[cfg(test)]`:
1. `metric_counter_type`
2. `metric_gauge_type`
3. `metric_format_no_labels`
4. `metric_format_with_labels`
5. `metric_format_contains_help_and_type`
6. `repl_metrics_default`
7. `repl_metrics_update_from_stats`
8. `repl_metrics_to_metrics_count` (should return several Metric values)
9. `repl_metrics_format_prometheus_nonempty`
10. `repl_metrics_compaction_rate_zero_when_no_entries`
11. `repl_metrics_compaction_rate_nonzero`
12. `repl_metrics_fanout_failure_rate_zero`
13. `repl_metrics_fanout_failure_rate_nonzero`
14. `aggregator_new_empty`
15. `aggregator_update_and_get`
16. `aggregator_remove`
17. `aggregator_format_all_nonempty`
18. `aggregator_totals`

---

### Updated `Cargo.toml` for claudefs-repl

Add lz4_flex and zstd to the [dependencies] section:

```toml
[package]
name = "claudefs-repl"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "ClaudeFS subsystem: Cross-site journal replication, cloud conduit (gRPC/mTLS)"

[[bin]]
name = "cfs-repl"
path = "src/main.rs"

[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
bincode.workspace = true
rand.workspace = true
bytes.workspace = true
lz4_flex.workspace = true
zstd.workspace = true

[lib]
name = "claudefs_repl"
path = "src/lib.rs"
```

### Updated `src/lib.rs`

```rust
#![warn(missing_docs)]

//! ClaudeFS replication subsystem: Cross-site journal replication, cloud conduit (gRPC/mTLS)

pub mod auth_ratelimit;
pub mod backpressure;
pub mod batch_auth;
pub mod checkpoint;
pub mod compression;
pub mod conduit;
pub mod engine;
pub mod error;
pub mod failover;
pub mod fanout;
pub mod health;
pub mod journal;
pub mod metrics;
pub mod pipeline;
pub mod report;
pub mod sync;
pub mod throttle;
pub mod topology;
pub mod uidmap;
pub mod wal;
```

## Implementation Requirements

1. **ALL four files must be output**: `compression.rs`, `backpressure.rs`, `metrics.rs`, updated `lib.rs`, updated `Cargo.toml`.

2. **No new crates beyond lz4_flex and zstd** — both are already in workspace.

3. **Use these imports in compression.rs**:
   ```rust
   use crate::conduit::EntryBatch;
   use crate::error::ReplError;
   use serde::{Deserialize, Serialize};
   ```
   And for the implementation: `lz4_flex`, `zstd`

4. **ReplError handling**: Look at the existing error.rs pattern. If `ReplError::Internal` or similar already exists, use it. Add a `Compression` variant if needed:
   ```rust
   #[error("compression error: {0}")]
   Compression(String),
   ```
   But DO NOT modify error.rs — instead, define an internal helper or use `map_err(|e| ReplError::Internal(e.to_string()))` if `Internal` exists.
   Actually, add a `Compression` variant to error.rs is needed. Let me clarify: you MUST output an updated `error.rs` too if you need a new error variant.

5. **Backpressure level ordering**: Derive `PartialOrd, Ord` requires variants in ascending severity order.

6. **Metrics format**: The Prometheus text format for a metric with no labels:
   ```
   # HELP claudefs_repl_entries_sent_total Total entries sent
   # TYPE claudefs_repl_entries_sent_total counter
   claudefs_repl_entries_sent_total 12345
   ```
   With labels:
   ```
   claudefs_repl_entries_sent_total{site_id="1"} 12345
   ```
   Use integer formatting for whole numbers (no `.0`), float for fractional.

7. **All tests pass**: `cargo test -p claudefs-repl`

8. **Zero clippy warnings**: `cargo clippy -p claudefs-repl -- -D warnings`

9. All public config/stats types derive `Debug, Clone` (and `Default` where applicable).

## Output Format

Output each file in full:

```rust
// File: crates/claudefs-repl/src/compression.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/backpressure.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/metrics.rs
<complete file>
```

```toml
// File: crates/claudefs-repl/Cargo.toml
<complete file>
```

```rust
// File: crates/claudefs-repl/src/lib.rs
<complete updated lib.rs>
```

If you need to update error.rs to add a Compression variant, also output:
```rust
// File: crates/claudefs-repl/src/error.rs
<complete updated error.rs>
```

Output ALL files in full. Do NOT omit any file or truncate code.
