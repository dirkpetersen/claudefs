# A3 Phase 31: Blocks 4-6 Implementation (76 Tests)

## Context

This task implements the final three test blocks for ClaudeFS's Data Reduction subsystem (A3), Phase 31. These tests verify performance characteristics, multi-tenant isolation, and production-like sustained operation.

**Completed:** Blocks 1-3 (79 tests)
**This Implementation:** +76 tests (Blocks 4-6)
**Expected Total After Phase 31:** 2,287 tests (2,132 baseline + 155 new)

**Files:** `crates/claudefs-reduce/tests/`
- `performance_scalability.rs` — Block 4: 25 tests
- `multitenancy_multisite.rs` — Block 5: 26 tests
- `soak_production_simulation.rs` — Block 6: 25 tests

---

## Architecture Overview

**Data Reduction Pipeline:**
1. Dedupe — BLAKE3 fingerprinting, CAS exact-match lookup
2. Compression — LZ4 (inline) / Zstd (async)
3. Encryption — AES-GCM authenticated encryption
4. Erasure Coding — 4+2 segment packing
5. Tiering — Flash → S3 migration

**Cluster characteristics:**
- Dedup shards distributed across nodes (consistent hash)
- S3 authoritative; flash is cache
- Cross-node consistency via metadata coordination
- Multi-tenant quotas and backpressure
- Crash recovery via journal replay

---

## Block 4: Performance & Scalability (25 Tests)

**File:** `crates/claudefs-reduce/tests/performance_scalability.rs`

### Overview
Tests performance characteristics under realistic cluster load. Verifies:
- Throughput under various data profiles
- Latency percentiles (p50, p99, p99.9)
- Write/read amplification factors
- Scaling from 4 to 16 nodes
- Resource utilization (memory, CPU, network)

### Metrics Collection

```rust
pub struct PerformanceMetrics {
    throughput_mbps: f64,       // MB/s
    latency_p50_us: u64,        // microseconds
    latency_p99_us: u64,
    latency_p99p9_us: u64,
    write_amplification: f64,   // bytes_written / bytes_input
    read_amplification: f64,
    memory_mb: u64,
    cpu_percent: f32,
    network_mbps: f64,
}

impl PerformanceMetrics {
    fn compare_to_baseline(&self, baseline: &PerformanceMetrics) -> bool {
        // Allow ±10% variance from baseline
        (self.throughput_mbps * 0.9) < baseline.throughput_mbps &&
        baseline.throughput_mbps < (self.throughput_mbps * 1.1)
    }
}
```

### Test Patterns

Performance tests:
1. Allocate test data (distinct file sizes per test)
2. Measure pipeline stages individually
3. Record metrics at test completion
4. Compare to Phase 30 baseline (±10% tolerance)

### Test List (25 tests)

1. **test_throughput_single_large_write_100gb**
   - Single 100GB write, no dedup
   - Measure throughput: expect ~800-1200 MB/s (NVMe baseline)
   - Assert: Throughput in expected range

2. **test_throughput_concurrent_writes_16_nodes_10gb_each**
   - 16 nodes write 10GB each (160GB total)
   - Aggregate throughput: expect ~10-15 GB/s
   - Assert: Linear scaling to ~10-15 GB/s

3. **test_throughput_with_dedup_enabled_90percent_similarity**
   - Write 1GB, 90% same data (10% new)
   - Dedup should reduce effective storage to ~10%
   - Assert: Output bytes ≤ 100MB (10% of input)

4. **test_throughput_with_compression_enabled_8x_ratio**
   - Write highly compressible data (zeros, repeated patterns)
   - LZ4 should achieve ~8:1 compression
   - Assert: Output ≤ 125MB per 1GB input

5. **test_throughput_with_ec_enabled_stripe_distribution**
   - Write 2MB segments (triggers EC 4+2)
   - Measure EC encoding overhead
   - Assert: EC overhead < 10% (parity creation fast)

