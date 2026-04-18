# A9: Test & Validation — Phase 1 Plan

**Agent:** A9 (Test & Validation)
**Phase:** 1 (Foundation)
**Duration:** ~1-2 weeks (Sessions 1-3)
**Date:** 2026-04-18
**Status:** Planning

## Objective

Establish foundational POSIX test framework and infrastructure for validating ClaudeFS. Phase 1 focuses on:

1. **Unit test harnesses** — Property-based tests for each crate's core traits
2. **POSIX framework** — Wrapper modules for pjdfstest, fsx, xfstests (Phase 2)
3. **Test infrastructure** — Result collection, reporting, CI integration
4. **Regression tracking** — Baseline metrics and change detection
5. **Documentation** — Test design and execution guide

## Dependencies

### On Builders (A1-A8)
- A1 (Storage): Trait interfaces (`read()`, `write()`, `allocate()`)
- A2 (Metadata): Raft consensus, KV store, inode ops
- A3 (Reduce): Data transform pipeline (dedup, compress, encrypt)
- A4 (Transport): RPC protocol, connection lifecycle
- A5 (FUSE): FUSE v3 daemon, syscall handling
- A6 (Replication): Journal replication, consistency
- A7 (Gateway): NFS/pNFS/S3 protocol handling
- A8 (Management): Prometheus metrics, admin API

### On Infrastructure (A11)
- CI/CD pipeline hooks (GitHub Actions)
- Test cluster provisioning (5 storage nodes, 2 client nodes)
- Prometheus/Grafana for test metrics collection

## Phase 1: Foundation Deliverables

### 1. Test Framework Architecture

**Location:** `crates/claudefs-tests/`

```
claudefs-tests/
├── src/
│   ├── lib.rs                          # Main exports
│   ├── test_collection.rs              # Test discovery & tracking
│   ├── posix_framework.rs              # Base POSIX validation
│   ├── property_tests.rs               # Proptest utilities
│   ├── fixtures.rs                     # Mock implementations
│   ├── metrics.rs                      # Test result tracking
│   └── harnesses/
│       ├── pjdfstest_harness.rs        # pjdfstest wrapper (Phase 1)
│       ├── fsx_harness.rs              # fsx wrapper (Phase 1)
│       ├── xfstests_harness.rs         # xfstests stub (Phase 2)
│       ├── connectathon_harness.rs     # Connectathon stub (Phase 2)
│       └── jepsen_harness.rs           # Jepsen stub (Phase 3)
└── tests/
    ├── integration_posix_basics.rs     # Basic file ops
    ├── integration_metadata.rs         # Permissions, links
    ├── integration_concurrency.rs      # Multi-thread safety
    └── integration_regression.rs       # Baseline tracking
```

### 2. Unit Test Harnesses (Per Crate)

**For each builder crate (A1-A8):**

```rust
#[cfg(test)]
mod posix_validation {
    use proptest::prelude::*;

    // 1. Property-based tests for core traits
    // 2. Mock implementations for dependencies
    // 3. Deterministic failure injection
    // 4. Metric collection
}
```

**Key test patterns:**

| Crate | Core Trait | Property Tests |
|-------|-----------|-----------------|
| A1 (Storage) | `StorageEngine` | Alloc/free consistency, concurrent writes, block alignment |
| A2 (Metadata) | `MetadataService` | Inode uniqueness, parent-child links, permission isolation |
| A3 (Reduce) | `ReductionPipeline` | Dedup ratio correctness, compression reversibility, key encryption |
| A4 (Transport) | `RpcClient` | Message ordering, retry semantics, connection pooling |
| A5 (FUSE) | `FuseSession` | Syscall translation, permission enforcement, cache coherency |
| A6 (Replication) | `CrossSiteReplicator` | Journal ordering, conflict detection, LWW correctness |
| A7 (Gateway) | `ProtocolGateway` | NFSv3 semantics, pNFS layout distribution, S3 consistency |
| A8 (Management) | `PrometheusExporter` | Metric completeness, scrape format, API auth |

### 3. POSIX Framework Modules

#### 3.1 `posix_framework.rs`

Base module for all POSIX validation:

