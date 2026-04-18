# OpenCode Input: A11 Phase 5 Block 2 — Compilation Fixes

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Task:** Fix 5 compilation errors in preemptible_lifecycle_tests.rs

**Status:** Implementation generated but has type mismatches that need fixing.

---

## Compilation Errors to Fix

The following 5 errors need fixing in `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs`:

### Error 1: Ambiguous numeric type for `abs()` — Line 150
```
error[E0689]: can't call method `abs` on ambiguous numeric type `{float}`
   --> crates/claudefs-tests/src/preemptible_lifecycle_tests.rs:150:42
    |
150 |         assert!((avg_recent - avg_older).abs() > 0.02);
```
**Fix:** Specify type explicitly as `f64` in the arithmetic operation.
**Current:** `(avg_recent - avg_older).abs()`
**Solution:** Cast to `f64` explicitly or ensure both operands are `f64`

### Error 2: Type mismatch u64 vs u128 — Line 301
```
error[E0308]: mismatched types
   --> crates/claudefs-tests/src/preemptible_lifecycle_tests.rs:301:28
    |
301 |         assert!(elapsed >= timeout_ms, "Should timeout after 500ms");
    |                 -------    ^^^^^^^^^^ expected `u64`, found `u128`
```
**Fix:** Cast `elapsed` from `u128` (result of `Instant::elapsed()`) to `u64` via `.as_millis() as u64`

### Error 3 & 4: String vs &str — Lines 347-348
```
error[E0308]: mismatched types
   --> crates/claudefs-tests/src/preemptible_lifecycle_tests.rs:347:33
    |
347 |             ("DisruptionCount", "1"),
    |                                 ^^^ expected `String`, found `&str`
    |
error[E0308]: mismatched types
   --> crates/claudefs-tests/src/preemptible_lifecycle_tests.rs:348:29
    |
348 |             ("TotalUptime", "0"),
    |                             ^^^ expected `String`, found `&str`
```
**Fix:** Convert `&str` to `String` using `.to_string()` method

### Error 5: Cannot dereference {integer} — Line 443
```
error[E0614]: type `{integer}` cannot be dereferenced
   --> crates/claudefs-tests/src/preemptible_lifecycle_tests.rs:443:57
    |
443 |                 let od_rate = get_on_demand_price(match *1 {
    |                                                         ^^ can't be dereferenced
```
**Fix:** This is invalid Rust syntax. Should be matching on a variable or literal without dereference. Fix to: `match "i4i.2xlarge" {` or similar based on context.

---

## Implementation Instructions

1. **Read current file:** `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs`
2. **Fix error 1 (line 150):** Add explicit `f64` type annotation
3. **Fix error 2 (line 301):** Cast `elapsed` to `u64` using `.as_millis() as u64`
4. **Fix errors 3-4 (lines 347-348):** Add `.to_string()` to convert `&str` to `String`
5. **Fix error 5 (line 443):** Remove `*1 {` and replace with appropriate match pattern
6. **Verify:** All fixes are minimal and type-correct
7. **Output:** Updated file with all 5 errors fixed

---

## Quality Checklist
- [ ] All 5 errors fixed
- [ ] No new warnings introduced
- [ ] Code compiles: `cargo build -p claudefs-tests`
- [ ] Tests pass: `cargo test -p claudefs-tests preemptible_lifecycle`
- [ ] Clippy clean: `cargo clippy -p claudefs-tests -- -D warnings`

---

**Model:** Use minimax-m2p5 (default)

**Expected Output:** Fixed `preemptible_lifecycle_tests.rs` file with all compilation errors resolved.
