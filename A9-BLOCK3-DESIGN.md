# A9 Block 3: Test Result Aggregation & Reporting — Design Document

**Prepared:** 2026-03-06
**Status:** Design phase (waiting for clean build from A10)
**Owner:** A9 (Test & Validation)
**Timeline:** 4-6 hours implementation (once build passes)

---

## Block 3 Objectives

Provide developers real-time visibility into test health through:
1. Per-crate JSON test result reports
2. GitHub Actions CI integration
3. PR comments with pass/fail summaries
4. Daily CHANGELOG entries with metrics
5. Flaky test detection & tracking system

---

## Architecture Overview

### Components

```
cargo test --lib
    ↓
Test execution with structured output
    ↓
Per-crate ReportBuilder aggregation
    ↓
JSON report files (target/test-reports/*.json)
    ↓
├─ GitHub Actions parsing
├─ PR comment generation
└─ Flaky test detection
    ↓
GitHub issues + CHANGELOG updates
```

### Data Flow

1. **Test Execution** (existing): `cargo test --lib` runs all tests
2. **Report Collection** (new): Capture pass/fail/duration per test
3. **Aggregation** (new): Use ReportBuilder to create per-crate JSON
4. **CI Integration** (new): Parse results, publish to GitHub
5. **Flaky Detection** (new): Track test reruns, detect patterns

---

## Implementation Plan

### Phase 3A: Test Result Collector (2 hours)

**Goal:** Capture test results from cargo output

**Implementation:**
1. Create `crates/claudefs-tests/src/test_collector.rs`:
   ```rust
   pub struct TestCollector {
       results: Vec<TestCaseResult>,
   }

   impl TestCollector {
       pub fn from_cargo_output(output: &str) -> Self { ... }
       pub fn write_json_reports(&self, output_dir: &Path) -> io::Result<()> { ... }
   }
   ```

2. Parse `cargo test --lib` JSON output:
   - Use `cargo test -- --format json` flag
   - Parse each test event (started, ok, failed)
   - Track duration from start/finished timestamps

3. Create per-crate JSON files:
   - `target/test-reports/claudefs-storage.json`
   - `target/test-reports/claudefs-meta.json`
   - etc.

4. Schema per file:
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

**Testing:**
- Unit tests for TestCollector parsing
- Integration test with actual cargo output

---

### Phase 3B: GitHub Actions Integration (2 hours)

**Goal:** Parse results in CI and report back to PRs

**Implementation:**
1. Update `.github/workflows/test-unit.yml`:
   ```yaml
   - name: Collect test results
     run: cargo test --lib -- --format json 2>&1 | tee test-output.json

   - name: Generate reports
     run: cargo run -p claudefs-tests --bin test-reporter -- test-output.json

   - name: Upload reports
     uses: actions/upload-artifact@v3
     with:
       name: test-reports
       path: target/test-reports/*.json

   - name: Comment on PR
     if: github.event_name == 'pull_request'
     uses: actions/github-script@v7
     with:
       script: |
         const fs = require('fs');
         const reports = glob.sync('target/test-reports/*.json');

         let comment = '## ✅ Test Results\n\n';
         let total_passed = 0, total_failed = 0;

         for (const file of reports) {
           const data = JSON.parse(fs.readFileSync(file));
           comment += `| ${data.crate} | ${data.passed}/${data.total_tests} | ${data.duration_secs.toFixed(1)}s |\n`;
           total_passed += data.passed;
           total_failed += data.failed;
         }

         github.rest.issues.createComment({
           issue_number: context.issue.number,
           owner: context.repo.owner,
           repo: context.repo.repo,
           body: comment + `\n**Total:** ${total_passed} passed, ${total_failed} failed`
         });
   ```

2. Create binary `bin/test-reporter`:
   - Reads JSON from cargo test output
   - Generates per-crate reports
   - Outputs markdown summary

3. PR Comment Template:
   ```markdown
   ## Test Results Summary

   | Crate | Result | Duration |
   |-------|--------|----------|
   | claudefs-storage | 1204/1204 ✅ | 12.3s |
   | claudefs-meta | 997/997 ✅ | 8.5s |
   | claudefs-tests | 847/847 ✅ | 5.2s |

   **Total:** 3048 passed | 0 failed | 26.0s
   ```

---

### Phase 3C: Flaky Test Detection (1.5 hours)

**Goal:** Identify and track intermittently failing tests

**Implementation:**
1. Create `crates/claudefs-tests/src/flaky_tracker.rs`:
   ```rust
   pub struct FlakyTracker {
       history: HashMap<String, Vec<TestRun>>,
   }

   struct TestRun {
       timestamp: u64,
       status: TestStatus,
       duration_ms: u64,
   }

   impl FlakyTracker {
       pub fn add_result(&mut self, test_name: &str, status: TestStatus) { ... }
       pub fn calculate_flake_rate(&self, test_name: &str) -> f64 { ... }
       pub fn suspicious_tests(&self, threshold: f64) -> Vec<String> { ... }
   }
   ```

