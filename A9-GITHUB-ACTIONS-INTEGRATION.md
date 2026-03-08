# A9 Block 3 Phase 3B: GitHub Actions Integration Plan

## Overview
Update GitHub Actions workflows to capture test results and integrate with test-reporter binary for automated PR comments and daily CHANGELOG updates.

## Files to Modify

### 1. `.github/workflows/test-unit.yml` (Primary)

**New steps to add after `cargo test --lib`:**

```yaml
name: Unit Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v3
        with:
          path: ~/.cargo/registry
          key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache cargo index
        uses: actions/cache@v3
        with:
          path: ~/.cargo/git
          key: ${{ runner.os }}-cargo-git-${{ hashFiles('**/Cargo.lock') }}

      - name: Cache cargo build
        uses: actions/cache@v3
        with:
          path: target
          key: ${{ runner.os }}-cargo-build-${{ hashFiles('**/Cargo.lock') }}

      - name: Run tests with JSON output
        run: |
          cargo test --lib -- --format json > test-output.json 2>&1 || true

      - name: Generate test reports
        run: |
          mkdir -p target/test-reports
          cargo run -p claudefs-tests --bin test-reporter -- test-output.json

      - name: Upload test reports
        uses: actions/upload-artifact@v3
        with:
          name: test-reports
          path: target/test-reports/*.json
          retention-days: 30

      - name: Upload test history
        uses: actions/upload-artifact@v3
        with:
          name: test-history
          path: target/test-history.json
          retention-days: 90

      - name: Comment on PR with test results
        if: github.event_name == 'pull_request'
        uses: actions/github-script@v7
        with:
          github-token: ${{ secrets.GITHUB_TOKEN }}
          script: |
            const fs = require('fs');
            const path = require('path');

            // Read all JSON reports
            let totalPassed = 0;
            let totalFailed = 0;
            let reportMarkdown = '## ✅ Test Results\n\n';
            reportMarkdown += '| Crate | Tests | Status | Duration |\n';
            reportMarkdown += '|-------|-------|--------|----------|\n';

            const reportsDir = 'target/test-reports';
            if (fs.existsSync(reportsDir)) {
              const files = fs.readdirSync(reportsDir).filter(f => f.endsWith('.json'));

              for (const file of files.sort()) {
                const data = JSON.parse(fs.readFileSync(path.join(reportsDir, file), 'utf8'));
                const status = data.failed === 0 ? '✅' : '❌';
                totalPassed += data.passed;
                totalFailed += data.failed;
                reportMarkdown += `| ${data.crate} | ${data.total_tests} | ${status} ${data.passed}/${data.total_tests} | ${data.duration_secs.toFixed(1)}s |\n`;
              }
            }

            reportMarkdown += `\n**Total:** ${totalPassed} passed`;
            if (totalFailed > 0) {
              reportMarkdown += `, ${totalFailed} failed`;
            }
            reportMarkdown += '\n';

            // Check for flaky tests
            if (fs.existsSync('target/test-issue-body.md')) {
              const issueBody = fs.readFileSync('target/test-issue-body.md', 'utf8');
              reportMarkdown += '\n### 🐛 Flaky Tests Detected\n' + issueBody;
            }

            github.rest.issues.createComment({
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: reportMarkdown
            });

      - name: Create flaky test issue
        if: |
          github.event_name == 'push' &&
          github.ref == 'refs/heads/main' &&
          hashFiles('target/test-issue-body.md') != ''
        uses: actions/github-script@v7
        with:
          github-token: ${{ secrets.GITHUB_TOKEN }}
          script: |
            const fs = require('fs');
            const issueBody = fs.readFileSync('target/test-issue-body.md', 'utf8');

            github.rest.issues.create({
              owner: context.repo.owner,
              repo: context.repo.repo,
              title: '🐛 Flaky Test Detected',
              body: issueBody,
              labels: ['flaky-test', 'test-validation']
            });

      - name: Fail if tests failed
        if: failure()
        run: exit 1
```

### 2. `.github/workflows/test-nightly.yml` (New)

**For nightly CHANGELOG updates:**

```yaml
name: Nightly Test Metrics

on:
  schedule:
    - cron: '0 0 * * *'  # Daily at midnight UTC
  workflow_dispatch:

jobs:
  metrics:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          token: ${{ secrets.GITHUB_TOKEN }}

      - uses: dtolnay/rust-toolchain@stable

      - name: Run tests
        run: cargo test --lib -- --format json > test-output.json 2>&1 || true

      - name: Generate reports
        run: |
          mkdir -p target/test-reports
          cargo run -p claudefs-tests --bin test-reporter -- test-output.json

      - name: Update CHANGELOG
        run: |
          cargo run -p claudefs-tests --bin changelog-updater

      - name: Commit CHANGELOG
        if: |
          hashFiles('CHANGELOG.md') != ''
        run: |
          git config user.name "ClaudeFS Agent"
          git config user.email "claudefs-agent@noreply.github.com"
          git add CHANGELOG.md
          git commit -m "[A9] Update daily test metrics" || true
          git push origin main
```

### 3. `.github/workflows/test-regression.yml` (Update)

**Add to existing regression test workflow:**

```yaml
# After running regression tests, add:
      - name: Archive regression test results
        if: always()
        uses: actions/upload-artifact@v3
        with:
          name: regression-test-results
          path: |
            target/regression-results.json
            target/regression-summary.txt
```

## CI Integration Checklist

- [ ] Update test-unit.yml with test-reporter steps
- [ ] Create test-nightly.yml for CHANGELOG updates
- [ ] Update test-regression.yml for artifact uploads
- [ ] Add `changelog-updater` binary for nightly job
- [ ] Test locally: `cargo run -p claudefs-tests --bin test-reporter -- test-output.json`
- [ ] Verify PR comment generation
- [ ] Verify flaky test issue creation
- [ ] Test CHANGELOG update workflow
- [ ] Commit all workflow changes

## Expected Behavior After Integration

### On PR:
1. Tests run and generate JSON reports
2. test-reporter binary parses results
3. PR comment posted with per-crate summary
4. If flaky tests detected, issue template prepared

### On main branch (daily):
1. Nightly tests run
2. Reports generated and aggregated
3. CHANGELOG.md updated with daily entry
4. Commit pushed to main
5. If flaky tests detected, GitHub issue auto-created

### On regression tests:
1. Results uploaded to artifacts
2. Tracked for performance trends
3. Compared against baseline

## Testing the Integration Locally

```bash
# 1. Generate test output (if not already done)
cargo test --lib -- --format json > test-output.json 2>&1 || true

# 2. Test the reporter
cargo run -p claudefs-tests --bin test-reporter -- test-output.json

# 3. Check outputs
ls -la target/test-reports/
cat target/test-reports/claudefs-storage.json | jq .

# 4. View markdown output
cargo run -p claudefs-tests --bin test-reporter -- test-output.json 2>&1 | grep -A 20 "Test Results"
```

## Notes

- JSON format from cargo test is official Rust format (stable)
- Report generation is idempotent (safe to run multiple times)
- Flaky detection requires history file (auto-created on first run)
- GitHub Actions tokens are pre-configured in secrets
- All binaries are part of claudefs-tests crate
