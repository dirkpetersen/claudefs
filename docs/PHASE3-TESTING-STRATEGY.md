# Phase 3 Testing Strategy

**Document Type:** Test Planning
**Owner:** A11 Infrastructure & CI (coordinating with A9 & A10)
**Last Updated:** 2026-03-01
**Phase:** 3 (Production Readiness)

## Overview

Phase 3 testing focuses on:
1. **Distributed correctness** — Jepsen tests for split-brain scenarios
2. **Crash consistency** — CrashMonkey tests for recovery from node failure
3. **Performance validation** — FIO benchmarks, latency characterization
4. **Security hardening** — Pen testing, fuzzing, unsafe review
5. **Integration validation** — Cross-crate interaction under stress

---

## Test Matrix

### Unit Tests (Baseline)

**Status:** ✅ All passing (3612+ tests)

| Crate | Tests | Status | Phase |
|-------|-------|--------|-------|
| claudefs-storage | 394 | ✅ Pass | 1-2 |
| claudefs-meta | 495 | ✅ Pass | 1-4 |
| claudefs-reduce | 90 | ✅ Pass | 1-3 |
| claudefs-transport | 528 | ✅ Pass | 1-2 |
| claudefs-fuse | 748 | ✅ Pass | 1-6 |
| claudefs-repl | 510 | ✅ Pass | 1-7 |
| claudefs-gateway | 686 | ✅ Pass | 1-7 |
| claudefs-mgmt | 743 | ✅ Pass | 1-8 |
| claudefs-tests | 1054 | ✅ Pass | 1-7 |
| claudefs-security | 148 | ✅ Pass | 1-2 |
| **TOTAL** | **~6438** | **~95%** | - |

### Integration Tests (Phase 3 Primary Focus)

| Test Suite | Coverage | Duration | Owner | Status |
|-----------|----------|----------|-------|--------|
| **Cross-crate** | A1↔A2, A2↔A4, A4↔A5, etc. | ~15 min | A9 | 🟡 Planning |
| **POSIX Compliance** | pjdfstest, xfstests, fsx | ~30 min | A9 | 🟡 Planning |
| **Jepsen** | Distributed consensus, faults | ~45 min | A9 | 🟡 Planning |
| **CrashMonkey** | Crash consistency, recovery | ~20 min | A9 | 🟡 Planning |
| **FIO Benchmark** | Throughput, IOPS, latency | ~10 min | A9 | 🟡 Planning |
| **Security Fuzzing** | RPC protocol, FUSE, NFS | ~30 min | A10 | 🟡 Planning |
| **Pen Testing** | Admin API, management, auth | ~20 min | A10 | 🟡 Planning |
| **TOTAL** | | **~2.5 hours** | | **🟡 Ready for CI** |

---

## Test Execution Timeline

### Week 1: Foundation (When CI Activated)

**Trigger:** Developer pushes workflows
**Duration:** 1-2 hours per run
**Frequency:** Nightly + manual on-demand

**Tests to run:**

```yaml
# nightly_tests.yml (proposed)
name: Nightly Test Suite
on:
  schedule:
    - cron: '0 22 * * *'  # 10pm US Pacific

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: cargo test --lib --all 2>&1 | tee test-results.log
      - if: failure()
        run: |
          echo "Test failures detected:"
          grep "^test.*FAILED" test-results.log
          # Triage: which tests? New or pre-existing?

  integration-tests:
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - run: cargo test -p claudefs-tests --test '*' 2>&1 | tee integration-results.log
      - if: failure()
        run: gh issue create --title "Integration test failure" --body "See logs"

  posix-subset:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: |
          # Run pjdfstest subset (not full suite, ~100 tests)
          cargo test -p claudefs-tests posix_compliance -- --include-ignored 2>&1 | tail -50
```

**Success criteria:**
- All unit tests pass
- Integration tests ≥90% pass rate
- POSIX subset ≥85% pass rate
- No new flaky tests detected

### Week 2-3: Scale Testing

**Trigger:** After first nightly run succeeds
**Duration:** 2-4 hours per run
**Frequency:** 2x/week (Tuesday, Friday)

**Tests to run:**

