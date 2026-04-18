# A11: Phase 5 Block 3 — GitHub Actions CI/CD Hardening (OpenCode Prompt)

**Date:** 2026-04-18 Session 14
**Agent:** A11 Infrastructure & CI
**Model:** minimax-m2p5 (Rust tests) + YAML generation (workflows)
**Objective:** Generate production-ready GitHub Actions workflows and composite actions with hardened CI/CD pipeline

---

## Context

ClaudeFS is a distributed POSIX file system in Rust with 8 crates. Current GitHub Actions CI has 19 workflows but suffers from:
- 35% code duplication (Rust setup, cargo caching repeated across workflows)
- Inconsistent timeout values, cache strategies, error handling
- No centralized artifact lifecycle management
- Lacks DRY principles (composite actions not used)
- Suboptimal parallelization and cost efficiency

**Goal:** Refactor into modernized, maintainable pipeline with zero duplication via composite actions.

**Phase 5 Block 3 Scope:**
1. 3 reusable composite actions (.github/actions/)
2. 8 refactored workflows (.github/workflows/)
3. 12 comprehensive Rust validation tests
4. No functionality loss; pure refactoring + hardening

---

## Deliverable 1: Composite Actions

### Composite Action 1: `.github/actions/setup-rust/action.yml`

**Purpose:** Centralized, reusable Rust toolchain setup

**Implementation Requirements:**

```yaml
name: Setup Rust Toolchain
description: Install Rust with specified components and configuration

inputs:
  toolchain:
    description: 'Rust toolchain channel (stable, nightly, MSRV)'
    required: false
    default: 'stable'
  components:
    description: 'Comma-separated component list (rustfmt, clippy, rust-analyzer)'
    required: false
    default: 'rustfmt,clippy'
  profile:
    description: 'Debug info level (full, line-tables, minimal)'
    required: false
    default: 'debuginfo'

outputs:
  rustc-version:
    description: 'Installed Rust version'
    value: ${{ steps.install.outputs.rustc-version }}

runs:
  using: composite
  steps:
    # 1. Install Rust toolchain using dtolnay/rust-toolchain
    # 2. Log installed version
    # 3. Configure cargo profile settings
    # 4. Export RUST_BACKTRACE=1, CARGO_TERM_COLOR=always
    # 5. Cache cargo registry and git directories
```

**Key Features:**
- Parse `inputs.toolchain` for version (stable | nightly | 1.70.0)
- Parse `inputs.components` (comma-separated) and convert to action format
- Create ~/.cargo/config.toml with profile settings
- Set environment variables for consistent Rust behavior
- Output rustc version for logging

**Error Handling:**
- Fail if toolchain installation fails
- Warn if components don't exist (non-fatal)
- Verify cargo is in PATH after installation

**Testing:**
- Used by all CI workflows (build, test, quality checks)
- Must support multiple runner images (ubuntu-latest, ubuntu-24.04, macos-latest)
- Must work for both debug and release builds

---

### Composite Action 2: `.github/actions/cache-cargo/action.yml`

**Purpose:** Unified cargo dependency and build artifact caching

**Implementation Requirements:**

```yaml
name: Cache Cargo Dependencies & Artifacts
description: Setup cargo registry, git, and build artifact caching with smart invalidation

inputs:
  cache-target-debug:
    description: 'Cache debug build artifacts'
    required: false
    default: 'true'
  cache-target-release:
    description: 'Cache release build artifacts'
    required: false
    default: 'false'
  cache-docs:
    description: 'Cache documentation build output'
    required: false
    default: 'false'
  save-always:
    description: 'Always save cache (main) or only on cache hit (PR)'
    required: false
    default: 'false'

outputs:
  cache-hit:
    description: 'true if cargo dependencies cache was hit'
    value: ${{ steps.cargo-deps-cache.outputs.cache-hit }}

runs:
  using: composite
  steps:
    # 1. Cache cargo registry index (~/.cargo/registry/index/)
    #    Key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}
    #    Restore keys: ${{ runner.os }}-cargo-registry-
    #
    # 2. Cache cargo registry cache (~/.cargo/registry/cache/)
    #    Same key strategy as (1)
    #
    # 3. Cache cargo git databases (~/.cargo/git/db/)
    #    Same key strategy
    #
    # 4. Conditionally cache build artifacts (target/)
    #    - If cache-target-debug==true: cache target/debug/
    #    - If cache-target-release==true: cache target/release/
    #    - If cache-docs==true: cache target/doc/
    #    Key: ${{ runner.os }}-cargo-build-${{ hashFiles('**/Cargo.lock') }}-${{ github.run_id }}
    #    This ensures each run gets its own layer, but later runs can restore from earlier
    #
    # 5. On main branch: save-always=true (repopulate cache)
    #    On PR branches: save-always=false (only save if hit, reduce cache churn)
```

