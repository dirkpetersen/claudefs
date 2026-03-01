# A9: Test & Validation — Phase 3: CI Integration + Advanced Tests

You are extending the `claudefs-tests` crate for the ClaudeFS distributed filesystem project. This is Agent A9 (Test & Validation), Phase 3.

## Working directory: /home/cfs/claudefs

## Context

Phases 1 and 2 created these modules in `crates/claudefs-tests/src/`:
Phase 1: harness, posix, proptest_storage, proptest_reduce, proptest_transport, integration, linearizability, crash, chaos, bench, connectathon (238 tests)
Phase 2: posix_compliance, jepsen, soak, regression, report (106 tests)
**Total so far: 344 tests**

## Task: Phase 3 — CI Integration + Advanced Tests (4 NEW modules)

Create 4 new modules and update lib.rs. Target: **~110 new tests** (total ~454).

### Module 1: `src/ci_matrix.rs` — CI Test Matrix Framework (~30 tests)

A framework for defining and executing CI test matrices:

```rust
use std::collections::HashMap;

/// A dimension of the test matrix (e.g., "os", "compression", "erasure")
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixDimension {
    pub name: String,
    pub values: Vec<String>,
}

impl MatrixDimension {
    pub fn new(name: &str, values: Vec<&str>) -> Self
    pub fn count(&self) -> usize
}

/// A single point in the test matrix
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixPoint {
    pub dimensions: HashMap<String, String>,
}

impl MatrixPoint {
    pub fn new(dimensions: HashMap<String, String>) -> Self
    pub fn get(&self, key: &str) -> Option<&str>
    pub fn label(&self) -> String  // "os=ubuntu,compression=lz4"
}

/// A test matrix (cartesian product of dimensions)
pub struct TestMatrix {
    pub dimensions: Vec<MatrixDimension>,
    pub excludes: Vec<HashMap<String, String>>,  // excluded combinations
}

impl TestMatrix {
    pub fn new() -> Self
    pub fn add_dimension(&mut self, dim: MatrixDimension) -> &mut Self
    pub fn exclude(&mut self, combo: HashMap<String, String>) -> &mut Self
    pub fn expand(&self) -> Vec<MatrixPoint>  // Cartesian product minus excludes
    pub fn count(&self) -> usize
}

/// A CI job configuration derived from a matrix point
#[derive(Debug, Clone)]
pub struct CiJob {
    pub name: String,
    pub matrix_point: MatrixPoint,
    pub steps: Vec<CiStep>,
    pub timeout_minutes: u32,
}

#[derive(Debug, Clone)]
pub struct CiStep {
    pub name: String,
    pub command: String,
    pub env: HashMap<String, String>,
}

impl CiJob {
    pub fn new(name: &str, point: MatrixPoint) -> Self
    pub fn add_step(&mut self, step: CiStep) -> &mut Self
    pub fn to_github_actions_yaml(&self) -> String  // basic YAML representation
}

/// Generate a default ClaudeFS CI matrix
pub fn default_claudefs_matrix() -> TestMatrix
/// Generate CI jobs from matrix
pub fn generate_ci_jobs(matrix: &TestMatrix, test_suite: &str) -> Vec<CiJob>
```

Unit tests (~30):
- test MatrixDimension creation and count
- test MatrixPoint get and label
- test TestMatrix add_dimension
- test TestMatrix expand (2x2 = 4 points)
- test TestMatrix exclude removes combinations
- test TestMatrix count matches expand().len()
- test CiJob creation and add_step
- test CiJob to_github_actions_yaml contains job name
- test default_claudefs_matrix has dimensions
- test generate_ci_jobs produces one job per matrix point
- test 3-dimension matrix produces correct cartesian product (2x3x2=12)
- test exclusion reduces count

### Module 2: `src/storage_tests.rs` — Storage Subsystem Integration Tests (~30 tests)

Deep integration tests for the claudefs-storage crate:

```rust
use claudefs_storage::{
    BuddyAllocator, AllocatorConfig, AllocatorStats,
    StorageEngine, StorageEngineConfig,
    Checksum, ChecksumAlgorithm, BlockHeader,
    MockIoEngine, IoEngine,
    StorageError, StorageResult,
};

/// Tests the BuddyAllocator's core invariants
/// - Alloc N blocks, verify they are all different
/// - Free all blocks, verify space is restored
/// - Test fragmentation detection
pub mod allocator_tests {
    // ~10 tests
}

/// Tests the MockIoEngine
/// - Write data, read it back, verify equality
/// - Test multiple writes at different offsets
/// - Verify IoStats tracking
pub mod io_engine_tests {
    // ~10 tests
}

/// Tests StorageEngine configuration and stats
/// - Create StorageEngineConfig with defaults
/// - Test StorageEngineStats initialization
/// - Test capacity tracking
pub mod engine_tests {
    // ~10 tests
}
```

