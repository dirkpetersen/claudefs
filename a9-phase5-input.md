# A9: Test & Validation — Phase 5: End-to-End Write Path Tests

You are extending the `claudefs-tests` crate for the ClaudeFS distributed filesystem project. This is Agent A9 (Test & Validation), Phase 5.

## Working directory: /home/cfs/claudefs

## Context

Current state: 589 tests in claudefs-tests, 21 modules.

## Task: Phase 5 — End-to-End Write Path + Advanced Integration (3 NEW modules)

Create 3 new modules. Target: **~90 new tests** (total ~679).

### Module 1: `src/write_path_e2e.rs` — End-to-End Write Path Tests (~35 tests)

Tests that exercise the write path across storage+reduce crates together.
This is the most valuable A9 test: ensuring the full data pipeline works.

First READ these files:
- `crates/claudefs-reduce/src/lib.rs` — public API
- `crates/claudefs-reduce/src/write_path.rs` — WritePathResult
- `crates/claudefs-storage/src/lib.rs` — public API
- `crates/claudefs-storage/src/engine.rs` — StorageEngine (if public)

```rust
//! End-to-end write path tests: data flows through reduce pipeline then to storage

use claudefs_reduce::{
    PipelineConfig, ReductionPipeline,
    Chunker, ChunkerConfig,
    CompressionAlgorithm,
    EncryptionAlgorithm, EncryptionKey,
};
use claudefs_storage::{
    Checksum, ChecksumAlgorithm,
    AllocatorConfig, BuddyAllocator,
    MockIoEngine, IoEngine,
};

/// Tests the full write pipeline: chunk → compress → encrypt → checksum → store
pub fn test_small_write_full_pipeline() { ... }
/// Tests that compressed+encrypted data can be decrypted+decompressed back to original
pub fn test_write_read_roundtrip() { ... }
/// Tests writing 1MB of data through the full pipeline
pub fn test_large_write_pipeline() { ... }
/// Tests pipeline with different compression algorithms
pub fn test_lz4_compression_pipeline() { ... }
pub fn test_zstd_compression_pipeline() { ... }
/// Tests pipeline with encryption disabled
pub fn test_no_encryption_pipeline() { ... }
/// Tests pipeline with no compression (NoCompression)
pub fn test_no_compression_pipeline() { ... }
/// Tests checksum verification after write
pub fn test_checksum_after_write() { ... }
/// Tests that pipeline stats track correctly
pub fn test_pipeline_stats_tracking() { ... }
/// Tests write of compressible data (all zeros) shows good compression ratio
pub fn test_compressible_data_ratio() { ... }
/// Tests write of incompressible data (random) shows ratio ~1.0
pub fn test_incompressible_data() { ... }
```

Write ~35 unit tests that combine APIs from claudefs_reduce and claudefs_storage.

Important: Use only PUBLIC API. Do not create private types. Create actual test data (random or deterministic bytes) and run them through the pipeline.

### Module 2: `src/concurrency_tests.rs` — Concurrency and Thread-Safety Tests (~25 tests)

Tests for concurrent access patterns:

```rust
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use tokio::sync::Semaphore;

/// Tests that BuddyAllocator is thread-safe under concurrent alloc/free
pub struct ConcurrentAllocatorTest {
    pub num_threads: u32,
    pub ops_per_thread: u32,
}

impl ConcurrentAllocatorTest {
    pub fn new(threads: u32, ops: u32) -> Self
    pub fn run(&self) -> ConcurrentTestResult
}

/// Tests concurrent reads of the same data produce consistent results
pub struct ConcurrentReadTest {
    pub data: Vec<u8>,
    pub num_readers: u32,
}

impl ConcurrentReadTest {
    pub fn new(data: Vec<u8>, readers: u32) -> Self
    pub fn run(&self) -> ConcurrentTestResult
}

/// Tests concurrent compression of different data chunks
pub struct ConcurrentCompressTest {
    pub chunks: Vec<Vec<u8>>,
}

impl ConcurrentCompressTest {
    pub fn new(chunks: Vec<Vec<u8>>) -> Self
    pub fn run(&self) -> ConcurrentTestResult
}

#[derive(Debug, Clone)]
pub struct ConcurrentTestResult {
    pub threads_completed: u32,
    pub ops_succeeded: u64,
    pub ops_failed: u64,
    pub data_races_detected: u32,  // always 0 if Rust is doing its job
    pub duration_ms: u64,
}

impl ConcurrentTestResult {
    pub fn is_success(&self) -> bool  // no failures, no data races
    pub fn throughput_ops_per_sec(&self) -> f64
}

/// Stress test: many threads writing to different keys in a HashMap under Mutex
pub fn stress_test_mutex_map(threads: u32, ops_per_thread: u32) -> ConcurrentTestResult

/// Tests Arc<Mutex<T>> pattern with many concurrent accesses
pub fn test_arc_mutex_under_load(threads: u32) -> ConcurrentTestResult

/// Tests RwLock concurrent read performance
pub fn test_rwlock_read_concurrency(readers: u32) -> ConcurrentTestResult
```

Unit tests (~25):
- test ConcurrentAllocatorTest creates successfully
- test ConcurrentReadTest with 2 readers
- test ConcurrentCompressTest with 4 chunks
- test ConcurrentTestResult is_success
- test ConcurrentTestResult throughput calculation
- test stress_test_mutex_map with 4 threads, 100 ops each
- test test_arc_mutex_under_load with 8 threads
- test test_rwlock_read_concurrency with 4 readers
- test ops_failed=0 when no errors
- Various concurrency scenario constructions

### Module 3: `src/snapshot_tests.rs` — Snapshot and Recovery Tests (~30 tests)

Tests for the snapshot and recovery systems:

```rust
use claudefs_reduce::snapshot::{SnapshotManager, SnapshotConfig, Snapshot, SnapshotInfo};
use claudefs_storage::recovery::{RecoveryManager, RecoveryConfig, RecoveryPhase};
use tempfile::TempDir;

/// Tests snapshot creation and listing
pub fn test_snapshot_create() { ... }
pub fn test_snapshot_list() { ... }
pub fn test_snapshot_by_name() { ... }

/// Tests snapshot retention policy
pub fn test_snapshot_retention_expiry() { ... }
pub fn test_snapshot_retention_count() { ... }

/// Tests snapshot info fields
pub fn test_snapshot_info_dedup_ratio() { ... }
pub fn test_snapshot_info_size() { ... }

/// Tests recovery manager initialization
pub fn test_recovery_config_default() { ... }
pub fn test_recovery_phase_sequence() { ... }
pub fn test_recovery_report_creation() { ... }
```

Import from `claudefs_reduce::snapshot` and `claudefs_storage::recovery`. Read the public APIs first.

Unit tests (~30):
- test SnapshotConfig default values
- test SnapshotManager creation
- test Snapshot creation with name
- test SnapshotInfo field access
- test snapshot list ordering
- test snapshot retention (expiry by time)
- test RecoveryConfig default
- test RecoveryManager creation
- test RecoveryPhase variants
- test RecoveryReport fields

## Requirements

1. All 3 new modules must compile with zero errors
2. All ~90 new tests must pass
3. Update `src/lib.rs` to add `pub mod` for all 3 new modules
4. READ the relevant lib.rs files before writing imports
5. Focus on combining APIs from multiple crates in write_path_e2e.rs

## Files to create/modify

1. `crates/claudefs-tests/src/write_path_e2e.rs` — NEW
2. `crates/claudefs-tests/src/concurrency_tests.rs` — NEW
3. `crates/claudefs-tests/src/snapshot_tests.rs` — NEW
4. `crates/claudefs-tests/src/lib.rs` — MODIFY

CRITICAL: Before writing snapshot_tests.rs, read crates/claudefs-reduce/src/snapshot.rs and crates/claudefs-storage/src/recovery.rs to understand the actual public API.

Output each file with clear delimiters:
```
=== FILE: path/to/file ===
<content>
=== END FILE ===
```
