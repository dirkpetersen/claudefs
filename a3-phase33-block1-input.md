# Phase 33 Block 1: Dynamic GC Tuning
## OpenCode Implementation Prompt

**Target:** Generate Rust source + integration tests for adaptive garbage collection
**Model:** minimax-m2p5
**Output:** ~600 LOC (source) + test code (20 tests)

---

## Context

ClaudeFS data reduction (A3) uses reference counting for garbage collection of content-addressed storage blocks. Phase 32 completed with 2,419 integration tests validating the baseline pipeline. Phase 33 adds production features; Block 1 focuses on workload-adaptive GC that:

1. Dynamically adjusts collection thresholds based on memory pressure
2. Tuning frequency based on workload characteristics (batch vs streaming vs idle)
3. Provides backpressure handling so GC doesn't stall the write I/O path
4. Implements mark-and-sweep audit to detect reference count inconsistencies

**Key constraint:** All unsafe code isolated to io_uring/FUSE/transport FFI boundaries. GC must be pure safe Rust.

---

## Architecture Context

### Existing `BlockRefCount` Trait (from Phase 31)

```rust
pub trait BlockRefCount {
    async fn increment(&mut self, block_id: BlockId) -> Result<(), ReduceError>;
    async fn decrement(&mut self, block_id: BlockId) -> Result<(), ReduceError>;
    async fn get_refcount(&self, block_id: BlockId) -> Result<u64, ReduceError>;
    async fn delete_block(&mut self, block_id: BlockId) -> Result<(), ReduceError>;
}
```

### Existing Block Store (from Phase 31)

```rust
pub struct BlockStore {
    blocks: Arc<RwLock<HashMap<BlockId, Block>>>,
    metadata: Arc<dyn MetadataEngine>,
    stats: Arc<ReduceStats>,
}
```

### Existing Write Path (from Phase 32)

```
1. Write arrives at metadata-local primary node
2. Deduplicate: BLAKE3 hash → lookup in metadata fingerprint index
3. Compress: LZ4 or Zstd (async, io_uring worker thread)
4. Encrypt: AES-GCM (async)
5. Reference counting: increment for new block or reuse existing (sync)
6. Persist to S3 (async, background)
```

---

## Requirements: Block 1 Implementation

### 1.1 Dynamic GC Controller

**Component:** `GcController` struct + trait

```rust
pub struct GcControllerConfig {
    /// Memory threshold (percentage): trigger aggressive GC when RSS > this
    pub high_memory_threshold: f64,  // default 80%

    /// Memory threshold (percentage): stop GC when RSS < this
    pub low_memory_threshold: f64,   // default 60%

    /// Workload sampling window (seconds)
    pub workload_sample_window_secs: u64,  // default 10

    /// Min/max collection interval
    pub min_collection_interval_ms: u64,  // default 100
    pub max_collection_interval_ms: u64,  // default 5000
}

pub trait GcController: Send + Sync {
    /// Check current memory pressure, return adjusted collection interval
    async fn should_collect(&self) -> Result<Option<Duration>, ReduceError>;

    /// Update workload statistics (call after write batches)
    async fn update_workload_stats(&mut self, batch_size: usize, write_rate_gb_s: f64);

    /// Trigger immediate collection (admin command)
    async fn force_collect(&mut self) -> Result<(), ReduceError>;

    /// Get current thresholds (for monitoring)
    async fn get_thresholds(&self) -> GcThresholds;
}
```

**Responsibilities:**
- Monitor `/proc/self/status` RSS (or use `procfs` crate for clean API)
- Track write batch size and throughput
- Classify workload (high/medium/low activity)
- Recommend collection interval (adaptive)
- Backpressure handling: if collection falls behind, slow down writers

**Key insight:** Collection frequency should be **inverse** to write rate. High write rate = defer collection (let S3 tiering relieve pressure). Low write rate = aggressive collection (background optimization).

### 1.2 Reference Count Validator (Mark-and-Sweep)

**Component:** `ReferenceCountValidator` struct

```rust
pub struct MarkAndSweepAudit {
    /// Blocks seen in inode tree (reachable)
    reachable_blocks: HashSet<BlockId>,

    /// Blocks with refcount > 0 but unreachable
    orphaned_blocks: Vec<BlockId>,

    /// Blocks with refcount overflow (impossible state)
    corrupted_refcounts: Vec<(BlockId, u64)>,

    /// Timestamp of last audit
    last_audit_time: Instant,
}

pub trait ReferenceCountValidator: Send + Sync {
    /// Run full mark-and-sweep audit (background task)
    async fn audit(&mut self) -> Result<MarkAndSweepAudit, ReduceError>;

    /// Correct detected inconsistencies (admin operation)
    async fn reconcile(&mut self, audit: &MarkAndSweepAudit) -> Result<(), ReduceError>;

    /// Register a block as reachable (call during inode tree walk)
    fn mark_reachable(&mut self, block_id: BlockId);
}
```

