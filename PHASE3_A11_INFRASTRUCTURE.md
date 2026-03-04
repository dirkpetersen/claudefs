# A11 Phase 3 Infrastructure & CI Roadmap

**Agent:** A11 (Infrastructure & CI) | **Phase:** 3 | **Status:** Planning & Implementation
**Model:** Claude Haiku 4.5 | **Effort:** ~32 hours (4 weeks, ~2h/day)

## Executive Summary

Phase 3 A11 work focuses on **build health**, **multi-node testing automation**, **policy integration validation**, and **production deployment templates**. This roadmap unblocks all builder agents (A1-A8) via faster CI feedback and auto-fix error recovery, enables A9 to run comprehensive POSIX/Jepsen suites on the full 10-node cluster, and prepares ClaudeFS for production deployment.

**Current State:**
- Build fixed (tonic-build dependency) ✅
- 1930+ tests passing, all 11 agents active
- GitHub Actions CI with 6 workflow files
- Infrastructure tools in place (watchdog, supervisor, cfs-dev CLI)
- Test cluster: 1 orchestrator + 9 preemptible nodes

---

## Tier 1: Quick Wins & Blockers (Hours 1-4)

### Task 1.1: Fix GitHub Actions Workflow Timeouts
**Priority:** CRITICAL | **Effort:** 1-2 hours | **Blocks:** All commits
- **Problem:** CI build sometimes times out on large workspace; clippy takes >20 min
- **Solution:**
  - Enable build cache for incremental rebuilds
  - Split `cargo clippy` into per-crate runs with timeout escalation
  - Add `--keep-going` to continue on warnings, report separately
  - Cache target/ more aggressively
- **Success:** CI build <15 min, clippy passes 100%, cache hit >80%
- **File:** `.github/workflows/ci-build.yml`

### Task 1.2: Add Automated CVE Scanning
**Priority:** HIGH | **Effort:** 1 hour | **Blocks:** A10 integration
- **Problem:** `cargo audit` doesn't fail CI; missing license compliance checks
- **Solution:**
  - Add `cargo deny check advisories` + `cargo deny check licenses`
  - Fail CI on CRITICAL/HIGH CVEs
  - Generate SBOM with `cargo-cyclonedx`
- **Success:** CI rejects CVE commits, license compliance checked
- **File:** `.github/workflows/security-scan.yml` (new)

### Task 1.3: Implement Automated OpenCode Error Recovery
**Priority:** HIGH | **Effort:** 2 hours | **Blocks:** A1-A8 velocity
- **Problem:** Build errors from OpenCode aren't caught until agent manually runs `cargo check`
- **Solution:**
  - Modify `cfs-supervisor.sh` to detect, classify, and auto-fix build errors
  - Parse `cargo check` output for compiler/clippy/dependency errors
  - Generate new OpenCode prompt with error context
  - Re-run OpenCode with error-fixing instructions (up to 3 attempts)
  - Alert agent on persistent failures
- **Success:** >90% of errors auto-fixed within 1 cycle
- **File:** `tools/cfs-supervisor.sh` (updated)

---

## Tier 2: Build Health & Stability (Hours 5-8)

### Task 2.1: Implement Per-Crate Test Parallelization
**Priority:** HIGH | **Effort:** 2 hours | **Blocks:** Faster feedback
- **Problem:** `cargo test --workspace` runs serially; 40 min total
- **Solution:**
  - Create `tools/cfs-parallel-test.sh` using GNU parallel
  - Run `cargo test -p CRATE` for each of 8 crates in parallel
  - Aggregate results into JSON report
- **Success:** Full test suite runs in <15 min, all tests atomic
- **Files:** `tools/cfs-parallel-test.sh`, `.github/workflows/tests-all.yml`

### Task 2.2: Add Build Artifact Caching & Distribution
**Priority:** MEDIUM | **Effort:** 2 hours | **Blocks:** Faster redeployment
- **Problem:** Every `cfs-dev deploy` rebuilds binary (5+ min delay)
- **Solution:**
  - Store release binaries in S3 keyed by git SHA
  - Skip build on cache hit (deploy <1 min)
  - Evict oldest 10 binaries to keep S3 cost low
- **Success:** Deploy on cache hit <1 min, 500 MB S3 storage
- **Files:** `tools/cfs-build-cache.sh`, `tools/cfs-dev` (updated)

### Task 2.3: Implement Failing Test Regression Registry
**Priority:** MEDIUM | **Effort:** 1 hour | **Blocks:** Preventing regressions
- **Problem:** Flaky tests aren't tracked; same failures repeat
- **Solution:**
  - Auto-commit regression cases from proptest failures
  - Report "new failures" vs "known regressions" in CI
- **Success:** All property-based test failures auto-registered
- **File:** `crates/claudefs-tests/proptest-regressions/` + CI integration

---

## Tier 3: Multi-Node Testing Infrastructure (Hours 9-16)

