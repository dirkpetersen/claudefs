# A9: Test & Validation — Session 2 Plan

**Date:** 2026-04-18 (Planned for Session 2)
**Agent:** A9 (Test & Validation)
**Phase:** Phase 2+ (Multi-Node Cluster Testing)
**Duration:** ~3-4 days
**Priority:** High (Critical path for Phase 2-3 validation)

## Objective

Transition from single-node test framework validation to **multi-node cluster testing infrastructure**. Execute first wave of POSIX test suites on provisioned test cluster (A11 Terraform infrastructure), establish performance baselines, and prepare for full Phase 3 production validation.

## Prerequisites (Dependencies)

### From A5: FUSE Client
- **Status Needed:** ✅ FUSE mount operational at `/mnt/claudefs`
- **Dependency:** FUSE v3 daemon, POSIX syscall translation
- **Alternative:** Can mock for Phase 1 local testing
- **Blocker Severity:** HIGH (required for pjdfstest, fsx, Connectathon)

### From A11: Test Cluster Infrastructure
- **Status Needed:** ✅ 5 storage nodes, 2 FUSE client nodes operational
- **Terraform Modules:** Network, storage cluster, client nodes, monitoring
- **Provisioning:** `terraform apply` in `tools/terraform/`
- **Blocker Severity:** HIGH (required for distributed tests)

### From A1-A8: Stable APIs
- **Status:** ✅ (All builder crates have published trait interfaces)
- **Blocker Severity:** LOW (already met)

## Deliverables (Session 2)

### Tier 1: Local POSIX Validation (Days 1-2)
**Objective:** Verify POSIX framework works on single node before scaling to cluster

**Deliverables:**
1. **pjdfstest Quick Run** (100 tests, ~10 minutes)
   - Execute: `cargo test --lib posix::pjdfs_quick` or equivalent
   - Target: ~100 tests, 95%+ pass rate
   - Acceptable failures: Platform-specific tests (xattrs, ACLs on non-Linux)
   - Output: JSON report saved to `test-results/pjdfstest-quick.json`

2. **fsx Quick Run** (1,000 operations, ~5 minutes)
   - Execute: `cargo test --lib soak::fsx_quick` or equivalent
   - Generate random file operations: read, write, truncate, mmap
   - Verify data consistency against in-memory oracle
   - Output: `test-results/fsx-quick.json`

3. **Concurrency Tests** (thread safety validation)
   - Execute: `cargo test --lib concurrency_tests`
   - Target: 200+ tests covering multi-threaded race conditions
   - Output: `test-results/concurrency.json`

4. **Performance Baseline** (establish reference metrics)
   - Run: `bench::run_fio_baseline()` or equivalent
   - Capture metrics: throughput (MB/s), latency (ms p50/p99)
   - Store baseline: `baselines/phase2-local-baseline.json`

**Success Criteria:**
- ✅ pjdfstest: 95%+ pass rate
- ✅ fsx: No data corruption detected
- ✅ Concurrency: 100% pass rate
- ✅ Performance: Baseline captured for regression tracking

### Tier 2: Cluster Provisioning & Validation (Days 2-3)
**Objective:** Bring up multi-node test cluster and verify infrastructure

**Deliverables:**
1. **Cluster Provisioning Script**
   - Script: `tools/a9-provision-test-cluster.sh` (new)
   - Steps:
     - Call `terraform apply` in `tools/terraform/` with `environment=staging`
     - Wait for all 9 nodes to reach `running` state (timeout: 5 min)
     - Verify security group rules allow inter-node communication
     - Verify IAM roles applied
     - Collect node IPs, DNS names to `cluster-config.json`
   - Success criteria: All 9 nodes operational

2. **FUSE Mount Verification**
   - On each FUSE client node: Mount ClaudeFS at `/mnt/claudefs`
   - Command: `cfs mount --cluster-name staging-cluster /mnt/claudefs`
   - Verify: Can create files, read/write, traverse directories
   - Capture logs to `cluster-logs/fuse-mount-*.log`
   - Success criteria: Both FUSE clients successfully mounted

