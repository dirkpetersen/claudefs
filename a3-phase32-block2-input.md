# A3: Phase 32 Block 2 — Single-Node Dedup on Real Cluster
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 14-18 integration tests validating dedup behavior from FUSE client through real storage node to S3
**Target File:** `crates/claudefs-reduce/tests/cluster_single_node_dedup.rs`
**Target LOC:** 700-850 lines of Rust test code
**Target Tests:** 14-18 comprehensive single-node dedup validation

---

## Context & Dependencies

### Phase 31 Completion
- All Phase 31 tests (2,284 total) validated dedup behavior on **single-machine simulation**
- Deterministic mocking: DedupCoordinator, MockS3Backend, predictable timing
- Verified: fingerprinting, caching, tiering, refcount, multi-tenant isolation, crash recovery

### Block 1 Prerequisite
- Phase 32 Block 1 validates cluster infrastructure (all health checks passed)
- Cluster is ready for workload testing

### Block 2 Objective
Lift Phase 31 dedup tests to **real cluster** where:
- Real FUSE client writes actual files to real ClaudeFS mount
- Real storage node processes requests with real io_uring I/O
- Real S3 backend (actual AWS S3, not mock) for tiering
- Real network latency and packet loss (may occur)
- Real memory constraints and GC behavior

### What Tests This Block
- Single storage node (not multi-node coordination yet — that's Block 3)
- Focus: dedup fingerprinting, caching, tiering, refcount tracking, quota enforcement
- Baseline performance: throughput, latency, resource utilization

---

## Test Specifications (14-18 Tests)

### Test 1: `test_cluster_dedup_basic_write_from_fuse_client`
**Purpose:** Verify basic dedup write path: FUSE client → storage node → fingerprint storage
**Implementation:**
- From FUSE client node, write 100MB test file to `/mnt/claudefs/dedup_basic_test.bin`
  - Use: `dd if=/dev/urandom bs=1M count=100 of=/mnt/claudefs/dedup_basic_test.bin`
- Verify: file exists and size is 100MB
- Query storage node Prometheus:
  - Metric: `claudefs_dedup_fingerprints_stored_total` (should increment by 25 for 4MB chunks)
  - Metric: `claudefs_dedup_bytes_written_total` (should be ~100MB)
- Check S3 bucket: `aws s3 ls s3://${BUCKET}/` should list fingerprint objects (at minimum metadata)
- Assert: file written, fingerprints stored, metrics updated
- Timeout: 60 seconds

### Test 2: `test_cluster_dedup_cache_hit_on_second_write`
**Purpose:** Verify dedup cache: writing same 100MB again results in cache hit (reduced storage I/O)
**Implementation:**
- Create reference file: `dd if=/dev/zero bs=1M count=100 of=/mnt/claudefs/reference.bin`
- Write first time: `cp /mnt/claudefs/reference.bin /mnt/claudefs/dedup_cache_test_1.bin`
- Record metric: `claudefs_dedup_cache_hits_total` (baseline)
- Write second time (identical data): `cp /mnt/claudefs/reference.bin /mnt/claudefs/dedup_cache_test_2.bin`
- Record metric: `claudefs_dedup_cache_hits_total` (after)
- Assert: cache_hits increased (second write had cache hit)
- Also verify: storage I/O reduced (check `claudefs_dedup_storage_io_bytes_total`)
- Timeout: 90 seconds

### Test 3: `test_cluster_dedup_fingerprint_persisted_to_s3`
**Purpose:** Verify fingerprints are persisted to S3 after tiering
**Implementation:**
- Write 200MB unique data: `dd if=/dev/urandom bs=1M count=200 of=/mnt/claudefs/dedup_s3_test.bin`
- Wait for tiering: `sleep 30` (allow background tiering to run)
- Query S3 bucket: `aws s3 ls s3://${BUCKET}/dedup-fingerprints/ --recursive | wc -l`
- Should see fingerprint objects in S3
- Verify object content: `aws s3 cp s3://${BUCKET}/dedup-fingerprints/<object> /tmp/fp.bin`
- Assert: fingerprints persisted to S3 (not empty, valid format)
- Timeout: 120 seconds (includes tiering latency)

### Test 4: `test_cluster_dedup_refcount_accurate_after_deletes`
**Purpose:** Verify refcount tracking: write 10 references, delete 8, verify refcount=2
**Implementation:**
- Create 10 copies of same 10MB file:
  ```bash
  dd if=/dev/urandom bs=1M count=10 of=/mnt/claudefs/rc_template.bin
  for i in {1..10}; do cp /mnt/claudefs/rc_template.bin /mnt/claudefs/rc_file_$i.bin; done
  ```
- Query metric: `claudefs_dedup_references_total` for template fingerprint (should be 10)
- Delete 8 copies: `rm /mnt/claudefs/rc_file_{1..8}.bin`
- Wait: `sleep 10` (allow refcount update)
- Query metric: `claudefs_dedup_references_total` (should be 2)
- Assert: refcount decremented correctly
- Timeout: 60 seconds

### Test 5: `test_cluster_dedup_coordination_real_rpc`
**Purpose:** Verify dedup coordination via real RPC (coordination shard, fingerprint routing)
**Implementation:**
- From client node 1: write 50MB file A
- From client node 2 (simultaneously or shortly after): write same 50MB file B
- Both should route fingerprints to same coordination shard
- Query: `claudefs_dedup_shard_queries_total` (both clients queried same shard)
- Query: `claudefs_dedup_coordination_conflicts_total` (should be 0 or low — LWW resolved)
- Verify: both files reference same fingerprints (query storage node state)
- Assert: coordination worked, no data duplication
- Timeout: 120 seconds

### Test 6: `test_cluster_dedup_throughput_baseline`
**Purpose:** Measure single-node dedup fingerprinting throughput (ops/sec)
**Implementation:**
- Write 1GB of data in chunks:
  ```bash
  for i in {1..20}; do
    dd if=/dev/urandom bs=1M count=50 of=/mnt/claudefs/throughput_test_$i.bin &
  done
  wait
  ```
  (20 parallel 50MB writes)
- Measure time: `time wait`
- Query metrics:
  - `claudefs_dedup_fingerprints_processed_total` (should be ~250K fingerprints for 1GB @ 4KB blocks)
  - `claudefs_dedup_processing_duration_seconds` (latency histogram)
- Calculate: fingerprints_total / elapsed_seconds = throughput (target: 50K-100K ops/sec)
- Assert: throughput >= 50_000 ops/sec
- Timeout: 180 seconds

### Test 7: `test_cluster_dedup_latency_p99_write_path`
**Purpose:** Measure write latency P99 (should be <100ms)
**Implementation:**
- Use storage node metrics: `claudefs_dedup_write_latency_seconds` (histogram)
- Query Prometheus: extract P99 bucket (0.1 seconds = 100ms)
- Assert: P99 < 100ms (0.1 seconds)
- If metric not available: fallback to timing individual writes:
  - 100 small writes (1MB each), measure min/avg/max
  - Calculate P99 from results
- Timeout: 120 seconds

### Test 8: `test_cluster_dedup_cache_eviction_under_memory_pressure`
**Purpose:** Verify dedup cache LRU eviction works with real memory constraints
**Implementation:**
- Fill cache: write many different 1MB files until cache is full
  ```bash
  for i in {1..1000}; do
    dd if=/dev/urandom bs=1M count=1 of=/mnt/claudefs/cache_test_$i.bin
  done
  ```
- Monitor: `claudefs_dedup_cache_memory_bytes` (should peak and stabilize)
- After: `claudefs_dedup_cache_evictions_total` (should increase)
- Write new file: verify hit rate drops (new file not in cache)
- Assert: cache memory bounded, evictions happening, no OOM
- Timeout: 300 seconds

### Test 9: `test_cluster_dedup_cross_tenant_isolation_real`
**Purpose:** Verify tenant fingerprints are isolated (tenant A can't see tenant B's data)
**Implementation:**
- If multi-tenant support available:
  - Write with tenant_id=A: `echo "A_DATA" | tee /mnt/claudefs/tenant_a_file.txt`
  - Write with tenant_id=B: `echo "B_DATA" | tee /mnt/claudefs/tenant_b_file.txt`
  - Query storage node: verify tenant_a fingerprints are tagged with A, tenant_b with B
  - Try tenant_a reading tenant_b fingerprints: should fail (permission denied or not found)
- Assert: tenants isolated
- If not supported: skip with `#[ignore]`
- Timeout: 60 seconds

### Test 10: `test_cluster_dedup_crash_recovery_real`
**Purpose:** Verify dedup recovers after storage node crash
**Implementation:**
- Write 100MB file: `dd if=/dev/urandom bs=1M count=100 of=/mnt/claudefs/crash_recovery_test.bin`
- Query: refcount for fingerprints (should be 1)
- Kill storage node process: `ssh storage-node-0 'sudo pkill -9 claudefs-storage'`
- Wait: 10 seconds
- Restart storage node: `ssh storage-node-0 'sudo systemctl restart claudefs-storage'`
- Wait: 30 seconds (recovery/replay)
- Query: refcount again (should still be 1, recovered from journal)
- Verify: file readable via FUSE mount
- Assert: data recovered, no data loss
- Timeout: 120 seconds

### Test 11: `test_cluster_dedup_coordinator_failover_real`
**Purpose:** Verify dedup coordination survives shard leader failure
**Implementation:**
- Identify: dedup coordinator shard leader (query cluster state)
- Write some data: establish initial state
- Kill coordinator leader process on that shard
- Continue writing: should trigger leader election
- Query: new leader elected (wait up to 10 seconds)
- Verify: dedup still working (writes succeed, fingerprints stored)
- Assert: failover successful, no writes lost
- Timeout: 60 seconds

### Test 12: `test_cluster_dedup_network_partition_recovery_real`
**Purpose:** Verify dedup recovers after network partition
**Implementation:**
- Identify: one storage node to partition
- Partition: `ssh storage-node-0 'sudo iptables -A INPUT -j DROP'` (drop all ingress)
- Write attempts: should timeout or fail gracefully
- Wait: 5 seconds
- Restore: `ssh storage-node-0 'sudo iptables -D INPUT -j DROP'`
- Wait: 10 seconds (recovery)
- Resume writes: should succeed
- Assert: partition handled gracefully, recovery works
- Timeout: 60 seconds

### Test 13: `test_cluster_dedup_metrics_accurate`
**Purpose:** Verify Prometheus metrics match internal state
**Implementation:**
- Baseline: query metrics (dedup_fingerprints_stored_total, dedup_cache_hits_total)
- Write operation: known amount of data
- Query metrics: increment should match expected
- Verify: metrics += expected_increment
- Multiple operations: 5-10 writes, verify all metrics consistent
- Assert: Prometheus metrics reflect actual operations
- Timeout: 120 seconds

### Test 14: `test_cluster_dedup_no_data_corruption`
**Purpose:** Verify checksums on reads (no corruption during write/store/retrieve)
**Implementation:**
- Create file with known content (e.g., all 0xAA bytes):
  `dd if=/dev/zero bs=1M count=50 of=/tmp/known.bin | tr '\0' '\252' > /tmp/known_aa.bin`
- Write to FUSE: `cp /tmp/known_aa.bin /mnt/claudefs/checksum_test.bin`
- Read back: `cp /mnt/claudefs/checksum_test.bin /tmp/readback.bin`
- Verify: `cmp /tmp/known_aa.bin /tmp/readback.bin` (identical)
- Also verify: checksums (query storage node `claudefs_dedup_checksum_failures_total` — should be 0)
- Assert: no corruption detected
- Timeout: 120 seconds

### Test 15: `test_cluster_dedup_quota_enforcement_active`
**Purpose:** Verify dedup respects tenant quota limits
**Implementation:**
- Set tenant A quota: 500MB
- Write 400MB: should succeed
- Write 200MB more: should fail (exceeds 500MB quota)
- Query metric: `claudefs_dedup_quota_rejected_writes_total` (should increment)
- Verify: file system reports "quota exceeded" error
- Assert: quota enforced
- Timeout: 120 seconds

### Test 16: `test_cluster_dedup_multi_region_replication`
**Purpose:** Verify fingerprints replicated to Site B via conduit
**Implementation:**
- Write data on Site A (storage-node-0)
- Wait: 30 seconds (replication lag)
- Query Site B storage node: fingerprints should be replicated
- Verify: replication lag metric < 5 seconds
- Assert: fingerprints visible on both sites
- If single-region only: skip with `#[ignore]`
- Timeout: 90 seconds

### Test 17: `test_cluster_tiering_real_s3_backend`
**Purpose:** Verify tiering to real S3 (not mock)
**Implementation:**
- Write 500MB hot data
- Wait: trigger tiering (background process should run)
- Query S3 bucket: `aws s3 ls s3://${BUCKET}/` (should see new objects)
- Verify: object sizes match chunks (4MB or configured size)
- Query metrics: `claudefs_tiering_bytes_to_s3_total` (should be ~500MB)
- Assert: data in S3, metrics updated
- Timeout: 180 seconds

### Test 18: `test_cluster_dedup_performance_vs_phase31`
**Purpose:** Verify cluster dedup performance within 10% of single-machine Phase 31 simulation
**Implementation:**
- Run Phase 31 throughput test (reference from local memory)
- Phase 31 baseline: ~80K fingerprints/sec (from earlier runs)
- Run same workload on real cluster
- Measure: actual throughput
- Calculate: (phase31_baseline - actual) / phase31_baseline
- Assert: difference <= 10% (allows for network latency overhead)
- Timeout: 180 seconds

---

## Implementation Guidelines

### Test Structure
```rust
#[cfg(test)]
mod cluster_single_node_dedup {
    use std::process::Command;
    use std::time::{Duration, Instant};

    // Helper functions for FUSE operations, SSH, metrics queries

    #[test]
    fn test_cluster_dedup_basic_write_from_fuse_client() {
        // Implementation per spec
    }

    // ... 17 more tests ...
}
```

### Helper Functions (Create Once, Reuse)
1. `write_file_fuse(path: &str, size_mb: usize) -> Result<(), String>`
   - Use `dd` command to write to FUSE mount
   - Return success or error message

2. `query_prometheus_metric(metric: &str) -> Result<f64, String>`
   - HTTP GET to /api/v1/query
   - Parse JSON, extract value

3. `read_file_fuse(path: &str) -> Result<Vec<u8>, String>`
   - Read from FUSE mount
   - Return bytes or error

4. `ssh_storage_node(cmd: &str) -> Result<String, String>`
   - SSH to designated storage node, run command

5. `s3_list_objects(prefix: &str) -> Result<Vec<String>, String>`
   - AWS S3 list objects
   - Return object names

6. `wait_for_metric(metric: &str, expected: f64, timeout_secs: u64) -> Result<(), String>`
   - Poll metric until value >= expected or timeout
   - Return success or timeout error

### Error Handling
- Use `Result<(), String>` for tests
- Clear, actionable error messages (operator knows what went wrong)
- If FUSE mount not available: skip with explicit error
- If storage node unreachable: fail with clear message

### Assertions
- Use standard `assert!()`, `assert_eq!()` macros
- Always include descriptive messages

### Test Execution
- Depends on Block 1 passing (cluster health)
- Tests mostly sequential (to avoid resource contention)
- Total runtime target: 15-20 minutes for all 18 tests

---

## Success Criteria

✅ **All 14-18 tests compile** without errors
✅ **All tests run** against real cluster
✅ **All tests pass** when cluster is up and healthy
✅ **Dedup behavior validated** end-to-end (FUSE → storage → S3)
✅ **Performance baseline** established
✅ **Quota enforcement** verified
✅ **Crash/failover recovery** tested
✅ **Zero clippy warnings** in generated code

---

## Files to Generate

- **Output File:** `crates/claudefs-reduce/tests/cluster_single_node_dedup.rs` (700-850 LOC)
  - Tests 1-18 as specified
  - Helper functions for FUSE, SSH, metrics, S3 operations
  - Clear error messages and logging

