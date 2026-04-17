# A3: Fix 6 Remaining Failing Tests

## Current Status
- 2065 tests passing
- 6 tests still failing
- Need to fix actual implementation bugs, not just test logic

## Failing Tests Breakdown

### Issue 1: multi_tenant_quotas.rs - test_dedup_ratio_calculation_basic and test_dedup_ratio_high_dedup

**Test code:**
```rust
quotas.record_write(TenantId(1), 1000, 800, 200).unwrap();
let ratio = quotas.get_dedup_ratio(TenantId(1));
assert!((ratio - 1.25).abs() < 0.01);
```

Parameters: `record_write(raw_bytes=1000, compressed_bytes=800, dedup_saved=200)`

Expected ratio: 1.25

**Current get_dedup_ratio() implementation:**
```rust
pub fn get_dedup_ratio(&self, tenant_id: TenantId) -> f64 {
    let dedup_saved = tenant_usage.get_dedup_saved_bytes();  // 200
    let used = tenant_usage.get_used_bytes();                 // 1000
    let total_raw = used.saturating_add(dedup_saved);         // 1200
    total_raw as f64 / used as f64                            // 1200/1000 = 1.2 ❌
}
```

But the test expects 1.25. The formula should be:
```
dedup_ratio = (used_bytes + dedup_saved_bytes) / used_bytes
            = (1000 + 200) / 1000
            = 1.2 ✗  (but test expects 1.25)
```

Wait, the test params are: `record_write(1000, 800, 200)` where:
- raw_bytes = 1000 (uncompressed input)
- compressed_bytes = 800 (after compression)
- dedup_saved = 200 (bytes saved by dedup)

The dedup_ratio should express how much data was duplicated/saved. Looking at the semantics:
- Total original data that would have been stored = raw_bytes + dedup_saved = 1000 + 200 = 1200
- Actual data stored = raw_bytes = 1000
- dedup_ratio = 1200 / 1000 = 1.2

But the test expects 1.25. Let me check the high_dedup test:
```rust
quotas.record_write(TenantId(1), 1000, 100, 900).unwrap();
let ratio = quotas.get_dedup_ratio(TenantId(1));
assert!((ratio - 10.0).abs() < 0.01);
```

Here: raw=1000, compressed=100, dedup_saved=900
Expected: 10.0

With current formula: (1000 + 900) / 1000 = 1.9 ❌
Expected: 10.0 = (1000 + 900 * ?) / 1000?

Ah! Maybe the dedup ratio should be:
```
(raw_bytes + dedup_saved) / compressed_bytes
= (1000 + 900) / 100
= 1900 / 100
= 19 ❌ (still not 10)
```

Or maybe it's using all three values differently. Let me think about what "dedup_ratio" means. Looking at the names in record_write:
- raw_bytes: uncompressed input
- compressed_bytes: after LZ4/Zstd compression
- dedup_saved: bytes saved by content-addressable storage

So the effective ratio of reduction should be:
```
ratio = (raw_bytes) / (compressed_bytes)
      = 1000 / 800 = 1.25 ✓  (matches first test!)
```

For high_dedup:
```
ratio = 1000 / 100 = 10 ✓  (matches!)
```

So **dedup_ratio should be raw_bytes / compressed_bytes**, not the dedup math!

But the current code uses `used_bytes` which equals the raw_bytes written (accumulated). Let me trace through record_write to confirm.

**Fix:** The get_dedup_ratio() calculation is wrong. It should use the ratio of raw data to compressed data, not include dedup_saved:

```rust
pub fn get_dedup_ratio(&self, tenant_id: TenantId) -> f64 {
    let usage = match self.usage.read() {
        Ok(u) => u,
        Err(_) => return 1.0,
    };

    let tenant_usage = match usage.get(&tenant_id) {
        Some(u) => u,
        None => return 1.0,
    };

    let used = tenant_usage.get_used_bytes();           // raw bytes
    let compressed = tenant_usage.get_compressed_bytes(); // compressed bytes

    if compressed == 0 {
        return 1.0;
    }

    used as f64 / compressed as f64
}
```

### Issue 2: test_quota_limit_zero_hard_limit_allows_writes

**Test:**
```rust
quotas.set_quota(TenantId(1), QuotaLimit::new(1000, 0, true)).unwrap();
quotas.record_write(TenantId(1), 10000, 10000, 0).unwrap();
let result = quotas.check_quota(TenantId(1), 10000).unwrap();
assert_eq!(result, QuotaAction::Allowed);
```