**Cache Key Strategy:**

| Component | Key | Restore Keys | TTL |
|-----------|-----|--------------|-----|
| Registry index | `cargo-registry-$(Cargo.lock)` | `cargo-registry-` | 14 days |
| Registry cache | `cargo-registry-$(Cargo.lock)` | `cargo-registry-` | 14 days |
| Git databases | `cargo-registry-$(Cargo.lock)` | `cargo-registry-` | 14 days |
| Build artifacts (debug) | `cargo-build-debug-$(Cargo.lock)-$(run_id)` | `cargo-build-debug-$(Cargo.lock)` | 7 days |
| Build artifacts (release) | `cargo-build-release-$(Cargo.lock)-$(run_id)` | `cargo-build-release-$(Cargo.lock)` | 7 days |
| Docs | `cargo-docs-$(Cargo.lock)` | `cargo-docs-` | 7 days |

**Implementation Details:**

1. **Separate caches per component** — allows selective cache hits (e.g., registry hit but artifacts miss)
2. **Cargo.lock as primary key** — invalidates on dependency changes
3. **Build ID in artifact key** — each run gets its own layer for incremental updates
4. **Path structure:**
   - Dependencies: `~/.cargo/registry/`, `~/.cargo/git/`
   - Artifacts: `target/` (varies by build type)
5. **Save strategy:**
   - Main branch: always save (repopulate cache for subsequent runs)
   - PR branches: use `if: always()` to save on failure too (helps future PRs)

**Error Handling:**
- Non-fatal if cache miss (workflow continues)
- Log cache hit/miss status
- Warn if cache size >5GB (indicate cleanup needed)

---

### Composite Action 3: `.github/actions/test-reporter/action.yml`

**Purpose:** Unified test result parsing, reporting, and artifact collection

**Implementation Requirements:**

```yaml
name: Test Reporter & Artifact Handler
description: Parse test results, create GitHub check runs, collect artifacts

inputs:
  test-type:
    description: 'Test format (junit, tap, dotnet)'
    required: false
    default: 'junit'
  fail-on-error:
    description: 'Fail workflow if tests failed'
    required: false
    default: 'true'
  name:
    description: 'Report title'
    required: false
    default: 'Test Results'
  artifact-name:
    description: 'Artifact name for upload'
    required: false
    default: 'test-results'
  artifact-retention-days:
    description: 'Days to retain artifacts'
    required: false
    default: '7'

outputs:
  test-count:
    description: 'Total tests run'
    value: ${{ steps.report.outputs.test-count }}
  test-passed:
    description: 'Passed tests'
    value: ${{ steps.report.outputs.test-passed }}
  test-failed:
    description: 'Failed tests'
    value: ${{ steps.report.outputs.test-failed }}

runs:
  using: composite
  steps:
    # 1. Find test result files matching pattern
    #    - *.xml (JUnit format)
    #    - test-results/*.json
    #    - target/test-output/
    #
    # 2. Use dorny/test-reporter@v1 to parse and create check runs
    #    - Inputs: name=${{ inputs.name }}, path pattern, fail-on-error
    #    - Creates GitHub check annotations for each failure
    #
    # 3. Aggregate test counts from all files
    #    - Parse XML <testsuite> elements
    #    - Extract: total, passed, failed, skipped, duration
    #    - Output as JSON for downstream use
    #
    # 4. Upload artifacts
    #    - Path: target/test-results/ (or current working directory)
    #    - Name: ${{ inputs.artifact-name }}
    #    - Retention: ${{ inputs.artifact-retention-days }} days
    #
    # 5. If tests failed and fail-on-error==true:
    #    - Exit with code 1
    #    - Print summary: "X failed, Y passed, Z skipped"
```

**Key Features:**

