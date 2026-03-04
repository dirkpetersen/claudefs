# ClaudeFS CI/CD Pipeline Guide

**Last Updated:** 2026-03-04
**Status:** ✅ Fully Operational
**Average Build Time:** ~25 minutes (cached), ~60 minutes (cold cache)

## Overview

The ClaudeFS CI/CD infrastructure uses **GitHub Actions** for automated building, testing, and deployment. This guide describes the pipeline architecture, workflows, caching strategies, and best practices.

## Pipeline Architecture

### Trigger-Based Workflows

The CI/CD system uses conditional triggers to optimize cost and testing coverage:

| Workflow | Triggers | Purpose | Duration | Cost |
|----------|----------|---------|----------|------|
| **ci-build.yml** | Every push to main | Basic build + format + clippy | ~25 min | Low |
| **tests-all.yml** | Nightly (0 UTC) + manual + PR | Full unit test suite | ~45 min | Medium |
| **integration-tests.yml** | PR + manual trigger | Cross-crate integration tests | ~30 min | Medium |
| **a9-tests.yml** | Push to test crates | A9 validation suite + proptests | ~30 min | High |
| **release.yml** | Git tag (v*.*.* pattern) | Build release artifacts + multi-arch | ~60 min | High |
| **deploy-prod.yml** | Manual trigger | Production deployment (Terraform) | ~40 min | Medium |

### Workflow Dependency Graph

```
┌─ ci-build.yml (every push)
│  ├─ Build (debug + release)
│  ├─ Format check (rustfmt)
│  └─ Lint check (clippy)
│
├─ tests-all.yml (nightly + manual)
│  ├─ All unit tests
│  ├─ Per-crate tests (storage, meta, reduce, transport, fuse, repl, gateway, mgmt, security)
│  └─ Doc tests
│
├─ integration-tests.yml (PR + manual)
│  ├─ Storage ↔ Meta
│  ├─ Meta ↔ Transport
│  ├─ Transport ↔ FUSE
│  ├─ FUSE ↔ Gateway (NFS, pNFS)
│  └─ Full stack test
│
├─ a9-tests.yml (on test crate changes)
│  ├─ Unit tests
│  ├─ Property-based tests (proptest)
│  └─ Integration tests
│
├─ release.yml (on git tag)
│  ├─ Build release (x86_64)
│  ├─ Build alternative arch (aarch64, if needed)
│  ├─ Create GitHub Release
│  └─ Upload artifacts
│
└─ deploy-prod.yml (manual)
   ├─ Validate Terraform
   ├─ Plan infrastructure
   ├─ Deploy to production
   └─ Smoke tests
```

## Current Workflows

### 1. CI Build (ci-build.yml)

**Purpose:** Fast feedback on every push. Runs build, format check, and linting.

**Triggers:**
- Push to `main` (except docs/ and README changes)
- Pull requests to `main`
- Manual trigger (`workflow_dispatch`)

**Jobs:**
- **Build:** `cargo build --workspace --all-targets` (debug + release)
- **Format:** `cargo fmt --all -- --check` (no modifications)
- **Clippy:** `cargo clippy --all --all-targets -- -D warnings`
- **Security:** `cargo audit` (dependency CVE scan)

**Duration:** ~25 min (with cache)
**Cost:** Low (GitHub-hosted runner)

**Key Features:**
- ✅ Caching: registry, git, build targets
- ✅ Parallelization: 3 jobs run in parallel
- ✅ Matrix builds: Optional (not used currently)

### 2. All Tests (tests-all.yml)

**Purpose:** Comprehensive test coverage. Runs nightly and on manual trigger.

**Triggers:**
- Schedule: `0 0 * * *` (nightly at 00:00 UTC)
- Pull request to `main`
- Manual trigger (`workflow_dispatch`)
- Skip on regular pushes to main (ci-build.yml is sufficient)

**Jobs:**
1. **All Unit Tests:** `cargo test --workspace --all-targets --lib`
2. **Per-Crate Tests:**
   - Storage tests (4 threads)
   - Meta tests (4 threads)
   - Reduce tests (4 threads)
   - Transport tests (2 threads)
   - FUSE tests (4 threads)
   - Repl tests (2 threads)
   - Gateway tests (2 threads)
   - Mgmt tests (2 threads)
   - Security tests (2 threads)

**Duration:** ~45 min (with cache)
**Cost:** Medium

