# A6: ClaudeFS Replication — Phase 5: Pipeline, Throttle, and Integration

You are implementing Phase 5 of the `claudefs-repl` crate for ClaudeFS.

## Already Implemented (Phase 1-4, 257 tests, 12 modules)

- `error.rs`, `journal.rs`, `wal.rs`, `topology.rs` — foundation types
- `conduit.rs` — in-process mock of gRPC channel
- `sync.rs` — ConflictDetector (LWW), BatchCompactor, ReplicationSync
- `uidmap.rs` — UID/GID translation
- `engine.rs` — ReplicationEngine coordinator
- `checkpoint.rs` — Point-in-time snapshots
- `fanout.rs` — FanoutSender for parallel multi-site dispatch
- `health.rs` — ReplicationHealthMonitor
- `report.rs` — ConflictReport, ReplicationStatusReport

lib.rs currently exports all 12 modules.

## Task: Phase 5 — Three New Modules + Integration

### Module 1: `src/throttle.rs` — Cross-Site Bandwidth Throttling

The throttle module controls the rate at which journal entries are sent to each remote site, preventing replication from consuming all available WAN bandwidth.

```rust
/// Bandwidth limit configuration.
pub struct ThrottleConfig {
    /// Maximum bytes per second to send to this site (0 = unlimited).
    pub max_bytes_per_sec: u64,
    /// Maximum entries per second (0 = unlimited).
    pub max_entries_per_sec: u64,
    /// Burst allowance: multiplier on max rate for short bursts (e.g., 2.0 = 2x burst).
    pub burst_factor: f64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_bytes_per_sec: 100 * 1024 * 1024,  // 100 MB/s default
            max_entries_per_sec: 10_000,
            burst_factor: 1.5,
        }
    }
}

/// Tracks token bucket state for one throttle dimension.
pub struct TokenBucket {
    capacity: u64,      // max tokens (burst capacity)
    tokens: f64,        // current tokens (fractional OK)
    refill_rate: f64,   // tokens per microsecond
    last_refill_us: u64, // when tokens were last topped up
}

impl TokenBucket {
    /// Create a new token bucket with the given capacity and refill rate (tokens/sec).
    pub fn new(capacity: u64, rate_per_sec: f64) -> Self

    /// Try to consume `amount` tokens. Returns true if successful, false if insufficient.
    /// Updates internal state (refills first based on elapsed time).
    pub fn try_consume(&mut self, amount: u64, now_us: u64) -> bool

    /// Returns current token count (floored to u64).
    pub fn available(&self, now_us: u64) -> u64

    /// Refill tokens based on elapsed time since last refill.
    pub fn refill(&mut self, now_us: u64)
}

/// Per-site throttle: combines byte-rate and entry-rate token buckets.
pub struct SiteThrottle {
    config: ThrottleConfig,
    byte_bucket: TokenBucket,
    entry_bucket: TokenBucket,
}

impl SiteThrottle {
    /// Create a new site throttle with the given config.
    pub fn new(config: ThrottleConfig) -> Self

    /// Check if we can send `byte_count` bytes and `entry_count` entries.
    /// Returns true if allowed and consumes tokens. Returns false if throttled.
    pub fn try_send(&mut self, byte_count: u64, entry_count: u64, now_us: u64) -> bool

    /// Get the current byte rate limit.
    pub fn max_bytes_per_sec(&self) -> u64

    /// Get the current entry rate limit.
    pub fn max_entries_per_sec(&self) -> u64

    /// Update the throttle config (e.g., admin changed bandwidth limit).
    pub fn update_config(&mut self, config: ThrottleConfig)

    /// Returns how many bytes are available (capped at max_bytes_per_sec).
    pub fn available_bytes(&self, now_us: u64) -> u64
}

/// Manages throttles for multiple remote sites.
pub struct ThrottleManager {
    per_site: HashMap<u64, SiteThrottle>,
    default_config: ThrottleConfig,
}

impl ThrottleManager {
    pub fn new(default_config: ThrottleConfig) -> Self

    /// Register a site with a specific throttle config.
    pub fn register_site(&mut self, site_id: u64, config: ThrottleConfig)

    /// Register a site with the default throttle config.
    pub fn register_site_default(&mut self, site_id: u64)

    /// Try to send for a specific site.
    pub fn try_send(&mut self, site_id: u64, byte_count: u64, entry_count: u64, now_us: u64) -> bool

    /// Remove a site's throttle.
    pub fn remove_site(&mut self, site_id: u64)

    /// Update throttle config for a site.
    pub fn update_site_config(&mut self, site_id: u64, config: ThrottleConfig)

    /// Get available bytes for a site.
    pub fn available_bytes(&self, site_id: u64, now_us: u64) -> u64
}
```

