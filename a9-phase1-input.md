# A9: Test & Validation — Phase 1: Test Infrastructure Crate

You are implementing the `claudefs-tests` crate for the ClaudeFS distributed filesystem project. This is the Test & Validation agent (A9) that provides the cross-cutting test infrastructure for all 8 builder crates.

## Working directory: /home/cfs/claudefs

## Task

Create a NEW crate `crates/claudefs-tests/` that serves as the test & validation infrastructure for ClaudeFS. This crate will:

1. Provide **property-based tests** for the core data transforms (storage, reduction, transport)
2. Provide **integration test scaffolding** that can test cross-crate interactions
3. Provide a **POSIX test suite runner** that wraps pjdfstest, fsx, xfstests
4. Provide a **crash consistency test framework** (CrashMonkey-style)
5. Provide **linearizability checking** utilities for Jepsen-style tests
6. Provide **FIO benchmark harness** wrapper
7. Provide **chaos/fault injection** utilities for distributed tests

## Existing Workspace

The workspace has these crates already (all passing tests):
- `claudefs-storage` — block store, io_uring, buddy allocator (90 tests)
- `claudefs-meta` — Raft, inode ops, KV store (495 tests)
- `claudefs-reduce` — dedupe, compression, encryption (223 tests)
- `claudefs-transport` — RPC, RDMA/TCP, circuit breaker, etc. (528 tests)
- `claudefs-fuse` — FUSE v3 daemon (stub)
- `claudefs-repl` — cross-site replication (stub)
- `claudefs-gateway` — NFSv3, pNFS, S3 (stub)
- `claudefs-mgmt` — management API, metrics (stub)

## What to Create

### File: `crates/claudefs-tests/Cargo.toml`

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
tracing.workspace = true
tracing-subscriber.workspace = true
bytes = "1"
rand = "0.8"
tempfile = "3"
proptest = "1.4"

# ClaudeFS crates under test
claudefs-storage = { path = "../claudefs-storage" }
claudefs-meta = { path = "../claudefs-meta" }
claudefs-reduce = { path = "../claudefs-reduce" }
claudefs-transport = { path = "../claudefs-transport" }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
proptest = "1.4"