```yaml
# weekly_scale_tests.yml
name: Weekly Scale Tests
on:
  schedule:
    - cron: '0 9 * * 2,5'  # 9am Tue & Fri

jobs:
  jepsen-distributed:
    runs-on: [self-hosted, large]  # Need 5+ nodes
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - run: |
          # Provision Jepsen cluster
          terraform apply -auto-approve -var="node_count=5"
      - run: |
          # Run Jepsen tests: leader election, log replication, etc.
          cargo test -p claudefs-tests jepsen -- --include-ignored 2>&1 | tee jepsen-results.log
      - run: |
          # Teardown
          terraform destroy -auto-approve
      - if: failure()
        run: |
          # Archive logs for post-mortem
          tar czf jepsen-failure-logs.tar.gz /var/log/cfs-agents
          gh release upload $(git describe --tags) jepsen-failure-logs.tar.gz

  crash-recovery:
    runs-on: [self-hosted, large]
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - run: terraform apply -auto-approve -var="node_count=3"
      - run: |
          # Start cluster, write some data
          cargo test -p claudefs-tests crash -- --include-ignored 2>&1 | tee crash-results.log
      - run: terraform destroy -auto-approve
      - if: failure()
        run: gh issue create --title "Crash recovery test failed" --body "See workflow logs"
```

### Week 4+: Performance & Security

**Trigger:** After scale tests pass consistently
**Duration:** 1-3 hours per run
**Frequency:** Weekly

**Tests to run:**

```yaml
# perf_security_tests.yml
name: Performance & Security Tests
on:
  schedule:
    - cron: '0 18 * * 1'  # 6pm Monday

jobs:
  fio-benchmark:
    runs-on: [self-hosted, large]
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - run: |
          # Baseline FIO run
          fio --name=seq-read --ioengine=libaio --iodepth=32 --rw=read --bs=4k --size=10G
          fio --name=rand-iops --ioengine=libaio --iodepth=64 --rw=randread --bs=4k --size=10G
      - run: |
          # Parse results, compare to baseline
          # Alert if throughput degrades >10%
          python3 tools/compare_benchmarks.py baseline.json results.json

  security-fuzz:
    runs-on: [self-hosted, large]
    timeout-minutes: 120
    steps:
      - uses: actions/checkout@v4
      - run: |
          # Fuzz RPC protocol with libfuzzer
          cargo +nightly fuzz -p claudefs-transport run -- -max_len=8192 -timeout=10
      - run: |
          # Fuzz FUSE interface
          cargo +nightly fuzz -p claudefs-fuse run -- -max_len=4096 -timeout=10
      - if: failure()
        run: |
          # Archive crash reproducer
          gh issue create --title "Fuzzer found crash" --body "See attached reproducer"

  pen-test:
    runs-on: [self-hosted, large]
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - run: |
          # Admin API pen testing (OWASP Top 10)
          python3 tools/pentest_admin_api.py https://localhost:9443
          # Results: injection, auth bypass, etc.
      - if: failure()
        run: gh issue create --title "Pentest found vulnerability" --label security
```

---

## Flaky Test Management

**Problem:** Some tests may pass/fail randomly due to:
- Timing-sensitive code (threading, async)
- Insufficient test isolation
- Resource contention (too many concurrent tests)

**Strategy:**

1. **Detect flakiness** in first 2 weeks of CI
   ```bash
   # Run each test 5x, see how many times it passes
   for i in {1..5}; do
     cargo test -p claudefs-fuse::file_locking_tests --lib || failures=$((failures+1))
   done
   if [ $failures -gt 0 ] && [ $failures -lt 5 ]; then
     echo "Flaky: $failures/5 failures"
   fi
   ```

2. **Triage flakiness**
   - True flake (race condition) → Mark with `#[ignore]` + comment + file issue
   - False flake (test isolation issue) → Fix immediately
   - False flake (resource contention) → Reduce parallelism or split test

3. **Fix in Phase 3**
   - Owner (A1-A10) fixes flaky tests
   - Remove `#[ignore]` once fixed
   - Validate with 10 consecutive runs

4. **Track flakiness metrics**
   - Week 1: Expect 5-10 flaky tests found
   - Week 2-3: <5 flaky tests
   - Week 4+: 0 flaky tests

---

## Test Prioritization

### Must-Pass (Blocking)

These must pass before production deployment:

1. **POSIX compliance** (pjdfstest subset)
   - Core operations: open, read, write, stat, mkdir, unlink
   - Error cases: ENOENT, EACCES, ENOSPC
   - Target: 90% pass rate

2. **Distributed consensus** (Jepsen)
   - Raft leader election under partitions
   - Log replication under failures
   - Metadata consistency
   - Target: 100% pass rate (no split-brain)

3. **Crash recovery** (CrashMonkey)
   - Clean shutdown + restart
   - Dirty shutdown (kill -9) + restart
   - Multi-node failure scenarios
   - Target: 100% recovery without data loss

