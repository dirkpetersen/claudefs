# A9: Test & Validation — Next Agent Instructions

**For the next A9 session or the next agent taking over A9**

## Current Status (As of 2026-04-18 Session 1)

### ✅ Completed Work (Phase 1-2 Foundation)
- **26,000+ lines** of test code in `crates/claudefs-tests/` (47 modules, 1,882 tests)
- **Test Infrastructure:**
  - POSIX test harnesses (pjdfstest, fsx, xfstests)
  - Property-based tests (proptest for data transforms)
  - Integration tests (single-node, concurrency, distributed)
  - Chaos injection framework (FaultInjector, Nemesis)
  - Performance benchmarking (FIO, latency tracking)
  - Test result collection (JSON parsing, aggregation)
  - Flaky test tracking (automatic GitHub issues)
  - CHANGELOG generation (automated updates)

- **3 Planning Documents Created:**
  1. `docs/A9-PHASE1-PLAN.md` — Formal test architecture (521 lines)
  2. `docs/A9-SESSION1-STATUS.md` — Infrastructure review (400 lines)
  3. `docs/A9-SESSION2-PLAN.md` — 4-day multi-node roadmap (363 lines)

- **2 Commits Made:**
  1. `bb8d0fe` — Phase 1 plan creation
  2. `d3dabd9` — Session 2 plan creation

### 🟡 Next Priority: Multi-Node Cluster Testing (Phase 2-3)

**Session 2 is ready to execute** — Full 4-day plan documented in `docs/A9-SESSION2-PLAN.md`

## What Needs to Happen Next

### Immediate (Next 1-2 days): Preparation
1. **Check Dependencies**
   - Verify A5 (FUSE Client) has working mount at `/mnt/claudefs`
   - Verify A11 (Infrastructure) has Terraform automation ready
   - If either is missing, coordinate with those agents or use mocks for local testing

2. **Local Test Validation** (Day 1)
   - Run: `cargo test --lib posix` (pjdfstest quick run)
   - Run: `cargo test --lib soak` (fsx quick run)
   - Run: `cargo test --lib concurrency_tests` (multi-thread safety)
   - Expected: 95%+ pass rate on all suites
   - If failures: Debug with builder agents (A1-A8), file issues

3. **Establish Local Performance Baseline** (Day 1)
   - Run: `cargo test --lib bench::run_fio_baseline()` or similar
   - Capture: Throughput (MB/s), latency (p50/p95/p99)
   - Store: `baselines/local-baseline-2026-04-18.json`

### Short-Term (Days 2-4): Cluster Testing
4. **Provision Test Cluster** (Day 2)
   - Create: `tools/a9-provision-test-cluster.sh` (if not exists)
   - Run: `bash tools/a9-provision-test-cluster.sh staging`
   - Verify: All 9 nodes (5 storage, 2 FUSE, 1 Jepsen, 1 conduit) running
   - Expected: 5-10 minutes for provisioning + verification

5. **Verify FUSE Mounts** (Day 2)
   - On FUSE client 1: `cfs mount --cluster-name staging-cluster /mnt/claudefs`
   - On FUSE client 2: Same as above
   - Verify: `mkdir /mnt/claudefs/test && touch /mnt/claudefs/test/file.txt`
   - Both clients should see the same files

6. **Run Full Multi-Node Tests** (Days 3-4)
   - **Day 3:**
     - pjdfstest full suite (847 tests, ~45 min)
     - fsx soak (100K ops, ~30 min)
   - **Day 4:**
     - Connectathon multi-client (both FUSE clients simultaneously, ~20 min)
     - Performance baseline on cluster (~30 min)
     - Regression analysis vs Phase 30 baselines

7. **Establish Cluster Performance Baseline** (Day 4)
   - Run FIO with 2 FUSE clients, 4 threads each (parallel load)
   - Capture: Throughput, latency percentiles, IOPS
   - Store: `baselines/cluster-baseline-2026-04-18.json`
   - Compare vs local baseline for regression detection

