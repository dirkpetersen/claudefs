# A1 Phase 11 Block 2: Flash Layer Defragmentation & GC — OpenCode Implementation

**Context:** ClaudeFS Storage Engine (A1), Phase 11 Block 2 implementation. Builds on Phase 11 Block 1 (online node scaling). All code must pass `cargo build` and `cargo test --release` with zero clippy warnings.

**Target:** Implement 3 modules (defrag_engine.rs, garbage_collector.rs, allocator_insights.rs) with 19 comprehensive tests totaling ~650 LOC.

---

## Architecture Overview

### System Context
- **Crate:** `crates/claudefs-storage`
- **Related Modules:** `block_allocator.rs` (buddy allocator), `tier_orchestrator.rs` (S3 tiering)
- **Key Constraint:** Background GC must not block foreground I/O
- **Target:** <2% overhead during normal I/O load

### Existing Related Code
- **block_allocator.rs** — Buddy allocator with free lists
- **write_journal.rs** — Write durability, segment packing
- **background_scheduler.rs** — Task scheduling with priorities
- **tier_orchestrator.rs** — S3 eviction policy

### Dependencies
- `tokio` — async runtime
- `parking_lot` — efficient locks
- `dashmap` — concurrent HashMap
- `tracing` — distributed tracing
- Standard: `std::sync::{Arc, Mutex, atomic::*}`, `std::collections::{VecDeque, HashMap, BTreeMap}`

---

## Module 1: defrag_engine.rs (~300 LOC)

**Purpose:** Identify fragmented regions, copy live segments, update extent maps atomically.

### Public API
```rust
pub struct DefragEngine {
    block_allocator: Arc<BlockAllocator>,
    extent_map: Arc<RwLock<ExtentMap>>,
    defrag_scheduler: Arc<BackgroundScheduler>,
    load_monitor: Arc<LoadMonitor>,
}

impl DefragEngine {
    pub fn new(
        block_allocator: Arc<BlockAllocator>,
        extent_map: Arc<RwLock<ExtentMap>>,
        defrag_scheduler: Arc<BackgroundScheduler>,
        load_monitor: Arc<LoadMonitor>,
    ) -> Self { ... }

    /// Find fragmented regions (>50% free in 10MB chunk)
    pub fn identify_defrag_candidates(&self) -> Result<Vec<DefragCandidate>, DefragError> { ... }

    /// Start defragmentation of a region
    pub async fn defragment_region(
        &self,
        candidate: DefragCandidate,
    ) -> Result<DefragResult, DefragError> { ... }

    /// Get defragmentation progress
    pub fn get_defrag_progress(&self) -> DefragProgress { ... }

    /// Pause defragmentation (e.g., due to high load)
    pub fn pause_defragmentation(&self) { ... }

    /// Resume defragmentation
    pub fn resume_defragmentation(&self) { ... }

    /// Estimate time to defragment region
    pub fn estimate_defrag_time(
        &self,
        bytes: u64,
    ) -> std::time::Duration { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum DefragError {
    #[error("fragmentation detection failed")]
    DetectionFailed,
    #[error("copy failed: {0}")]
    CopyFailed(String),
    #[error("extent map update failed")]
    ExtentMapUpdateFailed,
    #[error("snapshot reference prevents defrag")]
    SnapshotRefPresent,
    #[error("concurrent write conflict")]
    WriteConflict,
}

pub struct DefragCandidate {
    pub region_start_block: u64,
    pub region_size_blocks: u64,
    pub free_blocks: u64,
    pub live_segments: usize,
    pub estimated_copy_bytes: u64,
    pub fragmentation_ratio: f64,
}

pub struct DefragResult {
    pub region_start_block: u64,
    pub bytes_moved: u64,
    pub bytes_freed: u64,
    pub time_elapsed_ms: u64,
    pub segments_consolidated: usize,
}

pub struct DefragProgress {
    pub active_defrag_count: usize,
    pub bytes_moved_total: u64,
    pub bytes_freed_total: u64,
    pub estimated_next_defrag_ms: u64,
    pub load_adaptive_paused: bool,
}

pub struct LoadMonitor {
    current_load: Arc<AtomicU32>, // 0-100 percentage
}

impl LoadMonitor {
    pub fn get_load_percentage(&self) -> u32 { ... }
    pub fn should_pause_defrag(&self) -> bool {
        // Pause if >80% load
        self.get_load_percentage() > 80
    }
}
```

### Implementation Details

**Algorithm: Identify & Compact**
1. Scan extent map for free space: collect all free extents in 10MB chunks
2. For each chunk: calculate `fragmentation_ratio = free_space / chunk_size`
3. If ratio > 0.5: mark as defrag candidate
4. For defrag:
   - Read all live segments in candidate region
   - Write to new compact location (using buddy allocator)
   - Update extent map atomically (old extent → new extent)
   - Mark old region as free