6. **test_latency_small_write_p50_p99_p99p9**
   - 10,000 small writes (4KB each)
   - Measure latency percentiles
   - Assert: p50 < 100µs, p99 < 500µs, p99.9 < 2ms

7. **test_latency_write_path_stages_breakdown**
   - Single 1GB write
   - Time each stage: dedup, compress, encrypt, EC, S3 upload
   - Assert: No single stage dominates (balanced)

8. **test_amplification_write_amplification_with_tiering_active**
   - Write 1GB to flash, trigger tiering to S3
   - Measure: (EC parity bytes + S3 upload bytes) / input bytes
   - Assert: Write amp ≤ 2.0 (4+2 EC = 1.5x, plus overhead)

9. **test_amplification_read_amplification_ec_reconstruction**
   - EC stripe, read 1 block, must reconstruct via 4 of 6 blocks
   - Measure: (bytes read from disk) / (bytes user requested)
   - Assert: Read amp ≤ 2.0

10. **test_cache_hit_rate_vs_cache_size_curve**
    - Cache sizes: 16MB, 64MB, 256MB, 512MB, 1GB
    - Workload: 80% reads of 20% of data (working set)
    - Assert: Hit rate improves with cache size (diminishing returns)

11. **test_dedup_coordination_latency_p99_under_load**
    - 100k dedup ops/s
    - Measure dedup RPC latency
    - Assert: p99 < 500µs (metadata coordination responsive)

12. **test_quota_enforcement_latency_impact**
    - Write with quota checks enabled
    - Write without quota checks
    - Compare latency
    - Assert: Quota adds < 10% latency

13. **test_backpressure_response_time_degradation**
    - Sustained write rate exceeds capacity
    - Measure response time as backpressure activates
    - Assert: Graceful degradation (not cliff)

14. **test_scaling_nodes_linear_throughput_4_to_16_nodes**
    - Measure throughput with 4, 8, 12, 16 nodes
    - Assert: Throughput scales linearly (within 10%)

15. **test_scaling_dedup_shards_throughput_vs_shard_count**
    - Measure throughput with 4, 8, 16 dedup shards
    - Assert: More shards = higher throughput (less contention)

16. **test_scaling_gc_threads_throughput_impact**
    - GC thread counts: 1, 2, 4, 8
    - Measure GC completion time
    - Assert: More threads = faster GC (linear up to 4 threads)

17. **test_memory_usage_per_node_under_1tb_data**
    - 1TB total data across cluster
    - Measure memory per node
    - Assert: Memory < 10% of data size (acceptable overhead)

18. **test_memory_usage_cache_overhead_per_gb_cached**
    - Cache 1GB, 10GB, 100GB
    - Measure memory per GB cached
    - Assert: ~1% memory overhead per GB (metadata + LRU pointers)

19. **test_cpu_usage_dedup_coordination_per_100k_fps_s**
    - 100k dedup fingerprint lookups per second
    - Measure CPU usage
    - Assert: < 10% CPU per core (mostly I/O bound)

20. **test_cpu_usage_compression_per_gb_s**
    - Compress 100GB with LZ4
    - Measure CPU usage
    - Assert: CPU scales linearly with compression rate

21. **test_cpu_usage_encryption_per_gb_s**
    - Encrypt 100GB with AES-GCM
    - Measure CPU usage
    - Assert: AES-NI makes encryption < 5% CPU overhead

22. **test_disk_io_queue_depth_distribution_under_load**
    - Sustained 1GB/s writes
    - Measure NVMe I/O queue depth distribution
    - Assert: Queue depth 16-32 (efficient scheduling)

23. **test_network_bandwidth_utilized_vs_link_capacity**
    - Replicate data across nodes at line rate
    - Measure network utilization
    - Assert: >80% link utilization (good efficiency)

24. **test_recovery_time_rto_after_single_node_failure**
    - Node fails, measure time to recovery ready
    - Assert: RTO < 30 seconds (leader election + failover)