```rust
pub trait POSIXValidator {
    /// Validate file permissions match spec
    fn validate_permissions(&self, path: &Path) -> Result<()>;

    /// Validate inode consistency (links, refs)
    fn validate_inode_consistency(&self) -> Result<()>;

    /// Validate directory tree structure
    fn validate_directory_tree(&self) -> Result<()>;

    /// Validate timestamp semantics (mtime/ctime/atime)
    fn validate_timestamps(&self) -> Result<()>;

    /// Validate errno codes match POSIX spec
    fn validate_errno_codes(&self) -> Result<()>;
}
```

#### 3.2 `property_tests.rs`

Proptest harness for data transform pipelines:

```rust
pub mod generators {
    // Generate random but valid:
    // - File paths (up to 255 chars)
    // - Permission bits (valid u16 masks)
    // - File contents (random byte strings)
    // - Block patterns (for dedup testing)
}

pub mod invariants {
    // Verify across all operations:
    // - File size consistency
    // - Dedup ratio bounds (0.5-2.0x)
    // - Encryption decryption roundtrip
    // - Compression reversibility
}
```

#### 3.3 `fixtures.rs`

Mock implementations for testing in isolation:

```rust
pub struct MockStorageEngine { /* ... */ }
pub struct MockMetadataService { /* ... */ }
pub struct MockTransport { /* ... */ }
pub struct MockReductionPipeline { /* ... */ }

// Each mock can be configured for:
// - Failure injection (I/O errors, timeouts, corrupted data)
// - Deterministic behavior (no randomness for reproducibility)
// - Metrics collection (call counts, latencies)
```

#### 3.4 `test_collection.rs`

Test discovery and tracking:

```rust
pub struct TestCollection {
    tests: HashMap<String, TestMetadata>,
}

pub struct TestMetadata {
    pub name: String,
    pub phase: u32,
    pub category: TestCategory,
    pub status: TestStatus,        // Pending, Running, Passed, Failed, Flaky
    pub duration_ms: u64,
    pub failure_count: u32,
    pub last_run: SystemTime,
}

pub enum TestCategory {
    PoSIX,
    Concurrency,
    Distributed,
    Performance,
    Regression,
    Security,
}
```

#### 3.5 `metrics.rs`

Test result aggregation:

```rust
pub struct TestMetrics {
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub flaky: usize,
    pub skipped: usize,
    pub regression_count: usize,
    pub avg_duration_ms: f64,
}

impl TestMetrics {
    /// Export as Prometheus format for dashboards
    pub fn render_prometheus(&self) -> String { /* ... */ }

    /// Export as JSON for CI log aggregation
    pub fn to_json(&self) -> serde_json::Value { /* ... */ }
}
```

### 4. POSIX Test Harnesses

#### 4.1 pjdfstest Harness

**Location:** `crates/claudefs-tests/src/harnesses/pjdfstest_harness.rs`

```rust
pub struct PjdftestHarness {
    pub mount_point: PathBuf,
    pub test_dir: PathBuf,
}

impl PjdftestHarness {
    /// Initialize pjdfstest environment
    pub fn new(mount_point: PathBuf) -> Result<Self> { /* ... */ }

    /// Run pjdfstest suite
    pub fn run(&self) -> Result<TestMetrics> { /* ... */ }

    /// Run specific test group: permissions, links, symlinks, etc.
    pub fn run_group(&self, group: &str) -> Result<TestMetrics> { /* ... */ }

    /// Parse pjdfstest output and map to TestMetrics
    fn parse_output(&self, output: &str) -> Result<TestMetrics> { /* ... */ }
}
```

**Implementation:**
- Calls `pjdfstest` as subprocess
- Parses TAP (Test Anything Protocol) output
- Filters by test group (permissions, directory, links, symlinks, rename, unlink)
- Tracks flaky tests (pass/fail/pass pattern)

**Test groups (from pjdfstest):**
- `core`: basic file operations
- `permissions`: chmod/chown/umask
- `links`: hard links, symlinks, link limits
- `directory`: mkdir/rmdir, traversal, cycles
- `rename`: atomic rename, error cases
- `unlink`: delete semantics, race conditions
- `open`: create, truncate, append
- `misc`: timestamps, xattrs, special files

#### 4.2 fsx Harness

**Location:** `crates/claudefs-tests/src/harnesses/fsx_harness.rs`