**Current behavior:**
- soft_limit = 1000
- hard_limit = 0 (means disabled)
- Used = 10000
- Checking 10000 more bytes

Current logic:
```rust
if limit.hard_limit_bytes > 0 && new_used > limit.hard_limit_bytes {
    // 0 > 0 is false, so hard limit not checked ✓
}

if limit.soft_limit_bytes > 0 && new_used > limit.soft_limit_bytes {
    // 1000 > 0 && 20000 > 1000 = true → returns SoftLimitWarn ❌
}
```

The test expects Allowed. The issue: when hard_limit is 0, we should probably ignore all quota checks OR return Allowed. The test name "allows_writes" suggests zero hard_limit means "no hard limit enforcement".

But the soft_limit is being checked. Maybe when hard_limit is 0, it means the quota is disabled entirely?

Looking at the set_quota validation:
```rust
if limit.soft_limit_bytes > limit.hard_limit_bytes && limit.hard_limit_bytes != 0 {
    return Err(...);
}
```

This allows soft > hard only when hard == 0, suggesting hard_limit=0 means "unlimited".

So in check_quota, when hard_limit is 0, both soft and hard should be ignored:

```rust
pub fn check_quota(&self, tenant_id: TenantId, num_bytes: u64) -> Result<QuotaAction, ReduceError> {
    let quotas = self.quotas.read().map_err(|e| {
        ReduceError::InvalidInput(format!("Failed to acquire read lock: {}", e))
    })?;

    let usage = self.usage.read().map_err(|e| {
        ReduceError::InvalidInput(format!("Failed to acquire read lock: {}", e))
    })?;

    let limit = match quotas.get(&tenant_id) {
        Some(l) => l,
        None => return Ok(QuotaAction::Allowed),
    };

    // If hard limit is 0, quota is disabled (unlimited)
    if limit.hard_limit_bytes == 0 {
        return Ok(QuotaAction::Allowed);
    }

    let current_used = usage
        .get(&tenant_id)
        .map(|u| u.get_used_bytes())
        .unwrap_or(0);

    let new_used = current_used.saturating_add(num_bytes);

    if new_used > limit.hard_limit_bytes {
        warn!(
            "Tenant {:?} hard limit exceeded: {} > {}",
            tenant_id, new_used, limit.hard_limit_bytes
        );
        return Ok(QuotaAction::HardLimitReject);
    }

    if limit.soft_limit_bytes > 0 && new_used > limit.soft_limit_bytes {
        debug!(
            "Tenant {:?} soft limit warning: {} > {}",
            tenant_id, new_used, limit.soft_limit_bytes
        );
        return Ok(QuotaAction::SoftLimitWarn);
    }

    Ok(QuotaAction::Allowed)
}
```

### Issue 3: test_multiple_tenant_isolation

**Test:**
```rust
quotas.set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true)).unwrap();
quotas.set_quota(TenantId(2), QuotaLimit::new(500, 1000, true)).unwrap();
quotas.record_write(TenantId(1), 500, 500, 0).unwrap();
quotas.record_write(TenantId(2), 500, 500, 0).unwrap();
let usage1 = quotas.get_usage(TenantId(1)).unwrap();
let usage2 = quotas.get_usage(TenantId(2)).unwrap();
assert_eq!(usage1.get_used_bytes(), 500);
assert_eq!(usage2.get_used_bytes(), 500);
let result1 = quotas.check_quota(TenantId(1), 1600).unwrap();
let result2 = quotas.check_quota(TenantId(2), 600).unwrap();
assert_eq!(result1, QuotaAction::Allowed);  // 500 + 1600 = 2100 > 2000 hard limit
assert_eq!(result2, QuotaAction::HardLimitReject);
```

**Issue:** result1 expects Allowed but should be HardLimitReject since 2100 > 2000.

Wait, let me reread. Tenant1 has hard_limit=2000. Used=500, checking 1600 more = 2100 > 2000. Should be rejected.

But test expects Allowed. This test logic seems wrong...unless I'm misunderstanding. Let me check if there's test_get_usage_clones_data issue:

### Issue 4: test_get_usage_clones_data

```rust
quotas.record_write(TenantId(1), 100, 100, 0).unwrap();
let usage1 = quotas.get_usage(TenantId(1)).unwrap();
quotas.record_write(TenantId(1), 50, 40, 5).unwrap();
let usage2 = quotas.get_usage(TenantId(1)).unwrap();
assert_eq!(usage1.get_used_bytes(), 100);  // But used is 150 now after second write!
assert_eq!(usage2.get_used_bytes(), 150);
```

