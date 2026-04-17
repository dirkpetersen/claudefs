# A11 Phase 3 Session 3 — Implementation Plan

**Date:** 2026-04-17
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 **IN PROGRESS** — Session 3 Starting
**Target:** 100% Phase 3 completion (from 60-70% baseline)

---

## Executive Summary

Session 3 focuses on **consolidating infrastructure**, **optimizing builds**, and **implementing health monitoring** to reach production readiness. Previous sessions (1-2) established monitoring, dashboards, and CI workflows. This session fills remaining gaps and validates end-to-end automation.

**Key Objectives:**
1. ✅ Verify & optimize CI/CD pipeline (Tier 1-2)
2. ✅ Enhance OpenCode error recovery (Tier 1, Task 1.3)
3. ✅ Implement health monitoring agent (New, high-impact)
4. ✅ Document operational runbooks (Tier 6)
5. ⏳ Test multi-node automation (Tier 3, if time)

---

## Current Infrastructure State

### Existing Tools (Created Sessions 1-2)
- ✅ `cfs-watchdog.sh` — tmux session monitoring (3.8K)
- ✅ `cfs-supervisor.sh` — 15-min cron health checks (8.4K)
- ✅ `cfs-agent-launcher.sh` — Agent session management (6.4K)
- ✅ `cfs-parallel-test.sh` — Per-crate test parallelization (4.1K)
- ✅ `cfs-build-cache.sh` — S3 artifact caching (6.2K)
- ✅ `cfs-deploy.sh` — Deployment automation (5.9K)
- ✅ `cfs-test-cluster.sh` — Test infrastructure (5.1K)
- ✅ `cfs-cost-monitor.sh` — Budget enforcement (5.0K)

### GitHub Actions Workflows (17 total)
- ✅ `ci-build.yml` — Build + clippy (25-min timeout)
- ✅ `security-scan.yml` — CVE + license checks
- ✅ `tests-all.yml` — Full test suite
- ✅ `tests-parallel.yml` — Per-crate parallelization
- ✅ `deploy-multinode.yml` — Multi-node deployment
- ✅ `test-posix-nightly.yml` — POSIX suite (nightly)
- ✅ `perf-baseline.yml` — Performance benchmarking
- ✅ `a9-tests.yml` — A9 validation
- ✅ `cost-monitoring.yml` — Cost tracking
- + 8 more...

### Monitoring Stack (Sessions 1-2)
- ✅ Docker Compose monitoring stack
- ✅ 4 Grafana dashboards
- ✅ Prometheus configuration
- ✅ Metrics integration guide (400+ lines)
- ✅ Build metrics baseline
- ⏳ Per-crate metrics export (A1-A8 in progress)

---

## Session 3 Roadmap

### Phase 3A: CI/CD Optimization (2-4 hours)

**Priority 1: Enhance OpenCode Error Recovery**
- **File:** `tools/cfs-supervisor.sh`
- **Current:** Basic error detection + glm-5 model fix attempts
- **Enhancements:**
  - [ ] Add error categorization (dependency, type, API mismatch)
  - [ ] Route to minimax-m2p5 for complex fixes (current uses glm-5)
  - [ ] Implement exponential backoff (1s → 10s → 60s between retries)
  - [ ] Add error context collection (file diffs, related imports)
  - [ ] Improve success validation (compile + clippy pass)
  - [ ] Log all attempts to `/var/log/cfs-agents/opencode-fixes.log`
- **Success:** >90% of fixable errors auto-corrected within 3 attempts

**Priority 2: Optimize CI Build Cache**
- **File:** `.github/workflows/ci-build.yml`
- **Current:** Separate debug/release cache keys
- **Enhancements:**
  - [ ] Combine caches with single key for faster hits
  - [ ] Add incremental check (cargo check before full build)
  - [ ] Measure cache hit rate
  - [ ] Set aggressive cache cleanup (30 days)
- **Success:** Cache hit >85%, build time <15 min

**Priority 3: Document CI/CD Troubleshooting**
- **File:** `docs/CI_TROUBLESHOOTING.md` (NEW)
- **Content:**
  - When CI fails (steps to debug locally)
  - Cache invalidation (when to clear)
  - Workflow timeout escalation
  - OpenCode retry mechanism
  - Manual trigger procedures

