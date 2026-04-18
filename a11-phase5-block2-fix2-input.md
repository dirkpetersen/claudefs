# OpenCode Input: A11 Phase 5 Block 2 — Final Compilation Fixes

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Task:** Fix remaining 3 compilation errors in preemptible_lifecycle_tests.rs

---

## Remaining Errors

### Error 1: Cannot match f64 against string patterns — Line 443
```
let od_rate = get_on_demand_price(match *rate {
    "i4i.2xlarge" => "i4i.2xlarge",
    ...
});
```

**Problem:** `rate` is f64 (the hourly rate), but the match is trying to match strings. The test data is:
```rust
let cluster_config = vec![
    ("orchestrator", "c7a.2xlarge", false, 0.35),  // (name, instance_type, is_spot, rate)
    ("storage-a1", "i4i.2xlarge", true, 0.19),
    ...
];
for (_, instance_type, is_spot, rate) in &cluster_config {
    // We need to use instance_type, not rate!
}
```

**Fix:** Change the loop to properly destructure and use instance_type:
```rust
for (_, instance_type, is_spot, rate) in &cluster_config {
    let cost = calculate_instance_cost(*rate, uptime_hours);
    total_cost += cost;

    if *is_spot {
        let od_rate = get_on_demand_price(instance_type);  // Use instance_type string directly
        on_demand_equivalent += calculate_instance_cost(od_rate, uptime_hours);
    } else {
        on_demand_equivalent += cost;
    }
}
```

### Error 2: Ambiguous numeric type for `abs()` — Line 150
The fix didn't quite work. Need proper type inference:
```rust
assert!((avg_recent - avg_older).abs() > 0.02f64);
```
Should be:
```rust
assert!((avg_recent - avg_older).abs() > 0.02);
```
OR cast the subtraction result to ensure type is clear:
```rust
let diff: f64 = avg_recent - avg_older;
assert!(diff.abs() > 0.02);
```

### Error 3: E0308 type mismatch (unspecified in previous output)
Need to verify this compiles after other fixes.

---

## Implementation Instructions

1. Read the current file carefully
2. Fix the loop destructuring on line 438 to properly extract `instance_type`
3. On line 443, change from `match *rate { ... }` to simply `instance_type`
4. Verify the abs() call has proper type context
5. Test: `cargo test -p claudefs-tests preemptible_lifecycle`

---

## Expected Result

All errors should resolve:
- Error E0425 (undefined `_2`): Fixed by using correct variable
- Error E0689 (ambiguous type): Fixed with proper type context
- All remaining type errors: Should be resolved

---

## Quality Checklist
- [ ] Tests compile: `cargo build -p claudefs-tests`
- [ ] All tests pass: `cargo test -p claudefs-tests preemptible_lifecycle`
- [ ] Zero clippy errors on preemptible_lifecycle_tests.rs
- [ ] No new errors introduced

---

**Model:** minimax-m2p5

**Expected Output:** Fixed `preemptible_lifecycle_tests.rs` with all compilation errors resolved and tests passing.
