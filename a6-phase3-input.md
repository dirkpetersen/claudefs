# OpenCode Task: claudefs-repl Phase 3 — gRPC codegen + new modules

## Context

You are implementing Phase 3 production-readiness additions to `claudefs-repl`,
the cross-site journal replication crate for ClaudeFS (a distributed POSIX filesystem).

The crate currently has 817 passing tests, clean clippy, and these existing modules:
`auth_ratelimit`, `backpressure`, `batch_auth`, `catchup`, `checkpoint`,
`compression`, `conduit`, `conflict_resolver`, `engine`, `error`, `failover`,
`fanout`, `health`, `journal`, `journal_gc`, `journal_source`, `lag_monitor`,
`metrics`, `otel_repl`, `pipeline`, `recv_ratelimit`, `repl_audit`,
`repl_bootstrap`, `repl_maintenance`, `repl_qos`, `report`, `site_failover`,
`site_registry`, `sliding_window`, `split_brain`, `sync`, `throttle`,
`tls_policy`, `topology`, `uidmap`, `wal`, `active_active`

There is also a `proto/replication.proto` file but NO `build.rs` and NO
`tonic-build` dependency yet.

## Task Overview

Add the following to the crate (3 tasks):

### Task 1: `build.rs` for protobuf codegen

Create `crates/claudefs-repl/build.rs` that compiles `proto/replication.proto`
using `tonic-build`. This enables the actual gRPC service in `conduit.rs`.