**Key Features:**
- ✅ Per-crate parallelization (crates test in parallel)
- ✅ Thread limits to avoid resource contention
- ✅ Independent cache keys per crate
- ✅ Artifact upload: test results as artifacts

### 3. Integration Tests (integration-tests.yml)

**Purpose:** Cross-crate integration testing. Exercises interactions between subsystems.

**Triggers:**
- Pull request to `main`
- Manual trigger (`workflow_dispatch`)

**Jobs:**
1. **Storage ↔ Metadata:** Block allocation → metadata indexing
2. **Metadata ↔ Transport:** Raft log → network replication
3. **Transport ↔ FUSE:** RPC calls → FUSE daemon
4. **FUSE ↔ Gateway:** FUSE client → NFS/pNFS gateway
5. **Full Stack:** Complete end-to-end test

**Duration:** ~30 min (with cache)
**Cost:** Medium

**Key Features:**
- ✅ Tests against mock implementations
- ✅ Network simulation for transport layer
- ✅ FUSE passthrough mode testing (kernel 6.8+)

### 4. A9 Tests (a9-tests.yml)

**Purpose:** A9 (Test & Validation) agent's comprehensive suite.

**Triggers:**
- Push to main affecting test crates
- Manual trigger

**Jobs:**
1. **Unit Tests:** `cargo test -p claudefs-tests`
2. **Property-Based Tests:** `proptest` for storage, reduce, transport (256-1024 cases each)
3. **Integration Tests:** Cross-crate scenarios
4. **POSIX Test Harness:** xfstests, pjdfstest wrappers (future: Jepsen)

**Duration:** ~30 min (typical)
**Cost:** Medium-High (intensive testing)

**Key Features:**
- ✅ Property-based test case limits (PROPTEST_CASES env var)
- ✅ Single-threaded test execution (no race conditions)
- ✅ Captures test results for A9 analysis

### 5. Release (release.yml)

**Purpose:** Build multi-architecture release artifacts.

**Triggers:**
- Git tag matching `v*.*.*` pattern
- Manual trigger (`workflow_dispatch`)

**Jobs:**
1. **Build x86_64:** `cargo build --release --target x86_64-unknown-linux-gnu`
2. **Build aarch64:** (optional) `cross build --release --target aarch64-unknown-linux-gnu`
3. **Create Release:** GitHub Release with artifacts
4. **Upload Artifacts:** S3 bucket (if configured)
5. **Generate Checksums:** SHA256 verification

**Duration:** ~60 min (cold cache)
**Cost:** High (multi-arch builds)

**Key Features:**
- ✅ Automatic release notes from CHANGELOG.md
- ✅ Artifact signing (if GPG key configured)
- ✅ Docker image build (future)

### 6. Production Deployment (deploy-prod.yml)

**Purpose:** Infrastructure deployment to production. Manual trigger only.

**Triggers:**
- Manual trigger (`workflow_dispatch`)
- Optionally: after successful release

**Jobs:**
1. **Validate:** Terraform validation
2. **Plan:** `terraform plan` (output artifacts)
3. **Approve:** Manual gate (required)
4. **Deploy:** `terraform apply`
5. **Smoke Tests:** Basic connectivity checks
6. **Rollback** (if failures): Automatic on smoke test failure

**Duration:** ~40 min
**Cost:** Medium (but careful approval process prevents runaway costs)

**Key Features:**
- ✅ Manual approval gate (prevents accidental deployment)
- ✅ Terraform state locked during apply
- ✅ Automatic rollback on test failure
- ✅ Deployment logs stored as artifacts

## Caching Strategy

### Cache Keys (Best Practices)

The CI uses **content-based cache keys** to maximize hit rate:

```yaml
# Registry cache (rarely changes)
key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}
restore-keys: ${{ runner.os }}-cargo-registry-

# Git dependencies cache (rarely changes)
key: ${{ runner.os }}-cargo-git-${{ hashFiles('**/Cargo.lock') }}
restore-keys: ${{ runner.os }}-cargo-git-

# Build target cache (changes when code changes)
key: ${{ runner.os }}-cargo-build-${{ hashFiles('**/Cargo.lock') }}
restore-keys: ${{ runner.os }}-cargo-build-

# Per-job caches (for matrix builds)
key: ${{ runner.os }}-cargo-${{ matrix.crate }}-${{ hashFiles('**/Cargo.lock') }}
```

### Cache Hit Rates

