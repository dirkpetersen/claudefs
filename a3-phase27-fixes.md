# A3: Phase 27 — Fix Compilation Errors

## Errors to Fix

### 1. similarity_tier_stats.rs — ReduceError variant not found
**File:** crates/claudefs-reduce/src/similarity_tier_stats.rs
**Lines:** 220, 304
**Problem:** Using `ReduceError::InvalidChunk { reason }` but this variant doesn't exist in error.rs
**Solution:** Replace with `ReduceError::NotFound` which takes `(String)` as argument

Current (WRONG):
```rust
.ok_or_else(|| ReduceError::InvalidChunk {
    reason: format!("Workload {} not found", workload),
})?
```

Should be:
```rust
.ok_or_else(|| ReduceError::NotFound(
    format!("Workload {} not found", workload),
))?
```

### 2. similarity_tier_stats.rs — Ambiguous float type
**File:** crates/claudefs-reduce/src/similarity_tier_stats.rs
**Line:** 270
**Problem:** `(1.0 - delta_compression_ratio).max(0.0)` — rustc can't infer type
**Solution:** Explicitly specify type or restructure

Current (WRONG):
```rust
let compression_factor = (1.0 - delta_compression_ratio).max(0.0) * 0.4;
```

Should be:
```rust
let compression_factor = (1.0f64 - delta_compression_ratio).max(0.0) * 0.4;
```

### 3. adaptive_classifier.rs — Wrong method name
**File:** crates/claudefs-reduce/src/adaptive_classifier.rs
**Line:** 334 (around)
**Problem:** Method `recommend_s3_policy_internal` doesn't exist, should be `recommend_s3_policy`
**Solution:** Fix method name

Current (WRONG):
```rust
let s3_tiering_policy = AdaptiveClassifier::recommend_s3_policy_internal(fp);
```

Should be (instance method):
```rust
let s3_tiering_policy = self.recommend_s3_policy(workload)?;
```

Or if static method:
```rust
let s3_tiering_policy = AdaptiveClassifier::recommend_s3_policy(self, workload)?;
```

### 4. Minor: Unused variables (warnings only, not errors)
**Files:** similarity_coordinator.rs (line 343), recovery_enhancer.rs (line 364), similarity_tier_stats.rs (line 139)
**Solution:** Prefix with `_` to suppress warnings

---

## Steps

1. Fix `similarity_tier_stats.rs` lines 220, 304: Change `ReduceError::InvalidChunk` → `ReduceError::NotFound`
2. Fix `similarity_tier_stats.rs` line 270: Add `f64` type: `(1.0f64 - ...)`
3. Fix `adaptive_classifier.rs` line 334: Use correct method name
4. Add `_` prefix to unused variables (optional, just warnings)
5. Verify: `cargo check -p claudefs-reduce`
6. Run tests: `cargo test -p claudefs-reduce --lib`
