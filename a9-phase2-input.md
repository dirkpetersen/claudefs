# A9: Test & Validation — Phase 2: Extended Tests

You are extending the `claudefs-tests` crate for the ClaudeFS distributed filesystem project. This is Agent A9 (Test & Validation).

## Working directory: /home/cfs/claudefs

## Context

Phase 1 created the following modules in `crates/claudefs-tests/src/`:
- `harness.rs` — TestEnv, TestCluster
- `posix.rs` — pjdfstest, fsx, xfstests wrappers
- `proptest_storage.rs` — property-based storage tests (25 tests)
- `proptest_reduce.rs` — property-based reduction tests (25 tests)
- `proptest_transport.rs` — property-based transport tests (30 tests)
- `integration.rs` — integration test framework
- `linearizability.rs` — WGL linearizability checker
- `crash.rs` — crash consistency framework
- `chaos.rs` — fault injection
- `bench.rs` — FIO benchmark harness
- `connectathon.rs` — Connectathon runner

Total Phase 1 tests: 238

## Task: Phase 2 — Extended Tests (5 NEW modules)

Create 5 new modules in `crates/claudefs-tests/src/` and update `src/lib.rs` to include them. Target: **~130 new tests** (total ~368).

### Module 1: `src/posix_compliance.rs` — POSIX Compliance Test Cases (~30 tests)

A collection of programmatic POSIX compliance tests that run entirely in Rust against a filesystem path:

```rust
/// Tests specific POSIX behaviors that ClaudeFS must implement correctly
pub struct PosixComplianceSuite {
    pub root: PathBuf,
}

pub struct PosixTestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

pub struct PosixSuiteReport {
    pub results: Vec<PosixTestResult>,
    pub passed: usize,
    pub failed: usize,
}

impl PosixComplianceSuite {
    pub fn new(root: PathBuf) -> Self
    pub fn run_all(&self) -> PosixSuiteReport
    pub fn test_file_create_read_write(&self) -> PosixTestResult  // create, write, read, verify
    pub fn test_rename_atomicity(&self) -> PosixTestResult  // atomic rename over existing
    pub fn test_mkdir_rmdir(&self) -> PosixTestResult  // create/remove directory
    pub fn test_hardlink(&self) -> PosixTestResult  // hard link count increments
    pub fn test_symlink(&self) -> PosixTestResult  // symlink creation and follow
    pub fn test_truncate(&self) -> PosixTestResult  // truncate to 0 and to larger size
    pub fn test_seek_tell(&self) -> PosixTestResult  // seek/tell operations
    pub fn test_append_mode(&self) -> PosixTestResult  // O_APPEND mode
    pub fn test_permissions(&self) -> PosixTestResult  // mode bits (read, write, exec)
    pub fn test_timestamps(&self) -> PosixTestResult  // mtime/atime updated correctly
    pub fn test_concurrent_writes(&self) -> PosixTestResult  // two threads writing different ranges
    pub fn test_large_directory(&self) -> PosixTestResult  // 1000 files in a directory
    pub fn test_deep_path(&self) -> PosixTestResult  // 10 levels of nested directories
    pub fn test_special_filenames(&self) -> PosixTestResult  // files with spaces, dots, unicode
}
```

All `test_*` methods write to a temp directory under `self.root` (each gets its own subdir), then clean up after themselves. Tests use `std::fs` and `tempfile`.

Unit tests in this module:
- test PosixComplianceSuite::new()
- test that each test_* method returns PosixTestResult with name set
- test run_all() returns report with correct counts (use a temp dir)
- test PosixSuiteReport accumulation
- At least 15 real test functions that each run a POSIX operation and assert correctness

### Module 2: `src/jepsen.rs` — Jepsen-Style Distributed Test Framework (~30 tests)

A framework for Jepsen-style fault injection and history generation:

```rust
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

/// A Jepsen-style client operation on a distributed system
#[derive(Debug, Clone)]
pub struct JepsenOp {
    pub process: u32,          // which client process
    pub op_type: JepsenOpType,
    pub key: String,
    pub value: Option<i64>,
    pub timestamp: u64,        // monotonic ns since start
}

#[derive(Debug, Clone, PartialEq)]
pub enum JepsenOpType {
    Invoke,
    Ok,
    Fail,
    Info,
}

/// The type of operation (for a register model)
#[derive(Debug, Clone, PartialEq)]
pub enum RegisterOp {
    Read,
    Write(i64),
    CAS { expected: i64, new: i64 },
}

/// A complete Jepsen test history
#[derive(Debug, Default)]
pub struct JepsenHistory {
    pub ops: Vec<JepsenOp>,
}

impl JepsenHistory {
    pub fn new() -> Self
    pub fn invoke(&mut self, process: u32, key: &str, value: Option<i64>) -> u64
    pub fn complete_ok(&mut self, process: u32, key: &str, value: Option<i64>)
    pub fn complete_fail(&mut self, process: u32, key: &str)
    pub fn duration_ns(&self) -> u64
    pub fn ops_by_process(&self, process: u32) -> Vec<&JepsenOp>
    pub fn invocations(&self) -> Vec<&JepsenOp>
    pub fn completions(&self) -> Vec<&JepsenOp>
    pub fn is_well_formed(&self) -> bool  // every invoke has exactly one completion
}

/// A register model for linearizability checking
pub struct RegisterModel {
    pub state: i64,
}

impl RegisterModel {
    pub fn new(initial: i64) -> Self
    pub fn apply_read(&self) -> i64
    pub fn apply_write(&mut self, value: i64)
    pub fn apply_cas(&mut self, expected: i64, new: i64) -> bool
}

/// Checker: given a JepsenHistory, verify linearizability of a register model
pub struct JepsenChecker;
impl JepsenChecker {
    pub fn new() -> Self
    pub fn check_register(&self, history: &JepsenHistory) -> CheckResult
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub valid: bool,
    pub anomalies: Vec<String>,
    pub message: String,
}

/// A nemesis that injects faults during a test
pub struct Nemesis {
    pub active_faults: Vec<String>,
}
impl Nemesis {
    pub fn new() -> Self
    pub fn partition_random(&mut self) -> String  // returns fault id
    pub fn heal(&mut self, fault_id: &str)
    pub fn heal_all(&mut self)
    pub fn fault_count(&self) -> usize
}

/// Test configuration for a Jepsen-style test
pub struct JepsenTestConfig {
    pub num_clients: u32,
    pub ops_per_client: u32,
    pub nemesis_interval_ms: u64,
    pub test_duration_ms: u64,
}

impl Default for JepsenTestConfig {
    fn default() -> Self { /* reasonable defaults */ }
}
```

Unit tests (~30):
- test JepsenOp creation
- test JepsenHistory::invoke and complete_ok
- test JepsenHistory::is_well_formed (valid and invalid cases)
- test JepsenHistory::ops_by_process
- test RegisterModel read/write/CAS
- test JepsenChecker on a trivially linear history
- test JepsenChecker on a known non-linear history
- test Nemesis fault injection and healing
- test JepsenTestConfig default

### Module 3: `src/soak.rs` — Long-Running Soak Test Framework (~20 tests)

Infrastructure for running soak/stress tests:

```rust
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;

/// Configuration for a soak test run
#[derive(Debug, Clone)]
pub struct SoakConfig {
    pub duration: Duration,
    pub num_workers: u32,
    pub ops_per_sec_target: u64,
    pub verify_data: bool,
    pub seed: u64,
}

impl Default for SoakConfig { ... }

/// Statistics collected during a soak test
#[derive(Debug, Default)]
pub struct SoakStats {
    pub ops_completed: Arc<AtomicU64>,
    pub ops_failed: Arc<AtomicU64>,
    pub bytes_written: Arc<AtomicU64>,
    pub bytes_read: Arc<AtomicU64>,
    pub errors: Arc<std::sync::Mutex<Vec<String>>>,
}

impl SoakStats {
    pub fn new() -> Arc<Self>
    pub fn record_op(&self)
    pub fn record_failure(&self, err: String)
    pub fn record_write(&self, bytes: u64)
    pub fn record_read(&self, bytes: u64)
    pub fn snapshot(&self) -> SoakSnapshot
}

#[derive(Debug, Clone)]
pub struct SoakSnapshot {
    pub ops_completed: u64,
    pub ops_failed: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub error_count: usize,
    pub elapsed: Duration,
    pub ops_per_sec: f64,
    pub write_mb_per_sec: f64,
    pub read_mb_per_sec: f64,
}

/// A soak test that writes and reads files in a loop
pub struct FileSoakTest {
    pub config: SoakConfig,
    pub root: PathBuf,
}

impl FileSoakTest {
    pub fn new(root: PathBuf, config: SoakConfig) -> Self
    pub fn run_brief(&self) -> SoakSnapshot  // 1 second brief run for testing
}

/// Represents a single soak test worker's task
#[derive(Debug, Clone)]
pub struct WorkerTask {
    pub worker_id: u32,
    pub op: WorkerOp,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerOp { Write, Read, Delete, Verify }

/// Generate a deterministic sequence of worker tasks for testing
pub fn generate_task_sequence(worker_id: u32, seed: u64, count: usize) -> Vec<WorkerTask>
```

Unit tests (~20):
- test SoakConfig default
- test SoakStats record_op/record_failure
- test SoakSnapshot calculations
- test generate_task_sequence determinism
- test WorkerTask creation
- test FileSoakTest::new
- test SoakStats snapshot with multiple ops

### Module 4: `src/regression.rs` — Regression Test Registry (~25 tests)

A registry for tracking regressions found by the test suite:

```rust
use std::collections::HashMap;
use std::time::SystemTime;

/// Severity of a regression
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// A registered regression test case
#[derive(Debug, Clone)]
pub struct RegressionCase {
    pub id: String,          // e.g. "CLAUDEFS-42"
    pub title: String,
    pub severity: Severity,
    pub components: Vec<String>,  // e.g. ["claudefs-storage", "claudefs-meta"]
    pub description: String,
    pub reproduction: String,    // steps to reproduce
    pub fixed_in: Option<String>, // commit hash or version
}

/// Registry of all known regression test cases
pub struct RegressionRegistry {
    cases: HashMap<String, RegressionCase>,
}

impl RegressionRegistry {
    pub fn new() -> Self
    pub fn register(&mut self, case: RegressionCase)
    pub fn get(&self, id: &str) -> Option<&RegressionCase>
    pub fn by_severity(&self, severity: &Severity) -> Vec<&RegressionCase>
    pub fn by_component(&self, component: &str) -> Vec<&RegressionCase>
    pub fn fixed_cases(&self) -> Vec<&RegressionCase>
    pub fn open_cases(&self) -> Vec<&RegressionCase>
    pub fn count(&self) -> usize
    pub fn seed_known_issues(&mut self)  // seeds registry with known ClaudeFS issues
}

/// Result of running a regression test
#[derive(Debug, Clone)]
pub struct RegressionResult {
    pub case_id: String,
    pub reproduced: bool,
    pub details: String,
}

/// Runner for regression tests
pub struct RegressionRunner {
    pub registry: RegressionRegistry,
}

impl RegressionRunner {
    pub fn new() -> Self
    pub fn run_case(&self, id: &str, test_path: &std::path::Path) -> RegressionResult
    pub fn run_all(&self, test_path: &std::path::Path) -> Vec<RegressionResult>
    pub fn summary(&self, results: &[RegressionResult]) -> RegressionSummary
}

#[derive(Debug, Clone)]
pub struct RegressionSummary {
    pub total: usize,
    pub reproduced: usize,
    pub not_reproduced: usize,
    pub fixed_percent: f64,
}
```

Unit tests (~25):
- test RegressionCase creation
- test Severity ordering (Low < Medium < High < Critical)
- test RegressionRegistry register and get
- test by_severity filtering
- test by_component filtering
- test fixed_cases and open_cases
- test seed_known_issues creates entries
- test RegressionRunner summary calculation
- test RegressionResult creation

### Module 5: `src/report.rs` — Test Report Generation (~25 tests)