### Task 3.1: Implement Multi-Node Test Orchestration for A9
**Priority:** CRITICAL | **Effort:** 4 hours | **Blocks:** A9 test suite
- **Problem:** POSIX tests only run single-node; need full cluster
- **Solution:**
  - Create `tools/cfs-test-suite.sh` supporting: pjdfstest, xfstests, fsx, connectathon, jepsen, crashmonkey
  - Provision cluster, deploy ClaudeFS, run suite, collect results
  - Generate HTML report with per-test status, latencies, failure reasons
  - Integrate as `.github/workflows/tests-multi-node.yml` (nightly + manual)
- **Success:** Full POSIX test suite on cluster, results <1 hour, attribution to nodes/crates
- **Files:** `tools/cfs-test-suite.sh`, `.github/workflows/tests-multi-node.yml`

### Task 3.2: Implement Jepsen Distributed Consistency Testing
**Priority:** HIGH | **Effort:** 4 hours | **Blocks:** A9 linearizability
- **Problem:** Need automated fault injection for consistency validation
- **Solution:**
  - Create `tools/cfs-jepsen.sh` with:
    - Dedicated Jepsen controller
    - Nemesis handlers: network partition, clock skew, disk failures
    - Configurable test scenarios (read/write linearizability, replication consistency)
  - Support `--duration 3600` for overnight soak tests
- **Success:** Detects linearizability violations, <30 min per scenario
- **Files:** `tools/cfs-jepsen.sh`, `.github/workflows/tests-multi-node.yml`

### Task 3.3: Implement Crash Recovery & Failover Testing
**Priority:** HIGH | **Effort:** 4 hours | **Blocks:** A9 resilience
- **Problem:** Need automated failure scenario testing
- **Solution:**
  - Create `tools/cfs-failover-test.sh` covering:
    - Kill storage node leader → recovery from replicas
    - Partition site A from site B → cross-site failover
    - Fill disk to 95% → emergency handling
  - Measure downtime, consistency, recovery time
- **Success:** All scenarios recover within SLA (<5 min)
- **Files:** `tools/cfs-failover-test.sh`, `.github/workflows/tests-resilience.yml`

---

## Tier 4: Policy Integration Testing (Hours 17-22)

### Task 4.1: Implement A8 Policy Integration Test Harness
**Priority:** CRITICAL | **Effort:** 3 hours | **Blocks:** A8 Phase 3
- **Problem:** A8 needs CI validation for quota/QoS/WORM enforcement
- **Solution:**
  - Create `.github/workflows/a8-integration.yml`
  - Run A8 RPC integration tests against mock A2/A4 services
  - Validate: quota rejection, QoS rate limiting, WORM locks
  - Support `--mock-only` (5 min) vs `--cluster` (full validation)
- **Success:** <5 min mock runtime, cross-crate integrations validated
- **File:** `.github/workflows/a8-integration.yml`

### Task 4.2: Implement A6 Replication Integration Testing
**Priority:** HIGH | **Effort:** 3 hours | **Blocks:** A6 multi-site
- **Problem:** Need multi-site cluster validation for replication
- **Solution:**
  - Create `.github/workflows/a6-integration.yml`
  - 2-site cluster: 3 nodes site A, 2 nodes site B
  - Validate: journal replication (<1 sec latency), conflict resolution, failover recovery
  - Collect replication latency distribution
- **Success:** <1 sec replication p99, conflict resolution works, failover <30 sec
- **File:** `.github/workflows/a6-integration.yml`

---

## Tier 5: Production Deployment & Release (Hours 23-30)

### Task 5.1: Implement Release Binary Packaging
**Priority:** HIGH | **Effort:** 3 hours | **Blocks:** Production deployment
- **Problem:** No standardized release packaging
- **Solution:**
  - Create `tools/build-release.sh`
  - Build with LTO, strip debug symbols
  - Generate checksums (SHA256), tarball, release notes
  - Upload to GitHub Releases + S3
  - Trigger on git tags: `git tag v0.1.0` → CI builds + releases
- **Success:** Release binary <100 MB, automated on tag, <10 min build+upload
- **Files:** `tools/build-release.sh`, `.github/workflows/release.yml`

### Task 5.2: Implement Staged Deployment Templates
**Priority:** HIGH | **Effort:** 3 hours | **Blocks:** Production rollout
- **Problem:** No zero-downtime staged deployment procedure
- **Solution:**
  - Create `tools/deploy-staged.sh`
  - Canary (1 node) → Batch 1 (25%) → Batch 2 (50%) → Batch 3 (100%)
  - Auto-rollback if critical metrics exceed threshold
  - Procedure: keep 2 prior versions for rollback
- **Success:** Zero-downtime deployment, auto-rollback on regression, <1 hr total
- **Files:** `tools/deploy-staged.sh`, `docs/PRODUCTION_DEPLOYMENT.md`

### Task 5.3: Implement Performance Regression Detection
**Priority:** MEDIUM | **Effort:** 2 hours | **Blocks:** Production SLAs
- **Problem:** Performance regressions go undetected until production
- **Solution:**
  - Create `tools/benchmark.sh`
  - Measure I/O latencies (p50, p99, p999)
  - Compare to baseline (stored in S3)
  - Fail CI if >5% latency increase or >1% throughput loss
  - Baseline reset monthly
