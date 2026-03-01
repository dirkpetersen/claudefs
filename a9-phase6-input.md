# A9 Phase 6: Integration Tests for FUSE, Replication, and Gateway Crates

## Task
Add Phase 6 tests to the `claudefs-tests` crate. This is the Test & Validation
crate for ClaudeFS. The existing crate has 691 tests across 24 modules (Phases 1–5).

Phase 6 adds 5 new modules testing `claudefs-fuse`, `claudefs-repl`, and
`claudefs-gateway` crates, plus cross-crate integration. Target: ~100 new tests.

## Files to Create/Modify

### 1. Modify `crates/claudefs-tests/Cargo.toml`
Add three new dependencies (fuse, repl, gateway crates):

```toml
[package]
name = "claudefs-tests"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "ClaudeFS A9: Test & Validation — POSIX suites, integration tests, benchmarks, Jepsen, CrashMonkey"

[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
bincode.workspace = true
serde_json = "1.0"
tracing.workspace = true
tracing-subscriber.workspace = true
bytes = "1"
rand = "0.8"
tempfile = "3"
proptest = "1.4"

claudefs-storage = { path = "../claudefs-storage" }
claudefs-meta = { path = "../claudefs-meta" }
claudefs-reduce = { path = "../claudefs-reduce" }
claudefs-transport = { path = "../claudefs-transport" }
claudefs-fuse = { path = "../claudefs-fuse" }
claudefs-repl = { path = "../claudefs-repl" }
claudefs-gateway = { path = "../claudefs-gateway" }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
proptest = "1.4"

[lib]
name = "claudefs_tests"
path = "src/lib.rs"
```

### 2. Modify `crates/claudefs-tests/src/lib.rs`
Add 5 new module declarations after the existing ones:

```rust
pub mod fuse_tests;
pub mod repl_integration;
pub mod gateway_integration;
pub mod fault_recovery_tests;
pub mod pipeline_integration;
```

### 3. Create `crates/claudefs-tests/src/fuse_tests.rs`

Test the claudefs-fuse crate. Based on reading the source:

**Known types from claudefs-fuse:**
- `claudefs_fuse::error::{FuseError, Result}` — FuseError variants: Io, MountFailed{mountpoint,reason}, NotFound{ino}, PermissionDenied{ino,op}, NotDirectory{ino}, IsDirectory{ino}, NotEmpty{ino}, AlreadyExists{name}, InvalidArgument{msg}, PassthroughUnsupported, KernelVersionTooOld{required,found}, CacheOverflow, NotSupported{op}
- `FuseError::to_errno()` — returns i32 (libc errno)
- `claudefs_fuse::inode::{InodeId, InodeKind}` — InodeId is newtype around u64
- `claudefs_fuse::cache::{CacheConfig, CacheStats, MetadataCache}` — CacheConfig has: capacity, ttl_secs, negative_ttl_secs
- `claudefs_fuse::locking::{LockType, LockRecord, LockManager}` — LockType: Shared, Exclusive, Unlock; LockRecord: lock_type, owner(u64), pid(u32), start(u64), end(u64)
  - `LockManager::new()`, `try_lock(ino, req) -> Result<bool>`, `unlock(ino, owner)`, `has_conflicting_lock(ino, lock_type, start, end) -> bool`
- `claudefs_fuse::attr::FileAttr` — struct with: ino(u64), size(u64), blocks(u64), atime, mtime, ctime, crtime (all SystemTime), kind(fuser::FileType), perm(u16), nlink(u32), uid(u32), gid(u32), rdev(u32), blksize(u32), flags(u32)

Write ~20 tests. Use `#[cfg(test)]` and `mod tests { use super::*; }` pattern.

Test groups:
1. FuseError formatting (5 tests): NotFound, PermissionDenied, AlreadyExists, MountFailed, NotSupported — test Display via `.to_string()` contains expected substrings
2. FuseError errno mapping (3 tests): NotFound→ENOENT, PermissionDenied→EACCES, IsDirectory→EISDIR
3. CacheConfig (4 tests): default values (capacity=10000, ttl=30, neg_ttl=5), custom config construction, MetadataCache::new(config) works
4. LockManager (8 tests): new() creates empty state, Shared locks don't conflict with each other, Exclusive conflicts with Shared, Exclusive conflicts with Exclusive, Unlock removes lock, lock on different inodes don't interfere, byte-range non-overlap doesn't conflict, byte-range overlap does conflict