**Responsibilities:**
- Walk entire inode tree (via metadata engine) and collect all referenced blocks
- Compare against actual block refcounts
- Detect orphaned blocks (refcount > 0 but unreachable)
- Detect over-counted blocks (refcount inconsistency)
- Optionally correct counts (admin-driven reconciliation)

**Integration:** Audit runs as background task, reconciliation is opt-in (admin approval).

### 1.3 Backpressure Mechanism

When GC falls behind (collection interval growing), apply backpressure to writers:

```rust
pub struct GcBackpressure {
    /// If collection latency exceeds this, apply write slowdown
    pub stall_threshold_ms: u64,  // default 1000

    /// Current backpressure delay (adaptive)
    pub current_delay_us: u64,
}

pub fn apply_write_backpressure(&mut self, gc_latency_ms: u64) {
    if gc_latency_ms > self.stall_threshold_ms {
        // Increase delay exponentially, capped at 10ms per write
        self.current_delay_us = (self.current_delay_us * 1.5).min(10_000);
    } else {
        // Linear backoff when pressure reduces
        self.current_delay_us = (self.current_delay_us - 100).max(0);
    }
}
```

**Key:** Backpressure delay is **not** a mutex lock. Instead, callers `tokio::time::sleep()` before attempting write. This allows other tasks to progress.

### 1.4 Integration Points

**In existing write path:**

```rust
// After dedup, before increment refcount:
if let Some(interval) = gc_controller.should_collect().await? {
    if let Some(delay) = gc_backpressure.calculate_delay(gc_latency).await {
        tokio::time::sleep(delay).await;
    }
}

// Update workload stats after batch
gc_controller.update_workload_stats(batch_size, write_rate).await?;
```

**As background task:**

```rust
// In main event loop or separate task:
tokio::spawn({
    let mut controller = gc_controller.clone();
    async move {
        loop {
            if let Ok(Some(_interval)) = controller.should_collect().await {
                // Trigger actual block deletion
                delete_orphaned_blocks(&mut block_store, &mut controller).await.ok();
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
});
```

---

## Test Suite: 20 Integration Tests

All tests marked `#[ignore]` (cluster-only). Use existing `cluster_helpers.rs` (SSH, Prometheus queries).

### Category 1: Memory Pressure Adaptation (4 tests)

**test_gc_threshold_low_memory**
- Setup: Write 1GB of data, force memory > 80% usage (via cgroup if available, else synthetic)
- Expected: GC triggered, collection interval reduced
- Verify: RSS drops within 10 seconds

**test_gc_threshold_high_memory**
- Setup: Memory at 20% usage
- Expected: GC deferred, collection interval increased
- Verify: No immediate collection, collection interval > max_interval

**test_gc_backpressure_under_load**
- Setup: Continuous 100MB/s writes while GC collection latency > 1 second
- Expected: Write backpressure applied (5-10ms delay per write)
- Verify: Write rate decreases but doesn't stall, GC completes within SLA

**test_gc_recovery_after_pressure**
- Setup: High memory pressure, then writes drop to 10MB/s
- Expected: Backpressure linearly decreases, collection catches up
- Verify: Collection interval normalizes within 30 seconds

### Category 2: Workload-Aware Frequency (4 tests)

**test_gc_batch_writes_high_frequency**
- Setup: Large batch writes (1000 blocks per second for 30 seconds)
- Expected: Collection frequency increases (interval < 500ms)
- Verify: Collection happens every 10-20 writes

**test_gc_streaming_low_frequency**
- Setup: Streaming writes (10 blocks per second continuously)
- Expected: Collection deferred (interval > 1000ms)
- Verify: Collection happens every 100+ writes

**test_gc_idle_background_sweep**
- Setup: Write 500MB, then stop all writes
- Expected: Full mark-and-sweep audit triggered within 5 seconds
- Verify: Audit log shows all reachable blocks marked

**test_gc_mixed_workload_adaptation**
- Setup: Alternate between batch (1000/s for 30s) and streaming (10/s for 30s)
- Expected: Collection interval adapts smoothly between high/low frequencies
- Verify: No stalls or missed collections during transitions

### Category 3: Reference Count Consistency (6 tests)

**test_refcount_increment_decrement_balance**
- Setup: Create 10K blocks, increment refcount N times, decrement N times
- Expected: All blocks have refcount 0, eligible for GC
- Verify: Mark-and-sweep audit finds 0 orphaned blocks

**test_refcount_snapshot_safe**
- Setup: Create file, snapshot, modify file (increment different refcount)
- Expected: Snapshot blocks retain original refcount, don't decrease during GC
- Verify: Audit shows refcounts match expected values