A module for generating structured test reports:

```rust
use std::time::{Duration, SystemTime};
use serde::{Serialize, Deserialize};

/// Overall result of a test run
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TestStatus { Pass, Fail, Skip, Error }

/// A single test case result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub suite: String,
    pub status: TestStatus,
    pub duration: Duration,
    pub message: Option<String>,
    pub tags: Vec<String>,
}

/// A test suite report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuiteReport {
    pub name: String,
    pub timestamp: u64,  // Unix timestamp
    pub duration: Duration,
    pub cases: Vec<TestCaseResult>,
}

impl TestSuiteReport {
    pub fn new(name: &str) -> Self
    pub fn add_result(&mut self, result: TestCaseResult)
    pub fn passed(&self) -> usize
    pub fn failed(&self) -> usize
    pub fn skipped(&self) -> usize
    pub fn total(&self) -> usize
    pub fn pass_rate(&self) -> f64  // 0.0 to 1.0
    pub fn is_passing(&self) -> bool  // true if no failures
    pub fn to_json(&self) -> Result<String, serde_json::Error>
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error>
    pub fn to_junit_xml(&self) -> String  // JUnit XML format for CI
    pub fn summary_line(&self) -> String  // "PASS 12/15 (2 failed, 1 skipped) in 1.23s"
}

/// Aggregate report across multiple suites
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateReport {
    pub suites: Vec<TestSuiteReport>,
    pub generated_at: u64,
}

impl AggregateReport {
    pub fn new() -> Self
    pub fn add_suite(&mut self, suite: TestSuiteReport)
    pub fn total_passed(&self) -> usize
    pub fn total_failed(&self) -> usize
    pub fn total_tests(&self) -> usize
    pub fn overall_pass_rate(&self) -> f64
    pub fn to_json(&self) -> Result<String, serde_json::Error>
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error>
    pub fn print_summary(&self)  // prints a multi-line summary to stdout
}

/// Builder for constructing test suite reports
pub struct ReportBuilder {
    suite: TestSuiteReport,
    start_time: std::time::Instant,
}

impl ReportBuilder {
    pub fn new(suite_name: &str) -> Self
    pub fn pass(&mut self, test_name: &str, duration: Duration) -> &mut Self
    pub fn fail(&mut self, test_name: &str, duration: Duration, message: &str) -> &mut Self
    pub fn skip(&mut self, test_name: &str, reason: &str) -> &mut Self
    pub fn build(self) -> TestSuiteReport
}
```

Unit tests (~25):
- test TestCaseResult creation
- test TestSuiteReport new and add_result
- test passed/failed/skipped counts
- test pass_rate calculation
- test is_passing (only false when failures)
- test to_json/from_json roundtrip
- test to_junit_xml has correct structure
- test summary_line format
- test AggregateReport add_suite and counts
- test ReportBuilder pass/fail/skip/build
- test overall_pass_rate across multiple suites

## Requirements

1. **Add `serde_json = "1.0"` to `[dependencies]` in `Cargo.toml`** (needed for report.rs)
2. All 5 new modules must compile with zero errors
3. All ~130 new tests must pass
4. Update `src/lib.rs` to add `pub mod` declarations and `pub use` re-exports for all 5 new modules
5. No unsafe code
6. Tests use `tempfile::TempDir` for temporary directories
7. Async tests use `#[tokio::test]`
8. Property tests use `proptest!`

## Files to create/modify

1. `crates/claudefs-tests/src/posix_compliance.rs` — NEW
2. `crates/claudefs-tests/src/jepsen.rs` — NEW
3. `crates/claudefs-tests/src/soak.rs` — NEW
4. `crates/claudefs-tests/src/regression.rs` — NEW
5. `crates/claudefs-tests/src/report.rs` — NEW
6. `crates/claudefs-tests/src/lib.rs` — MODIFY (add new pub mods and pub uses)
7. `crates/claudefs-tests/Cargo.toml` — MODIFY (add serde_json dependency)

Output each file with clear delimiters:
```
=== FILE: path/to/file ===
<content>
=== END FILE ===
```