The test name "clones_data" suggests get_usage() should return a snapshot/clone, not a live reference. But if the underlying AtomicU64 is updated, even a clone will see the new value since it shares the Arc<AtomicU64>.

The test seems to expect that usage1 stays at 100 even after more writes. This is impossible with AtomicU64 sharing.

**Fix:** Maybe get_usage should clone the values into a static snapshot? Or maybe the test expectations are wrong?

Actually, looking more carefully: we need to fix the test understanding. The purpose seems to be that each get_usage call should return the data state at that moment. Since we use Arc<AtomicU64>, this won't work.

Hmm, but changing the data structure would be significant. Let me re-examine what the implementation should be doing. The QuotaUsage has:
```rust
pub used_bytes: Arc<AtomicU64>
```

When we clone QuotaUsage, we're cloning the Arc, so both usage1 and usage2 point to the same AtomicU64. This is by design for atomic updates.

But the test expects usage1 to remain at 100 after later writes. This is a design conflict.

**Possible fix:** Make QuotaUsage store snapshots instead of references:
```rust
pub struct QuotaUsage {
    pub tenant_id: TenantId,
    pub used_bytes: u64,        // Snapshot, not Arc<AtomicU64>
    pub compressed_bytes: u64,
    pub dedup_saved_bytes: u64,
    pub last_update_ms: u64,
}
```

But then record_write wouldn't work since it needs atomicity. We'd need to redesign this.

Actually, I think the test is just poorly written. Let me check the actual error message again...it says:
```
assertion `left == right` failed
  left: 150
 right: 100
```

So usage1.get_used_bytes() returns 150, not 100. This confirms the Arc sharing issue.

Maybe the fix is to make get_usage() return a true snapshot of the atomic values at that moment:

```rust
pub fn get_usage(&self, tenant_id: TenantId) -> Option<QuotaUsage> {
    let usage = self.usage.read().ok()?;
    let tenant_usage = usage.get(&tenant_id)?.clone();

    // Create a snapshot with current atomic values
    Ok(QuotaUsage {
        tenant_id,
        used_bytes: Arc::new(AtomicU64::new(tenant_usage.get_used_bytes())),
        compressed_bytes: Arc::new(AtomicU64::new(tenant_usage.get_compressed_bytes())),
        dedup_saved_bytes: Arc::new(AtomicU64::new(tenant_usage.get_dedup_saved_bytes())),
        last_update_ms: tenant_usage.last_update_ms,
    })
}
```

This way each get_usage() call creates new Arc<AtomicU64> instances with snapshotted values.

Let me also check test_zero_access_count for tiering_advisor:

### Issue 5: tiering_advisor::test_zero_access_count

```rust
let metrics = AccessMetrics {
    segment_id: 1,
    size_bytes: 10_000_000,
    last_access_age_sec: 200 * 86400,  // 200 days
    access_count: 0,
    compression_ratio: 1.5,
    dedup_ratio: 1.0,
};
let score = advisor.recommend(&metrics);
assert_eq!(score.recommendation, TieringRecommendation::ArchiveS3);
```

With default config: flash_threshold=30, cold_threshold=90, archive_threshold=365

200 days < 365, so not archive. The current determine_recommendation returns ColdS3 for this range.

**Fix:** Need to adjust thresholds or logic. Looking at the current code:
```rust
if metrics.access_count >= 100 && score > 0.6 {
    return TieringRecommendation::Flash;
}

if age_days < self.config.flash_threshold_days {
    TieringRecommendation::Flash
} else if age_days >= self.config.archive_threshold_days {
    TieringRecommendation::ArchiveS3
} else if age_days >= self.config.cold_threshold_days {
    TieringRecommendation::ColdS3
} else {
    TieringRecommendation::WarmS3
}
```

With age_days=200 (< 365), it returns ColdS3. But test expects ArchiveS3.

Possible fix: Lower the archive threshold from 365 to something like 180 days? Or change the logic?

Actually, maybe "200 days old with zero accesses" should be considered "ancient" even if < 365 days. The intent is that zero-access old data should tier immediately.

Let me just propose the concrete fixes below.

## Summary of Fixes

1. **get_dedup_ratio()**: Use `raw_bytes / compressed_bytes` formula instead of `(used + dedup_saved) / used`
2. **check_quota()**: Return Allowed when hard_limit_bytes == 0 (quota disabled)
3. **get_usage()**: Create snapshot of atomic values (new Arc<AtomicU64> with current values)
4. **determine_recommendation()**: May need to lower archive_threshold or add special handling for zero-access ancient data

Let me start with fixes 1-3 which are clear.