**IMPORTANT**: Do NOT use `claudefs_fuse::attr::FileAttr` directly since it depends on `fuser::FileType` which may not be easy to construct. Focus tests on error types, cache config, and locking which are self-contained.

### 4. Create `crates/claudefs-tests/src/repl_integration.rs`

Test the claudefs-repl crate. Based on reading the source:

**Known types from claudefs-repl:**
- `claudefs_repl::compression::{CompressionAlgo, CompressionConfig, CompressedBatch, BatchCompressor}`
  - `CompressionAlgo`: None, Lz4 (default), Zstd — impl Default (Lz4), `is_compressed() -> bool`
  - `CompressionConfig`: algo, zstd_level(i32), min_compress_bytes(usize) — Default: Lz4/3/256
  - `CompressedBatch`: batch_seq(u64), source_site_id(u64), original_bytes(usize), compressed_bytes(usize), algo(CompressionAlgo), data(Vec<u8>) — `compression_ratio() -> f64`, `is_beneficial() -> bool`
  - `BatchCompressor::new(config) -> Self`, `compress(batch: &EntryBatch) -> Result<CompressedBatch>`, `decompress(compressed: &CompressedBatch) -> Result<EntryBatch>`
- `claudefs_repl::backpressure::{BackpressureLevel, BackpressureConfig, BackpressureController, BackpressureManager}`
  - `BackpressureLevel`: None, Mild, Moderate, Severe, Halt — impl PartialOrd (ordinal order: None < Mild < ... < Halt)
  - `BackpressureLevel::suggested_delay_ms() -> u64`, `is_halted() -> bool`, `is_active() -> bool`
  - `BackpressureConfig::default()` — mild_queue_depth:1000, moderate_queue_depth:10000, severe_queue_depth:100000, halt_queue_depth:1000000, error_count_moderate:3, error_count_severe:10, error_count_halt:20
  - `BackpressureController::new(config)`, `set_queue_depth(depth)`, `record_success()`, `record_error()`, `force_halt()`, `clear_halt()`, `current_level() -> BackpressureLevel`
  - `BackpressureManager::new()`, `get_or_create(site_id: u64)`, `set_queue_depth(site_id, depth)`, `record_success(site_id)`, `record_error(site_id)`, `halted_sites() -> Vec<u64>`
- `claudefs_repl::metrics::{Metric, ReplMetrics, MetricsAggregator}`
  - `Metric::counter(name, help, labels, value) -> Metric`, `Metric::gauge(name, help, labels, value) -> Metric`, `format() -> String`
  - `ReplMetrics::default()` — all fields zero; fields: site_id, entries_tailed, entries_compacted_away, batches_dispatched, entries_sent, bytes_sent, throttle_stalls, fanout_failures, lag_entries, pipeline_running(bool)
  - `MetricsAggregator::new()`, `update(metrics: ReplMetrics)`, `format_all() -> String`, `total_entries_sent() -> u64`, `total_bytes_sent() -> u64`
- `claudefs_repl::conduit::{ConduitConfig, EntryBatch}`
  - `ConduitConfig::new(local_site_id, remote_site_id)`, `ConduitConfig::default()`
  - `EntryBatch`: batch_seq(u64), source_site_id(u64), entries(Vec<JournalEntry>) — derives Serialize+Deserialize
- `claudefs_repl::journal::JournalEntry` — probably has: seq(u64), timestamp(u64), op_type, inode_id, data

Write ~22 tests:
1. CompressionAlgo (4 tests): default is Lz4, None::is_compressed()=false, Lz4::is_compressed()=true, Zstd::is_compressed()=true
2. CompressionConfig (3 tests): default values correct, custom config construction
3. CompressedBatch (3 tests): compression_ratio with equal sizes=1.0, is_beneficial when compressed < original, is_beneficial=false when compressed >= original
4. BackpressureLevel (4 tests): ordering (None < Mild < Moderate < Severe < Halt), suggested_delay_ms values, is_halted only on Halt, is_active on non-None
5. BackpressureController (5 tests): starts at None, queue depth triggers Mild at 1000, error_count triggers Moderate at 3, force_halt → Halt, clear_halt returns to error-based level
6. Metric (3 tests): counter format contains "# TYPE counter", gauge format contains "# TYPE gauge", metric with labels formats correctly

**IMPORTANT for BackpressureController**: To check the level, the method may be `current_level()`. Check if there's a `compute_level()` or similar. Use what's available. If unsure, just test what's clearly available.