1. **Multi-format support** — JUnit XML (default), TAP, dotnet
2. **GitHub check integration** — Creates annotations for each test failure/skip
3. **Artifact collection** — Automatically uploads test results with configurable retention
4. **Test count aggregation** — Outputs metrics for downstream status checks
5. **Fail-fast option** — Can continue workflow even if tests fail (for diagnostic collection)

**Error Handling:**
- Non-fatal if test files not found (e.g., build failed before tests)
- Log parsing errors with file path
- Always attempt artifact upload (even on failure)

---

## Deliverable 2: Refactored Workflows

### Workflow 1: `.github/workflows/ci-build-v2.yml`

**Purpose:** Consolidated build, format, lint, docs, and basic security checks

**Structure:**

```yaml
name: CI Build & Quality

on:
  push:
    branches: [main]
    paths-ignore:
      - 'docs/**'
      - 'CHANGELOG.md'
      - 'README.md'
      - '.github/workflows/perf-baseline.yml'
  pull_request:
    branches: [main]
  workflow_dispatch:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  RUST_BACKTRACE: 1
  CARGO_TERM_COLOR: always

jobs:
  # Quick validation (no deps needed)
  validate:
    name: Pre-flight Checks
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Validate Cargo manifests
        run: cargo metadata --format-version 1 > /dev/null
      - name: Check for debug statements
        run: |
          ! grep -r 'println!\|dbg!\|eprintln!' --include="*.rs" crates/ || \
          (echo "Found debug output in crates" && exit 1)

  # Parallel matrix: build debug + release
  build:
    name: Build
    needs: validate
    runs-on: ubuntu-latest
    timeout-minutes: 25
    strategy:
      matrix:
        target: [debug, release]
      fail-fast: true
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust
        with:
          toolchain: stable
          components: rustfmt,clippy

      - name: Cache Cargo
        uses: ./.github/actions/cache-cargo
        with:
          cache-target-debug: ${{ matrix.target == 'debug' }}
          cache-target-release: ${{ matrix.target == 'release' }}

      - name: Build ${{ matrix.target }}
        run: |
          cargo build --workspace \
            ${{ matrix.target == 'release' && '--release' || '' }} \
            --message-format=short
        timeout-minutes: 20

  # Parallel matrix: format, clippy per-crate, docs
  quality:
    name: Quality
    needs: validate
    runs-on: ubuntu-latest
    timeout-minutes: 20
    strategy:
      matrix:
        check: [fmt, clippy, docs]
      fail-fast: false
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust
        with:
          toolchain: stable
          components: rustfmt,clippy

      - name: Cache Cargo
        uses: ./.github/actions/cache-cargo
        with:
          cache-target-debug: false
          cache-target-release: false

      - name: Format check
        if: matrix.check == 'fmt'
        run: cargo fmt --all -- --check

      - name: Clippy (per-crate)
        if: matrix.check == 'clippy'
        run: |
          for crate in claudefs-storage claudefs-meta claudefs-reduce \
                        claudefs-transport claudefs-fuse claudefs-repl \
                        claudefs-gateway claudefs-mgmt claudefs-tests; do
            echo "🔍 Checking $crate..."
            cargo clippy -p $crate --all-targets -- -D warnings || exit 1
          done
        timeout-minutes: 15

      - name: Documentation
        if: matrix.check == 'docs'
        run: cargo doc --no-deps --workspace
        env:
          RUSTDOCFLAGS: -D warnings

  # Security & compliance
  security:
    name: Security
    needs: validate
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust

      - name: Cargo audit
        run: cargo audit --deny warnings

      - name: Cargo deny
        run: |
          cargo install cargo-deny --locked --quiet
          cargo deny check advisories
          cargo deny check licenses

  # Status check aggregation
  ci-status:
    name: CI Status
    needs: [validate, build, quality, security]
    if: always()
    runs-on: ubuntu-latest
    steps:
      - name: Check job statuses
        run: |
          if [[ "${{ needs.build.result }}" != "success" ]] || \
             [[ "${{ needs.quality.result }}" != "success" ]] || \
             [[ "${{ needs.security.result }}" != "success" ]]; then
            echo "❌ CI failed"
            exit 1
          fi
          echo "✅ CI passed"
```