Implement the tests as a proper module with `#[cfg(test)]` test blocks.

Tests must:
1. Import from `claudefs_storage` (already a dependency in Cargo.toml)
2. Use `tempfile::TempDir` for any filesystem operations
3. Test actual API behavior from the storage crate
4. Not require io_uring (use MockIoEngine)

Unit tests (~30):
- BuddyAllocator: new, alloc, free, remaining_capacity, stats
- AllocatorConfig: default values, custom configuration
- Checksum: crc32 roundtrip, blake3 roundtrip, zero data, large data
- ChecksumAlgorithm variants
- MockIoEngine: new, submit_write, submit_read, read back what was written
- IoStats tracking after operations
- StorageEngineConfig: validate, defaults
- StorageError variants and Display

### Module 3: `src/meta_tests.rs` — Metadata Subsystem Integration Tests (~25 tests)

Integration tests for the claudefs-meta crate:

```rust
use claudefs_meta::{
    types::*,
    inode::*,
    directory::*,
    // etc.
};
```

Focus on testing the public types and APIs without requiring a running cluster:
- InodeId, FileType, InodeAttrs creation and serialization
- Directory operations in memory
- Raft log entry serialization/deserialization
- KV store operations (put, get, delete, scan)
- Type roundtrips with bincode serialization

Unit tests (~25):
- Test key type constructors and methods from claudefs_meta::types
- Test serde serialization roundtrips for core types
- Test directory entry creation and listing
- Test basic KV store operations (use in-memory mock if available)
- Test inode attribute creation

**Important:** Read `crates/claudefs-meta/src/lib.rs` and `crates/claudefs-meta/src/types.rs` first to understand the available public API before writing tests. Import only what is actually pub in the crate.

### Module 4: `src/reduce_tests.rs` — Data Reduction Integration Tests (~25 tests)

Integration tests for the claudefs-reduce crate:

```rust
use claudefs_reduce::{
    Chunker, ChunkerConfig, Chunk, CasIndex,
    CompressionAlgorithm,
    EncryptionAlgorithm, EncryptionKey, EncryptedChunk,
    ChunkHash, SuperFeatures,
    ReductionPipeline, PipelineConfig, ReducedChunk, ReductionStats,
    ReduceError,
};
```

Tests (~25):
- Chunker: create with config, chunk small data (< min_size), chunk large data
- Verify chunk count and sizes are within CDC bounds
- Verify total bytes of chunks == input bytes
- CompressionAlgorithm: test LZ4 compress/decompress roundtrip with 4KB data
- CompressionAlgorithm: test Zstd compress/decompress roundtrip
- EncryptionKey: generate, verify key material length
- Test AES-GCM encrypt/decrypt roundtrip
- Test that different keys produce different ciphertexts
- ChunkHash: create from bytes, verify determinism
- ReductionPipeline: create with config, process small data, verify ReducedChunk output
- ReductionStats: verify fields after pipeline run

**Important:** Read `crates/claudefs-reduce/src/lib.rs` to see what's actually public before writing.

## Requirements

1. All 4 new modules must compile with zero errors
2. All ~110 new tests must pass
3. Update `src/lib.rs` to add `pub mod` for all 4 new modules
4. No unsafe code
5. Import only public items from dependency crates
6. For storage_tests.rs, meta_tests.rs, reduce_tests.rs: READ the respective lib.rs files first to understand the public API

## Existing Cargo.toml dependencies

The claudefs-tests crate already depends on:
- claudefs-storage, claudefs-meta, claudefs-reduce, claudefs-transport (all in Cargo.toml)
- serde_json, proptest, tempfile, tokio

## Files to create/modify

1. `crates/claudefs-tests/src/ci_matrix.rs` — NEW
2. `crates/claudefs-tests/src/storage_tests.rs` — NEW (use actual claudefs_storage API)
3. `crates/claudefs-tests/src/meta_tests.rs` — NEW (use actual claudefs_meta API)
4. `crates/claudefs-tests/src/reduce_tests.rs` — NEW (use actual claudefs_reduce API)
5. `crates/claudefs-tests/src/lib.rs` — MODIFY

CRITICAL: Before writing storage_tests.rs, read crates/claudefs-storage/src/lib.rs to know what is pub.
CRITICAL: Before writing meta_tests.rs, read crates/claudefs-meta/src/lib.rs AND crates/claudefs-meta/src/types.rs.
CRITICAL: Before writing reduce_tests.rs, read crates/claudefs-reduce/src/lib.rs.

Output each file with clear delimiters:
```
=== FILE: path/to/file ===
<content>
=== END FILE ===
```
