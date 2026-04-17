# A3 Phase 30: Fix Integration Tests — Corrected Module APIs

## Objective

Fix the 4 integration test modules to use correct module APIs discovered from inspection of actual implementations. The tests were generated with incorrect method names and signatures; this prompt corrects them to use real APIs.

## Real Module APIs (From Source Code Inspection)

### 1. WriteCoalescer (`crates/claudefs-reduce/src/write_coalescer.rs`)
```rust
pub struct WriteOp {
    pub inode_id: u64,
    pub offset: u64,
    pub timestamp_ms: u64,
    // NO: priority field (doesn't exist)
}

pub struct CoalesceConfig {
    pub max_gap_bytes: u64,
    pub max_coalesced_bytes: u64,
    pub window_ms: u64,
}

impl WriteCoalescer {
    pub fn new(config: CoalesceConfig) -> Self
    pub fn add(&mut self, op: WriteOp)  // NOT try_add()
    pub fn flush_ready(&mut self, now_ms: u64) -> Vec<CoalescedWrite>
    pub fn flush_inode(&mut self, inode_id: u64) -> Option<CoalescedWrite>
    pub fn flush_all(&mut self) -> Vec<CoalescedWrite>
    pub fn pending_count(&self) -> usize
}
```

### 2. TenantIsolator (`crates/claudefs-reduce/src/tenant_isolator.rs`)
```rust
pub struct TenantId(pub u64);

impl TenantIsolator {
    pub fn register_tenant(&mut self, policy: TenantPolicy)
    pub fn get_policy(&self, tenant_id: TenantId) -> Option<&TenantPolicy>
    pub fn get_usage(&self, tenant_id: TenantId) -> Option<&TenantUsage>
    pub fn record_write(&mut self, tenant_id: TenantId, bytes: u64) -> Result<(), TenantError>
    pub fn list_tenants(&self) -> Vec<TenantId>
    pub fn tenants_over_quota(&self) -> Vec<TenantId>
    // NO: route_hash() method (doesn't exist)
}
```

### 3. MetricsHandle (`crates/claudefs-reduce/src/metrics.rs`)
```rust
pub struct ReductionMetrics {
    pub fn record_chunk(&self, bytes_in: u64, bytes_out: u64)
    pub fn record_dedup_hit(&self)
    pub fn record_compress(&self, bytes_in: u64, bytes_out: u64)
    pub fn dedup_ratio(&self) -> f64
    pub fn compression_ratio(&self) -> f64
    pub fn overall_reduction_ratio(&self) -> f64
}

pub struct MetricsHandle {
    pub fn new() -> Self
    pub fn metrics(&self) -> Arc<ReductionMetrics>
    pub fn snapshot(&self) -> MetricsSnapshot
    // NO: record_metric() or export_prometheus() (doesn't exist)
}
```

### 4. CacheCoherency (`crates/claudefs-reduce/src/cache_coherency.rs`)
```rust
pub struct CacheKey { ... }
pub enum InvalidationEvent { ... }

pub struct CoherencyTracker {
    pub fn new() -> Self
    pub fn register(&mut self, key: CacheKey, version: CacheVersion, size_bytes: u64)
    pub fn invalidate(&mut self, event: &InvalidationEvent) -> Vec<CacheKey>
    pub fn is_valid(&self, key: &CacheKey, version: &CacheVersion) -> bool
    pub fn valid_entry_count(&self) -> usize
}
```

### 5. DefragPlanner (`crates/claudefs-reduce/src/defrag_planner.rs`)
```rust
// Check actual API - has likely been implemented differently
// Use pattern from other similar modules
```

---

## Test File Fixes Required

### integration_write_path.rs

Status: Mostly correct, but verify:
- `process_write()` return type and stats structure
- `fingerprint_count()` API (check if returns i64 or u64)

### integration_read_path.rs

Status: Multiple API fixes needed:
- Fix cache_coherency imports (use `CoherencyTracker`, not `CacheCoherency`)
- Fix `InvalidationEvent` usage pattern
- Remove incorrect `plan_replay()` calls
- Use correct metrics API methods
- Fix `check_consistency()` calls to use proper recovery_enhancer API

### integration_tier_migration.rs

Status: Multiple API fixes needed:
- Fix `plan_defrag()` - check actual DefragPlanner API
- Verify S3 blob assembly API matches implementation
- Check snapshot_catalog API matches `SnapshotCatalog::create()`
- Verify tier migration policies actual methods

### integration_performance.rs

Status: Multiple API fixes needed:
- Remove `priority` field from `WriteOp` struct
- Fix `WriteCoalescer::try_add()` → `add()` (doesn't return Result)
- Fix `TenantIsolator::route_hash()` → use actual API methods
- Fix metrics recording - use `ReductionMetrics` methods, not `MetricsHandle::record_metric()`
- Fix `CacheCoherency` → `CoherencyTracker`
- Fix pipeline_monitor alert recording

---

## Implementation Instructions for OpenCode

1. **Read each test file** and identify all compile errors
2. **For each error:**
   - Look up the correct method name in the API list above
   - Replace incorrect calls with correct ones
   - Add missing imports if needed
   - Remove unused variables that cause warnings
3. **Testing strategy:**
   - Use `#[tokio::test]` for async tests, `#[test]` for sync tests
   - Keep tests deterministic (no flaky timings)
   - Use real module instances where possible
   - Verify the test actually exercises the intended code path
4. **Output corrected test files** that compile with `cargo test -p claudefs-reduce`

---

## Files to Modify

1. `crates/claudefs-reduce/tests/integration_write_path.rs`
2. `crates/claudefs-reduce/tests/integration_read_path.rs`
3. `crates/claudefs-reduce/tests/integration_tier_migration.rs`
4. `crates/claudefs-reduce/tests/integration_performance.rs`

Output complete corrected versions that will compile successfully.