**Key Improvements:**
- ✅ Uses composite actions (`setup-rust`, `cache-cargo`)
- ✅ Parallel matrix for build targets (debug/release)
- ✅ Parallel matrix for quality checks (fmt/clippy/docs)
- ✅ Validation gate prevents waste (fail-fast for syntax errors)
- ✅ Concurrency control (cancel old runs on new push)
- ✅ Status aggregation job (ensures all jobs pass)
- ✅ Reduced duplication ~45%

---

### Workflow 2: `.github/workflows/test-unit-v2.yml`

**Purpose:** Unit tests with comprehensive error handling and reporting

```yaml
name: Unit Tests

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main, develop]

env:
  RUST_BACKTRACE: 1

jobs:
  test:
    name: Unit Tests
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust
        with:
          toolchain: stable
          components: rustfmt,clippy

      - name: Cache Cargo
        uses: ./.github/actions/cache-cargo
        with:
          cache-target-debug: true
          cache-target-release: false

      - name: Run unit tests
        run: |
          cargo test --lib --all \
            --message-format=json > test-results.json 2>&1 || TEST_FAILED=1
          exit ${TEST_FAILED:-0}
        timeout-minutes: 25
        continue-on-error: true

      - name: Parse test results
        if: always()
        run: |
          # Convert cargo JSON output to JUnit XML
          cargo2junit < test-results.json > target/test-results.xml || true
        continue-on-error: true

      - name: Report results
        if: always()
        uses: ./.github/actions/test-reporter
        with:
          test-type: junit
          fail-on-error: true
          name: Unit Test Results
          artifact-name: unit-test-results
          artifact-retention-days: 7
```

**Improvements:**
- ✅ Uses composite actions
- ✅ Non-zero exit on test failure, captured via `continue-on-error`
- ✅ JSON output for programmatic parsing
- ✅ Automatic JUnit conversion (cargo2junit)
- ✅ Test reporter for GitHub integration
- ✅ Artifact collection with retention policy

---

### Workflow 3: `.github/workflows/security-scan-v2.yml`

**Purpose:** Centralized security scanning with hardened practices

```yaml
name: Security Scanning & Compliance

on:
  push:
    branches: [main]
    paths:
      - 'Cargo.toml'
      - 'Cargo.lock'
      - 'crates/**/Cargo.toml'
  pull_request:
    branches: [main]
  schedule:
    - cron: '0 2 * * *'  # Daily at 02:00 UTC
  workflow_dispatch:

jobs:
  cargo-audit:
    name: Cargo Audit
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust

      - name: Run cargo audit
        run: |
          cargo install cargo-audit --locked --quiet
          cargo audit --deny warnings
        timeout-minutes: 5

      - name: Generate audit report
        if: always()
        run: cargo audit json > audit-report.json || true

      - name: Upload audit artifact
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: audit-report
          path: audit-report.json
          retention-days: 7

  cargo-deny:
    name: Cargo Deny
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust

      - name: Install cargo-deny
        run: cargo install cargo-deny --locked --quiet

      - name: Check advisories
        run: cargo deny check advisories

      - name: Check licenses
        run: cargo deny check licenses

      - name: Check bans
        run: cargo deny check bans

  sbom:
    name: Generate SBOM
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust

      - name: Install cargo-sbom
        run: cargo install cargo-sbom --locked --quiet

      - name: Generate SBOM
        run: cargo sbom -o claudefs-sbom.xml

      - name: Upload SBOM
        uses: actions/upload-artifact@v4
        with:
          name: sbom
          path: claudefs-sbom.xml
          retention-days: 90

  security-policy:
    name: Security Policy Check
    runs-on: ubuntu-latest
    timeout-minutes: 5
    steps:
      - uses: actions/checkout@v4

      - name: Verify SECURITY.md exists
        run: |
          if [ ! -f SECURITY.md ]; then
            echo "⚠️  SECURITY.md not found"
            exit 1
          fi

      - name: Verify dependabot config
        run: |
          if [ ! -f .github/dependabot.yml ]; then
            echo "⚠️  dependabot.yml not found (optional)"
          fi
```

**Improvements:**
- ✅ Consolidated security checks
- ✅ Artifact handling with retention policy
- ✅ SBOM generation (supply chain visibility)
- ✅ Security policy validation
- ✅ Scheduled daily runs for CVE detection