3. **Cluster Health Check**
   - Verify Raft consensus active (3-node quorum in Site A)
   - Verify replication to Site B (2 nodes)
   - Verify metadata service responding
   - Check network connectivity between all nodes (mTLS)
   - Output: `cluster-health-report.json` with per-node status

4. **Cluster Topology Verification**
   - Fetch cluster membership from metadata service
   - Verify 5 storage nodes registered, 2 FUSE clients, 1 Jepsen controller
   - Verify consistent hash ring initialized
   - Check SWIM gossip convergence (all nodes aware of each other)
   - Output: `cluster-topology.json`

**Success Criteria:**
- ✅ All 9 nodes provisioned and running
- ✅ Both FUSE mounts operational
- ✅ Raft consensus healthy (no leader elections in 60 sec)
- ✅ Cross-site replication active
- ✅ Network connectivity verified

### Tier 3: Multi-Node POSIX Testing (Days 3-4)
**Objective:** Run POSIX suites on multi-node cluster, execute from both FUSE clients

**Deliverables:**
1. **pjdfstest Full Suite on Cluster**
   - Execute on FUSE client 1: Full pjdfstest (847 tests)
   - Groups tested: core, permissions, links, symlinks, directory, rename, unlink, open, misc
   - Simultaneously mount on FUSE client 2, verify consistency
   - Run time: ~45 minutes
   - Output: `test-results/cluster-pjdfstest-full.json`
   - Success criteria: 95%+ pass rate, no hang or crash

2. **fsx Soak Test on Cluster**
   - Execute on FUSE client 1: fsx with 100K-1M operations
   - Run time: ~1-2 hours
   - Monitor: Memory usage, CPU, disk I/O during test
   - Verify: No data corruption detected
   - Output: `test-results/cluster-fsx-soak.json`
   - Success criteria: 100% data integrity, no crashes

3. **Connectathon Multi-Client Test**
   - Execute from both FUSE clients simultaneously:
     - Client 1: File creation, linking, locking
     - Client 2: Concurrent reads, deletes, rename operations
   - Verify consistency: Both clients see same metadata
   - Run time: ~20 minutes
   - Output: `test-results/connectathon-multi-client.json`
   - Success criteria: 100% pass, no race conditions or corruption

4. **Performance Baseline on Cluster**
   - Run FIO with parallel clients (2 FUSE clients, 4 threads each)
   - Capture: Throughput, latency (p50/p95/p99), IOPS
   - Store: `baselines/phase2-cluster-baseline.json`
   - Compare vs local baseline for regression
   - Success criteria: Baseline captured, no >20% regression vs local

**Success Criteria:**
- ✅ pjdfstest full: 95%+ pass (all client combinations)
- ✅ fsx soak: 100% data integrity
- ✅ Connectathon: No consistency violations
- ✅ Performance: Baselines captured, <20% regression vs Phase 30

### Tier 4: Regression Detection & Reporting (Day 4)
**Objective:** Establish baseline metrics for ongoing regression tracking

**Deliverables:**
1. **Regression Baseline Initialization**
   - Create `baselines/phase2-session2-baseline.json` with:
     - Test counts per module (847 pjdfstest, 100K fsx ops, etc.)
     - Performance metrics (throughput MB/s, latency ms)
     - Per-test duration averages
   - Store in git for version control
   - Success criteria: Baseline file committed

2. **Flaky Test Report**
   - Execute all tests 3 times, track pass/fail/pass patterns
   - Identify tests with >5% flake rate
   - Generate GitHub issues for flaky tests
   - Format: `flaky-tests-session2.json`
   - Success criteria: Flaky tests identified and tracked