**For BatchCompressor tests**: To create an EntryBatch for compression testing, you need to know JournalEntry. If JournalEntry is complex to construct, just create an EntryBatch with empty entries vec: `EntryBatch { batch_seq: 1, source_site_id: 1, entries: vec![] }`. The compressor should still work on an empty batch.

### 5. Create `crates/claudefs-tests/src/gateway_integration.rs`

Test the claudefs-gateway crate. Based on reading the source:

**Known types from claudefs-gateway:**
- `claudefs_gateway::wire::{validate_nfs_fh, validate_nfs_filename, validate_nfs_path, validate_nfs_count, validate_s3_key}`
  - `validate_nfs_fh(data: &[u8]) -> Result<()>` — ok if 1-64 bytes, err if empty or >64
  - `validate_nfs_filename(name: &str) -> Result<()>` — ok if 1-255 non-null, no '/', err otherwise
  - `validate_nfs_path(path: &str) -> Result<()>` — must start with '/', no null, max 1024 bytes
  - `validate_nfs_count(count: u32) -> Result<()>` — 1..=1_048_576
  - `validate_s3_key(key: &str) -> Result<()>` — 1-1024 bytes, no leading slash
- `claudefs_gateway::session::{SessionId, SessionProtocol, ClientSession, SessionManager}`
  - `SessionId::new(id: u64) -> Self`, `as_u64(self) -> u64`
  - `SessionProtocol`: Nfs3, S3, Smb3
  - `ClientSession::new(id, protocol, client_ip: &str, uid: u32, gid: u32, now: u64) -> Self`
  - `ClientSession::touch(now)`, `record_op(now, bytes)`, `add_mount(path)`, `remove_mount(path)`, `is_idle(now, timeout_secs) -> bool`
  - `SessionManager::new()`, `create_session(protocol, client_ip, uid, gid, now) -> SessionId`, `get_session(id: SessionId) -> Option<ClientSession>`, `expire_idle(now: u64, timeout_secs: u64) -> usize`, `count() -> usize`, `end_session(id: SessionId) -> bool`, `list_sessions() -> Vec<ClientSession>`
- `claudefs_gateway::config::ExportConfig`
  - Fields: path(String), allowed_clients(Vec<String>), read_only(bool), root_squash(bool), anon_uid(u32), anon_gid(u32)
  - `ExportConfig::default_rw(path: &str) -> Self` — creates read-write export
  - `ExportConfig::default_ro(path: &str) -> Self` — creates read-only export
- `claudefs_gateway::export_manager::{ExportStatus, ActiveExport, ExportManager}`
  - `ExportStatus`: Active, Draining, Disabled
  - `ActiveExport::new(config, root_fh, root_inode)`, `is_active() -> bool`, `can_remove() -> bool`
  - `ExportManager::new()`, `add_export(config, root_inode) -> Result<FileHandle3>`, `remove_export(path: &str) -> bool`, `list_exports() -> Vec<ActiveExport>`, `count() -> usize`, `is_exported(path: &str) -> bool`, `export_paths() -> Vec<String>`, `total_clients() -> u32`
- `claudefs_gateway::protocol::FileHandle3` — `FileHandle3::from_inode(ino: u64) -> Self`
- `claudefs_gateway::error::{GatewayError, Result}` — GatewayError::ProtocolError{reason}

Write ~22 tests:
1. Wire validation — NFS fh (5 tests): empty fails, 1-byte ok, 64-byte ok, 65-byte fails, valid fh ok
2. Wire validation — NFS filename (4 tests): empty fails, normal name ok, name with '/' fails, name with null fails
3. Wire validation — NFS path (4 tests): no leading slash fails, "/" ok, long path ok, path with null fails
4. Wire validation — count (2 tests): 0 fails, 1 ok, 1MB ok
5. Session management (5 tests): create session, session id increments, record_op updates bytes, is_idle detects timeout, add/remove mount
6. ExportManager (5 tests): new is empty (count()==0), add_export succeeds, duplicate add fails with Err, count() correct, is_exported returns true after add

**IMPORTANT**: For `ExportConfig`, use `ExportConfig::default_rw("/export/data")` or `ExportConfig::default_ro("/export/data")` as constructors.

### 6. Create `crates/claudefs-tests/src/fault_recovery_tests.rs`