8. **Generate Reports & Commit** (Day 4)
   - Flaky tests identified: `test-results/flaky-tests-session2.json`
   - Any regressions >20% vs Phase 30: File GitHub issues
   - CHANGELOG update: Document all test results
   - Final commits: Per Session 2 plan (4 commits suggested)

### Medium-Term (Week 2+): Phase 3 Production Validation
9. **Advanced Testing** (requires full cluster)
   - Jepsen split-brain tests (partition tolerance)
   - CrashMonkey crash consistency (if tool available)
   - 24hr+ soak tests (memory/CPU stability)
   - Advanced chaos injection (simultaneous failures)

10. **Ongoing Maintenance**
    - Weekly regression reports (automated via `changelog_generator.rs`)
    - Flaky test pattern analysis (GitHub issues auto-filed)
    - Performance trend tracking (baselines updated weekly)

## Key Files to Know

### Test Infrastructure (Ready to Use)
```
crates/claudefs-tests/src/
├── lib.rs                           # Main exports (47 modules)
├── posix.rs                         # pjdfstest, fsx, xfstests harnesses
├── posix_compliance.rs              # POSIX validation suite
├── test_collector.rs                # JSON parsing + result aggregation
├── flaky_tracker.rs                 # Flaky test detection
├── changelog_generator.rs           # Automated CHANGELOG updates
├── chaos.rs                         # FaultInjector, network topology
├── jepsen.rs                        # Partition tolerance tests
├── concurrency_tests.rs             # Multi-thread safety
├── crash.rs                         # Crash simulation
├── bench.rs                         # FIO benchmarking
└── [35+ more modules...]            # Integration tests per subsystem
```

### Planning Documents
```
docs/
├── A9-PHASE1-PLAN.md                # Formal test architecture (DONE)
├── A9-SESSION1-STATUS.md            # Infrastructure review (DONE)
├── A9-SESSION2-PLAN.md              # 4-day cluster testing roadmap (READY)
├── posix.md                         # POSIX test suite overview
├── agents.md                        # Agent breakdown + phasing
└── decisions.md                     # Architecture decisions (for context)
```

### Infrastructure & Configuration
```
tools/
├── terraform/                       # Cluster provisioning (A11)
│   ├── main.tf
│   ├── variables.tf
│   └── README.md
├── a9-provision-test-cluster.sh     # Create this! (not yet exists)
└── cfs-dev                          # Developer CLI

monitoring/
├── prometheus.yml                   # Scrape config
└── alerts.yml                       # Alert rules
```

### Results Storage (Create These)
```
test-results/                        # Session results
├── pjdfstest-quick.json
├── pjdfstest-full.json
├── fsx-soak.json
├── connectathon-multi-client.json
├── cluster-performance.json
└── flaky-tests.json

baselines/                           # Performance baselines
├── local-baseline-*.json
├── cluster-baseline-*.json
└── phase2-session2-baseline.json
```

## Commit Message Format

```
[A9] Session 2: <Tier>: <Description>

<Detailed explanation of what was tested, results>

- <Key achievement 1>
- <Key achievement 2>
- <Key achievement 3>

<Test results summary>

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>
```

### Example Commits (Session 2)
1. `[A9] Session 2: Tier 1 Complete — Local POSIX validation (pjdfstest/fsx/concurrency)`
2. `[A9] Session 2: Tier 2 Complete — Cluster provisioning + FUSE mount verification`
3. `[A9] Session 2: Tier 3 Complete — Multi-node testing (pjdfstest/fsx/Connectathon)`
4. `[A9] Session 2: Tier 4 Complete — Baselines established, regression tracking initialized`
5. `[A9] Update CHANGELOG — Phase 2 multi-node testing complete`

## Testing Matrix (Quick Reference)