Include at least **22 tests** for:
- TokenBucket: new, try_consume succeeds, try_consume fails (not enough), refill over time, available()
- SiteThrottle: new, try_send success, try_send fails on bytes, try_send fails on entries, update_config
- ThrottleManager: register, try_send, remove_site, update_site_config, available_bytes
- Unlimited throttle (0 rate = unlimited)
- Burst capacity
- Zero byte/entry request always succeeds
- available_bytes after consumption

### Module 2: `src/pipeline.rs` — Replication Pipeline

Combines journal tailing + compaction + throttling + fanout into a single pipeline abstraction.

```rust
/// Configuration for the full replication pipeline.
pub struct PipelineConfig {
    pub local_site_id: u64,
    /// Batch up to this many entries before dispatching.
    pub max_batch_size: usize,
    /// Wait up to this long (ms) to fill a batch before sending anyway.
    pub batch_timeout_ms: u64,
    /// Whether to compact entries before sending.
    pub compact_before_send: bool,
    /// Whether to apply UID mapping before sending.
    pub apply_uid_mapping: bool,
}

impl Default for PipelineConfig { ... }

/// Current pipeline statistics.
pub struct PipelineStats {
    pub entries_tailed: u64,
    pub entries_compacted_away: u64,
    pub batches_dispatched: u64,
    pub total_entries_sent: u64,
    pub total_bytes_sent: u64,
    pub throttle_stalls: u64,
    pub fanout_failures: u64,
}

/// The replication pipeline state.
pub enum PipelineState {
    Idle,
    Running,
    Draining,
    Stopped,
}

/// The replication pipeline: tails journal → compacts → throttles → fanout.
pub struct ReplicationPipeline {
    config: PipelineConfig,
    state: Arc<tokio::sync::Mutex<PipelineState>>,
    stats: Arc<tokio::sync::Mutex<PipelineStats>>,
    throttle: Arc<tokio::sync::Mutex<ThrottleManager>>,
    fanout: Arc<FanoutSender>,
    uid_mapper: Arc<UidMapper>,
}

impl ReplicationPipeline {
    /// Create a new pipeline.
    pub fn new(
        config: PipelineConfig,
        throttle: ThrottleManager,
        fanout: FanoutSender,
        uid_mapper: UidMapper,
    ) -> Self

    /// Start the pipeline.
    pub async fn start(&self)

    /// Stop the pipeline.
    pub async fn stop(&self)

    /// Get current state.
    pub async fn state(&self) -> PipelineState

    /// Get current statistics snapshot.
    pub async fn stats(&self) -> PipelineStats

    /// Process a batch manually (for testing; in production, driven by journal tailer).
    /// Applies compaction, UID mapping, throttle check, and fanout.
    pub async fn process_batch(&self, entries: Vec<JournalEntry>, now_us: u64) -> Result<usize, ReplError>

    /// Update throttle config for a site.
    pub async fn update_throttle(&self, site_id: u64, config: ThrottleConfig)
}
```

Include at least **20 tests** for:
- Create pipeline with default config
- Start/stop state transitions
- process_batch sends to fanout
- Stats updated on process_batch
- Empty batch (noop)
- Compaction reduces entries
- stop() transitions to Stopped
- Multiple process_batch calls accumulate stats
- Pipeline state after start/stop
- update_throttle doesn't panic

### Updated `src/lib.rs`

```rust
pub mod checkpoint;
pub mod conduit;
pub mod engine;
pub mod error;
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

### Implementation Requirements

1. **No new crate additions** — only existing deps.

2. **Imports in throttle.rs**:
   ```rust
   use std::collections::HashMap;
   ```
   No crate imports needed.

3. **Imports in pipeline.rs**:
   ```rust
   use crate::conduit::EntryBatch;
   use crate::error::ReplError;
   use crate::fanout::FanoutSender;
   use crate::journal::JournalEntry;
   use crate::sync::BatchCompactor;
   use crate::throttle::{ThrottleConfig, ThrottleManager};
   use crate::uidmap::UidMapper;
   use std::sync::Arc;
   ```

4. **TokenBucket.refill** is called by try_consume to top up before consuming.

5. **Unlimited throttle**: if `max_bytes_per_sec == 0`, the byte bucket always returns true for try_consume.
   If `max_entries_per_sec == 0`, the entry bucket always returns true.

6. **PipelineStats fields** are plain u64 (not atomic). Stats are updated inside Mutex.

7. **`#[derive(Debug, Clone)]`** on all public config/stats types.

8. **All tests pass** `cargo test -p claudefs-repl`.

9. **Zero clippy warnings** `cargo clippy -p claudefs-repl -- -D warnings`.

### Output Format

```rust
// File: crates/claudefs-repl/src/throttle.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/pipeline.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/lib.rs
<complete updated lib.rs with all 14 modules>
```

Output ALL three files. Do NOT modify other files.