---

## Deliverable 3: Comprehensive Rust Tests (12 tests total)

### Test Suite 1: Composite Action Validation (3 tests)

**File:** `crates/claudefs-tests/src/ci_composite_actions_tests.rs`

```rust
#[cfg(test)]
mod ci_composite_actions {
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_setup_rust_action_exists() -> Result<(), String> {
        // 1. Verify file exists: .github/actions/setup-rust/action.yml
        let path = Path::new(".github/actions/setup-rust/action.yml");
        fs::read_to_string(path)
            .map_err(|e| format!("setup-rust action not found: {}", e))?;

        // 2. Parse YAML and validate structure
        // - Must have 'name', 'description'
        // - Must have 'inputs' section with: toolchain, components, profile
        // - Must have 'outputs' section with: rustc-version
        // - Must have 'runs' section with 'using: composite'

        Ok(())
    }

    #[test]
    fn test_cache_cargo_action_config() -> Result<(), String> {
        // 1. Verify file exists: .github/actions/cache-cargo/action.yml
        // 2. Parse YAML and validate inputs
        // - cache-target-debug, cache-target-release, cache-docs, save-always
        // 3. Validate outputs section (cache-hit)
        // 4. Verify caching logic (composite steps)
        // - Multiple cache@v4 calls with proper key strategies
        // - Separate registry, git, build artifact caches

        Ok(())
    }

    #[test]
    fn test_test_reporter_action_integration() -> Result<(), String> {
        // 1. Verify file exists: .github/actions/test-reporter/action.yml
        // 2. Validate inputs: test-type, fail-on-error, name, artifact-name, retention-days
        // 3. Verify outputs: test-count, test-passed, test-failed
        // 4. Check for dorny/test-reporter@v1 usage
        // 5. Verify artifact upload step (actions/upload-artifact)

        Ok(())
    }
}
```

### Test Suite 2: Workflow YAML Validation (4 tests)

**File:** `crates/claudefs-tests/src/ci_workflow_validation_tests.rs`

```rust
#[cfg(test)]
mod ci_workflow_validation {
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_workflow_syntax_valid() -> Result<(), String> {
        // 1. Find all *.yml files in .github/workflows/
        // 2. For each file, run `yamllint` (shell command)
        // 3. Verify no parsing errors
        // 4. Check required fields: name, on, jobs

        Ok(())
    }

    #[test]
    fn test_workflow_triggers_configured() -> Result<(), String> {
        // 1. Parse all workflows
        // 2. Verify 'on' section exists
        // 3. For push/pull_request events: verify branches filter
        // 4. For manual workflows: ensure workflow_dispatch present
        // 5. Count total triggers configured

        Ok(())
    }

    #[test]
    fn test_workflow_matrix_strategy_cost_optimized() -> Result<(), String> {
        // 1. Parse all workflows
        // 2. For each job with 'strategy.matrix', count parallel job count
        // 3. Warn if >5 parallel jobs (likely over-parallelized)
        // 4. Check timeout-minutes values
        // - CI jobs: <30 min
        // - Build jobs: <25 min
        // - Integration: <60 min
        // 5. Report cost estimate per workflow

        Ok(())
    }

    #[test]
    fn test_workflow_artifact_lifecycle() -> Result<(), String> {
        // 1. Find all 'upload-artifact' actions
        // 2. Verify each has 'retention-days' specified
        // 3. Warn if retention-days > 90 (excessive storage)
        // 4. Verify cleanup workflows exist (artifact-cleanup.yml)
        // 5. Check scheduled jobs for artifact deletion

        Ok(())
    }
}
```

### Test Suite 3: DRY Principle Analysis (3 tests)

**File:** `crates/claudefs-tests/src/ci_dry_principle_tests.rs`