Also update `crates/claudefs-repl/Cargo.toml` to add `tonic-build` as a build
dependency (it's already available in the cargo registry):

```toml
[build-dependencies]
tonic-build = "0.12"
```

The `build.rs` should:
- Use `tonic_build::configure()` to compile the proto
- Set `out_dir` so generated code lands in `OUT_DIR`
- Be simple and idempotent (no custom options needed for v1)
- Compile `proto/replication.proto`

### Task 2: Three new Phase 3 modules

Add these three new source files to `crates/claudefs-repl/src/`:

#### 2a. `repl_snapshot.rs` — Initial snapshot transfer for new replicas

A new replica joining the cluster needs to receive a full state snapshot before
it can start streaming journal entries. This module implements:

```rust
/// Configuration for snapshot transfers.
pub struct SnapshotConfig {
    pub chunk_size_bytes: usize,  // default: 64KB
    pub compression: CompressionAlgo,  // default: Lz4
    pub max_concurrent_chunks: usize,  // default: 4
    pub transfer_timeout_ms: u64,  // default: 300_000 (5 min)
}

/// A snapshot chunk (one piece of the full state transfer).
pub struct SnapshotChunk {
    pub snapshot_id: u64,       // unique ID for this snapshot session
    pub chunk_index: u32,       // 0-based chunk sequence
    pub total_chunks: u32,      // total chunks in snapshot
    pub data: Vec<u8>,          // raw chunk data (compressed if config says so)
    pub algo: CompressionAlgo,  // compression used
    pub crc32: u32,             // checksum for this chunk
    pub is_final: bool,         // true on last chunk
}

/// Snapshot metadata describing what was snapshotted.
pub struct SnapshotMeta {
    pub snapshot_id: u64,
    pub source_site_id: u64,
    pub taken_at_ms: u64,           // Unix ms when snapshot started
    pub shard_cursors: HashMap<u32, u64>,  // per-shard last-included seq
    pub total_bytes_uncompressed: u64,
    pub chunk_count: u32,
}

/// Phase of a snapshot transfer.
pub enum SnapshotPhase {
    Idle,
    Preparing { snapshot_id: u64 },
    Sending { snapshot_id: u64, chunks_sent: u32, total_chunks: u32 },
    Receiving { snapshot_id: u64, chunks_received: u32, total_chunks: u32 },
    Verifying { snapshot_id: u64 },
    Complete { snapshot_id: u64, duration_ms: u64 },
    Failed { snapshot_id: u64, reason: String },
}

/// Statistics for snapshot operations.
pub struct SnapshotStats {
    pub snapshots_sent: u64,
    pub snapshots_received: u64,
    pub snapshots_failed: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub last_snapshot_duration_ms: u64,
}

/// Manages snapshot transfer between sites.
pub struct SnapshotManager {
    config: SnapshotConfig,
    local_site_id: u64,
    phase: SnapshotPhase,
    stats: SnapshotStats,
}
```

Key `SnapshotManager` methods:
- `new(local_site_id: u64, config: SnapshotConfig) -> Self`
- `initiate_send(dest_site_id: u64) -> Result<SnapshotMeta, SnapshotError>`
- `initiate_receive(meta: SnapshotMeta) -> Result<(), SnapshotError>`
- `process_chunk(chunk: SnapshotChunk) -> Result<Option<SnapshotChunk>, SnapshotError>` — state machine step
- `complete_send() -> Result<(), SnapshotError>`
- `is_idle(&self) -> bool`
- `phase(&self) -> &SnapshotPhase`
- `stats(&self) -> &SnapshotStats`

Error enum `SnapshotError` with variants:
- `AlreadyInProgress`
- `InvalidChunk { expected: u32, got: u32 }`
- `ChecksumMismatch { chunk_index: u32 }`
- `Timeout { snapshot_id: u64 }`
- `CompressionError(String)`

CRC32 checksum: use simple additive byte sum for CRC32 approximation (sum all
bytes in `data` mod 2^32, no external dep). Use `u32` arithmetic.

Write proptest tests testing:
- Chunk round-trip (decompose and verify)
- Phase transitions (idle → preparing → sending → complete)
- Stats accumulation
- Error cases (invalid chunk index, wrong snapshot_id)

#### 2b. `repl_coordinator.rs` — Multi-site coordination state machine

Coordinates the overall replication state across all connected sites. Maintains
a high-level view of which sites are in sync, which are lagging, and which need
catch-up or snapshot.

```rust
/// Decision made by the coordinator for a given site.
pub enum CoordinatorDecision {
    /// Site is in sync; continue streaming.
    ContinueStreaming,
    /// Site is lagging; request expedited catch-up.
    TriggerCatchup { site_id: u64, from_seq: u64 },
    /// Site is too far behind; needs full snapshot.
    TriggerSnapshot { site_id: u64 },
    /// Site is unreachable; enter failover mode.
    EnterFailover { site_id: u64 },
    /// Nothing to do right now.
    Idle,
}

/// Configuration for the coordinator.
pub struct CoordinatorConfig {
    pub lag_threshold_catchup: u64,  // entries behind before triggering catchup (default: 10_000)
    pub lag_threshold_snapshot: u64, // entries behind before requiring snapshot (default: 1_000_000)
    pub unreachable_threshold_ms: u64, // ms with no heartbeat before "unreachable" (default: 30_000)
    pub check_interval_ms: u64,      // how often to run coordinator logic (default: 5_000)
}

/// Per-site view maintained by the coordinator.
pub struct SiteCoordinatorView {
    pub site_id: u64,
    pub last_heartbeat_ms: u64,  // Unix ms of last heartbeat
    pub local_seq: u64,          // our latest seq
    pub remote_seq: u64,         // remote's latest seq (from heartbeat)
    pub lag_entries: u64,        // computed lag
    pub state: SiteCoordState,
}

pub enum SiteCoordState {
    Synced,
    LagWarning,
    Catchup,
    SnapshotNeeded,
    Unreachable,
    Recovering,
}

/// Statistics for coordinator decisions.
pub struct CoordinatorStats {
    pub catchup_triggers: u64,
    pub snapshot_triggers: u64,
    pub failover_triggers: u64,
    pub check_cycles: u64,
}

/// The multi-site coordinator.
pub struct ReplicationCoordinator {
    config: CoordinatorConfig,
    local_site_id: u64,
    sites: HashMap<u64, SiteCoordinatorView>,
    stats: CoordinatorStats,
}
```

Key methods:
- `new(local_site_id: u64, config: CoordinatorConfig) -> Self`
- `add_site(site_id: u64) -> ()` — register a site
- `remove_site(site_id: u64) -> ()` — unregister a site
- `update_heartbeat(site_id: u64, remote_seq: u64, now_ms: u64) -> ()` — update site view
- `update_local_seq(seq: u64) -> ()` — update our latest seq
- `check_all(now_ms: u64) -> Vec<CoordinatorDecision>` — run coordinator logic, return decisions
- `site_view(site_id: u64) -> Option<&SiteCoordinatorView>`
- `stats() -> &CoordinatorStats`

Write tests for:
- Decision to trigger catchup when lag > threshold
- Decision to trigger snapshot when lag >> threshold
- Decision to enter failover when heartbeat is stale
- Adding/removing sites
- Stats increment correctly
- All SiteCoordState transitions

#### 2c. `repl_config.rs` — Runtime configuration with hot-reload support

Manages the replication configuration at runtime, supporting hot-reload without
restart. Config can be updated via admin API (A8) or config file.

```rust
/// Complete replication runtime configuration.
pub struct ReplConfig {
    pub local_site_id: u64,
    pub max_batch_size: usize,        // default: 1000
    pub batch_timeout_ms: u64,        // default: 100
    pub compress_algo: String,        // "none"|"lz4"|"zstd", default: "lz4"
    pub max_lag_before_catchup: u64,  // default: 10_000
    pub max_lag_before_snapshot: u64, // default: 1_000_000
    pub heartbeat_interval_ms: u64,   // default: 5_000
    pub ack_timeout_ms: u64,          // default: 10_000
    pub window_size: usize,           // default: 32
    pub enable_tls: bool,             // default: true
    pub repl_threads: usize,          // default: 2
    pub metrics_prefix: String,       // default: "claudefs_repl"
}

impl Default for ReplConfig { ... }

/// A configuration update (partial update — only changed fields).
pub struct ConfigDiff {
    pub max_batch_size: Option<usize>,
    pub batch_timeout_ms: Option<u64>,
    pub compress_algo: Option<String>,
    pub max_lag_before_catchup: Option<u64>,
    pub max_lag_before_snapshot: Option<u64>,
    pub heartbeat_interval_ms: Option<u64>,
    pub ack_timeout_ms: Option<u64>,
    pub window_size: Option<usize>,
    pub enable_tls: Option<bool>,
    pub repl_threads: Option<usize>,
    pub metrics_prefix: Option<String>,
}

/// Error for config operations.
pub enum ConfigError {
    InvalidValue { field: String, reason: String },
    ValidationFailed(String),
}

/// Versioned config snapshot.
pub struct ConfigVersion {
    pub version: u64,
    pub config: ReplConfig,
    pub updated_at_ms: u64,
}

/// Runtime config manager supporting hot-reload.
pub struct ReplConfigManager {
    current: RwLock<ConfigVersion>,
    // version counter
}
```

Key methods:
- `new(initial: ReplConfig) -> Self`
- `current(&self) -> ReplConfig` — get current config snapshot
- `version(&self) -> u64` — get current version number
- `apply_diff(&self, diff: ConfigDiff, now_ms: u64) -> Result<u64, ConfigError>` — apply partial update, return new version
- `validate(config: &ReplConfig) -> Result<(), ConfigError>` — validate a full config
- `history_len(&self) -> usize` — for test/debug

Validation rules:
- `max_batch_size` in 1..=100_000
- `batch_timeout_ms` in 1..=60_000
- `compress_algo` must be one of "none", "lz4", "zstd"
- `window_size` in 1..=1024
- `repl_threads` in 1..=32
- `heartbeat_interval_ms` in 100..=300_000

Write proptest tests testing:
- Default config passes validation
- Invalid values are rejected with meaningful errors
- apply_diff increments version correctly
- Concurrent reads while applying diff (use std::sync::RwLock)
- Multiple sequential diffs stack correctly

### Task 3: Update `lib.rs` to export the 3 new modules

Add to `crates/claudefs-repl/src/lib.rs`:
```rust
/// Snapshot transfer for new replica bootstrap.
pub mod repl_snapshot;
/// Multi-site coordination state machine.
pub mod repl_coordinator;
/// Runtime configuration management with hot-reload.
pub mod repl_config;
```

## Constraints

- NO external dependencies beyond what's already in Cargo.toml + tonic-build
- Available Cargo.toml dependencies (already in workspace):
  - tokio, thiserror, anyhow, serde, serde_json, prost, tonic, tracing,
    tracing-subscriber, bincode, rand, sha2, bytes, lz4_flex, zstd, hmac,
    zeroize
- Only add `tonic-build` as a build-dependency
- ALL code must compile with `cargo build -p claudefs-repl`
- ALL tests must pass with `cargo test -p claudefs-repl`
- ZERO clippy warnings (the crate has `#![warn(missing_docs)]` so all public
  items need doc comments)
- No `unsafe` code (A6 does not use unsafe)
- Use `std::sync::RwLock` in `repl_config.rs` (no tokio needed there)
- Use `HashMap` from `std::collections`

## Files to create/modify

1. CREATE `crates/claudefs-repl/build.rs`
2. MODIFY `crates/claudefs-repl/Cargo.toml` — add `[build-dependencies]`
3. CREATE `crates/claudefs-repl/src/repl_snapshot.rs`
4. CREATE `crates/claudefs-repl/src/repl_coordinator.rs`
5. CREATE `crates/claudefs-repl/src/repl_config.rs`
6. MODIFY `crates/claudefs-repl/src/lib.rs` — add the 3 new `pub mod` lines

## Existing module references (imports you may need)

From `crate::compression`:
```rust
pub enum CompressionAlgo { None, Lz4, Zstd }
```

From `crate::error`:
```rust
pub enum ReplError { ... }
```

## Quality bar

- Each new module should have 20-30 tests (unit + proptest)
- Tests must exercise all public methods and all error cases
- All public structs/enums/fns need `///` doc comments
- Use `thiserror::Error` for error types

## Output format

For each file, output the COMPLETE file content with a header comment like:
```
=== FILE: crates/claudefs-repl/build.rs ===
<full content>
=== END FILE ===
```

Then a cargo build command and test output.