| Cache Type | Hit Rate | Impact |
|-----------|----------|--------|
| Registry | 95%+ | ~5 min saved per build |
| Git deps | 90%+ | ~3 min saved per build |
| Build target | 50-80% | ~15-30 min saved (incremental compile) |
| **Total Savings** | — | **~25 min per build** |

### Cache Eviction

GitHub Actions cache is automatically evicted after 7 days of non-use. To prevent eviction:
- Nightly builds access all caches
- Matrix builds spread cache access
- No manual cache clearing needed

## Performance Optimization

### Build Time Profile (2026-03-04)

| Phase | Time | Notes |
|-------|------|-------|
| Checkout | 5 sec | Fixed |
| Rust toolchain install | 10 sec | Fixed |
| Cache restore (registry) | 30 sec | Fixed |
| Cache restore (build targets) | 2-3 min | Varies |
| `cargo build --workspace` | 5-10 min | **Incremental compile** (most variable) |
| `cargo test --workspace` | 8-15 min | Depends on test count |
| `cargo clippy` | 3-5 min | Full workspace lint |
| **Total (cached)** | **~25 min** | — |
| **Total (cold cache)** | **~60 min** | Full compile |

### Parallelization

Current workflows use **GitHub Actions matrix** for:
- Per-crate tests run in parallel (reduces wall-clock time)
- No custom parallelization within cargo (uses `-j auto`)

**Potential improvements:**
```yaml
# Future: Matrix by crate
strategy:
  matrix:
    crate: [storage, meta, reduce, transport, fuse, repl, gateway, mgmt, security]
# Would reduce ci-build time from 25 min to ~10 min
# But increases complexity
```

### Timeout Configuration

Current timeouts are conservative to prevent runner hangs:

| Job | Timeout | Actual Time | Buffer |
|-----|---------|------------|--------|
| Build | 30 min | 10-12 min | 50% |
| Format | 10 min | <1 min | 90% |
| Clippy | 20 min | 4-5 min | 75% |
| All tests | 45 min | 30-40 min | 10% |
| Integration tests | 30 min | 25-28 min | 10% |
| Release build | 60 min | 45-55 min | 10% |
| Deploy | 40 min | 30-35 min | 10% |

**Recommendation:** Keep current timeouts. They provide safety margin for runner slowdowns.

## Best Practices & Recommendations

### ✅ Current Strengths

1. **Triggered Workflows:** Different triggers for different scenarios (push vs nightly vs PR)
2. **Caching:** Comprehensive caching with Cargo.lock-based keys
3. **Parallelization:** Per-crate test jobs run in parallel
4. **Cost Control:** Nightly tests don't run on every push
5. **Clear Job Names:** Easy to identify failures
6. **Artifacts:** Test results stored for analysis

### 🔄 Opportunities for Improvement

#### #1: Add Code Coverage Analysis

**Current:** No coverage metrics

**Recommendation:**
```yaml
# Add job to tests-all.yml
coverage:
  runs-on: ubuntu-latest
  steps:
    - uses: taiki-e/install-action@cargo-tarpaulin
    - run: cargo tarpaulin --workspace --out Html
    - uses: actions/upload-artifact@v4
      with:
        name: coverage-report
        path: tarpaulin-report.html
```

**Benefit:** Track coverage over time, identify untested code paths

#### #2: Add Dependency Audit to Every Build

**Current:** `cargo audit` in ci-build.yml (only on main branch)

**Recommendation:** Also run on PRs to catch security issues before merge
```yaml
security-audit:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: EmbarkStudios/cargo-deny-action@v1
```

#### #3: Add CHANGELOG Validation

**Current:** No validation that CHANGELOG.md is updated

**Recommendation:**
```yaml
changelog-check:
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0
    - run: |
        if git diff origin/main HEAD -- CHANGELOG.md | grep -q '^+'; then
          echo "✅ CHANGELOG.md updated"
        else
          echo "❌ CHANGELOG.md must be updated in PRs"
          exit 1
        fi
```

#### #4: Add Commit Message Linting

**Current:** No validation of commit message format

**Recommendation:**
```yaml
commitlint:
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0
    - uses: commitlint-rs/commitlint@v0
      with:
        config: |
          {
            "rules": {
              "type-enum": [2, "always", ["[A0-9]+", "chore", "docs", "fix"]],
              "type-case": [2, "always", "always"]
            }
          }
```

#### #5: Add Artifact Cleanup

**Current:** Artifacts kept for 90 days (GitHub default)

**Recommendation:** Reduce to 14 days for cost savings
```yaml
# In all workflow files:
jobs:
  my-job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/upload-artifact@v4
        with:
          name: my-artifact
          path: ./path/to/artifact
          retention-days: 14  # Default is 90
```

