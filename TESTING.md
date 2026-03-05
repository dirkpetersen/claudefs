# ClaudeFS Testing Infrastructure

This document describes the comprehensive testing strategy for ClaudeFS across all tiers: unit tests, integration tests, POSIX compliance, distributed consistency, crash resilience, and performance benchmarks.

## Test Tiers

### Tier 1: Unit Tests (Fast, Local)
Location: `crates/*/src/lib.rs`, `#[test]` modules

- **Scope:** Single-crate functionality, trait implementations, data structure correctness
- **Run:** `cargo test --lib` (~2-5 minutes for full workspace)
- **CI:** Runs on every PR, blocks merge if failing
- **Owner:** Each builder agent (A1-A8)

**Per-crate baseline:**
- A1 (storage): 1204 tests — io_uring, block allocator, FDP hints
- A2 (metadata): 997 tests — Raft, KV store, inode ops
- A3 (reduce): ~1927 tests — dedupe, compression, encryption, tiering
- A4 (transport): 1304 tests — RDMA, TCP, RPC protocol
- A5 (fuse): ~1000 tests — FUSE daemon, caching, passthrough
- A6-A8: Additional tests
- A9 (tests): Integration harnesses, POSIX wrappers, property-based tests
- **Total:** ~6300+ tests

### Tier 2: Integration Tests (Medium, Multi-Crate)
Location: `crates/claudefs-tests/src/*.rs`, `#[test] #[ignore]` or test functions in integration modules

- **Scope:** Cross-crate workflows (read-modify-write, dedup pipeline, replication, FUSE mount)
- **Requirements:** ClaudeFS daemon running, storage mounted
- **Run:** `cargo test --test '*' -- --ignored` or `cargo test -p claudefs-tests integration_` (requires local mount)
- **CI:** Runs on staging branch before release, ~15-30 minutes
- **Owner:** A9 (Test & Validation)

**Examples:**
- `write_path_e2e.rs` — Write → dedupe (A3) → block allocation (A1)
- `fuse_tests.rs` — FUSE mount (A5) → metadata (A2) → transport (A4)
- `pipeline_integration.rs` — Chunk pipeline coordination (A3 + A1)

### Tier 3: POSIX Compliance (Slow, Full Validation)
Location: `crates/claudefs-tests/src/posix.rs`, external binaries (pjdfstest, fsx, xfstests)

- **Scope:** Kernel VFS contract validation
- **Run:**
  - `cargo run -p claudefs-tests --bin posix-runner -- pjdfstest` (~1-2 hours)
  - `cargo run -p claudefs-tests --bin posix-runner -- fsx` (~4-8 hours soak)
  - `cargo run -p claudefs-tests --bin posix-runner -- xfstests` (~6-12 hours)
- **CI:** Nightly on staging, ~24 hour full suite
- **Owner:** A9

**Key tests:**
- **pjdfstest:** POSIX path/inode atomicity (6000+ assertions)
- **fsx:** Random read-write, crash recovery consistency
- **xfstests:** Generic Linux FS tests (40+ test groups, 1000+ cases)

### Tier 4: Distributed Consistency (Very Slow, Multi-Node)
Location: `crates/claudefs-tests/src/jepsen.rs`, `crates/claudefs-tests/src/connectathon.rs`

- **Scope:** Network partitions, node crashes, replication convergence
- **Requirements:** Multi-node test cluster (5 storage, 2 clients, Jepsen controller)
- **Run:**
  - Jepsen: `cargo run -p claudefs-tests --bin jepsen-runner -- --nemesis partition-leader` (~1-2 hours per scenario)
  - Connectathon: `cargo run -p claudefs-tests --bin connectathon-runner -- --protocol nfs,smb` (~2-4 hours)
- **CI:** Weekly on staging + release candidates
- **Owner:** A9 (orchestrates), A11 (provisions cluster)

**Jepsen scenarios:**
- `register` — Linearizable register under partition
- `split-brain` — Metadata server split-brain with quorum recovery
- `leader-election` — Raft leader election timing under failure
- `cross-site-replication` — Replica sync consistency, missing writes

**Connectathon:**
- Multi-client concurrent ops via NFS + SMB
- File locking, delegation coherence
- Large file transfers (>1GB)