5. Verify checksums before/after to ensure no corruption

**Key Properties:**
- ✅ Non-blocking (async, respects load monitoring)
- ✅ Atomic extent map updates (via RwLock transaction)
- ✅ Crash-safe (can resume mid-defrag)
- ✅ Snapshot-aware (don't defrag regions with snapshot refs)
- ✅ Load-adaptive (pause at >80% I/O utilization)

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Fragment detection
    #[tokio::test]
    async fn test_defrag_fragmentation_detection() {
        // Given: 10MB region with 5MB live, 5MB free (fragmented)
        // When: identify_defrag_candidates() called
        // Then: region returned as candidate with fragmentation_ratio = 0.5
        // Given: 10MB region with 9.5MB live, 0.5MB free (not fragmented)
        // Then: NOT included in candidates
    }

    // Test 2: Copy correctness
    #[tokio::test]
    async fn test_defrag_copy_correctness() {
        // Given: segment with data blocks [A, B, C] + checksums
        // When: defragment_region() copies to new location
        // Then: checksum(A+B+C) same before/after
        //       no blocks lost or corrupted
    }

    // Test 3: Atomicity
    #[tokio::test]
    async fn test_defrag_atomicity() {
        // Given: defrag in progress
        // When: crash happens mid-defrag
        // Then: extent map either has old mapping or new mapping (not partial)
        //       no split-brain state
    }

    // Test 4: Load adaptive
    #[tokio::test]
    async fn test_defrag_load_adaptive() {
        // Given: load at 75%, defrag active
        // Then: defrag continues
        // When: load jumps to 85%
        // Then: defrag pauses
        // When: load drops to 70%
        // Then: defrag resumes
    }

    // Test 5: Performance overhead
    #[tokio::test]
    async fn test_defrag_performance() {
        // Given: normal I/O load, defrag running
        // When: measure I/O latency
        // Then: p99 latency increase <2% vs no defrag
    }

    // Test 6: Snapshot handling
    #[tokio::test]
    async fn test_defrag_with_snapshots() {
        // Given: snapshot S holds reference to segment in defrag region
        // When: defragment_region() called
        // Then: returns SnapshotRefPresent error
        //       region not defragmented until snapshot released
    }

    // Test 7: Extent map consistency
    #[tokio::test]
    async fn test_defrag_extent_map_consistency() {
        // Given: defrag complete
        // When: query extent map for old region
        // Then: get "moved to new location" info
        // When: query extent map for new region
        // Then: segments found with same data
    }

    // Test 8: Concurrent writes
    #[tokio::test]
    async fn test_defrag_concurrent_writes() {
        // Given: defrag in progress on region R
        // When: write to unrelated region R2
        // Then: write succeeds immediately (no blocking)
        // When: write to region R (being defragged)
        // Then: write goes to old location until defrag completes
    }

    // Test 9: Crash recovery
    #[tokio::test]
    async fn test_defrag_crash_recovery() {
        // Given: defrag 50% complete, crash
        // When: restart
        // Then: defrag can resume from 50% checkpoint (not from 0%)
        //       no duplicate copies
    }
}
```

---

## Module 2: garbage_collector.rs (~200 LOC)

**Purpose:** Remove segments marked for deletion after ref count = 0.

### Public API
```rust
pub struct GarbageCollector {
    extent_map: Arc<RwLock<ExtentMap>>,
    pending_deletes: Arc<DashMap<SegmentId, PendingDelete>>,
    gc_scheduler: Arc<BackgroundScheduler>,
}

impl GarbageCollector {
    pub fn new(
        extent_map: Arc<RwLock<ExtentMap>>,
        gc_scheduler: Arc<BackgroundScheduler>,
    ) -> Self { ... }

    /// Mark segment for deletion (after refcount = 0)
    pub fn mark_for_deletion(
        &self,
        segment_id: SegmentId,
    ) -> Result<(), GcError> { ... }

    /// Check if segment should be deleted
    pub fn check_deletion_ready(
        &self,
        segment_id: SegmentId,
    ) -> Result<bool, GcError> { ... }

    /// Perform garbage collection (batch delete)
    pub async fn perform_gc(
        &self,
    ) -> Result<GcStats, GcError> { ... }

    /// Get current GC statistics
    pub fn get_gc_stats(&self) -> GcStats { ... }