### Phase 3B: Health Monitoring Agent (4-6 hours)

**Priority 1: Implement Node Health Monitor**
- **Location:** `crates/claudefs-mgmt/src/health_monitor.rs` (NEW)
- **Components:**
  - [ ] CPUMonitor — Track CPU usage, throttling
  - [ ] MemoryMonitor — Track RAM usage, OOM risk
  - [ ] DiskIOMonitor — Track I/O queue depth, latency
  - [ ] NetworkMonitor — Track replication lag, RPC latency
  - [ ] Health aggregator — Combined node state
- **Metrics Export:** Prometheus on port 9008
- **Tests:** Unit + integration (~40-50 tests)
- **Success:** Detects degradation within 30 seconds

**Priority 2: Automatic Recovery Actions**
- **Location:** `crates/claudefs-mgmt/src/recovery_actions.rs` (NEW)
- **Actions:**
  - [ ] HighCPU: Reduce worker threads, pause background tasks
  - [ ] HighMemory: Shrink caches, flush buffers
  - [ ] HighDiskIO: Reduce parallelism, throttle client writes
  - [ ] HighLatency: Failover to alternate path, shed load
  - [ ] NodeOffline: Rebalance data, update routing
- **Integration:** Coordinate with A4 (transport layer)
- **Tests:** Action triggering + validation (~20-30 tests)

**Priority 3: Health Reporting**
- **Location:** Integration in `crates/claudefs-mgmt/src/admin_api.rs`
- **Endpoints:**
  - [ ] `GET /admin/health` — Node health summary
  - [ ] `GET /admin/metrics` — Prometheus metrics
  - [ ] `GET /admin/stats` — Cluster-wide stats
  - [ ] `POST /admin/recovery` — Manual recovery trigger
- **Success:** API responds <100ms, accurate health state

### Phase 3C: Test Infrastructure Enhancements (4-6 hours)

**Priority 1: Multi-Node Test Orchestration**
- **File:** `tools/cfs-test-orchestrator.sh` (NEW)
- **Capabilities:**
  - [ ] Provision on-demand 10-node cluster
  - [ ] Deploy ClaudeFS across nodes
  - [ ] Run POSIX suites (pjdfstest, xfstests, fsx)
  - [ ] Collect results + attribution to nodes/crates
  - [ ] Generate HTML report
  - [ ] Tear down on completion
- **Integration:** GitHub Actions (`.github/workflows/tests-multi-node.yml`)
- **Success:** Full POSIX suite in <2 hours, 0 data loss

**Priority 2: Failover Testing Automation**
- **File:** `tools/cfs-failover-test.sh` (NEW)
- **Scenarios:**
  - [ ] Kill storage node leader → replica recovery
  - [ ] Partition site A from site B → failover
  - [ ] Fill disk to 95% → emergency handling
  - [ ] Network latency spike → timeout & failover
  - [ ] Metadata shard split → recovery
- **Metrics:** Downtime, consistency, recovery time
- **Success:** All scenarios recover within SLA (<5 min)

### Phase 3D: Documentation & Runbooks (3-4 hours)

**Priority 1: Debugging & Troubleshooting**
- **File:** `docs/DEBUGGING_RUNBOOK.md` (NEW)
- **Sections:**
  - Build failure investigation
  - Test failure diagnosis
  - Cluster health diagnostics
  - Performance degradation analysis
  - Cross-site replication lag detection
  - Node recovery procedures
  - Agent restart procedures
  - Emergency procedures

**Priority 2: Scaling & Capacity Planning**
- **File:** `docs/SCALING_GUIDE.md` (NEW)
- **Sections:**
  - When to scale (thresholds)
  - How to add nodes
  - Metadata shard rebalancing
  - Cost tracking + budget
  - Instance type selection
  - Multi-site considerations

**Priority 3: Operational Procedures**
- **File:** `docs/OPERATIONS_RUNBOOK.md` (NEW)
- **Sections:**
  - Daily checks
  - Weekly maintenance
  - Monthly backups
  - Disaster recovery
  - Performance tuning
  - Cost optimization

### Phase 3E: Monitoring Integration (Ongoing)

**Priority 1: Coordinate with A1-A8**
- Provide metrics module templates
- Review implementations
- Test metrics endpoints
- Integrate into Prometheus