25. **test_recovery_time_rpo_data_loss_on_node_failure**
    - Node fails, measure data loss window
    - Assert: RPO = journal lag (~1-5 seconds)

---

## Block 5: Multi-Tenant & Multi-Site Operations (26 Tests)

**File:** `crates/claudefs-reduce/tests/multitenancy_multisite.rs`

### Overview
Tests multi-tenant isolation, quotas, and cross-site replication. Verifies:
- Quota enforcement per tenant
- Dedup isolation (shared vs per-tenant)
- Cache separation
- GC isolation
- Cross-site write consistency

### Multi-Tenant Simulation

```rust
pub struct TenantContext {
    tenant_id: u32,
    quota_bytes: u64,
    consumed_bytes: Arc<AtomicUsize>,
}

pub struct MultiTenantCluster {
    tenants: HashMap<u32, TenantContext>,
    nodes: Vec<MockStorageNode>,
}

impl MultiTenantCluster {
    fn write(&self, tenant_id: u32, data: &[u8]) -> Result<BlockId, Error> {
        let tenant = self.tenants.get(&tenant_id)?;
        let consumed = tenant.consumed_bytes.load(Ordering::SeqCst);
        if consumed + data.len() > tenant.quota_bytes {
            return Err(Error::QuotaExceeded);
        }
        // Proceed with write...
    }
}
```

### Test Patterns

Multi-tenant tests verify:
- Quotas enforced independently
- Data isolation (tenant A can't see B's blocks)
- Dedup decision per-tenant or shared (design choice)
- GC doesn't affect other tenants

### Test List (26 tests)

1. **test_tenant_isolation_write_from_tenant_a_not_visible_b**
   - Tenant A writes "hello", Tenant B attempts read
   - Assert: Tenant B reads fail (permission/visibility)

2. **test_tenant_isolation_quota_enforcement_separate_budgets**
   - Tenant A: 100MB quota
   - Tenant B: 50MB quota
   - Assert: Each tenant's writes limited independently

3. **test_tenant_isolation_dedup_across_tenants_not_shared**
   - Tenant A writes "data", Tenant B writes "data" (same)
   - Design decision: dedup per-tenant or cross-tenant?
   - Assert: Behavior matches design (test documents choice)

4. **test_tenant_isolation_cache_not_shared_between_tenants**
   - Tenant A populates cache with block X
   - Tenant B accesses block X (different file)
   - Assert: Cache hit/miss depends on design (test documents)