### Tier 5: Crash Recovery (Medium, Single-Node Crash Simulation)
Location: `crates/claudefs-tests/src/crash.rs`

- **Scope:** Crash-consistency invariants (fsync, ordered writes, recovery)
- **Mechanism:** CrashMonkey-style fault injection via io_uring error injection
- **Run:** `cargo run -p claudefs-tests --bin crash-runner -- --workload small-writes` (~30 minutes)
- **CI:** Nightly on main branch
- **Owner:** A9

**Invariants checked:**
- Fsync guarantee: data visible after fsync() always persists
- Directory atomicity: rename commits or fails, never partial
- Reference counts: inode/block refcounts never underflow
- Journal consistency: Raft log and storage in sync after recovery

### Tier 6: Performance & Regression (Medium, Benchmark)
Location: `crates/claudefs-tests/src/bench.rs`, `tests/fio-baseline.json`

- **Scope:** Throughput, latency, IOPS, dedup effectiveness
- **Run:** `cargo run -p claudefs-tests --bin fio-runner -- config/fio-workload.ini` (~15 minutes per workload)
- **CI:** Nightly on staging
- **Owner:** A9

**Key metrics:**
- Sequential throughput: 1200+ MB/s (stripe across 5 nodes)
- Random IOPS: 100k+ (read), 50k+ (write) at p99 latency <500µs
- Dedup ratio: 2.5x (8KB chunks, 2-pass LZ4, AES-GCM)
- Latency percentiles: p50 <100µs, p99 <500µs, p999 <2ms

**Workloads:**
- `seq-read` — Large sequential reads (1GB files)
- `seq-write` — Large sequential writes with sync
- `rand-read` — Random 4KB reads across 100GB dataset
- `rand-write` — Random 4KB writes with compressible data
- `mixed` — 70% read, 30% write, locality-sensitive

## Running Tests Locally

### Prerequisites
```bash
# Install test binaries (one-time)
cargo install pjdfstest fsx  # or: apt-get install pjdfstest fsx

# Ensure ClaudeFS daemon can start
export CLAUDEFS_STORAGE_DIR=/tmp/cfs-test-storage  # Local NVMe for testing
mkdir -p $CLAUDEFS_STORAGE_DIR
```

### Quick Check (2-5 min)
```bash
cargo test --lib  # All unit tests
```

### Integration Tests (requires mounted filesystem)
```bash
# Start ClaudeFS locally (single-node)
cargo run -p claudefs-fuse -- mount /tmp/cfs-mount &
sleep 2

# Run integration tests
cargo test -p claudefs-tests --lib integration_

# Unmount
fusermount -u /tmp/cfs-mount
```

### Full POSIX Suite (1-4 hours)
```bash
# Mount ClaudeFS
cargo run -p claudefs-fuse -- mount /tmp/cfs-mount &

# Run pjdfstest (1-2 hours, 6000+ assertions)
cd /tmp/cfs-mount
pjdfstest

# Run fsx overnight (4-8 hours, crash recovery validation)
cargo run -p claudefs-tests --bin fsx-runner -- --size 1GB --duration 8h

# Run xfstests (6-12 hours, comprehensive FS tests)
cargo run -p claudefs-tests --bin xfstests-runner
```

## CI/CD Integration

### GitHub Actions Workflows

**`.github/workflows/test-unit.yml`** — Runs on every PR
- Triggers: PR open, push to any branch
- Steps: `cargo test --lib`
- Time: ~5 minutes
- Failure: Blocks merge

**`.github/workflows/test-integration.yml`** — Runs on PR to staging/main
- Triggers: PR to staging or main
- Setup: Provisions 3-node test cluster (A11 orchestrates)
- Steps: Mount ClaudeFS, run integration tests
- Time: ~30-45 minutes
- Failure: Blocks merge to main

**`.github/workflows/test-posix-nightly.yml`** — Nightly on main
- Triggers: Scheduled 2 AM UTC daily
- Setup: Full test cluster
- Steps: pjdfstest, fsx (overnight), xfstests
- Time: 12-24 hours
- Failure: Alerts on Slack (non-blocking for CI/CD)

**`.github/workflows/test-jepsen.yml`** — Weekly on staging
- Triggers: Scheduled Sunday 2 AM UTC, or manual on release candidates
- Setup: Full test cluster (5 storage, 2 clients, Jepsen controller)
- Steps: Jepsen partition tests, Connectathon
- Time: 4-6 hours
- Failure: Blocks release

