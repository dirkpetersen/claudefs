# A3: Fix 9 Failing Tests in claudefs-reduce

## Context

The A3 (Data Reduction) crate has 9 failing tests across two modules:
- `multi_tenant_quotas.rs` (6 failures)
- `tiering_advisor.rs` (3 failures)

## Test Failures Analysis

### Issue 1: multi_tenant_quotas.rs — Usage Not Created on set_quota

**Failing tests:**
- test_dedup_ratio_calculation_basic
- test_dedup_ratio_high_dedup
- test_get_usage_clones_data
- test_multiple_tenant_isolation
- test_quota_limit_zero_hard_limit_allows_writes
- test_update_quota_limits

**Root cause:**
Tests call `set_quota()` and immediately call `get_usage()` expecting it to exist. But `get_usage()` only returns entries that have already been written to via `record_write()`.

```rust
// Current behavior:
pub fn set_quota(&self, tenant_id: TenantId, limit: QuotaLimit) -> Result<(), ReduceError> {
    // ... validation ...
    let mut quotas = self.quotas.write()?;
    quotas.insert(tenant_id, limit.clone());
    // PROBLEM: usage entry is NOT created here
    Ok(())
}

pub fn get_usage(&self, tenant_id: TenantId) -> Option<QuotaUsage> {
    let usage = self.usage.read().ok()?;
    usage.get(&tenant_id).cloned()  // Returns None if tenant never wrote
}
```

**Fix:** When setting a quota, also create a usage entry for that tenant (initialize to zero).

```rust
pub fn set_quota(&self, tenant_id: TenantId, limit: QuotaLimit) -> Result<(), ReduceError> {
    if limit.soft_limit_bytes > limit.hard_limit_bytes && limit.hard_limit_bytes != 0 {
        return Err(ReduceError::InvalidInput(
            "soft_limit cannot exceed hard_limit".to_string(),
        ));
    }

    let mut quotas = self.quotas.write().map_err(|e| {
        ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
    })?;
    quotas.insert(tenant_id, limit.clone());

    // NEW: Ensure usage entry exists for this tenant
    let mut usage = self.usage.write().map_err(|e| {
        ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
    })?;
    usage.entry(tenant_id).or_insert_with(|| QuotaUsage::new(tenant_id));

    info!("Set quota for tenant {:?}: {:?}", tenant_id, limit);
    Ok(())
}
```

### Issue 2: tiering_advisor.rs — Size Score Boundaries Incorrect

**Failing tests:**
- test_calculate_size_score
- test_high_access_count_overrides_age
- test_zero_access_count

**Root cause:**
In `calculate_size_score()`, the thresholds are wrong. Current code:
```rust
fn calculate_size_score(&self, size_mb: f64) -> f64 {
    if size_mb >= 100.0 {
        1.0
    } else if size_mb >= 10.0 {
        0.7
    } else if size_mb >= 1.0 {
        0.4
    } else {
        0.1
    }
}
```

But the test `test_calculate_size_score` expects:
```rust
assert!((advisor.calculate_size_score(500.0) - 1.0).abs() < 0.01);    // >= 100.0 → 1.0 ✓
assert!((advisor.calculate_size_score(50.0) - 1.0).abs() < 0.01);     // FAILS: expects 1.0, gets 0.7
assert!((advisor.calculate_size_score(5.0) - 0.4).abs() < 0.01);      // >= 1.0 → 0.4 ✓
assert!((advisor.calculate_size_score(0.5) - 0.1).abs() < 0.01);      // < 1.0 → 0.1 ✓
```

**Issue:** 50.0 MB should score 1.0 but currently scores 0.7. Test expects consistent "large enough" threshold around 50+ MB.

**Fix:** Adjust thresholds to match test expectations. 50 MB should be "high priority" (1.0):
```rust
fn calculate_size_score(&self, size_mb: f64) -> f64 {
    if size_mb >= 50.0 {    // Changed from 100.0
        1.0
    } else if size_mb >= 10.0 {
        0.7
    } else if size_mb >= 1.0 {
        0.4
    } else {
        0.1
    }
}
```

### Issue 3: tiering_advisor.rs — determine_recommendation Logic

**Failing tests:**
- test_high_access_count_overrides_age (expects Flash despite 100 days age)
- test_zero_access_count (expects ArchiveS3 for old data)

**Root cause:**
The `determine_recommendation()` method doesn't properly weight access_count to override age. Look at the scoring weights in `recommend()`:
```rust
let total_score = (age_score * 0.4)
    + (size_score * 0.3)
    + (access_score * 0.2)        // access only 20% weight
    + (compression_penalty * 0.1);
```

When access_count is 1000 (score 1.0), size is 10MB (score 0.7), and age is 100 days (score 0.4):
```
total_score = (0.4 * 0.4) + (0.7 * 0.3) + (1.0 * 0.2) + (0.1 * ...)
            = 0.16 + 0.21 + 0.20 + ... = ~0.6
```

But `determine_recommendation()` checks age_days first:
```rust
fn determine_recommendation(&self, age_days: u64, metrics: &AccessMetrics, total_score: f64) -> TieringRecommendation {
    if age_days < self.config.flash_threshold_days {
        TieringRecommendation::Flash
    } else if total_score > 0.7 {
        TieringRecommendation::WarmS3
    } else if total_score > 0.4 {
        TieringRecommendation::ColdS3
    } else {
        TieringRecommendation::ArchiveS3
    }
}
```

Since 100 > 30 (flash_threshold_days), it doesn't return Flash. Then score 0.6 is > 0.4, so it returns ColdS3.

**Fix:** Check high access count first to override age:
```rust
fn determine_recommendation(
    &self,
    age_days: u64,
    metrics: &AccessMetrics,
    total_score: f64,
) -> TieringRecommendation {
    // High access count overrides age — keep hot data on flash
    if metrics.access_count >= 100 && total_score > 0.6 {
        return TieringRecommendation::Flash;
    }

    if age_days < self.config.flash_threshold_days {
        TieringRecommendation::Flash
    } else if total_score > 0.7 {
        TieringRecommendation::WarmS3
    } else if total_score > 0.4 {
        TieringRecommendation::ColdS3
    } else {
        TieringRecommendation::ArchiveS3
    }
}
```

## Summary of Changes

### multi_tenant_quotas.rs
- `set_quota()`: Create usage entry when setting quota

### tiering_advisor.rs
- `calculate_size_score()`: Lower threshold from 100.0 to 50.0 MB
- `determine_recommendation()`: Check high access count first to override age-based tiering

## Expected Outcome

All 9 tests should pass:
- ✅ test_dedup_ratio_calculation_basic
- ✅ test_dedup_ratio_high_dedup
- ✅ test_get_usage_clones_data
- ✅ test_multiple_tenant_isolation
- ✅ test_quota_limit_zero_hard_limit_allows_writes
- ✅ test_update_quota_limits
- ✅ test_calculate_size_score
- ✅ test_high_access_count_overrides_age
- ✅ test_zero_access_count

## Build and Test
```bash
cd /home/cfs/claudefs
cargo test -p claudefs-reduce --lib
# Expected: 2071 passed (2062 + 9)
```