- **Success:** <5 min benchmark, regression detection, CI block on threshold
- **Files:** `tools/benchmark.sh`, `.github/workflows/perf-regression.yml`

---

## Tier 6: Operational Runbooks & Documentation (Hours 31-34)

### Task 6.1: Create Debugging & Troubleshooting Runbook
**Priority:** MEDIUM | **Effort:** 2 hours
- **Contents:**
  - Build failure investigation
  - Test failure diagnosis
  - Cluster health diagnostics
  - Performance degradation analysis
  - Cross-site replication lag detection
  - Node recovery procedures
- **File:** `docs/DEBUGGING_RUNBOOK.md`

### Task 6.2: Create Scaling & Capacity Planning Guide
**Priority:** MEDIUM | **Effort:** 1 hour
- **Contents:**
  - When to scale (thresholds)
  - How to add nodes (`cfs-dev scale --nodes 5`)
  - Metadata shard rebalancing
  - Cost tracking + budget management
  - Instance type selection
- **File:** `docs/SCALING_GUIDE.md`

### Task 6.3: Create CI/CD Troubleshooting Guide
**Priority:** LOW | **Effort:** 1 hour
- **Contents:**
  - Why did commit break CI?
  - Run tests locally
  - Manual GitHub Actions trigger
  - Cache hit rate checking
  - Flaky test debugging
- **File:** `docs/CI_TROUBLESHOOTING.md`

---

## Cross-Agent Integration Points

| Agent | Depends On | Unblocked By |
|-------|-----------|------------|
| **A8** | Task 4.1 | A8 can validate quota/QoS/WORM RPC integration |
| **A9** | Task 3.1-3.3 | A9 can run full POSIX/Jepsen on cluster |
| **A10** | Task 1.2 | A10 can validate security fixes in pipeline |
| **A1-A7** | Task 1.3, 2.1 | Faster feedback loop (15-min CI cycle) |
| **A6** | Task 4.2 | A6 can validate multi-site replication |

---

## Implementation Sequence

**Week 1 (Quick Wins):**
1. Task 1.1 — Fix CI timeouts
2. Task 1.3 — OpenCode error recovery
3. Task 2.1 — Parallel test execution
4. Task 1.2 — CVE scanning

**Week 2 (Multi-Node Testing):**
5. Task 3.1 — POSIX test orchestration
6. Task 3.2 — Jepsen testing
7. Task 4.1 — A8 integration testing

**Week 3 (Production Ready):**
8. Task 5.1 — Release packaging
9. Task 5.2 — Staged deployment
10. Task 4.2 — A6 replication testing

**Week 4 (Polish & Docs):**
11. Task 6.1-6.3 — Operational runbooks
12. Task 2.2 — Build artifact caching
13. Task 5.3 — Performance regression detection

---

## Success Metrics

| Metric | Current | Target |
|--------|---------|--------|
| **CI Cycle Time** | 30-40 min | <15 min |
| **Build Cache Hit** | 60% | >80% |
| **OpenCode Errors Auto-Fixed** | 0% | >90% |
| **POSIX Test Coverage** | Unit only | Full cluster |
| **Jepsen Linearizability** | Manual | Automated CI |
| **Production Deployment Time** | Manual | <1 hr zero-downtime |
| **Release Cycle** | Manual | Automated on tag |

---

## Cost Impact

| Component | Cost | Notes |
|-----------|------|-------|
| Multi-node test cluster | $0/day | Already preemptible |
| S3 build cache | $0.10/day | 500 MB storage |
| GitHub Actions | $0/month | Free for public |
| **Total incremental** | <$5/month | Negligible |

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Multi-node test flakiness | Start with mock tests, scale gradually |
| OpenCode error loop | 3-attempt max, escalate to manual |
| Perf regression false positives | 10% threshold + require 2 consecutive regressions |
| Cache invalidation bugs | Store git SHA + Cargo.lock hash |
| Deployment rollback failure | Keep N-2 versions, practice monthly |

---

## Status Tracking

- [ ] Task 1.1: Fix CI timeouts
- [ ] Task 1.2: CVE scanning
- [ ] Task 1.3: OpenCode error recovery
- [ ] Task 2.1: Parallel test execution
- [ ] Task 2.2: Build cache + distribution
- [ ] Task 2.3: Regression registry
- [ ] Task 3.1: Multi-node test orchestration
- [ ] Task 3.2: Jepsen testing
- [ ] Task 3.3: Failover testing
- [ ] Task 4.1: A8 integration harness
- [ ] Task 4.2: A6 replication testing
- [ ] Task 5.1: Release packaging
- [ ] Task 5.2: Staged deployment
- [ ] Task 5.3: Perf regression detection
- [ ] Task 6.1: Debugging runbook
- [ ] Task 6.2: Scaling guide
- [ ] Task 6.3: CI/CD troubleshooting

---

**Last Updated:** 2026-03-04 | **A11 Agent:** Claude Haiku 4.5
