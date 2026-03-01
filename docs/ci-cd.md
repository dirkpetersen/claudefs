# CI/CD Pipeline Architecture

Phase 2 continuous integration and deployment infrastructure for ClaudeFS.

## Overview

The ClaudeFS CI/CD pipeline automates building, testing, and validation of all code changes through GitHub Actions. The pipeline is designed to catch regressions early and enforce quality standards across all 8 crates.

## GitHub Actions Workflows

### 1. Main CI Pipeline (`ci.yml`)

**Trigger:** Push to `main`, all pull requests

**Jobs:**

#### Cargo Check
- Syntax validation and compilation check
- Runs on every push (fast gate-keeping)
- ~30 seconds

#### Tests (Parallel Matrix)
- **Fast test suites** (no device I/O):
  - `claudefs-meta`: Metadata service, Raft consensus, KV store (417 tests)
  - `claudefs-reduce`: Dedup, compression, encryption (223 tests)
  - `claudefs-transport`: RPC, TCP, TLS, QoS (58 tests)
  - Total: ~3 minutes

- **Storage tests** (device I/O via io_uring):
  - `claudefs-storage`: Block allocator, NVMe passthrough (60 tests)
  - Timeout: 45 minutes (allows for slow io_uring simulation on CI runners)
  - Runs in parallel with fast tests

#### Clippy (Linter)
- Code quality and best practices
- Fails on any warnings (`-D warnings`)
- ~1 minute

#### Rustfmt (Formatter)
- Code style consistency
- Enforces `cargo fmt --all -- --check`
- ~30 seconds

#### Documentation
- Validates all doc comments compile
- Enforces `RUSTDOCFLAGS=-D warnings`
- ~1 minute