**`.github/workflows/perf-baseline.yml`** — Nightly on main
- Triggers: Scheduled 1 AM UTC daily
- Setup: Full test cluster with high-performance config
- Steps: FIO suite against baseline
- Time: ~30 minutes
- Failure: Alerts if >10% regress, non-blocking

### CI Matrix Strategy
- **PR (all branches):** Unit tests only, 5 min, blocks merge
- **PR to staging/main:** Add integration tests, 45 min, blocks merge
- **Nightly on main:** POSIX + perf, 12-24 hours, non-blocking, Slack alert
- **Weekly on staging:** Jepsen, 4-6 hours, can block release
- **Release (tag push):** Full suite (unit + integration + posix + jepsen), 24 hours, must pass to publish

## Performance Baseline

File: `tests/fio-baseline.json`

```json
{
  "date": "2026-03-01",
  "cluster": {
    "storage_nodes": 5,
    "client_nodes": 2
  },
  "workloads": {
    "seq_read": {
      "throughput_mbps": 1200,
      "latency_p99_us": 500,
      "variance_pct": 5
    },
    "rand_iops": {
      "read_iops": 100000,
      "write_iops": 50000,
      "latency_p99_us": 500
    },
    "dedup": {
      "ratio": 2.5,
      "cpu_pct": 15,
      "memory_mb": 512
    }
  }
}
```

## Test Failure Triage

### Unit Test Fails
1. **Builder action:** Fix in source code (A1-A8 agent responsible)
2. **PR blocks:** Yes, must pass before merge
3. **Owner:** Agent who wrote the code

### Integration Test Fails
1. **Check crate:** Multiple crates involved — file GitHub issue with all relevant agents tagged
2. **Example:** Cross-crate deadlock in write_path_e2e → tag A1 + A2 + A3 + A5
3. **PR blocks:** Yes if on staging/main branch
4. **Owner:** A9 files issue, builders investigate their crate

### POSIX Test Fails (pjdfstest, fsx, xfstests)
1. **Symptom:** Kernel VFS contract violation (e.g., fsync not persisting, rename partial)
2. **Investigation:** A9 + builder narrow down which operation failed
3. **PR blocks:** Yes on staging/main, non-blocking on feature branches
4. **Owner:** Builders fix invariant violation in their crate; A9 validates fix

### Jepsen Test Fails
1. **Symptom:** Linearizability violation, split-brain detection failure, etc.
2. **Investigation:** Jepsen history analysis (A9 has tools), identify sequence of events
3. **PR blocks:** Yes, blocks release
4. **Owner:** Multiple builders coordinate fix; A9 validates with replay

### Performance Regress
1. **Detection:** Nightly FIO results show >10% drop vs. baseline
2. **Investigation:** Profile individual crates (A1 io_uring, A3 dedupe, A4 transport)
3. **PR blocks:** Non-blocking CI, but high priority issue
4. **Owner:** A9 files GitHub issue with perf data, builders investigate

## Continuous Improvement

### Monthly Test Review
- Last Friday of month, A9 + builders sync
- Review: flaky tests, new failure categories, performance trends
- Updates: baseline refresh, new test cases for recent features
- Output: GitHub issue with Q1 test roadmap

### Quarterly POSIX Suite Expansion
- Integrate new xfstests groups
- Update pjdfstest for new POSIX behaviors
- Validate against POSIX compatibility matrix

### Yearly Security Audit
- A10 fuzzing campaign (FUSE protocol, RPC, NFS gateway)
- Code coverage analysis (sanitizers: ASan, TSan, UBSan)
- Dependency CVE sweep + audit results published

## Dashboards & Monitoring

### Grafana Dashboards (A8/A11 manage)
- **Test Health:** Pass rate per crate, flaky test trend, latency p99
- **Performance:** Throughput, IOPS, latency histogram, dedup effectiveness
- **Cluster:** Node health, replication lag, leader election time

### GitHub Status
- Per-branch CI status in repo homepage
- Latest test run results in CHANGELOG
- Regression alerts → GitHub issue assigned to responsible builder

---

**Document owner:** A9 (Test & Validation) | **Last updated:** 2026-03-05 | **Next review:** 2026-04-01