Test fault recovery patterns. These tests are standalone (no external deps beyond what's already available).

**Tests for cross-crate error handling and recovery patterns:**

Use these imports:
- `claudefs_storage::error::StorageError`
- `claudefs_meta::error::MetaError`
- `claudefs_reduce::error::ReduceError`
- `claudefs_transport::error::TransportError`
- `claudefs_fuse::error::FuseError`
- `claudefs_repl::error::ReplError`
- `claudefs_gateway::error::GatewayError`

Write ~20 tests:
1. Error type constructability (7 tests, one per crate): verify each error type can be constructed and displayed
2. Error conversion/display: error messages contain expected text
3. Recovery config structures (5 tests): use `claudefs_storage::recovery::{RecoveryConfig, RecoveryManager, RecoveryPhase}` — test phase ordering, config defaults
4. Fault tolerance patterns (3 tests): test that operations gracefully handle missing/invalid data
5. Error propagation patterns (5 tests): demonstrate error wrapping with `anyhow::anyhow!()`, `?` operator patterns

**IMPORTANT**: Avoid constructing complex types that require many dependencies. Focus on:
- Error types (thiserror-derived) that can be constructed with simple fields
- Config structs with Default implementations
- Simple state machine types

### 7. Create `crates/claudefs-tests/src/pipeline_integration.rs`

Cross-crate pipeline integration tests. Focus on testing data flows between multiple crates.

**Tests that exercise interactions between crates:**

Write ~18 tests combining:
1. Reduce + Storage pipeline (5 tests): `claudefs_reduce::pipeline::ReductionPipeline` with `claudefs_storage::block::BuddyAllocator` and `claudefs_storage::io::MockIoEngine` — test full data round-trips through compress+encrypt+store
2. Meta + Storage interaction (4 tests): Use `claudefs_meta::inode::InodeId` with storage allocator, verify inode ID operations
3. Replication entry batch serialization (4 tests): Create `EntryBatch` from `claudefs_repl::conduit`, serialize with `bincode`, deserialize, verify roundtrip
4. Gateway + Protocol integration (5 tests): Combine wire validation with session management — simulate a client session creating/validating NFS requests

**For the ReductionPipeline tests**, import from:
- `claudefs_reduce::pipeline::ReductionPipeline`
- `claudefs_reduce::config::ReductionConfig`
- `claudefs_storage::block::BuddyAllocator`
- `claudefs_storage::io::MockIoEngine`

These are already tested in `write_path_e2e.rs` so you have a pattern to follow.

For entry batch serialization:
```rust
use claudefs_repl::conduit::EntryBatch;
use claudefs_repl::journal::JournalEntry;
// Create a minimal EntryBatch and roundtrip through bincode
let batch = EntryBatch { batch_seq: 42, source_site_id: 1, entries: vec![] };
let encoded = bincode::serialize(&batch).unwrap();
let decoded: EntryBatch = bincode::deserialize(&encoded).unwrap();
assert_eq!(decoded.batch_seq, 42);
```

## Important Rules

1. ALL tests must compile and pass (no `todo!()`, no `unimplemented!()`)
2. Use `#[test]` (not `#[tokio::test]`) unless async is strictly necessary
3. Use `#[allow(unused_imports)]` at the top of modules if needed to avoid warnings
4. Follow the exact same test pattern as existing modules (see snapshot_tests.rs for reference)
5. Each module starts with `//! Module description` doc comment
6. Use `assert!`, `assert_eq!`, `.is_ok()`, `.is_err()`, `.unwrap()` as appropriate
7. Do NOT test internal/private items — only public API
8. Keep tests simple and focused — no complex setup, no network calls
9. If you're unsure about a specific type's API, use simpler test cases that you're confident about

## Reference: Existing Test Pattern
```rust
//! [Module] tests for ClaudeFS

use some_crate::module::Type;

fn helper_function() -> Type {
    // ...
}

#[test]
fn test_something() {
    let result = Type::new();
    assert!(result.is_ok() || result.is_valid());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specific_behavior() {
        // ...
    }
}
```

## What to output

For each file, output the COMPLETE file content starting with a line like:
```
=== FILE: crates/claudefs-tests/Cargo.toml ===
```

Then the full content of the file.

Then another `===` separator for the next file.

Output all 7 files (Cargo.toml, lib.rs, fuse_tests.rs, repl_integration.rs, gateway_integration.rs, fault_recovery_tests.rs, pipeline_integration.rs).