[lib]
name = "claudefs_tests"
path = "src/lib.rs"
```

### Modules to implement in `src/`:

#### `src/lib.rs`
Top-level re-exports and `pub mod` declarations for all test modules.

#### `src/harness.rs` — Test Harness
A general-purpose test environment setup:
- `struct TestEnv` — temporary directory, tracing setup, runtime
- `impl TestEnv { fn new() -> Self, fn tempdir(&self) -> &Path, fn runtime(&self) -> &tokio::runtime::Runtime }`
- `struct TestCluster` — simulates a multi-node cluster in memory
- `impl TestCluster { fn single_node() -> Self, fn three_node() -> Self, fn node_count(&self) -> usize }`
- Count: ~20 unit tests

#### `src/posix.rs` — POSIX Test Suite Runner
Wrapper around external POSIX test tools:
- `struct PjdfsRunner` — runs pjdfstest against a mounted path
  - `fn new(mount_path: PathBuf) -> Self`
  - `fn run_suite(&self, suite: &str) -> PjdfsResult`
  - `fn run_all(&self) -> Vec<PjdfsResult>`
- `struct PjdfsResult { total: usize, passed: usize, failed: usize, skipped: usize, test_name: String }`
- `struct FsxRunner` — wraps the fsx (file system exerciser) tool
  - `fn new(test_file: PathBuf) -> Self`
  - `fn with_ops(mut self, ops: u64) -> Self`
  - `fn with_seed(mut self, seed: u64) -> Self`
  - `fn run(&self) -> FsxResult`
- `struct FsxResult { ops_completed: u64, ops_failed: u64, duration_secs: f64 }`
- `struct XfstestsRunner` — wraps xfstests
  - `fn new(test_dir: PathBuf, scratch_dir: PathBuf) -> Self`
  - `fn run_group(&self, group: &str) -> XfstestsResult`
- `struct XfstestsResult { passed: Vec<String>, failed: Vec<String>, skipped: Vec<String> }`
- Helper: `fn detect_pjdfstest_binary() -> Option<PathBuf>` — looks in PATH and /usr/local/bin
- Helper: `fn detect_fsx_binary() -> Option<PathBuf>`
- Count: ~20 unit tests (test the harness itself, mock tool runners)

#### `src/proptest_storage.rs` — Property-Based Tests for Storage
Uses proptest to test claudefs-storage invariants:
- `fn arb_block_size() -> impl Strategy<Value = u64>` — generates valid block sizes
- `fn arb_placement_hint() -> impl Strategy<Value = PlacementHint>` — arbitrary placement hints
- Test: `proptest! { fn block_id_roundtrip(id in 0u64..u64::MAX) { ... } }`
- Test: `proptest! { fn block_size_alignment(size in arb_block_size()) { assert!(size % 4096 == 0 || size == 0) } }`
- Test: checksum roundtrip (write data, compute checksum, verify)
- Test: allocator alloc/free invariants — alloc N blocks, free them, assert capacity restored
- Count: ~25 proptest tests

#### `src/proptest_reduce.rs` — Property-Based Tests for Data Reduction
Uses proptest to test claudefs-reduce invariants:
- `fn arb_data(max_size: usize) -> impl Strategy<Value = Vec<u8>>` — generates random byte slices
- Test: `proptest! { fn compression_roundtrip(data in arb_data(65536)) { compress then decompress, assert original == result } }`
- Test: `proptest! { fn encryption_roundtrip(data in arb_data(65536), key in arb_key()) { encrypt then decrypt, assert equality } }`
- Test: BLAKE3 fingerprint determinism — same data always gives same hash
- Test: FastCDC chunking — reassembling chunks gives original data
- Test: Dedup ratio never > 1.0 (can't produce more data than input after dedup)
- Count: ~25 proptest tests

#### `src/proptest_transport.rs` — Property-Based Tests for Transport
- Test: message encode/decode roundtrip
- Test: protocol version compatibility (lower version always accepted by higher)
- Test: circuit breaker state machine invariants (never goes from Open to Closed without reset)
- Test: rate limiter token bucket invariants
- Count: ~20 proptest tests

#### `src/integration.rs` — Integration Test Framework
Integration tests that exercise multiple crates together:
- `struct IntegrationTestSuite` with `fn run_all(&self) -> IntegrationReport`
- `struct IntegrationReport { tests_run: usize, tests_passed: usize, tests_failed: usize, failures: Vec<String> }`
- Test: storage + checksum — write block, verify checksum, read back and verify
- Test: reduction pipeline — write data through compression+encryption, read and verify
- Test: transport protocol framing — encode RPC message, decode it
- Test: metadata types roundtrip — serialize inode, deserialize, assert equality
- Count: ~25 tests

#### `src/linearizability.rs` — Linearizability Checker
A Jepsen-style linearizability checking utility:
- `struct Operation<T> { invoke_time: u64, complete_time: u64, input: T, output: T }`
- `struct History<T> { ops: Vec<Operation<T>> }`
- `impl<T: Clone + Eq> History<T> { fn is_linearizable(&self) -> bool }` — WGL algorithm
- `struct LinearizabilityReport { is_linear: bool, violation: Option<String> }`
- Simple key-value model: `struct KvModel { state: HashMap<String, String> }`
- `trait Model<T> { fn init() -> Self; fn step(&mut self, input: &T) -> T; fn is_valid(&self, input: &T, output: &T) -> bool }`
- Count: ~20 tests (including cases with known linear/non-linear histories)

#### `src/crash.rs` — Crash Consistency Test Framework
CrashMonkey-style crash consistency testing:
- `struct CrashPoint { offset: u64, description: String }` — identifies where to inject a crash
- `struct CrashConsistencyTest { name: String, setup: Box<dyn Fn(&Path)>, operation: Box<dyn Fn(&Path)>, verify: Box<dyn Fn(&Path) -> bool> }`
- `struct CrashSimulator` — simulates power-failure at specified byte offsets
  - `fn simulate_crash_at(&self, path: &Path, crash_point: &CrashPoint) -> Result<(), CrashError>`
  - `fn run_test(&self, test: &CrashConsistencyTest, crash_points: &[CrashPoint]) -> CrashReport`
- `struct CrashReport { test_name: String, crash_points_tested: usize, recoveries_succeeded: usize, recoveries_failed: usize }`
- `enum CrashError { IoError(String), SimulationFailed(String) }`
- Count: ~20 tests

#### `src/chaos.rs` — Chaos/Fault Injection Utilities
For simulating distributed system failures:
- `enum FaultType { NetworkPartition { from: NodeId, to: NodeId }, NodeCrash(NodeId), PacketLoss { rate: f64 }, LatencySpike { delay_ms: u64 }, DiskFull(NodeId) }`
- `struct FaultInjector` — manages a set of active faults
  - `fn inject(&mut self, fault: FaultType) -> FaultHandle`
  - `fn clear(&mut self, handle: FaultHandle)`
  - `fn clear_all(&mut self)`
- `struct FaultHandle(u64)` — opaque handle to remove a fault
- `type NodeId = u32`
- `struct NetworkTopology` — tracks which partitions are active
  - `fn can_reach(&self, from: NodeId, to: NodeId) -> bool`
- Count: ~20 tests

#### `src/bench.rs` — FIO and Performance Benchmark Harness
Performance test infrastructure:
- `struct FioConfig { rw: FioRwMode, bs: String, iodepth: u32, numjobs: u32, runtime_secs: u32, filename: PathBuf }`
- `enum FioRwMode { Read, Write, RandRead, RandWrite, ReadWrite, RandRW }`
- `struct FioRunner { config: FioConfig }`
  - `fn run(&self) -> FioResult`
- `struct FioResult { read_bw_kb: Option<u64>, write_bw_kb: Option<u64>, read_iops: Option<u64>, write_iops: Option<u64>, read_lat_us_p99: Option<u64>, write_lat_us_p99: Option<u64> }`
- `fn detect_fio_binary() -> Option<PathBuf>`
- `fn parse_fio_json(output: &str) -> Result<FioResult, anyhow::Error>` — parse fio --output-format=json
- Count: ~20 tests (test parsers and config builders, not actual fio execution)

#### `src/connectathon.rs` — Connectathon NFS Test Suite Runner
Wrapper around the Connectathon NFS test suite:
- `struct ConnectathonRunner { mount_path: PathBuf, test_dir: PathBuf }`
  - `fn new(mount_path: PathBuf) -> Self`
  - `fn run_basic(&self) -> ConnectathonResult`
  - `fn run_general(&self) -> ConnectathonResult`
  - `fn run_special(&self) -> ConnectathonResult`
  - `fn run_all(&self) -> ConnectathonReport`
- `struct ConnectathonResult { suite: String, passed: usize, failed: usize, not_run: usize }`
- `struct ConnectathonReport { basic: ConnectathonResult, general: ConnectathonResult, special: ConnectathonResult }`
- Count: ~15 tests

## Requirements

1. **All modules must compile** — the crate must `cargo build` with zero errors
2. **All tests must pass** — `cargo test` for the claudefs-tests crate must pass
3. **No unsafe code** — this is a test crate; no unsafe blocks needed
4. **Error handling** — use `thiserror` for the crate's own errors, `anyhow` at test entry points
5. **Async** — use `tokio::test` for async tests
6. **Property-based tests** — use `proptest` for data transform tests
7. **Tracing** — initialize tracing in TestEnv::new() with `tracing_subscriber`
8. **NO actual fio/pjdfstest/xfstests execution** — tests should use mocks/simulators
   - The runners are designed to be used from integration tests and CI, not unit tests
   - Unit tests in this crate should test the harness structs themselves (parsing, config, detection)
9. Use `tempfile::TempDir` for temporary directories in tests
10. All structs should derive `Debug` where possible, `Clone` where sensible
11. Total target: **165+ tests** passing

## Shared Conventions

- Error types: `thiserror` for library errors
- Async: Tokio
- Logging: `tracing` crate with structured spans
- Testing: `proptest` for data transforms

## Important

Write ALL the files needed:
1. `crates/claudefs-tests/Cargo.toml`
2. `crates/claudefs-tests/src/lib.rs`
3. `crates/claudefs-tests/src/harness.rs`
4. `crates/claudefs-tests/src/posix.rs`
5. `crates/claudefs-tests/src/proptest_storage.rs`
6. `crates/claudefs-tests/src/proptest_reduce.rs`
7. `crates/claudefs-tests/src/proptest_transport.rs`
8. `crates/claudefs-tests/src/integration.rs`
9. `crates/claudefs-tests/src/linearizability.rs`
10. `crates/claudefs-tests/src/crash.rs`
11. `crates/claudefs-tests/src/chaos.rs`
12. `crates/claudefs-tests/src/bench.rs`
13. `crates/claudefs-tests/src/connectathon.rs`

Output each file with clear delimiters like:
```
=== FILE: crates/claudefs-tests/Cargo.toml ===
<content>
=== END FILE ===
```