3. **CHANGELOG Update**
   - Document Session 2 achievements
   - Format: `docs/A9-SESSION2-RESULTS.md`
   - Include: Test counts, pass rates, baselines, any regressions
   - Success criteria: CHANGELOG updated and committed

4. **GitHub Issues (if regressions found)**
   - File issue for each regression vs Phase 30 baselines
   - Title format: `[A9] Regression: <component> <metric> <degradation>`
   - Labels: `regression`, `test-failure`, component-specific
   - Success criteria: Issues filed and tracked

## Execution Timeline

### Day 1 (Local POSIX Validation)
```
09:00 - 11:00  Tier 1: Local pjdfstest quick run
11:00 - 11:30  Break + review
11:30 - 12:30  Tier 1: fsx quick run
12:30 - 14:00  Lunch
14:00 - 16:00  Tier 1: Concurrency tests + baseline capture
16:00 - 17:00  Results review + local baseline storage
```

### Day 2 (Cluster Provisioning)
```
09:00 - 10:00  Tier 2: Cluster provisioning script setup
10:00 - 11:00  Tier 2: Run terraform apply + wait for nodes
11:00 - 12:00  Tier 2: FUSE mount verification on both clients
12:00 - 13:00  Lunch
13:00 - 14:00  Tier 2: Cluster health checks
14:00 - 15:00  Tier 2: Cluster topology verification
15:00 - 17:00  Tier 2: Final validation + node log collection
```

### Day 3 (Multi-Node Testing)
```
09:00 - 09:30  Tier 3: Pre-test cluster state check
09:30 - 10:45  Tier 3: pjdfstest full suite (first 30 min)
10:45 - 11:15  Tier 3: pjdfstest results review
11:15 - 12:00  Tier 3: Connectathon multi-client (basic tests)
12:00 - 13:00  Lunch
13:00 - 15:00  Tier 3: fsx soak test 100K ops
15:00 - 16:00  Tier 3: Performance baseline on cluster
16:00 - 17:00  Tier 3: Results collection + storage
```

### Day 4 (Regression Detection & Reporting)
```
09:00 - 10:00  Tier 4: Baseline initialization
10:00 - 11:00  Tier 4: Flaky test analysis
11:00 - 12:00  Tier 4: GitHub issues (if regressions)
12:00 - 13:00  Lunch
13:00 - 14:00  Tier 4: CHANGELOG update + documentation
14:00 - 15:00  Tier 4: Final review + commit + push
15:00 - 17:00  Slack time + issue resolution
```

## Success Metrics

| Metric | Target | Status |
|--------|--------|--------|
| pjdfstest pass rate (local) | 95%+ | Pending |
| fsx data integrity (local) | 100% | Pending |
| Concurrency tests pass | 100% | Pending |
| Cluster nodes provisioned | 9/9 | Pending |
| FUSE mounts operational | 2/2 | Pending |
| pjdfstest pass rate (cluster) | 95%+ | Pending |
| fsx soak pass rate | 100% | Pending |
| Connectathon pass rate | 100% | Pending |
| Performance regression | <20% vs baseline | Pending |
| Baselines captured | Yes | Pending |
| Flaky tests identified | N/A (target: 0) | Pending |

## Implementation Notes

### Test Execution Commands
```bash
# Local pjdfstest quick
cargo test --lib posix::pjdfs_quick -- --test-threads=1

# fsx quick (assumes mount at /mnt/claudefs)
cargo test --lib soak::fsx_quick -- --test-threads=1

# Concurrency tests
cargo test --lib concurrency_tests -- --test-threads=4

# Cluster provisioning
bash tools/a9-provision-test-cluster.sh staging

# pjdfstest on cluster (from FUSE client 1)
ssh fuse-client-1 "cd /claudefs && cargo test --lib posix::pjdfs_full -- --test-threads=1"

# Connectathon (from both clients)
ssh fuse-client-1 "cargo test --lib connectathon::multi_client" &
ssh fuse-client-2 "cargo test --lib connectathon::multi_client" &
wait
```