4. **Security** (A10 audit)
   - Address A10 CRITICAL findings
   - Address A10 HIGH findings (if feasible)
   - Fuzzing: no new crashes
   - Target: 0 unmitigated CRITICAL/HIGH findings

### Should-Pass (High Priority)

These should pass, but can be deferred to Phase 4 if needed:

1. **Performance baselines** (FIO)
   - 4K random read: >100K IOPS
   - 1MB sequential read: >1GB/s (if NVMe)
   - Latency p99: <10ms

2. **Multi-protocol** (NFSv3, S3)
   - Connectathon subset
   - S3 compatibility
   - Target: 80%+ pass rate

3. **Replication** (A6)
   - Cross-site data consistency
   - Failover without data loss
   - Target: 100% data consistency, <1 second RPO

### Nice-to-Have (Low Priority)

These are good to have but not blocking:

1. **Scale testing** (>10 nodes)
2. **Long-running soak** (48+ hours)
3. **ML-specific workloads** (AI inference patterns)

---

## Test Result Analysis

### Metrics to Track

| Metric | Target | Action if Miss |
|--------|--------|-----------------|
| Unit test pass rate | 100% | Investigate immediately |
| Integration pass rate | ≥95% | Investigate, may defer some |
| POSIX compliance | ≥90% | File issue, plan fix |
| Jepsen success rate | 100% | Critical: potential data loss |
| Crash recovery | 100% | Critical: potential data loss |
| Flaky test count | 0 after week 2 | File issue, fix in next cycle |
| Build time | <30 min | Optimize if >35 min |
| Total test time | <2 hours | Optimize if >2.5 hrs |

### Failure Response

**If POSIX compliance <90%:**
- Identify which operations are failing
- File issue for respective agent (A1-A8)
- Prioritize failures (metadata vs data vs admin)
- Target: Fix by end of week

**If Jepsen test fails:**
- **STOP.** Potential data loss bug.
- Archive logs and reproducer
- Investigate with A2 (Raft) + A6 (Replication)
- Do not merge until fixed

**If Crash recovery fails:**
- **STOP.** Potential data loss bug.
- Same as Jepsen failure
- May indicate A1 (storage) or A2 (metadata) issues

---

## CI/CD Integration

### Test Triggering

**Automatic triggers:**
- Every push to main → unit tests (fast: 15 min)
- Nightly 10pm → unit + integration + POSIX (medium: 1 hr)
- Weekly (Tue/Fri) → jepsen + crash + security (slow: 2.5 hrs)

**Manual triggers (developer/CI engineer):**
```bash
# Run specific test immediately
cfs-dev trigger-test --suite jepsen

# Run performance benchmark
cfs-dev trigger-test --suite fio

# Run full validation
cfs-dev trigger-test --all
```

### Test Reporting

**GitHub workflow status:**
```
✅ ci-build: Build + fmt + clippy (30 min)
✅ tests-all: Unit tests (45 min)
✅ integration-tests: Cross-crate + POSIX (60 min)
⏳ a9-tests: Jepsen + CrashMonkey (in progress...)
```

**Dashboard (Grafana):**
- Test pass rate (%) over time
- Test duration over time (detect regressions)
- Flaky test count
- Agent test coverage

**GitHub Issues (with labels):**
- `label: flaky` — Flaky tests
- `label: performance` — Performance regression
- `label: security` — Security-related failures
- `label: P0` — Blocking issues

---

## Success Criteria for Phase 3

| Criterion | Target | Measurement |
|-----------|--------|-------------|
| **Unit tests** | 100% pass | All 6438 tests pass |
| **Integration** | ≥95% pass | Cross-crate tests ≥95% |
| **POSIX** | ≥90% pass | pjdfstest subset ≥90% |
| **Jepsen** | 100% pass | 0 split-brain scenarios |
| **Crash** | 100% recover | All recovery scenarios work |
| **Build time** | <30 min | Measured in CI |
| **Test time** | <2 hours | Total nightly suite |
| **Flaky tests** | 0 | After week 2 |
| **Security** | 0 CRITICAL | A10 findings fixed |
| **Perf baseline** | Documented | FIO results recorded |

---

## Post-Phase-3 (Phase 4)

### Test Scope Expansion
- Connectathon full suite (NFS compliance)
- Extended soak tests (48+ hours)
- Scale-to-20-nodes testing
- AI/ML workload simulation

### Test Infrastructure
- Dedicated test lab (5+ always-on nodes)
- Automated result aggregation (Splunk/DataDog)
- Performance regression detection
- Automated rollback on test failures

---

**Document Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-01
**Next Review:** 2026-03-08 (post first CI run)