**test_refcount_dedup_block_sharing**
- Setup: Write identical data twice (exact dedup), both references same block
- Expected: Block refcount = 2, GC doesn't delete until both references gone
- Verify: Delete first file → refcount 1, delete second → refcount 0 + eligible for GC

**test_refcount_similarity_delta_update**
- Setup: Write similar block (delta compression), original refcount incremented
- Expected: When reference block deleted, delta block becomes invalid
- Verify: Audit detects orphaned delta reference, quarantines it

**test_refcount_orphaned_block_detection**
- Setup: Corrupted metadata points to non-existent block, block has refcount 0
- Expected: Audit marks block as orphaned
- Verify: Orphaned block list includes this block

**test_refcount_multi_snapshot_complex**
- Setup: 5 snapshots with overlapping blocks, delete snapshots in random order
- Expected: Refcounts stay consistent, no premature block deletion
- Verify: After all deletes, audit shows no orphaned blocks

### Category 4: Mark-and-Sweep Audit (6 tests)

**test_mark_sweep_finds_all_reachable**
- Setup: 100K live blocks across 50 files
- Expected: Audit marks all 100K as reachable
- Verify: Audit output matches expected live set (no false negatives)

**test_mark_sweep_detects_orphans**
- Setup: Intentionally create orphaned block (refcount 1, unreachable)
- Expected: Audit detects it
- Verify: Orphaned block list contains it

**test_mark_sweep_corrects_overcounts**
- Setup: Corrupt refcount in metadata (set to 5 when should be 1)
- Expected: Audit detects discrepancy, reconcile corrects it
- Verify: After reconciliation, refcount = 1

**test_mark_sweep_concurrent_safe**
- Setup: Run mark-and-sweep audit while continuous writes (100MB/s)
- Expected: Audit completes without stalling writers
- Verify: Write throughput stays >80MB/s during audit

**test_mark_sweep_large_index_performance**
- Setup: 100M blocks (simulated, or real if cluster available)
- Expected: Audit completes in <1 second
- Verify: Measure audit latency, confirm <1s

**test_mark_sweep_recovery_after_crash**
- Setup: Kill process during audit, restart
- Expected: Audit state recovers cleanly (no partial marks)
- Verify: Restart audit produces consistent results with pre-crash baseline

---

## Implementation Notes

### Memory Monitoring

```rust
use std::fs::File;
use std::io::Read;

pub fn get_rss_bytes() -> Result<u64, std::io::Error> {
    let mut file = File::open("/proc/self/status")?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    content
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "VmRSS not found"))
}
```

### Metrics Export

Export to Prometheus (via existing stats):

```rust
pub struct GcMetrics {
    pub collection_interval_ms: f64,
    pub memory_pressure_percent: f64,
    pub workload_category: String,  // "batch", "streaming", "idle"
    pub write_backpressure_us: u64,
    pub audit_latency_ms: u64,
    pub orphaned_blocks: u64,
    pub corrupted_refcounts: u64,
}
```

### Error Handling

Use existing `ReduceError` enum:

```rust
pub enum ReduceError {
    MemoryPressureHigh,
    GcAuditFailed,
    RefcountCorruption,
    // ... existing variants
}
```

---

## Acceptance Criteria

✅ **Compilation:** `cargo build -p claudefs-reduce --tests` produces zero errors
✅ **Tests:** All 20 tests pass with `cargo test -p claudefs-reduce --test cluster_gc_dynamic -- --ignored --test-threads=1`
✅ **Safety:** Zero unsafe code (all GC logic is pure safe Rust)
✅ **Performance:** Collection doesn't increase write latency >10ms p99
✅ **Metrics:** Prometheus export working (verify via `curl localhost:9090/metrics | grep gc_`)

---

## Deliverables

1. **Rust source code:**
   - `crates/claudefs-reduce/src/gc_controller.rs` (~200 LOC)
   - `crates/claudefs-reduce/src/reference_count_validator.rs` (~180 LOC)
   - `crates/claudefs-reduce/src/gc_backpressure.rs` (~120 LOC)
   - Updates to `src/lib.rs` to export new modules

2. **Integration test code:**
   - `crates/claudefs-reduce/tests/cluster_gc_dynamic.rs` (~500 LOC, 20 tests)

3. **Documentation:**
   - Inline code comments explaining GC state machine
   - Test comments explaining setup/expected behavior

---

## References

- **Phase 32 tests:** `crates/claudefs-reduce/tests/cluster_*.rs` (11 existing test files)
- **Cluster helpers:** `crates/claudefs-reduce/tests/cluster_helpers.rs`
- **Existing stats:** `crates/claudefs-reduce/src/stats.rs` (reference for metrics)
- **Metadata engine:** docs/decisions.md D4 (Raft group topology)
- **Reference counting:** docs/reduction.md (GC problem section)

---

**Status:** ✅ Ready for OpenCode generation