```rust
pub struct FsxHarness {
    pub mount_point: PathBuf,
    pub test_file: PathBuf,
}

impl FsxHarness {
    /// Initialize fsx environment
    pub fn new(mount_point: PathBuf) -> Result<Self> { /* ... */ }

    /// Run fsx with operation sequence
    pub fn run(&self, operations: u64, duration_sec: u64) -> Result<TestMetrics> { /* ... */ }

    /// Run fsx with mmap operations specifically
    pub fn run_with_mmap(&self, operations: u64) -> Result<TestMetrics> { /* ... */ }

    /// Parse fsx output for consistency violations
    fn parse_output(&self, output: &str) -> Result<TestMetrics> { /* ... */ }
}
```

**Implementation:**
- Calls `fsx` with random operation sequences
- Operations: read, write, truncate, mmap, fallocate
- In-memory oracle (simulate expected state)
- Detect: data corruption, wrong offsets, uninitialized data

**Configuration:**
- Quick mode: 1000 ops (< 1 min)
- Soak mode: 1M ops (hours)
- With/without mmap, with/without fallocate

#### 4.3 xfstests Harness (Phase 2 stub)

**Location:** `crates/claudefs-tests/src/harnesses/xfstests_harness.rs`

```rust
pub struct XftestsHarness {
    pub mount_point: PathBuf,
    pub test_dir: PathBuf,
}

impl XftestsHarness {
    /// Run xfstests quick suite (Phase 2)
    pub fn run_quick(&self) -> Result<TestMetrics> { /* ... */ }

    /// Run xfstests full suite (Phase 3)
    pub fn run_full(&self) -> Result<TestMetrics> { /* ... */ }
}
```

### 5. Integration Test Modules

#### 5.1 `integration_posix_basics.rs`

Basic file operations (Phase 1):

```rust
#[tokio::test]
async fn test_create_read_delete() { /* ... */ }

#[tokio::test]
async fn test_mkdir_traverse() { /* ... */ }

#[tokio::test]
async fn test_rename_atomicity() { /* ... */ }

#[tokio::test]
async fn test_permissions_enforcement() { /* ... */ }

#[tokio::test]
async fn test_symlink_traversal() { /* ... */ }
```

#### 5.2 `integration_metadata.rs`

Metadata consistency (Phase 1):

```rust
#[tokio::test]
async fn test_inode_uniqueness() { /* ... */ }

#[tokio::test]
async fn test_parent_child_links() { /* ... */ }

#[tokio::test]
async fn test_hardlink_count() { /* ... */ }

#[tokio::test]
async fn test_timestamp_updates() { /* ... */ }

#[tokio::test]
async fn test_permission_isolation() { /* ... */ }
```

#### 5.3 `integration_concurrency.rs`

Multi-thread safety (Phase 1):

```rust
#[tokio::test]
async fn test_concurrent_writes_same_file() { /* ... */ }

#[tokio::test]
async fn test_concurrent_mkdir_same_parent() { /* ... */ }

#[tokio::test]
async fn test_concurrent_hardlinks_race() { /* ... */ }

#[tokio::test]
async fn test_concurrent_rename_cycles() { /* ... */ }
```

#### 5.4 `integration_regression.rs`

Baseline tracking (Phase 1):

```rust
#[tokio::test]
async fn test_regression_perf_baseline() {
    // Store baseline metrics (latency, throughput, dedup ratio)
    // Compare against previous baseline
    // Alert on >10% regression
}
```

### 6. Test Execution Pipeline

**Phase 1 CI flow:**

```bash
# On every push to main:
1. Run unit tests (per-crate): cargo test --lib
2. Run integration tests: cargo test --test '*'
3. Run pjdfstest (on single mount): harness.run_group("core")
4. Run fsx quick (1000 ops): harness.run(1000, 60)
5. Collect metrics (JSON + Prometheus)
6. Upload to results S3 bucket
7. Update CHANGELOG with results
8. Alert if regression detected
```

**Phase 2+ CI flow (on full cluster):**

```bash
# Weekly on test cluster:
1. Run all Phase 1 tests (single node)
2. Run pjdfstest full suite (all test groups)
3. Run fsx soak (1M ops, 8+ hours)
4. Run Connectathon (multi-node, all clients simultaneously)
5. Run Jepsen partition tests (basic split-brain)
6. Collect + aggregate metrics
7. Generate weekly report
```

### 7. Result Tracking & Reporting

**Test result format (JSON):**