    /// Set GC threshold (trigger collection when >N segments pending)
    pub fn set_gc_threshold(&self, threshold: usize) { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum GcError {
    #[error("segment not found")]
    NotFound,
    #[error("segment still has references")]
    ReferencesRemain,
    #[error("erase failed: {0}")]
    EraseFailed(String),
    #[error("concurrent delete conflict")]
    DeleteConflict,
}

pub struct PendingDelete {
    pub segment_id: SegmentId,
    pub ref_count: Arc<AtomicUsize>,
    pub marked_time: std::time::Instant,
}

pub struct GcStats {
    pub segments_collected: usize,
    pub bytes_freed: u64,
    pub time_elapsed_ms: u64,
    pub pending_count: usize,
    pub slow_erase_count: usize,
}
```

### Implementation Details

**Algorithm: Batch Collection**
1. Periodically (every 10 min or >100 pending): scan pending_deletes
2. For each pending: check if ref_count = 0
3. If ready: erase from storage device, remove from pending
4. Collect erase time, handle slow erases (>1s) gracefully
5. Report freed space to tier orchestrator for S3 decisions

**Key Properties:**
- ✅ Batch collection (efficient, predictable)
- ✅ Reference counting accurate
- ✅ Handles slow drives (erase can take seconds)
- ✅ Metrics tracking (freed space, erase latency)

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Reference counting
    #[tokio::test]
    async fn test_gc_reference_counting() {
        // Given: segment with refcount = 2
        // When: decrease to 1
        // Then: mark_for_deletion() fails (ReferencesRemain)
        // When: decrease to 0
        // Then: mark_for_deletion() succeeds
    }

    // Test 2: Batch collection
    #[tokio::test]
    async fn test_gc_batch_collection() {
        // Given: 150 segments marked for deletion
        // When: perform_gc() called
        // Then: all 150 collected in single batch
        //       time_elapsed < 5s
    }

    // Test 3: Concurrent deletes
    #[tokio::test]
    async fn test_gc_concurrent_deletes() {
        // Given: 10 segments ready to delete
        // When: perform_gc() called
        // Then: all deleted atomically (no partial state)
    }

    // Test 4: Slow drives
    #[tokio::test]
    async fn test_gc_slow_drives() {
        // Given: device erase latency = 2s per segment
        // When: perform_gc() with 10 segments
        // Then: doesn't hang, completes in <30s
        //       reports slow_erase_count = 10
    }

    // Test 5: Metrics
    #[tokio::test]
    async fn test_gc_metrics_tracking() {
        // Given: GC collects 50 segments (100MB)
        // When: perform_gc() completes
        // Then: get_gc_stats() shows segments_collected=50, bytes_freed=100MB
    }

    // Test 6: GC threshold
    #[tokio::test]
    async fn test_gc_threshold_collection() {
        // Given: GC threshold = 100 segments
        // When: 99 segments pending
        // Then: automatic collection not triggered
        // When: 100th segment marked
        // Then: collection triggered
    }
}
```

---

## Module 3: allocator_insights.rs (~150 LOC)

**Purpose:** Report fragmentation metrics, predict when defrag needed.

### Public API
```rust
pub struct AllocatorInsights {
    extent_map: Arc<RwLock<ExtentMap>>,
    defrag_engine: Arc<DefragEngine>,
    gc: Arc<GarbageCollector>,
}

impl AllocatorInsights {
    pub fn new(
        extent_map: Arc<RwLock<ExtentMap>>,
        defrag_engine: Arc<DefragEngine>,
        gc: Arc<GarbageCollector>,
    ) -> Self { ... }

    /// Get fragmentation metrics
    pub fn get_fragmentation_metrics(&self) -> FragmentationMetrics { ... }

    /// Predict if defragmentation needed soon
    pub fn predict_defrag_needed(&self) -> DefragPrediction { ... }

    /// Calculate composite allocator health score (0-100)
    pub fn get_allocator_health_score(&self) -> u32 { ... }

    /// Get per-pool allocation stats
    pub fn get_pool_stats(&self) -> Vec<PoolStats> { ... }
}

pub struct FragmentationMetrics {
    pub total_blocks: u64,
    pub allocated_blocks: u64,
    pub free_blocks: u64,
    pub fragmented_regions: usize,
    pub largest_free_extent_blocks: u64,
    pub fragmentation_ratio: f64, // 0.0 = perfect, 1.0 = worst
}

pub struct DefragPrediction {
    pub defrag_needed: bool,
    pub urgency: DefragUrgency, // None, Low, Medium, High, Critical
    pub predicted_time_to_urgency_ms: u64,
    pub recommended_action: String,
}

pub enum DefragUrgency {
    None,
    Low,      // <60% fragmentation
    Medium,   // 60-75% fragmentation
    High,     // 75-90% fragmentation
    Critical, // >90% fragmentation
}

pub struct PoolStats {
    pub pool_id: PoolId,
    pub total_capacity: u64,
    pub used_space: u64,
    pub free_space: u64,
    pub fragmentation_ratio: f64,
}
```

### Implementation Details

**Algorithm: Analyze & Predict**
1. Scan extent map: collect all extents, free space
2. Calculate metrics: fragmentation_ratio = (total - largest_free_extent) / total
3. Predict urgency based on ratio + recent churn rate
4. Calculate health score: 100 - (fragmentation_ratio * 100)

**Key Properties:**
- ✅ Accurate fragmentation calculation
- ✅ Predicts defrag need with high confidence
- ✅ Health score composite (0-100 scale)
- ✅ Per-pool statistics

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Fragmentation metrics
    #[tokio::test]
    async fn test_fragmentation_metrics() {
        // Given: 1000 blocks, 600 allocated, 400 free (scattered)
        // When: get_fragmentation_metrics()
        // Then: allocated_blocks = 600, free_blocks = 400
        //       fragmentation_ratio between 0.3-0.5
    }

    // Test 2: Defrag prediction
    #[tokio::test]
    async fn test_defrag_prediction() {
        // Given: fragmentation_ratio = 0.85 (high fragmentation)
        // When: predict_defrag_needed()
        // Then: defrag_needed = true, urgency = High
        // Given: fragmentation_ratio = 0.2 (low fragmentation)
        // Then: defrag_needed = false, urgency = None
    }

    // Test 3: Health score
    #[tokio::test]
    async fn test_allocator_health_score() {
        // Given: fragmentation_ratio = 0.0 (perfect)
        // When: get_allocator_health_score()
        // Then: score = 100
        // Given: fragmentation_ratio = 0.5
        // Then: score = 50
    }

    // Test 4: High churn
    #[tokio::test]
    async fn test_insights_under_churn() {
        // Given: rapid alloc/free cycles (10K ops/sec)
        // When: compute metrics every 1s
        // Then: metrics stable (no thrashing), health score smooth
    }
}
```

---

## Integration Testing

These 3 modules work together:

```
AllocatorInsights
    → identifies_fragmented_regions
    → predicts_defrag_urgency

DefragEngine
    → triggered_by_Insights
    → compacts_regions
    → marks_for_deletion

GarbageCollector
    → collects_deleted_segments
    → frees_space
    → reports_metrics_to_Insights

Loop: Insights → DefragEngine → GarbageCollector → Insights
```

---

## Code Style & Conventions

**Async Runtime:**
- All I/O via Tokio
- `#[tokio::test]` for async tests
- Load monitoring in background

**Error Handling:**
- Define `#[derive(thiserror::Error)]` error types
- Use `Result<T, ModuleError>` returns
- Test all error paths

**Concurrency:**
- Use `Arc<RwLock<T>>` for extent map (frequent reads, occasional writes)
- Use `Arc<DashMap<K, V>>` for pending deletes (concurrent updates)
- Use `Arc<AtomicUsize>` for ref counts with `Ordering::SeqCst`

**Testing:**
- Property-based for metrics accuracy (`proptest`)
- Concurrency tests with thread spawning
- Memory bounds: track Arc refcounts
- Naming: `test_<subsystem>_<property>`

**Logging:**
- Use `tracing::info!`, `tracing::warn!` for important events
- Include timing and block counts

---

## Expected Output

**Files to create:**
1. `crates/claudefs-storage/src/defrag_engine.rs` (~300 LOC, 9 tests)
2. `crates/claudefs-storage/src/garbage_collector.rs` (~200 LOC, 6 tests)
3. `crates/claudefs-storage/src/allocator_insights.rs` (~150 LOC, 4 tests)

**Total:** ~650 LOC, 19 tests

**Validation:**
- `cargo build -p claudefs-storage` succeeds
- `cargo test -p claudefs-storage --lib` passes 19/19 new tests + 1301 Phase 10 tests
- `cargo clippy -p claudefs-storage -- -D warnings` has zero errors
- No regressions

---

## Notes for Implementation

1. **Load Monitoring:** Assume `LoadMonitor` provides `get_load_percentage()` and `should_pause_defrag()` methods.

2. **Reference Counting:** Assume segments use `Arc<AtomicUsize>` for ref counts. GC checks `== 0` before deletion.

3. **Crash Recovery:** Store defrag progress in metadata service (shard → (completed_extents, timestamp)). Resume on restart.

4. **Slow Devices:** Don't block GC on erase. If erase takes >1s, spawn separate task, continue other work.

5. **Metrics Export:** Integrate with Prometheus (via `metrics` crate if available, else use `tracing` counters).

6. **Thread Safety:** All public methods must be thread-safe. Test with `Arc<T>` + thread spawning.

---

## Deliverable Checklist

- [ ] All 3 modules compile
- [ ] All 19 tests pass
- [ ] Zero clippy warnings
- [ ] No regressions in Phase 10 tests (1301+)
- [ ] Code follows ClaudeFS conventions
- [ ] Error types properly defined
- [ ] Async/await correctly used
- [ ] Thread safety verified
- [ ] Test coverage >90% per module
- [ ] Documentation complete
- [ ] Ready for `cargo build && cargo test --release`