5. **test_tenant_isolation_gc_doesn't_affect_other_tenants**
   - Tenant A: write 1000 blocks, GC runs
   - Tenant B: concurrent writes
   - Assert: Tenant B writes proceed (not blocked by A's GC)

6. **test_tenant_quota_increase_allows_more_writes**
   - Tenant A: 100MB quota, fills to 100MB
   - Write fails (quota exceeded)
   - Admin increases quota to 200MB
   - Write succeeds
   - Assert: Quota increase effective immediately

7. **test_tenant_quota_decrease_triggers_enforcement**
   - Tenant A: 100MB quota, 50MB consumed
   - Admin decreases quota to 40MB
   - New write attempted (would exceed 40MB)
   - Assert: Backpressure activates (new writes rejected)

8. **test_tenant_quota_overage_backpressure_soft_limit**
   - Soft limit: 80% quota, hard limit: 100%
   - Write to 90% (soft limit exceeded)
   - Assert: Backpressure activates but writes still proceed (warn)

9. **test_tenant_quota_hard_limit_rejects_new_writes**
   - Hard limit: 100% quota
   - Write to 100% (hard limit reached)
   - New write attempted
   - Assert: Write rejected (ENOSPC)

10. **test_tenant_quota_soft_limit_recovery_after_gc**
    - Tenant at 90% quota (soft limit)
    - GC runs, frees 20% (via dedup/compression)
    - Assert: New writes allowed (quota < soft limit)

11. **test_tenant_deletion_cascading_cleanup**
    - Tenant A: 100 blocks
    - Delete tenant
    - Assert: All 100 blocks marked for GC (metadata cleaned)

12. **test_tenant_account_multi_write_path_quota**
    - Write 1GB to quota
    - Measure: which metric affects quota (input, compressed, stored)?
    - Assert: Quota consumption matches design (document choice)

13. **test_multisite_write_consistency_site_a_primary**
    - Site A: primary (receives write first)
    - Write 1GB
    - Assert: Write durability at Site A (before replication)

14. **test_multisite_write_consistency_site_b_async_replica**
    - Site A writes block X (time T1)
    - Site B learns of block X (time T1 + 2s replication lag)
    - Assert: Site B eventually consistent (within 5s)

15. **test_multisite_write_conflict_same_block_both_sites**
    - Site A writes inode I at T1, Site B writes inode I at T2
    - T1 > T2 (A's write is newer)
    - Assert: LWW resolves to A's version (timestamp wins)

16. **test_multisite_dedup_coordination_across_sites**
    - Site A and B write same fingerprint
    - Dedup shard distributes across both sites
    - Assert: Dedup coordination works (shards on both sites)

17. **test_multisite_tiering_decision_consistency**
    - Same block on Site A and B
    - Tiering triggered on Site A (80% full)
    - Block tiered to S3
    - Site B should learn decision (replicated)
    - Assert: Both sites tier at same time (consistency)

18. **test_multisite_cache_coherency_read_after_write_consistency**
    - Write at Site A, cache populated
    - Read at Site B (same block)
    - Assert: B's read gets A's write (replicated)

19. **test_multisite_site_failure_recovery_from_replica**
    - Site A fails
    - Site B (replica) takes over
    - Assert: Data available from Site B (RPO = replication lag)

20. **test_multisite_network_partition_site_latency_spike**
    - Partition for 5s
    - Metadata writes slow at Site A (waiting for B's ack)
    - Partition heals
    - Assert: Writes resume, consistency maintained

21. **test_multisite_split_brain_majority_quorum_prevails**
    - Partition: Site A (1 node) vs Site B (2 nodes)
    - Quorum on Site B should remain available
    - Assert: Majority prevails (Site B active, Site A unavailable)

22. **test_multisite_gc_coordination_both_sites_same_decision**
    - GC at Site A decides to delete block X
    - GC at Site B should make same decision
    - Assert: Both sites agree (no conflicting GC decisions)

23. **test_multisite_quota_enforcement_replicated**
    - Tenant quota replicated to both sites
    - Quota exceeded at Site A
    - Assert: Backpressure at Site B too (replicated quota)

24. **test_multisite_tenant_isolation_across_sites**
    - Tenant A at Site A writes block X
    - Tenant B at Site B writes block Y
    - Assert: Isolation maintained across sites

25. **test_multisite_disaster_recovery_switchover_time_rto**
    - Site A fails
    - Switchover to Site B
    - Assert: RTO < 5 minutes (acceptable for DR)

26. **test_multisite_snapshot_consistency_across_sites**
    - Create snapshot S1 at Site A
    - Replicate to Site B
    - Assert: S1 identical at both sites (consistent snapshot)

---

## Block 6: Long-Running Soak & Production Simulation (25 Tests)

**File:** `crates/claudefs-reduce/tests/soak_production_simulation.rs`

### Overview
Tests sustained operation over hours/days and production-like workloads. Verifies:
- Memory stability (no leaks)
- CPU stability (no runaway threads)
- No deadlocks over extended runtime
- Cache working set stable
- Tiering sustains over 24 hours
- Production workload patterns

### Soak Test Framework

```rust
pub struct SoakTestConfig {
    duration_hours: u32,
    write_rate_mbps: u64,
    reads_per_second: u64,
    workload_pattern: WorkloadPattern,
}

pub enum WorkloadPattern {
    Constant,           // 1GB/s sustained
    Varying,            // Peak 2GB/s, valley 100MB/s
    OLTP,               // 90% read, 10% write
    OLAP,               // Large sequential scans
    Batch,              // Nightly archive job
}

pub struct SoakMetrics {
    start_memory_mb: u64,
    end_memory_mb: u64,
    memory_leak: bool,  // memory_end > memory_start * 1.1
    deadlock_detected: bool,
    panics: Vec<String>,
    recovery_from_errors: usize,
}
```

### Test Patterns

Soak tests:
1. Run for N hours (typically 2-4 hours per test due to CI time)
2. Collect metrics every 5 minutes
3. Check for memory leaks (memory growth > 10%)
4. Detect deadlocks (watchdog timeout)
5. Compare to Phase 30 baseline

### Test List (25 tests)

1. **test_soak_24hr_sustained_1gb_s_write_throughput**
   - 24 hours at 1GB/s writes (simulated via fast clock)
   - Assert: No memory leaks, no deadlocks, avg throughput stable

2. **test_soak_24hr_varying_workload_peak_valleys**
   - Variable load: peak 2GB/s, valley 100MB/s, vary every 1hr
   - Assert: System adapts (backpressure/throttle)

3. **test_soak_24hr_memory_leak_detection**
   - Sample memory every 5 min for 24hr
   - Assert: Memory growth < 10% (no leaks)

4. **test_soak_24hr_cpu_efficiency_no_runaway_threads**
   - CPU usage steady throughout 24hr
   - Assert: CPU growth < 5% (no thread leak)

5. **test_soak_24hr_no_deadlocks_detected**
   - Watchdog monitors for hangs
   - Assert: No hangs > 1 minute (no deadlocks)

6. **test_soak_24hr_cache_working_set_stable**
   - Cache misses recorded every 5 min
   - Assert: Misses stable after warmup (5 min)

7. **test_soak_gc_cycles_proper_cleanup**
   - Multiple GC cycles over 24hr
   - Measure: blocks deleted, refcount consistency
   - Assert: Each GC cycle leaves consistent state

8. **test_soak_tiering_sustained_s3_uploads**
   - Sustained tiering to S3 over 24hr
   - Assert: No orphan blocks (all uploads complete or rolled back)

9. **test_soak_dedup_fingerprint_cache_stable**
   - Fingerprint cache hit rate stable
   - Assert: Cache hit rate ≥ 95% (after warmup)

10. **test_soak_journal_log_rotation_no_buildup**
    - Journal rotates periodically
    - Assert: Journal size bounded (no unbounded growth)

11. **test_production_sim_oltp_workload_mixed_reads_writes**
    - 90% reads, 10% writes (typical OLTP)
    - Workload: small random I/O
    - Assert: Read latency stable, write latency stable

12. **test_production_sim_oltp_metadata_heavy_lookups**
    - Many small metadata lookups (inode, directory)
    - Assert: Lookup latency < 1ms (metadata cached)

13. **test_production_sim_olap_scan_large_sequential**
    - Large sequential scans (1TB+)
    - Assert: Scan throughput > 500MB/s

14. **test_production_sim_batch_nightly_large_archive**
    - Simulate nightly batch job (500GB write)
    - Assert: Completes within SLA (e.g., 1 hour)

15. **test_production_sim_backup_incremental_daily**
    - Daily incremental backup (10% of data changes)
    - Assert: Backup completes in expected time

16. **test_production_sim_media_ingest_burst_load**
    - Sudden burst (media ingest): 10GB/s for 10 minutes
    - Assert: System absorbs burst (backpressure, tiering)

17. **test_production_sim_vm_clone_dedup_heavy**
    - VM clone (high dedup: 95% same, 5% unique)
    - Assert: Storage efficient (dedup ratio 20:1)

18. **test_production_sim_database_snapshot_consistency**
    - Snapshot during DB writes
    - Assert: Snapshot consistent (ACID properties)

19. **test_production_sim_ransomware_encrypted_files**
    - Random encrypted payload (low compressibility)
    - Assert: Storage still tracks (compression ratio ~1:1)

20. **test_production_sim_compliance_retention_worm_enforcement**
    - WORM retention policy (7-year hold)
    - Assert: Blocks not deleted until retention expires

21. **test_production_sim_key_rotation_no_data_loss**
    - Key rotation during sustained writes
    - Assert: No data loss, writes resume post-rotation

22. **test_production_sim_node_failure_recovery_background**
    - Node fails during sustained load
    - Recovery happens in background
    - Assert: Writes continue (may degrade)

23. **test_production_sim_snapshot_backup_incremental**
    - Create snapshot → incremental backup → new snapshot
    - Multiple cycles
    - Assert: Snapshot chain consistent

24. **test_production_sim_tenant_quota_violation_corrective_action**
    - Tenant over quota (GC should run)
    - Assert: GC frees space, quota recovers

25. **test_production_sim_disaster_recovery_failover_scenario**
    - Site A fails → failover to Site B → Site A recovers
    - Assert: Data consistent throughout (RTO < 5 min)

---

## Shared Test Utilities

**File:** `crates/claudefs-reduce/tests/chaos_utils.rs`

Provides shared infrastructure used by Blocks 3-6:

```rust
// Mock S3 backend with configurable failures
pub struct MockS3Backend { ... }

// Chaos injection framework
pub struct ChaosInjector { ... }

// Multi-node/multi-tenant cluster simulator
pub struct MockCluster { ... }

// Performance metrics collection
pub struct PerformanceMetrics { ... }

// Workload generators
pub fn generate_oltp_workload(...) -> Vec<WorkloadOp>
pub fn generate_olap_workload(...) -> Vec<WorkloadOp>
pub fn generate_burst_workload(...) -> Vec<WorkloadOp>

// Utility: timestamp every pipeline stage
pub fn instrument_write(data: &[u8]) -> InstrumentedWrite { ... }
```

---

## Implementation Notes

### Async Runtime
- All tests use `#[tokio::test]`
- Timeouts via `tokio::time::timeout()` (prevent hangs)
- Long-running tests may use `tokio::time::sleep()` with logical time advancement

### Determinism & Reproducibility
- Use seeded `rand::StdRng` (reproducible failures/workload)
- No wall-clock timing assertions
- Logical event counters
- Baseline metrics from Phase 30 used for regression detection

### Performance Assertions
- Use ranges: `assert!(throughput > 900MB/s && throughput < 1100MB/s)` (±10%)
- Avoid hardcoded exact values
- Baseline thresholds tunable per environment

### Cleanup
- Each test allocates `/tmp/claudefs_test_<uuid>/`
- Cleanup on completion (even on failure)
- No cross-test contamination

---

## Success Criteria

✅ All 76 tests compile (no warnings)
✅ All 76 tests pass (100% pass rate)
✅ No memory leaks (memory growth < 10% over soak duration)
✅ No deadlocks (watchdog detects hangs)
✅ Performance within ±10% of Phase 30 baseline
✅ Production workload patterns handle correctly

---

## Files to Create

1. `crates/claudefs-reduce/tests/performance_scalability.rs` — Block 4
2. `crates/claudefs-reduce/tests/multitenancy_multisite.rs` — Block 5
3. `crates/claudefs-reduce/tests/soak_production_simulation.rs` — Block 6

---

## Build & Test

```bash
cargo test -p claudefs-reduce --test performance_scalability
cargo test -p claudefs-reduce --test multitenancy_multisite
cargo test -p claudefs-reduce --test soak_production_simulation
cargo test -p claudefs-reduce  # All tests
cargo clippy -p claudefs-reduce
```

Expected: 76 new tests passing, 0 warnings.

---
