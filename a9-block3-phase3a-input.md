# A9 Block 3 Phase 3A: Test Result Collector Implementation

## Task Summary
Implement the Test Result Collector module for ClaudeFS test result aggregation and reporting. This module parses cargo test JSON output and generates per-crate JSON test reports.

## Requirements

### Module: `test_collector.rs`
Location: `crates/claudefs-tests/src/test_collector.rs`

Create a Rust module that:

1. **Parses cargo test JSON output:**
   - Accept structured JSON from `cargo test --lib -- --format json`
   - Parse test events: started, ok, failed, skipped
   - Extract test name, status, duration from timestamps

2. **Data structures:**
   ```rust
   #[derive(Serialize, Deserialize, Debug, Clone)]
   pub struct TestCaseResult {
       pub name: String,
       pub status: TestStatus,
       pub duration_ms: u64,
   }

   #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
   pub enum TestStatus {
       Pass,
       Fail,
       Skip,
   }

   #[derive(Serialize, Deserialize, Debug)]
   pub struct TestSuiteReport {
       pub crate_name: String,
       pub timestamp: u64,
       pub duration_secs: f64,
       pub total_tests: usize,
       pub passed: usize,
       pub failed: usize,
       pub skipped: usize,
       pub flaky_count: usize,
       pub tests: Vec<TestCaseResult>,
   }

   pub struct TestCollector {
       results: HashMap<String, Vec<TestCaseResult>>,
   }
   ```

3. **Core methods:**
   - `pub fn from_cargo_output(output: &str) -> Result<Self>` — Parse JSON from cargo test output
   - `pub fn add_result(&mut self, crate_name: &str, result: TestCaseResult)` — Add test result
   - `pub fn get_report(&self, crate_name: &str) -> Option<TestSuiteReport>` — Build report for crate
   - `pub fn write_json_reports(&self, output_dir: &Path) -> io::Result<()>` — Write all reports as JSON
   - `pub fn summary(&self) -> TestSummary` — Get aggregated summary

4. **JSON output format:**
   Each crate gets `target/test-reports/{crate_name}.json`:
   ```json
   {
     "crate": "claudefs-storage",
     "timestamp": 1234567890,
     "duration_secs": 123.45,
     "total_tests": 1204,
     "passed": 1204,
     "failed": 0,
     "skipped": 0,
     "flaky_count": 0,
     "tests": [
       {
         "name": "block_allocator::tests::test_alloc_basic",
         "status": "pass",
         "duration_ms": 45
       }
     ]
   }
   ```

5. **Parsing logic:**
   - Use serde_json to parse cargo JSON format
   - Handle multiline output with cargo test line format
   - Group results by crate (extract from test name: `{crate}::{module}::{test}`)
   - Calculate duration from start/finished timestamps
   - Handle failures/skips gracefully

### Integration into lib.rs
Add to `crates/claudefs-tests/src/lib.rs`:
```rust
pub mod test_collector;
pub use test_collector::{TestCollector, TestCaseResult, TestStatus, TestSuiteReport};
```

### Dependencies
Add to `crates/claudefs-tests/Cargo.toml` if not present:
- `serde = { version = "1", features = ["derive"] }`
- `serde_json = "1"`
- `time = "0.3"` (for timestamp generation)

### Testing
Include comprehensive unit tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_empty() { }

    #[test]
    fn test_collector_single_pass() { }

    #[test]
    fn test_collector_mixed_results() { }

    #[test]
    fn test_collector_groups_by_crate() { }

    #[test]
    fn test_json_report_format() { }

    #[test]
    fn test_write_json_reports() { }

    #[test]
    fn test_cargo_json_parsing() { }
}
```

### Error Handling
- Use `anyhow::Result<T>` for public methods
- Gracefully handle malformed JSON
- Skip unparseable test events (log warning)
- Return detailed errors for file I/O failures

### Documentation
Add doc comments to all public types and methods.

## Success Criteria
- ✅ Module compiles without errors
- ✅ `cargo check` passes for claudefs-tests crate
- ✅ All unit tests pass
- ✅ Generates valid JSON reports matching schema
- ✅ Correctly groups results by crate
- ✅ Handles edge cases (empty results, malformed JSON)
- ✅ Ready for integration with test-reporter binary

## Context
This is Phase 3A of A9 Block 3 (Test Result Aggregation & Reporting). This module is the foundation for:
- GitHub Actions integration (Phase 3B)
- Flaky test detection (Phase 3C)
- CHANGELOG automation (Phase 3D)

See `A9-BLOCK3-DESIGN.md` for full architecture.