**Priority 2: Alert Rule Validation**
- [ ] Deploy locally
- [ ] Simulate conditions (high CPU, disk full, etc.)
- [ ] Verify alert firing
- [ ] Test notification routing

**Priority 3: Production Validation**
- [ ] Load testing with metrics enabled
- [ ] Dashboard query validation
- [ ] Alert responsiveness
- [ ] Metric retention check

---

## Implementation Sequence

### Day 1-2: CI/CD Optimization (Phase 3A)
1. Enhance OpenCode error recovery in cfs-supervisor.sh
2. Optimize GitHub Actions cache strategy
3. Document CI/CD troubleshooting
4. Validate with test suite run

### Day 2-3: Health Monitoring (Phase 3B)
1. Implement CPUMonitor + MemoryMonitor
2. Implement DiskIOMonitor + NetworkMonitor
3. Implement recovery actions
4. Integrate into admin API
5. Add Prometheus export

### Day 3-4: Test Infrastructure (Phase 3C)
1. Implement multi-node orchestration
2. Implement failover testing
3. Integrate into CI/CD
4. Run validation suite

### Day 4-5: Documentation (Phase 3D)
1. Write debugging runbook
2. Write scaling guide
3. Write operations runbook
4. Create operational checklists

---

## Success Metrics

| Metric | Current | Target |
|--------|---------|--------|
| **CI Build Time** | 20-30 min | <15 min |
| **Cache Hit Rate** | 70% | >85% |
| **OpenCode Auto-Fix Rate** | ~60% | >90% |
| **Health Monitor Latency** | N/A | <30 sec |
| **Recovery Action Time** | N/A | <5 min |
| **Multi-Node Test Time** | N/A | <2 hours |
| **Failover Recovery SLA** | N/A | <5 min |
| **Documentation Coverage** | 60% | 100% |

---

## Files to Create/Modify

### Create (NEW)
- [ ] `docs/A11-PHASE3-SESSION3-PROGRESS.md`
- [ ] `docs/CI_TROUBLESHOOTING.md`
- [ ] `docs/DEBUGGING_RUNBOOK.md`
- [ ] `docs/SCALING_GUIDE.md`
- [ ] `docs/OPERATIONS_RUNBOOK.md`
- [ ] `tools/cfs-test-orchestrator.sh`
- [ ] `tools/cfs-failover-test.sh`
- [ ] `tools/cfs-health-monitor.sh`
- [ ] `crates/claudefs-mgmt/src/health_monitor.rs`
- [ ] `crates/claudefs-mgmt/src/recovery_actions.rs`

### Modify (ENHANCEMENT)
- [ ] `tools/cfs-supervisor.sh` (OpenCode recovery)
- [ ] `.github/workflows/ci-build.yml` (cache optimization)
- [ ] `.github/workflows/ci-build.yml` (add incremental check)
- [ ] `crates/claudefs-mgmt/src/admin_api.rs` (add health endpoints)
- [ ] `crates/claudefs-mgmt/src/lib.rs` (add health_monitor module)

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| OpenCode error loop | Max 3 attempts, escalate to manual |
| Multi-node test flakiness | Start with mock, scale gradually |
| Recovery action side effects | Dry-run mode before auto-execution |
| Cache invalidation bugs | Store git SHA + Cargo.lock hash |
| Deployment rollback failure | Keep N-2 versions, practice monthly |

---

## Timeline Estimate

- **Phase 3A (CI/CD):** 2-4 hours
- **Phase 3B (Health Monitoring):** 4-6 hours
- **Phase 3C (Test Infrastructure):** 4-6 hours
- **Phase 3D (Documentation):** 3-4 hours
- **Integration & Validation:** 2-3 hours
- **Buffer:** 2-3 hours
- **Total:** 17-26 hours

**Expected Completion:** 2026-04-20 to 2026-04-22

---

## Success Criteria (Phase 3 → 100%)

- [ ] All CI/CD optimizations implemented & validated
- [ ] Health monitoring agent functional & integrated
- [ ] Recovery actions tested & working
- [ ] Multi-node test automation operational
- [ ] All documentation completed
- [ ] Full test suite passing (6300+)
- [ ] Build time <15 min
- [ ] OpenCode error recovery >90% effective
- [ ] Production readiness verified

---

## Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>
