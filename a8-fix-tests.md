# A8: Fix 4 Failing Unit Tests in claudefs-mgmt

**Status:** Phase 2 integration, 961 passing, 4 failing tests

**Current failures:**
1. `event_sink::tests::test_event_sink_severity_serialization` — line 499
   - Expects JSON to contain "critical" (lowercase)
   - Issue: EventSeverity::Critical serializes as "Critical" (uppercase)
   - Fix: Update EventSeverity enum to serialize_with lowercase, or check for "Critical" in test

2. `performance_tracker::tests::test_percentile_bucket_from_samples_multiple` — line 319
   - Test: `let samples = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000]`
   - Assertion: `assert_eq!(bucket.p50, 500)` fails with left=600, right=500
   - Issue: Percentile calculation is off (likely rounding or index error)
   - Fix: Correct percentile calculation logic for even-length sample array (10 elements)

3. `resource_limiter::tests::test_quota_enforcer_at_soft_limit_not_exceeded` — line 291
   - Setup: QuotaEnforcer with hard_limit=1000, soft_limit=80%
   - Usage: 750 bytes
   - Assertion: `assert!(enforcer.at_soft_limit())` fails
   - Issue: Soft limit threshold logic is wrong (should be true when 750 >= 800)
   - Fix: Verify soft limit comparison logic

4. `usage_reporter::tests::test_usage_reporter_detect_burst` — line 426
   - Records two snapshots with different read/write throughput
   - Expects burst detection alert (Some)
   - Assertion: `assert!(alert.is_some())` fails
   - Issue: Burst detection algorithm not working correctly
   - Fix: Review burst detection logic and threshold calculations

## Files to Fix

1. `crates/claudefs-mgmt/src/event_sink.rs` — EventSeverity serialization
2. `crates/claudefs-mgmt/src/performance_tracker.rs` — PercentileBucket::from_samples() logic
3. `crates/claudefs-mgmt/src/resource_limiter.rs` — QuotaEnforcer::at_soft_limit() logic
4. `crates/claudefs-mgmt/src/usage_reporter.rs` — UsageReporter::detect_burst() logic

## Requirements

- Fix all 4 issues so tests pass
- Keep existing module interfaces unchanged
- No clippy warnings
- All 961+ tests must pass after fix
- No changes to test files themselves

## Notes

- These are logic bugs in implementation, not test problems
- EventSeverity likely needs to serialize lowercase (check existing Serialize implementations in the module)
- Percentile calculations should handle edge cases (even vs odd sample counts)
- Soft limit threshold should be >= comparison (750 is >= 80% of 1000)
- Burst detection needs to compare throughput changes between snapshots correctly