### Local Tests (Can run on orchestrator)
```
pjdfstest quick:     100 tests, ~10 min, should be 95%+ pass
fsx quick:           1K ops, ~5 min, should be 100% data integrity
concurrency tests:   200+, ~15 min, should be 100% pass
property tests:      400+, ~10 min, should be 100% pass
```

### Cluster Tests (Requires FUSE mount + A11 infrastructure)
```
pjdfstest full:      847 tests, ~45 min, should be 95%+ pass
fsx soak:            100K-1M ops, ~1-2 hrs, should be 100% integrity
Connectathon:        Multi-client consistency, ~20 min, 100% pass
Jepsen:              Partition tests, ~30 min, no split-brain
Performance:         FIO throughput/latency, ~30 min, <20% regression
```

## Critical Success Factors

1. **A5 FUSE Mount Must Work** ← Blocker #1
   - If missing: Use in-memory mock, coordinate with A5
   - Test: `mkdir /mnt/claudefs/test && touch /mnt/claudefs/test/file`

2. **A11 Terraform Must Work** ← Blocker #2
   - If missing: Manual provisioning, coordinate with A11
   - Test: `terraform plan` shows 9 nodes to be created

3. **Network Connectivity Between Nodes**
   - mTLS must work between all nodes
   - Test: `ssh -i key storage-node-1 "curl https://storage-node-2:9001/metrics"`

4. **Enough Disk Space in /tmp** (~100 GB recommended)
   - For test artifacts, logs, baselines
   - Test: `df -h /tmp`

## Common Issues & Fixes

| Issue | Diagnosis | Fix |
|-------|-----------|-----|
| pjdfstest hangs | Check logs in `/tmp/pjdfstest-*.log` | Kill test, investigate specific test |
| FUSE mount fails | Check `cfs mount` logs | Verify A5 client is built, mTLS certs exist |
| Cluster provisioning fails | Check `terraform apply` output | Verify AWS IAM roles, security groups, quotas |
| Performance regression >30% | Check CPU/memory usage during test | Profile with `perf`, check for contention |
| Flaky test (fails 1/3 times) | Check test dependencies | Add determinism, reduce timing sensitivity |
| Network timeout between nodes | Check firewall rules | Verify security groups allow inter-node traffic |

## References

- **Main Test Crate:** `crates/claudefs-tests/` (26K lines, 47 modules)
- **POSIX Overview:** `docs/posix.md`
- **Agent Roles:** `docs/agents.md` (see A9 section, page 7-8)
- **Architecture:** `docs/decisions.md` (context for why tests are designed this way)
- **Session 2 Plan:** `docs/A9-SESSION2-PLAN.md` (detailed 4-day roadmap)

## When This Handoff Was Created

- **Date:** 2026-04-18 (2026-04-18 Session 1)
- **Agent:** A9 (Claude Haiku 4.5)
- **Status:** ✅ Phase 1-2 foundation complete, Phase 2+ ready to execute
- **Next Agent Instructions Location:** This file

---

## Quick Start for Next A9 Session

```bash
# 1. Verify dependencies
ssh fuse-client-1 "df /mnt/claudefs" && echo "FUSE mount OK"
cd /home/cfs/claudefs && terraform -C tools/terraform/ plan && echo "Terraform OK"

# 2. Run local tests (Day 1)
cargo test --lib posix --no-fail-fast 2>&1 | tee test-results/local-tests.log

# 3. Check for regressions
grep -i "^test.*ok" test-results/local-tests.log | wc -l
echo "Expected: 300+ tests passed"

# 4. If local OK, provision cluster and run Tier 2-4
bash tools/a9-provision-test-cluster.sh staging
ssh fuse-client-1 "cargo test --lib connectathon" &
ssh fuse-client-2 "cargo test --lib connectathon" &
wait

# 5. Generate reports and commit
cargo run --bin a9-report-generator > test-results/session2-summary.json
git add -A && git commit -m "[A9] Session 2 Complete: Multi-node validation" && git push
```

Good luck! 🚀
