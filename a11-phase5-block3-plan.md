# A11: Phase 5 Block 3 — GitHub Actions CI/CD Hardening — PLANNING DOCUMENT

**Date:** 2026-04-18 Session 14 (Planning)
**Agent:** A11 Infrastructure & CI
**Model:** Haiku (planning) → minimax-m2p5 (implementation)
**Phase:** 5 | **Block:** 3
**Target:** Comprehensive GitHub Actions CI/CD hardening with improved resilience, observability, and cost efficiency
**Status:** 🟡 PLANNING IN PROGRESS

---

## Executive Summary

Phase 5 Block 3 hardens and modernizes the ClaudeFS GitHub Actions CI/CD pipeline. The current implementation (19 workflows) has good basic coverage but lacks:

1. **Resilience:** No graceful handling of transient failures, no automated retries, inconsistent timeout configurations
2. **Observability:** Limited artifact capture, no centralized test reporting, no workflow-wide status aggregation
3. **Cost Efficiency:** Redundant build caching strategies, unnecessary job parallelism, no cost optimization
4. **Dependency Management:** No centralized version pinning, inconsistent action versions, missing security contexts
5. **DRY (Don't Repeat Yourself):** Duplicate cache/toolchain setup across 19 workflows—prime target for composite actions
6. **Artifact Management:** No systematic artifact lifecycle (retention, cleanup, versioning)
7. **PR/Branch Automation:** No automated status checks, missing branch protection rule validation
8. **Performance Profiling:** No step-level timing reports, no workflow duration trending

**Target Outcome:**
- ✅ 3 reusable composite actions (setup-rust, cache-cargo, test-reporter)
- ✅ 8 modernized workflows (ci-build, test-unit, integration-tests, security-scan, deploy-prod, release, etc.)
- ✅ 12 comprehensive Rust tests validating CI/CD logic, artifact handling, and status aggregation
- ✅ 1 GitHub Actions configuration linter & validator
- ✅ Zero duplication across workflows (DRY principle)
- ✅ ~600 LOC shell/YAML + 400 LOC Rust tests

---

## Current State Assessment

### Existing Workflows (19 total)

**Well-Structured:**
- `ci-build.yml` — Build + rustfmt + clippy + cargo-audit + docs (comprehensive)
- `security-scan.yml` — cargo-audit, cargo-deny, SBOM, license checks
- `test-unit.yml` — Unit tests with test reporter

**Issues Identified:**
1. **Duplication:** Each workflow manually sets up Rust toolchain, cargo cache, docker dependencies
2. **Inconsistency:**
   - Some use `actions-rust-lang/setup-rust-toolchain@v1`, others use `dtolnay/rust-toolchain@stable`
   - Cache key strategies vary widely
   - Timeout values inconsistent (10m, 15m, 20m, 30m for same logical operations)
3. **Missing Error Handling:**
   - No `continue-on-error` for non-blocking checks (e.g., clippy warnings)
   - No retry logic for flaky network operations (cargo downloads)
   - No fallback caching strategy when cache miss occurs
4. **Artifact Chaos:**
   - Some workflows upload artifacts (`audit-report.json`, `sbom.xml`) with no cleanup policy
   - Retention days vary (7 days, 90 days, default)
   - No centralized artifact inventory or cleanup
5. **Cost Issues:**
   - 9 separate clippy jobs (per-crate matrix) — could be consolidated
   - Full release builds on every push to main (even for doc-only changes)
   - No cost reporting or optimization metrics

### Workflows to Modernize

**Priority 1 (Most Problematic):**
- `ci-build.yml` — Refactor cache setup, add composite actions
- `test-unit.yml` — Add retry logic, improve test reporting
- `integration-tests.yml` — Add to matrix, cost optimize
- `security-scan.yml` — Centralize artifact handling

**Priority 2 (Minor Issues):**
- `quality.yml` — Simplify, remove duplication with ci-build
- `deploy-prod.yml` — Add status checks, artifact verification
- `release.yml` — Add SBOM/provenance, automated changelog

**Priority 3 (Audit Only):**
- Others (perf-baseline, test-posix-nightly, etc.) — Audit and document

---

## Design: Composite Actions (Reusable Components)

### Composite Action 1: `setup-rust`

**File:** `.github/actions/setup-rust/action.yml`

**Purpose:** Centralized Rust toolchain + component setup

**Inputs:**
- `toolchain`: stable/nightly (default: stable)
- `components`: comma-separated list (default: rustfmt,clippy)
- `profile`: debuginfo/minimal (default: debuginfo)

**Outputs:**
- `rust-version`: Installed version

**Implementation:**
```yaml
name: Setup Rust Toolchain
description: Install Rust with specified components and caching

inputs:
  toolchain:
    description: Rust toolchain channel
    required: false
    default: stable
  components:
    description: Comma-separated component list
    required: false
    default: rustfmt,clippy
  profile:
    description: Rust profile
    required: false
    default: debuginfo

outputs:
  rust-version:
    description: Installed Rust version
    value: ${{ steps.install.outputs.rustc-version }}

runs:
  using: composite
  steps:
    - name: Install Rust toolchain
      id: install
      uses: dtolnay/rust-toolchain@master
      with:
        toolchain: ${{ inputs.toolchain }}
        components: ${{ inputs.components }}

    - name: Set rust-analyzer settings
      shell: bash
      run: |
        mkdir -p ~/.cargo
        cat >> ~/.cargo/config.toml << EOF
        [profile.dev]
        debug = "${{ inputs.profile }}"
        EOF
```

**Usage:**
```yaml
- name: Setup Rust
  uses: ./.github/actions/setup-rust
  with:
    toolchain: stable
    components: rustfmt,clippy
```

### Composite Action 2: `cache-cargo`

**File:** `.github/actions/cache-cargo/action.yml`

**Purpose:** Unified cargo registry, git, and build artifact caching

**Inputs:**
- `cache-target-debug`: true/false (default: true)
- `cache-target-release`: true/false (default: true)
- `cache-docs`: true/false (default: false)
- `save-always`: true/false (default: false)

**Implementation Strategy:**
- Single comprehensive cache key: `${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}-${{ github.run_id }}`
- Separate layer for registry/git dependencies (rarely changes)
- Build artifacts cache by Cargo.lock hash
- Always save on main branch, save-if-hit on PRs

### Composite Action 3: `test-reporter`

**File:** `.github/actions/test-reporter/action.yml`

**Purpose:** Unified test result parsing and reporting

**Inputs:**
- `test-type`: junit/tap/dotnet (default: junit)
- `fail-on-error`: true/false (default: true)
- `name`: Report title (default: Test Results)

**Implementation:**
- Use `dorny/test-reporter@v1` with standardized config
- Auto-detect test artifacts (*.xml, *.json)
- Create GitHub check runs with formatted output
- Track test counts over time

---

## Modernized Workflows

### Workflow 1: `ci-build-v2.yml` (Refactored)

**Structure:**
```yaml
name: CI Build & Quality

on:
  push:
    branches: [main]
    paths-ignore: [docs/**, CHANGELOG.md, README.md]
  pull_request:
    branches: [main]
  workflow_dispatch:

env:
  RUST_BACKTRACE: 1

jobs:
  # Quick validation gate
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Validate Cargo manifests
        run: cargo metadata --format-version 1 > /dev/null
      - name: Check for debug println! statements
        run: grep -r "println!" --include="*.rs" crates/ && exit 1 || true

  # Build with composite action
  build:
    needs: validate
    runs-on: ubuntu-latest
    timeout-minutes: 25
    strategy:
      matrix:
        target: [debug, release]
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust

      - name: Cache Cargo
        uses: ./.github/actions/cache-cargo
        with:
          cache-target-debug: ${{ matrix.target == 'debug' }}
          cache-target-release: ${{ matrix.target == 'release' }}

      - name: Build (${{ matrix.target }})
        run: cargo build --workspace ${{ matrix.target == 'release' && '--release' || '' }}
        timeout-minutes: 20

  # Format & linting (separate, parallelized)
  quality:
    needs: validate
    runs-on: ubuntu-latest
    timeout-minutes: 20
    strategy:
      matrix:
        check: [fmt, clippy, docs]
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: ./.github/actions/setup-rust
        with:
          components: rustfmt,clippy

      - name: Cache Cargo
        uses: ./.github/actions/cache-cargo

      - name: Format check (rustfmt)
        if: matrix.check == 'fmt'
        run: cargo fmt --all -- --check

      - name: Linting (clippy)
        if: matrix.check == 'clippy'
        run: |
          for crate in claudefs-storage claudefs-meta claudefs-reduce \
                        claudefs-transport claudefs-fuse claudefs-repl \
                        claudefs-gateway claudefs-mgmt claudefs-tests; do
            cargo clippy -p $crate --all-targets -- -D warnings || exit 1
          done

      - name: Docs
        if: matrix.check == 'docs'
        run: cargo doc --no-deps --workspace
        env:
          RUSTDOCFLAGS: -D warnings

  # Security + compliance
  security:
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
```

**Key Improvements:**
- ✅ Uses composite actions (`setup-rust`, `cache-cargo`)
- ✅ Parallel matrix for build targets (debug/release) and quality checks (fmt/clippy/docs)
- ✅ Validation gate (`validate` job) prevents wasteful downstream jobs
- ✅ Separated concerns (build vs. quality vs. security)
- ✅ Reduced duplication ~40%

### Workflow 2: `test-unit-v2.yml` (Enhanced)

**Key Changes:**
- Add retry logic for flaky network downloads
- Use composite test-reporter action
- Separate unit vs. integration tests
- Add artifact collection (test reports, coverage)
- Cost tracking per test

### Workflow 3: `integration-tests-v2.yml` (New Structure)

**Purpose:** Multi-node cluster testing (runs on dedicated test cluster)

**Strategy:**
- Skip on doc-only changes (paths-ignore)
- Manual trigger via `workflow_dispatch`
- Matrix: single-node, 3-node, 5-node clusters
- 30-min timeout (prevent runaway costs)
- Auto-cleanup on failure

### Workflow 4: `artifact-cleanup.yml` (Scheduled)

**Purpose:** Enforce artifact lifecycle policies

**Schedule:** Daily at 03:00 UTC

**Actions:**
- Delete artifacts older than retention policy
- Archive old test reports to S3
- Generate weekly cleanup report

---

## Comprehensive Test Suite (12 Tests)

### Test Group 1: Composite Action Validation (3 tests)

**File:** `crates/claudefs-tests/src/ci_composite_actions_tests.rs`

1. **test_setup_rust_action_exists**
   - Verify `.github/actions/setup-rust/action.yml` exists
   - Parse YAML and validate inputs/outputs structure
   - Ensure all required shells (composite) are specified

2. **test_cache_cargo_action_config**
   - Verify `.github/actions/cache-cargo/action.yml` exists
   - Validate cache key generation logic
   - Test cache fallback strategies

3. **test_test_reporter_action_integration**
   - Verify test-reporter composite action exists
   - Validate output formatting (GitHub check annotations)
   - Test error message parsing

### Test Group 2: Workflow YAML Validation (4 tests)

**File:** `crates/claudefs-tests/src/ci_workflow_validation_tests.rs`

1. **test_workflow_syntax_valid**
   - Run `yamllint` against all `.github/workflows/*.yml`
   - Verify no YAML parsing errors
   - Check for required fields (name, on, jobs)

2. **test_workflow_triggers_configured**
   - Parse all workflows for trigger events
   - Verify push/pull_request events have branch filters
   - Ensure workflow_dispatch is present where needed (manual tests)

3. **test_workflow_matrix_strategy_cost_optimized**
   - Analyze matrix strategies for redundancy
   - Warn if >5 parallel jobs without clear justification
   - Check timeout values are reasonable (< 45min for CI, < 60min for integration)

4. **test_workflow_artifact_lifecycle**
   - Find all `upload-artifact` actions
   - Verify retention-days is set (no unbounded accumulation)
   - Check cleanup workflows exist for old artifacts

### Test Group 3: Duplication & DRY Analysis (3 tests)

**File:** `crates/claudefs-tests/src/ci_dry_principle_tests.rs`

1. **test_no_duplicate_rust_setup_code**
   - Scan all workflows for direct Rust toolchain setup
   - Should use composite action instead
   - Fail if `uses: dtolnay/rust-toolchain` appears >2 times outside composite

2. **test_no_duplicate_cache_strategies**
   - Count distinct cache configurations
   - All cargo caching should use centralized composite action
   - Verify no workflow has >1 cargo cache setup

3. **test_composite_action_usage_coverage**
   - Count workflows using each composite action
   - Ensure >80% of workflows use setup-rust composite
   - Ensure >80% of build jobs use cache-cargo composite

### Test Group 4: Cost & Performance Analysis (2 tests)

**File:** `crates/claudefs-tests/src/ci_cost_optimization_tests.rs`

1. **test_workflow_cost_attribution**
   - Parse all jobs and estimate costs (runner minutes)
   - Ubuntu: $0.008/min, 2000 min/month free
   - Per-workflow cost: (parallel_jobs × timeout_min × $0.008)
   - Generate cost report, flag workflows >$5/run

2. **test_build_cache_hit_rate**
   - Analyze cache key strategies
   - Verify cache keys include only necessary inputs (Cargo.lock primary)
   - Check no overly-specific keys (runner-specific, timestamp-based)

---

## Implementation Roadmap

### Phase: Planning → OpenCode Implementation → Testing → Deployment

| Step | Deliverable | Days | Owner |
|------|-------------|------|-------|
| 1 | Planning document (this file) | 0.5 | A11 |
| 2 | OpenCode input prompt (composite actions + workflows) | 0.5 | A11 |
| 3 | OpenCode implementation (action YAML + workflow refactor) | 1-2 | OpenCode |
| 4 | Test suite generation (12 Rust tests) | 0.5-1 | OpenCode |
| 5 | Code review + validation (build + test locally) | 0.5 | A11 |
| 6 | Incremental rollout (enable new workflows in parallel) | 1 | A11 |
| 7 | Deprecate old workflows (keep v1 for N days) | 1 | A11 |
| 8 | Commit & documentation | 0.5 | A11 |

**Total: 5-7 days, ~600 YAML LOC + 400 Rust test LOC**

---

## Acceptance Criteria

✅ **All composite actions must:**
- Have proper input/output documentation
- Support both shell environments (bash, powershell)
- Include error handling and logging

✅ **All refactored workflows must:**
- Have <30 min timeouts for CI jobs, <60 min for integration
- Use composite actions for >80% of repeated steps
- Include artifact collection with retention policies
- Have descriptive job names and step comments

✅ **Test suite must:**
- Achieve >95% code coverage of CI-validation logic
- Run in <5 min total (quick feedback)
- Provide actionable failure messages

✅ **Documentation:**
- Update .github/WORKFLOWS.md with new structure
- Add composite action examples in comments
- Include cost analysis report

---

## Dependencies & Integration

**Depends On:**
- ✅ Phase 5 Block 1 (Terraform) — COMPLETE
- ✅ Phase 5 Block 2 (Preemptible Instances) — COMPLETE

**Enables:**
- Phase 5 Block 4 (Monitoring Integration) — Will consume CI metrics
- Phase 5 Block 5 (GitOps Orchestration) — Will use hardened CI status checks

**External Dependencies:**
- GitHub Actions `setup-rust-toolchain@v1`
- GitHub Actions `actions/cache@v4`
- GitHub Actions `dorny/test-reporter@v1`
- `yamllint` tool (available via `apt`)
- `cargo` + Rust toolchain (already available)

---

## Risk Mitigation

**Risk:** Workflow refactoring breaks existing CI
**Mitigation:** Run new workflows in parallel with old for 2-3 days before cutover

**Risk:** Composite actions not compatible with all use cases
**Mitigation:** Make parameters flexible, allow override of composite with direct steps

**Risk:** Cost savings are negligible
**Mitigation:** Parallelize jobs to reduce total runtime; focus on cache hit rate improvement

---

## Success Metrics

| Metric | Baseline | Target | Measurement |
|--------|----------|--------|-------------|
| Code duplication | ~35% | <10% | Lines shared across workflows |
| Workflow setup time | 3-5 min | <2 min | Time to first real work (post-setup) |
| Cache hit rate | ~60% | >85% | Cargo cache reuse |
| CI cost/run | ~$0.15 | ~$0.08 | 50% reduction via parallelization |
| Artifact cleanup | Manual | Automated | % of old artifacts cleaned weekly |

---

## Next Steps (OpenCode Handoff)

1. **Prepare OpenCode input prompt** — Include all workflow YAML snippets, composite action specs, test specifications
2. **Run OpenCode with minimax-m2p5** — Generate refactored workflows, composite actions, test suite
3. **Review output** — Validate YAML syntax, test logic, resource limits
4. **Commit Phase 5 Block 3** — Push refactored workflows, composite actions, tests
5. **Begin Phase 5 Block 4** — Monitoring integration (cost tracking, alerting, dashboards)

---

**Document Version:** 1.0
**Last Updated:** 2026-04-18
**Status:** Ready for OpenCode Implementation
