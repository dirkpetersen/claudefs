# A9 Block 3 Phase 3C: Flaky Test Detection Implementation

## Task Summary
Implement the Flaky Test Tracker module for ClaudeFS test validation. This module detects intermittently failing tests by tracking run history and calculating flake rates.

## Requirements

### Module: `flaky_tracker.rs`
Location: `crates/claudefs-tests/src/flaky_tracker.rs`

Create a Rust module that:

1. **Data structures:**
   ```rust
   #[derive(Serialize, Deserialize, Debug, Clone, Copy)]
   pub enum TestStatus {
       Pass,
       Fail,
       Skip,
   }

   #[derive(Serialize, Deserialize, Debug, Clone)]
   pub struct TestRun {
       pub timestamp: u64,
       pub status: TestStatus,
       pub duration_ms: u64,
   }

   #[derive(Serialize, Deserialize, Debug)]
   pub struct FlakyTestRecord {
       pub name: String,
       pub total_runs: usize,
       pub failures: usize,
       pub flake_rate: f64,
       pub recent_runs: VecDeque<TestStatus>,
       pub last_run_time: u64,
   }

   pub struct FlakyTracker {
       history: HashMap<String, VecDeque<TestRun>>,
       max_history: usize,
   }
   ```

2. **Core methods:**
   - `pub fn new(max_history: usize) -> Self` — Create tracker (default 40 runs)
   - `pub fn add_result(&mut self, test_name: &str, status: TestStatus, duration_ms: u64)` — Add test result
   - `pub fn calculate_flake_rate(&self, test_name: &str) -> f64` — Get flake rate (0-1)
   - `pub fn suspicious_tests(&self, threshold: f64) -> Vec<FlakyTestRecord>` — Find suspicious tests
   - `pub fn load_from_file(path: &Path) -> Result<Self>` — Load history from JSON
   - `pub fn save_to_file(&self, path: &Path) -> io::Result<()>` — Save history to JSON
   - `pub fn get_record(&self, test_name: &str) -> Option<FlakyTestRecord>` — Get full record

3. **Flake detection logic:**
   - Track last 40 runs per test (configurable)
   - Calculate flake rate: `failures / total_runs`
   - Flag if rate > 5% (configurable threshold)
   - Include recent run pattern (last 10 results)
   - Track timestamp of last run

4. **History persistence:**
   - Store in `target/test-history.json`:
   ```json
   {
     "test_name": "block_allocator::tests::test_alloc_basic",
     "total_runs": 45,
     "failures": 2,
     "flake_rate": 0.044,
     "recent_runs": ["pass", "pass", "fail", "pass", "pass"],
     "last_run_time": 1234567890
   }
   ```
   - Load and merge with new results on each run
   - Keep 40 most recent runs per test

5. **GitHub Issue generation format:**
   ```markdown
   ## Flaky Test Detected: fsinfo::tests::stats_age_secs_reflects_elapsed

   **Flake Rate:** 12.5% (5 failures / 40 runs)
   **Recent Pattern:** PASS FAIL PASS FAIL PASS FAIL

   ### Details
   - Last seen: 2026-03-06 12:34:56 UTC
   - First failure: 2026-03-01 08:15:22 UTC
   - Median duration: 45ms

   ### Recommendations
   - Increase timing buffer (sleep duration)
   - Add test isolation/cleanup
   - Review for race conditions
   - Consider removing from critical path if >10% flake rate

   **Tag:** `flaky-test`
   ```

### Integration into lib.rs
Add to `crates/claudefs-tests/src/lib.rs`:
```rust
pub mod flaky_tracker;
pub use flaky_tracker::{FlakyTracker, FlakyTestRecord, TestRun, TestStatus};
```

### Dependencies
Same as test_collector:
- `serde`, `serde_json`, `time`

### Testing
Include comprehensive unit tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tracker() { }

    #[test]
    fn test_single_pass() { }

    #[test]
    fn test_single_failure() { }

    #[test]
    fn test_flake_rate_calculation() { }

    #[test]
    fn test_suspicious_tests_threshold() { }

    #[test]
    fn test_history_persistence() { }

    #[test]
    fn test_max_history_rotation() { }

    #[test]
    fn test_recent_pattern_tracking() { }

    #[test]
    fn test_multiple_tests() { }
}
```

### Error Handling
- Use `anyhow::Result<T>` for public methods
- Gracefully handle missing history file (start fresh)
- Handle corrupted JSON (rebuild from scratch)
- Return detailed errors for file I/O

### Documentation
Add doc comments to all public types and methods.

## Success Criteria
- ✅ Module compiles without errors
- ✅ `cargo check` passes for claudefs-tests crate
- ✅ All unit tests pass
- ✅ Correctly calculates flake rates
- ✅ Persists history to JSON correctly
- ✅ Detects suspicious tests above threshold
- ✅ Handles edge cases (empty, single run, 100% pass/fail)
- ✅ Ready for integration with CI

## Context
This is Phase 3C of A9 Block 3 (Test Result Aggregation & Reporting). This module:
- Depends on: test_collector.rs output
- Used by: CI workflows for automatic issue creation
- Follows: Phase 3A (test_collector) and 3B (GitHub Actions integration)

See `A9-BLOCK3-DESIGN.md` for full architecture.