#### Code Coverage
- `cargo-tarpaulin` generates coverage report
- Uploads to Codecov for tracking
- Optional (doesn't block merge, for trending)
- ~3 minutes

#### Release Build
- Validates release profile compiles and links
- Catches link-time issues early
- ~2 minutes

**Total Pipeline Time:** ~10-15 minutes (serial gates: check → parallel tests, ~45 min storage tests, clippy, fmt, doc; coverage; release)

### 2. Nightly Integration Tests (`nightly.yml`)

**Trigger:** Daily at 2 AM UTC, manual dispatch via `workflow_dispatch`

**Jobs:**

#### Extended Test Suite
- Full `cargo test --all --lib` with extended timeout
- Stress tests for storage (single-threaded io_uring passthrough simulation)
- ~90 minutes

#### Security Audit
- `cargo audit` via `rustsec/audit-check-action`
- Scans dependencies for known CVEs
- Fails if vulnerabilities found
- ~2 minutes

#### Benchmark Suite (Placeholder)
- Skeleton for future benchmark tracking
- Will measure latency, throughput per component
- Not yet implemented in Phase 2

**Total Nightly Time:** ~120 minutes

### 3. Commit Lint (`commit-lint.yml`)

**Trigger:** PR submission/update

**Validation:**
- All commit messages must start with `[A#]` where `#` is agent 1-11
- Example: `[A2] Implement Raft leader election`
- Enforces per-agent accountability per docs/agents.md

## Deployment Workflow

### Pull Request Flow

1. **Developer pushes branch** with commits tagged `[A#]`
2. **GitHub creates PR** with commit lint check
3. **Commit lint validates** format (must be `[A#] ...`)
4. **Main CI runs:**
   - Cargo check (must pass)
   - Test matrix (all must pass)
   - Clippy (no warnings allowed)
   - Rustfmt (must be formatted)
   - Doc validation (must compile)
5. **Developer reviews** check results in PR
6. **Merge to main** (when all checks pass)
7. **Post-merge nightly run** for extended testing (future: automated performance gate)

### Per-Agent Deployment

Each agent has a designated prefix in commits:
| Agent | Prefix | Crate | Role |
|-------|--------|-------|------|
| A1 | `[A1]` | `claudefs-storage` | Storage engine |
| A2 | `[A2]` | `claudefs-meta` | Metadata service |
| A3 | `[A3]` | `claudefs-reduce` | Data reduction |
| A4 | `[A4]` | `claudefs-transport` | Transport layer |
| A5 | `[A5]` | `claudefs-fuse` | FUSE client |
| A6 | `[A6]` | `claudefs-repl` | Replication |
| A7 | `[A7]` | `claudefs-gateway` | Protocol gateways |
| A8 | `[A8]` | `claudefs-mgmt` | Management |
| A9 | `[A9]` | (test suite) | Test & validation |
| A10 | `[A10]` | (security) | Security audit |
| A11 | `[A11]` | (infrastructure) | Infrastructure & CI |

## Phase 2 Status

### Current Implementation
- ✅ `ci.yml`: Complete main pipeline (8 jobs, parallel test matrix)
- ✅ `nightly.yml`: Complete integration test suite with security audit
- ✅ `commit-lint.yml`: Commit format validation
- ✅ README badges added

### Deployment Blocker

**GitHub Token Scope Issue:** The current GitHub token lacks `workflow` scope, preventing push of `.github/workflows/*` files.

**Workaround:** Files are prepared in the repo but cannot be pushed to GitHub. See GitHub Issue #12.

**Resolution Required:** Update the GitHub token in Secrets Manager (`cfs/github-token`) to include `workflow` scope.

## Future Enhancements

### Phase 2 (Planned)
1. Automated performance regression testing (nightly)
2. Integration test matrix (multi-node cluster simulation)
3. POSIX test suite integration (pjdfstest, fsx)
4. Cross-platform builds (Linux targets: x86-64, ARM64)

### Phase 3 (Production)
1. Automated deployment to staging cluster
2. End-to-end cluster testing (Jepsen, CrashMonkey)
3. Performance benchmarking with historical tracking
4. Kubernetes deployment templates
5. Artifact signing and provenance tracking

## Monitoring and Alerts

### CI Health Dashboard
- GitHub Actions status visible at: https://github.com/dirkpetersen/claudefs/actions
- Per-agent filtering via commit prefix (`[A2]`, etc.)
- Nightly run status tracked separately

### Alert Channels
- GitHub PR checks (appear in PR UI automatically)
- Email notifications for repository watchers on main build failures
- Integration ready with Slack/Discord webhooks (future)

## Local Development

### Run CI Locally (Before Push)

```bash
# Quick checks
cargo check --all --lib
cargo clippy --all --lib -- -D warnings
cargo fmt --all -- --check

# Full test suite
cargo test --all --lib

# Single crate (faster for focused work)
cargo test -p claudefs-meta --lib
```

### Reproduce CI Failure

```bash
# If nightly test fails, reproduce with extended timeout
RUST_BACKTRACE=full cargo test --all --lib -- --test-threads=1

# If storage test fails
cargo test -p claudefs-storage --lib -- --test-threads=1 --nocapture
```

## Cost and Performance

### GitHub Actions Runtime

| Workflow | Frequency | Runtime | Cost |
|----------|-----------|---------|------|
| CI (push/PR) | Every push | 15 min | Free (GH provides quota) |
| Nightly | Daily 2 AM UTC | 120 min | ~$0.01-0.02/run |
| Commit lint | Per PR | 30 sec | Free |

**Monthly:** ~30 CI runs (active development) + 30 nightly runs = ~1000 build minutes = **well within GitHub's free tier** (3000 min/month for public repos).

## Troubleshooting

### Workflow fails locally but passes CI

**Cause:** Likely compiler version difference or platform-specific behavior.

**Solution:**
1. Check Rust version: `rustc --version` (CI uses `stable`)
2. Ensure clean build: `cargo clean && cargo test --all --lib`
3. Check for platform assumptions: `uname -a`

### CI passes but code breaks on cluster

**Cause:** CI runs on single machine; cluster tests require multi-node setup.

**Solution:**
- See `docs/agents.md` Phase 2+ integration testing section
- A9 (Test & Validation) owns multi-node Jepsen tests

### Nightly audit fails with CVE

**Cause:** New vulnerability in transitive dependency.

**Solution:**
1. Check `cargo audit` output for details
2. If transitive (not our direct dep), consider upgrading parent crate
3. If blocklisted, use `cargo audit` deny-lists if upstream isn't fixed
4. Document in GitHub Issue, assign to A10 (Security)

## References

- **Workflow Syntax:** https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions
- **Action Library:** https://github.com/actions (checkout, cache, upload-artifact, etc.)
- **Rust Toolchain Action:** https://github.com/dtolnay/rust-toolchain
- **Cargo Tarpaulin:** https://github.com/xd009642/tarpaulin
- **rustsec Audit:** https://github.com/rustsec/audit-check-action