### Result Storage Structure
```
test-results/
├── pjdfstest-quick.json
├── fsx-quick.json
├── concurrency.json
├── cluster-pjdfstest-full.json
├── cluster-fsx-soak.json
├── connectathon-multi-client.json
├── cluster-performance.json
└── flaky-tests-session2.json

baselines/
├── phase2-local-baseline.json
├── phase2-cluster-baseline.json
└── phase2-session2-baseline.json
```

### Failure Handling

| Issue | Mitigation |
|-------|-----------|
| FUSE mount fails | Use mock filesystem (in-memory), defer physical testing |
| Cluster provisioning fails | Check Terraform logs, verify IAM roles, retry with `--force` |
| pjdfstest hangs | Kill test after 10 min timeout, investigate specific test |
| fsx data corruption | Enable fsx verbose logging, check write path consistency |
| Network partition | Verify mTLS certificates, check firewall rules |
| Performance regression >30% | File issue, investigate with profiling |

## Blockers & Dependencies

### Blocker 1: FUSE Mount (CRITICAL)
- **Blocker:** A5 needs to provide working FUSE client binary
- **Dependency:** FUSE v3 daemon with mTLS, passthrough mode
- **Mitigation:** Use in-memory mock mount for Phase 1 local testing
- **Timeline:** A5 should deliver before Day 2 of this session

### Blocker 2: Cluster Provisioning (CRITICAL)
- **Blocker:** A11 needs Terraform automation working
- **Dependency:** EC2 provisioning, security groups, IAM roles
- **Mitigation:** Manual provisioning if Terraform fails
- **Timeline:** A11 should have working Terraform before Day 2

### Blocker 3: Test Framework Compilation (LOW)
- **Status:** ✅ Verified — all 1,882 tests compile
- **Mitigation:** Already completed

## Testing Matrix

### Test Scope
```
Phase 2 Session 2 Testing Matrix:

POSIX Tests:
- pjdfstest quick (100 tests, local) ✅
- pjdfstest full (847 tests, cluster) ✅
- fsx quick (1K ops, local) ✅
- fsx soak (100K ops, cluster) ✅

Multi-Node Tests:
- Concurrency (multi-threaded) ✅
- Connectathon (2 FUSE clients) ✅

Performance:
- Baseline collection (local + cluster) ✅
- Regression detection (vs Phase 30) ✅

Reporting:
- Flaky test tracking ✅
- GitHub issues for regressions ✅
- CHANGELOG update ✅
```

## References

- **Test Framework:** `crates/claudefs-tests/src/` (47 modules)
- **Infrastructure:** `tools/terraform/` (A11 provisioning)
- **FUSE Client:** `crates/claudefs-fuse/` (A5)
- **POSIX Docs:** `docs/posix.md`
- **Phase 1 Plan:** `docs/A9-PHASE1-PLAN.md`
- **Session 1 Status:** `docs/A9-SESSION1-STATUS.md`

---

**Commit Pattern:**
- Commit 1: `[A9] Session 2: Local POSIX validation complete (pjdfstest/fsx/concurrency)`
- Commit 2: `[A9] Session 2: Cluster provisioning + FUSE mount verification`
- Commit 3: `[A9] Session 2: Multi-node testing complete (pjdfstest/fsx/Connectathon)`
- Commit 4: `[A9] Session 2: Baselines established, regression tracking initialized`
- Commit 5: `[A9] Update CHANGELOG — Phase 2 multi-node testing complete`

**Expected output by Session 2 end:**
- ✅ 1,850+ local tests passing
- ✅ 2,200+ cluster tests passing
- ✅ Performance baselines established (local + cluster)
- ✅ Regression detection framework initialized
- ✅ Ready for Phase 3 production validation (crash consistency, Jepsen, CrashMonkey)