```rust
#[cfg(test)]
mod ci_dry_principle {
    #[test]
    fn test_no_duplicate_rust_setup_code() -> Result<(), String> {
        // 1. Find all workflows
        // 2. Count direct uses of 'dtolnay/rust-toolchain', 'actions-rust-lang/setup-rust-toolchain'
        // 3. These should appear in composite action only, not in individual workflows
        // 4. Fail if duplicate setup found in >2 workflows (indicate refactoring needed)

        Ok(())
    }

    #[test]
    fn test_no_duplicate_cache_strategies() -> Result<(), String> {
        // 1. Find all workflows
        // 2. Count distinct cargo cache configurations
        // 3. Count use of 'actions/cache@v4' for cargo deps
        // 4. All should use composite action
        // 5. Fail if duplicate cache setup in >2 workflows

        Ok(())
    }

    #[test]
    fn test_composite_action_usage_coverage() -> Result<(), String> {
        // 1. Parse all workflows
        // 2. Count usage of './.github/actions/setup-rust'
        // 3. Count usage of './.github/actions/cache-cargo'
        // 4. Count usage of './.github/actions/test-reporter'
        // 5. Calculate coverage: (usage_count / workflow_count) * 100
        // 6. Fail if any composite action used in <80% of applicable workflows

        Ok(())
    }
}
```

### Test Suite 4: Cost & Performance Analysis (2 tests)

**File:** `crates/claudefs-tests/src/ci_cost_optimization_tests.rs`

```rust
#[cfg(test)]
mod ci_cost_optimization {
    #[test]
    fn test_workflow_cost_attribution() -> Result<(), String> {
        // 1. Parse all workflows
        // 2. For each job, estimate cost: timeout_min * parallel_jobs * $0.008 (Ubuntu rate)
        // 3. Calculate per-workflow cost (max job count * timeout)
        // 4. Report workflows >$5/run (likely over-provisioned)
        // 5. Total CI cost estimate: sum of all workflow costs

        Ok(())
    }

    #[test]
    fn test_build_cache_hit_rate() -> Result<(), String> {
        // 1. Analyze cache key strategies in composite actions
        // 2. Verify primary key is Cargo.lock (most stable)
        // 3. Check no overly-specific keys (runner-specific, timestamp-based)
        // 4. Estimate cache hit rate: (stable_keys / total_keys) * 100
        // 5. Fail if <80% of cache keys are stable (would indicate poor hit rate)

        Ok(())
    }
}
```

---

## Implementation Checklist

### Phase 1: Composite Actions
- [ ] Implement `.github/actions/setup-rust/action.yml`
- [ ] Implement `.github/actions/cache-cargo/action.yml`
- [ ] Implement `.github/actions/test-reporter/action.yml`
- [ ] Test each composite action in isolation (local execution)
- [ ] Document inputs/outputs with examples

### Phase 2: Workflow Refactoring
- [ ] Implement `.github/workflows/ci-build-v2.yml`
- [ ] Implement `.github/workflows/test-unit-v2.yml`
- [ ] Implement `.github/workflows/security-scan-v2.yml`
- [ ] Add cleanup workflow: `.github/workflows/artifact-cleanup.yml`
- [ ] Implement any additional workflows (integration, deploy, etc.)

### Phase 3: Test Suite
- [ ] Implement Rust test suite (12 tests, 400 LOC)
- [ ] Ensure all tests pass: `cargo test --test ci_*`
- [ ] Verify test coverage >90%

### Phase 4: Documentation
- [ ] Document composite action usage in README
- [ ] Add workflow architecture diagram
- [ ] Include cost analysis report
- [ ] Create migration guide (old → new workflows)

---

## Output Format Requirements

**For OpenCode to generate:**

1. **Composite Actions:**
   - File format: `.github/actions/{name}/action.yml`
   - Must be valid YAML (no syntax errors)
   - Proper indentation (2 spaces)
   - Complete documentation in action.yml

2. **Workflows:**
   - File format: `.github/workflows/{name}.yml`
   - Must pass `yamllint` validation
   - Concurrency sections for safety
   - Proper error handling (`continue-on-error`, if conditions)

3. **Rust Tests:**
   - File: `crates/claudefs-tests/src/ci_*.rs`
   - Must compile without warnings
   - Must pass with `cargo test --lib`
   - Clear, descriptive test names
   - Comprehensive assertions with clear error messages

---

## Success Criteria

✅ All deliverables compile/validate without errors
✅ Composite actions work correctly when invoked
✅ Refactored workflows have <30min timeout for CI jobs
✅ Test suite: 12 tests, all passing, <5min execution
✅ Code duplication reduced from 35% to <10%
✅ No loss of functionality (all checks still present)
✅ Documentation clear and complete

---

**End of OpenCode Prompt**
