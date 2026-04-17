# A9 Block 3: Test Reporter Binary & Changelog Generator Implementation

## Task Summary
Implement two components:
1. **test-reporter binary** — CLI tool to parse cargo test JSON and generate reports
2. **changelog_generator.rs module** — Automated CHANGELOG.md entry generation

## Part 1: test-reporter Binary

Location: `crates/claudefs-tests/src/bin/test-reporter.rs`

### Binary spec:
```bash
# Usage
cargo run -p claudefs-tests --bin test-reporter -- <cargo-test-output-file>

# Reads:
# - JSON file from cargo test --lib -- --format json
# - target/test-history.json (if exists)

# Outputs:
# - Per-crate JSON reports to target/test-reports/*.json
# - Flaky test detection results
# - Markdown summary to stdout
# - GitHub issue body to target/test-issue-body.md (if flaky tests found)
```

### Features:
1. **Parse cargo test output:**
   - Read JSON format from file
   - Call TestCollector::from_cargo_output()
   - Generate per-crate reports

2. **Output files:**
   - `target/test-reports/{crate}.json` — Per-crate test results
   - `target/test-history.json` — Updated flaky test history
   - `target/test-issue-body.md` — GitHub issue template (if flaky tests found)
   - Stdout: Markdown summary

3. **Markdown output format:**
   ```markdown
   ## ✅ Test Results Summary

   | Crate | Result | Duration | Tests |
   |-------|--------|----------|-------|
   | claudefs-storage | 1204/1204 ✅ | 12.3s | PASS |
   | claudefs-meta | 997/997 ✅ | 8.5s | PASS |
   | claudefs-reduce | 2020/2020 ✅ | 15.2s | PASS |

   **Total:** 4221 passed | 0 failed | 35.9s ⏱️

   ### Issues Found
   - Flaky test detected: 1 test >5% flake rate
   - See GitHub issue template below
   ```

4. **Implementation:**
   - Use clap for CLI args
   - Call TestCollector and FlakyTracker from lib
   - Write JSON reports via TestCollector::write_json_reports()
   - Update history via FlakyTracker::save_to_file()
   - Format markdown output
   - Create issue template if flaky tests found

### Error handling:
- Print helpful errors to stderr
- Exit with code 0 if all tests pass
- Exit with code 1 if any tests fail
- Exit with code 2 if flaky tests detected (warning)

## Part 2: changelog_generator Module

Location: `crates/claudefs-tests/src/changelog_generator.rs`

Create module that:

1. **Data structures:**
   ```rust
   #[derive(Serialize, Deserialize, Debug)]
   pub struct ChangelogEntry {
       pub date: String,       // "2026-03-06"
       pub test_stats: TestStats,
       pub crate_statuses: Vec<CrateStatus>,
       pub issues_resolved: Vec<String>,
       pub flaky_alerts: Vec<String>,
       pub upcoming: Vec<String>,
   }

   #[derive(Serialize, Deserialize, Debug)]
   pub struct TestStats {
       pub unit_tests_total: usize,
       pub unit_tests_passed: usize,
       pub unit_tests_failed: usize,
       pub integration_tests_total: usize,
       pub flaky_test_count: usize,
   }

   #[derive(Serialize, Deserialize, Debug)]
   pub struct CrateStatus {
       pub crate_name: String,
       pub test_count: usize,
       pub status: String,  // "✅", "🟡", "🔴"
   }
   ```

2. **Core methods:**
   - `pub fn from_reports(reports: &[TestSuiteReport]) -> Self` — Create from test reports
   - `pub fn with_issues_resolved(mut self, issues: Vec<String>) -> Self` — Add resolved issues
   - `pub fn with_flaky_alerts(mut self, alerts: Vec<String>) -> Self` — Add flaky test alerts
   - `pub fn with_upcoming(mut self, upcoming: Vec<String>) -> Self` — Add upcoming tasks
   - `pub fn generate_markdown(&self) -> String` — Format for CHANGELOG.md
   - `pub fn update_changelog(path: &Path, entry: &ChangelogEntry) -> io::Result<()>` — Append to CHANGELOG

3. **Markdown format:**
   ```markdown
   ## 2026-03-06

   ### Test Health ✅
   - **Unit tests:** 6300+ passing (0 failures)
   - **Integration tests:** Pending (CI integration in progress)
   - **Flaky tests:** 0 (all fixed)

   ### Crate Status
   - A1 (storage): 1301 tests ✅
   - A2 (metadata): 1035 tests ✅
   - A3 (reduce): 2020 tests ✅
   - A4 (transport): 800 tests ✅
   - A5 (fuse): 1073 tests ✅
   - A6 (repl): 878 tests ✅
   - A7 (gateway): 600 tests ✅
   - A8 (mgmt): 965 tests ✅
   - A9 (tests): Block 3 implementation complete

   ### Issues Resolved
   - Issue #22: fsinfo flaky test (FIXED by A5)
   - Issue #24: transport compilation (FIXED by A10)

   ### Flaky Alerts
   - No flaky tests detected ✅

   ### Upcoming
   - Block 4: Performance baseline (with A11)
   - Phase 4: Production deployment planning
   ```

4. **Automation:**
   - CLI option to append daily entry
   - Only append once per day
   - Preserve existing CHANGELOG entries
   - Format: newest entries at top

### Integration into lib.rs
Add to `crates/claudefs-tests/src/lib.rs`:
```rust
pub mod changelog_generator;
pub use changelog_generator::{ChangelogEntry, TestStats, CrateStatus};
```

### Dependencies
- Same as test_collector and flaky_tracker
- `chrono` for date formatting (optional, can use time crate)

### Testing
Include comprehensive unit tests for both:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_changelog_entry_from_reports() { }

    #[test]
    fn test_changelog_markdown_format() { }

    #[test]
    fn test_changelog_update_appends() { }

    #[test]
    fn test_changelog_preserves_existing() { }

    #[test]
    fn test_reporter_output_format() { }

    #[test]
    fn test_reporter_exit_codes() { }

    #[test]
    fn test_github_issue_template() { }
}
```

### Error Handling
- Use `anyhow::Result<T>`
- Gracefully handle malformed reports
- Create CHANGELOG if doesn't exist
- Robust file I/O error handling

### Documentation
Add doc comments to all public APIs.

## Success Criteria
- ✅ test-reporter binary compiles and runs
- ✅ Generates valid per-crate JSON reports
- ✅ Markdown summary output is well-formatted
- ✅ GitHub issue template generated for flaky tests
- ✅ changelog_generator creates/updates CHANGELOG correctly
- ✅ Daily entries append without duplicating
- ✅ All unit tests pass
- ✅ Exit codes match specifications

## Context
These components complete Phase 3B & 3D of A9 Block 3:
- Phase 3B: GitHub Actions integration (test-reporter used in CI)
- Phase 3D: Daily CHANGELOG updates (via changelog_generator)

See `A9-BLOCK3-DESIGN.md` for full architecture and integration points.