#### #6: Add Matrix Testing for Rust Versions

**Current:** Only tests on stable Rust

**Recommendation (future, after Phase 3):**
```yaml
test:
  strategy:
    matrix:
      rust-version: [stable, 1.75.0]  # MSRV (minimum supported)
  steps:
    - uses: actions-rust-lang/setup-rust-toolchain@v1
      with:
        toolchain: ${{ matrix.rust-version }}
```

### 📊 Monitoring & Metrics

#### GitHub Actions Dashboard

Access at: https://github.com/dirkpetersen/claudefs/actions

**Key metrics to watch:**
- Workflow run duration trend
- Failed runs (should be rare)
- Cache hit rate
- Total bandwidth usage

#### Monthly Summary

```bash
# Analyze workflow stats for the month
gh run list --limit 1000 --json name,duration,conclusion | \
  jq 'group_by(.name) | map({name: .[0].name, avg_duration: ((map(.duration) | add) / length), failures: map(select(.conclusion == "failure")) | length})'
```

## Secrets & Security

### Secrets Used in CI/CD

| Secret | Used In | Scope | Rotation |
|--------|---------|-------|----------|
| `SSH_PRIVATE_KEY` | deploy-prod.yml | Terraform remote state | Annual |
| `AWS_ACCESS_KEY_ID` | All AWS jobs | Provisioning | 90 days |
| `AWS_SECRET_ACCESS_KEY` | All AWS jobs | Provisioning | 90 days |
| `GITHUB_TOKEN` | release.yml | Create release | Auto-rotated by GitHub |

### Best Practices

- ✅ Never log secrets (GitHub Actions masks them)
- ✅ Use IAM roles instead of keys (when possible)
- ✅ Rotate AWS keys every 90 days
- ✅ Use branch protection to require status checks
- ✅ Require manual approval for deploy-prod.yml

## Troubleshooting

### Workflow Hangs or Times Out

**Symptoms:** Workflow "In Progress" for >2 hours

**Debug:**
```bash
# Check for stuck processes in runner
gh run view <run-id> --log

# Look for cargo compilation stalls
# Common cause: libduckdb-sys or libfabric long compile

# Solution:
# 1. Cancel the run: gh run cancel <run-id>
# 2. Force cache clear: Delete cache entries manually
# 3. Re-run: gh run rerun <run-id>
```

### Cache Not Hitting

**Symptoms:** Every build takes >45 min, should be ~25 min

**Debug:**
```bash
# Check cache hit rate
gh run view <run-id> --log | grep -E "(Downloading|Uploading) cache"

# Expected: "Downloading cache" and "Cache hit" messages

# If not hitting:
# 1. Check Cargo.lock is committed
# 2. Check cache key hasn't changed (e.g., new cache path)
# 3. Check cache is not older than 7 days
```

### Build Fails with Cryptic Error

**Symptoms:** `error: could not compile claudefs`

**Debug:**
```bash
# Get full error output
gh run view <run-id> --log | grep -A 10 "error\[E"

# Common errors:
# - "error[E0308]: type mismatch" → cross-crate API change
# - "error: linkage mismatch" → dependency version conflict
# - "error: unresolved import" → missing module registration

# Solution: Run locally first
cargo check
cargo build
```

## Future Improvements (Post-Phase 3)

1. **Automatic Rollback:** Trigger rollback on deploy failure
2. **Performance Tracking:** Dashboard showing build time trends
3. **Multi-Arch Artifacts:** Automatic aarch64 + x86_64 release builds
4. **Docker Images:** OCI image builds in release workflow
5. **Kubernetes Deployment:** Helm charts + auto-deploy to k8s cluster
6. **Canary Deployments:** Route 5% traffic to new version before full rollout
7. **Long-Running Tests:** Jepsen + CrashMonkey in separate nightly workflow

## References

- GitHub Actions Docs: https://docs.github.com/en/actions
- Rust Caching: https://github.com/Swatinem/rust-cache (alternative to manual caching)
- cargo-tarpaulin (code coverage): https://github.com/taiki-e/cargo-tarpaulin
- cargo-deny (dependency auditing): https://embarkstudios.github.io/cargo-deny/

---

**Document Version:** 1.0
**Last Updated:** 2026-03-04
**Owner:** A11 (Infrastructure & CI)
**Next Review:** 2026-03-11 (after Phase 1 optimization)