```json
{
  "timestamp": "2026-04-18T14:30:00Z",
  "phase": 1,
  "suite": "pjdfstest-core",
  "status": "passed",
  "total": 847,
  "passed": 845,
  "failed": 2,
  "flaky": 0,
  "skipped": 0,
  "duration_sec": 1235,
  "failures": [
    {
      "name": "chmod/02.t",
      "error": "permission denied on read",
      "expected": "EACCES (13)",
      "actual": "EPERM (1)"
    }
  ],
  "regressions": []
}
```

**Regression detection:**

```rust
pub fn detect_regressions(
    baseline: &TestMetrics,
    current: &TestMetrics,
) -> Vec<Regression> {
    // Failed tests that were previously passing
    // Performance >10% worse than baseline
    // Flaky tests appearing (was stable before)
}
```

## Implementation Plan

### Session 1 (This Session)
- [ ] Read architecture, decisions, agents docs ✅
- [ ] Create Phase 1 specification (`docs/A9-PHASE1-PLAN.md`) ✅
- [ ] Design POSIX test framework (this document)
- [ ] Commit plan to main: `[A9] Phase 1: Test & Validation Framework — Planning Complete`

### Session 2
- [ ] Set up `claudefs-tests` crate structure (OpenCode)
- [ ] Implement `posix_framework.rs` and `fixtures.rs` (OpenCode)
- [ ] Implement `test_collection.rs` and `metrics.rs` (OpenCode)
- [ ] Create `pjdfstest_harness.rs` + stub `fsx_harness.rs` (OpenCode)
- [ ] Create 3 integration test modules: posix_basics, metadata, concurrency (OpenCode)
- [ ] All tests passing: cargo test --test '*'
- [ ] Commit: `[A9] Phase 1: POSIX Test Framework — Implementation Complete (Block 1)`

### Session 3
- [ ] Set up CI/CD GitHub Actions for test execution (Claude)
- [ ] Add Prometheus metrics export from test framework (OpenCode)
- [ ] Create test result aggregation pipeline (Claude + shell)
- [ ] Baseline metrics collection (first run)
- [ ] Regression detection testing
- [ ] CHANGELOG update with Phase 1 results
- [ ] Commit: `[A9] Phase 1 Complete: POSIX Framework + CI Integration`

## Test Counts (Phase 1 Target)

| Category | Est. Count | Notes |
|----------|-----------|-------|
| Unit tests (per-crate property tests) | 400 | 50/crate for A1-A8 |
| pjdfstest groups (basic + metadata) | 847 | Official pjdfstest |
| fsx quick run | 1 | 1000 operations, tracked as success/failure |
| Integration tests (posix_basics, metadata, concurrency) | 50 | 15-20/module |
| Regression tracking tests | 10 | Baseline collection |
| **Phase 1 Total** | **~1,300** | Includes property-based variations |

## Success Criteria

By end of Phase 1:
- ✅ pjdfstest running and reporting results
- ✅ fsx basic run working
- ✅ Unit tests for all 8 crates passing
- ✅ Integration tests for basic file ops, metadata, concurrency passing
- ✅ Test result tracking system in place (JSON + Prometheus)
- ✅ CI/CD hooks ready for Phase 2 cluster testing
- ✅ Regression baseline established
- ✅ Full documentation for test execution

## Blockers / Dependencies

| Blocker | Status | Owner | Mitigation |
|---------|--------|-------|-----------|
| pjdfstest binary installed on CI | Pending | A11 | Include in orchestrator user-data script |
| fsx binary availability | Pending | A11 | Included in LTP package |
| Mount point for testing | Pending | A5 | Mock FUSE or in-memory for Phase 1 |
| A1-A8 trait APIs stable | ✅ Complete | Builders | Phase 1 uses existing interfaces |

## References

- `docs/posix.md` — POSIX test suite overview
- `docs/agents.md` — Full agent plan and dependencies
- `docs/decisions.md` — Architecture decisions (trait design, API stability)
- pjdfstest: https://github.com/pjd/pjdfstest
- fsx: https://github.com/linux-test-project/ltp/blob/master/testcases/kernel/fs/fsx-mac/fsx.c
- xfstests: https://git.kernel.org/pub/scm/fs/xfs/xfstests-dev.git

---

**Next:** Session 2 will implement the full test framework via OpenCode, starting with `claudefs-tests` crate structure and POSIX validation framework.