2. Track results across runs:
   - Store test history in `target/test-history.json`
   - Calculate flake rate: `failures / total_runs`
   - Flag if rate > 5%

3. GitHub Issue creation:
   - When flake rate exceeds 5%
   - Include: test name, flake rate, recent failures
   - Tag with `flaky-test` label

4. Sample output:
   ```markdown
   ## Flaky Test Detected: fsinfo::tests::stats_age_secs_reflects_elapsed

   Flake rate: 12.5% (5 failures / 40 runs)
   Last 5 runs: PASS FAIL PASS FAIL PASS

   Recommendations:
   - Increase timing buffer (sleep duration)
   - Add test isolation/cleanup
   - Consider removing from critical path
   ```

---

### Phase 3D: Daily CHANGELOG Updates (1 hour)

**Goal:** Keep CHANGELOG.md synchronized with test metrics

**Implementation:**
1. Create `crates/claudefs-tests/src/changelog_generator.rs`:
   ```rust
   pub fn generate_daily_entry(
       reports: &[TestSuiteReport],
       flaky_tests: &[FlakyTestAlert],
   ) -> String { ... }
   ```

2. Daily entry format:
   ```markdown
   ## 2026-03-06

   ### Test Health
   - **Unit tests:** 6300+ passing (0 failures)
   - **Integration tests:** Pending (CI integration in progress)
   - **POSIX compliance:** pjdfstest ready (nightly)
   - **Flaky tests:** 0 (all fixed)

   ### Crate Status
   - A1 (storage): 1301 tests ✅
   - A2 (metadata): 1035 tests ✅
   - A3 (reduce): 2020 tests ✅
   - A9 (tests): Block 3 in progress

   ### Issues Resolved
   - Issue #22: fsinfo flaky test (FIXED by A5)

   ### Upcoming
   - Block 3: Result aggregation (in progress)
   - Block 4: Performance baseline (with A11)
   ```

3. Automated update via CI:
   - Nightly job appends to CHANGELOG.md
   - Commit with message: `[A9] Update daily test metrics`
   - Push to main

---

## File Structure

**New files to create:**
```
crates/claudefs-tests/src/
├── test_collector.rs      (→ pub mod)
├── flaky_tracker.rs       (→ pub mod)
├── changelog_generator.rs (→ pub mod)
└── bin/
    └── test-reporter.rs   (→ binary)

.github/workflows/
└── test-unit-reporting.yml (updated)
```

**Modified files:**
```
crates/claudefs-tests/src/lib.rs  (→ add pub mods)
.github/workflows/test-unit.yml   (→ add reporting steps)
CHANGELOG.md                       (→ daily updates)
```

---

## Testing Strategy

### Unit Tests (for new modules)
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_collector_parses_json() { ... }

    #[test]
    fn test_flaky_detector_calculates_rate() { ... }

    #[test]
    fn test_changelog_format() { ... }
}
```

### Integration Tests
- Parse real cargo test output
- Verify JSON schema
- Test GitHub Actions comment generation

### Manual Verification
1. Run `cargo test --lib`
2. Check `target/test-reports/` for JSON files
3. Verify PR comment format
4. Check flaky test detection on next run

---

## Success Criteria

- ✅ All per-crate JSON reports generated
- ✅ PR comments display correctly
- ✅ Flaky test detection catches intermittent failures
- ✅ CHANGELOG automatically updates daily
- ✅ No manual intervention required
- ✅ Developers have real-time visibility into test health

---

## Deployment

**Phase 3B Release (once A10 build fixed):**
1. Implement test_collector.rs + test_reporter binary
2. Update test-unit.yml workflow
3. Add flaky_tracker.rs module
4. Implement changelog_generator.rs
5. Run full `cargo test --lib`
6. Verify all JSON reports generated
7. Test PR comment generation
8. Commit: `[A9] Block 3: Test result aggregation & reporting`
9. Push to main

**Expected results:**
- ~6300 tests passing
- Per-crate reports in target/test-reports/
- PR comments showing summary
- Flaky test detection active

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| JSON parsing fails on cargo output | Use official cargo JSON format spec |
| PR comment too large | Paginate or summarize per crate |
| Flaky detection too sensitive | Use 5% threshold, 40-run history |
| Missing test data | Fallback to aggregating from TAP output |

---

## Next Steps (After A10 Fixes Build)

1. **Immediately:**
   - Implement test_collector.rs (1-2 hours)
   - Add test_reporter binary (1-2 hours)
   - Update CI workflow (30 min)

2. **Then:**
   - Implement flaky_tracker.rs (1.5 hours)
   - Set up changelog automation (1 hour)

3. **Verify:**
   - Full `cargo test --lib` passing
   - All per-crate reports generated
   - PR comments working
   - Flaky detection active

4. **Proceed to Block 4:**
   - Performance baseline (with A11)
   - FIO benchmarking setup

---

**Readiness:** ✅ Design complete, ready to implement once A10 fixes build errors
**Owner:** A9 (Test & Validation)
**Dependencies:** A10 must fix claudefs-security compilation errors
**ETA:** 4-6 hours implementation time after build fix
