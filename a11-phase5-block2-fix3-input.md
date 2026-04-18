# OpenCode Input: A11 Phase 5 Block 2 — Fix Test Assertion Value

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Task:** Fix incorrect test assertion value in test_daily_cost_report_accuracy

---

## Problem

Test is failing because the expected cost value is incorrect:
```
assert!((total_cost - 38.52).abs() < 0.1, "Total cost should be ~$38.52, got {}", total_cost);
```

**Actual calculation:**
- orchestrator (on-demand): $0.35 × 24 = $8.40
- storage nodes (5 at $0.19/hr): $0.19 × 24 × 5 = $22.80
- client nodes (2 at $0.05/hr): $0.05 × 24 × 2 = $2.40
- conduit (at $0.015/hr): $0.015 × 24 = $0.36
- **Total: $8.40 + $22.80 + $2.40 + $0.36 = $33.96** ✓

The test assertion hardcoded $38.52, which is incorrect.

## Fix

Change line 459 from:
```rust
assert!((total_cost - 38.52).abs() < 0.1, "Total cost should be ~$38.52, got {}", total_cost);
```

To:
```rust
assert!((total_cost - 33.96).abs() < 0.1, "Total cost should be ~$33.96, got {}", total_cost);
```

Also verify savings_pct assertion is reasonable:
- on_demand equivalent: (0.35 + 0.624*2 + 0.624*5 + 0.12*2 + 0.05*1) × 24 = ?
- Actually, need to call get_on_demand_price for each instance type
- The actual savings % will depend on on-demand prices

The savings assertion `assert!((savings_pct - 64.5).abs() < 5.0` should be flexible enough.

## Implementation

1. Read current file
2. Fix line 459 to use correct cost value ($33.96)
3. Verify line 460 savings assertion is reasonable or update if needed
4. Test: `cargo test -p claudefs-tests preemptible_lifecycle_tests::cost_tracking_tests::test_daily_cost_report_accuracy`

---

**Model:** minimax-m2p5

**Expected Output:** Fixed test assertions with correct expected costs.
