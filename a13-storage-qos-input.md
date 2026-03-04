# Task: Write storage_qos_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-storage` crate focusing on QoS enforcement, I/O scheduling priority, and capacity tracking watermarks.

## File location
`crates/claudefs-security/src/storage_qos_security_tests.rs`

## Module structure
```rust
//! Storage QoS/scheduling/capacity security tests.
//!
//! Part of A10 Phase 13: Storage QoS & scheduling security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_storage::qos_storage::{
    WorkloadClass, QosDecision, IoType, QosPolicy, TokenBucket, BandwidthTracker,
    IoRequest, QosEnforcerStats, QosEnforcer,
};
use claudefs_storage::io_scheduler::{
    IoPriority, ScheduledIo, IoSchedulerConfig, IoSchedulerStats, IoScheduler,
};
use claudefs_storage::capacity::{
    CapacityLevel, TierOverride, WatermarkConfig, SegmentTracker, CapacityTrackerStats,
    CapacityTracker,
};
use claudefs_storage::{BlockRef, BlockSize, IoRequestId, IoOpType};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests. For IoRequestId, try IoRequestId(1) or just u64. For IoOpType, try IoOpType::Read or similar. For BlockRef, try BlockRef { device_idx: 0, block_id: BlockId(0), size: BlockSize::Block4K }.

## Existing tests to AVOID duplicating
- Previous storage test modules cover allocator, block cache, quota, wear leveling, hot swap, erasure, superblock, device pool, compaction, snapshot

DO NOT duplicate these. Focus on QoS enforcement, I/O scheduling, capacity watermarks.

## Test categories (25 tests total)

### Category 1: QoS Token Bucket & Bandwidth (5 tests)

1. **test_token_bucket_consume** — Create TokenBucket::new(capacity=100, rate=10.0). Verify available() == 100. Try consume 50 tokens at time 0. Verify true. Verify available() == 50. Try consume 60 tokens. Verify false (only 50 available).

2. **test_token_bucket_refill** — Create bucket(capacity=10, rate=10.0). Consume all 10 tokens at time 0. At time 1_000_000_000 (1 second), refill. Verify available() == 10 (refill rate 10/sec). (FINDING: verify refill doesn't exceed capacity).

3. **test_bandwidth_tracker_current** — Create BandwidthTracker::new(window_ns=1_000_000_000). Record 1MB at time 0. Verify current_mbps() is approximately 1.0. Record another 1MB. Verify approximately 2.0.

4. **test_qos_policy_default** — Create QosPolicy::default(). Document default workload class, priority, and limits. Verify it has reasonable defaults.

5. **test_workload_class_display** — Verify WorkloadClass variants display correctly: AiTraining, Database, etc. Verify all variants exist and are distinct.

### Category 2: QoS Enforcer (5 tests)

6. **test_qos_enforcer_allow_within_limits** — Create QosEnforcer. Set policy for tenant "t1" with max_iops=Some(100). Create IoRequest for tenant "t1". Call check_request(). Verify QosDecision::Allow.

7. **test_qos_enforcer_throttle_when_exceeded** — Create enforcer. Set policy for tenant "t1" with max_iops=Some(1). Create request, check (allow, consumes token). Create another request immediately. Check. Verify QosDecision::Throttle or Reject (token exhausted).

8. **test_qos_enforcer_no_policy_allows** — Create enforcer. Create request for tenant "unknown" (no policy set). Check request. Verify QosDecision::Allow (no policy = no restriction). (FINDING: missing policy defaults to allow — verify this is intentional).

9. **test_qos_enforcer_stats_tracking** — Create enforcer. Set policy. Check 3 requests. Verify stats().total_requests == 3. Record completion for tenant. Verify total_bytes_processed updated.

10. **test_qos_enforcer_remove_policy** — Create enforcer. Set policy for "t1". Verify get_policy("t1") returns Some. Remove policy. Verify get_policy("t1") returns None. Check request for "t1". Verify Allow (no policy).

### Category 3: I/O Scheduler Priority (5 tests)

11. **test_io_scheduler_priority_ordering** — Verify IoPriority ordering: Critical < High < Normal < Low (lower value = higher priority). Verify IoPriority::Critical.is_high() returns true.

12. **test_io_scheduler_dequeue_priority** — Create IoScheduler. Enqueue 1 Normal, 1 Critical, 1 Low request. Dequeue. Verify Critical comes first. Dequeue. Verify Normal next. Dequeue. Verify Low last.

13. **test_io_scheduler_max_queue_depth** — Create scheduler with max_queue_depth=3. Enqueue 3 requests (OK). Try enqueue 4th. Verify error (queue full). (FINDING: queue depth limit prevents memory exhaustion).

14. **test_io_scheduler_inflight_tracking** — Create scheduler. Enqueue and dequeue request. Verify inflight_count() == 1. Call complete(id). Verify inflight_count() == 0.

15. **test_io_scheduler_drain_priority** — Create scheduler. Enqueue 3 Normal and 2 Critical requests. Drain Normal priority. Verify returns 3 requests. Verify priority_depth(Normal) == 0. Verify priority_depth(Critical) == 2.

### Category 4: Capacity Watermarks (5 tests)

16. **test_capacity_level_normal** — Create CapacityTracker with total=1000 bytes, default watermarks. Update usage to 50%. Verify level() == Normal. Verify usage_pct() == 50.

17. **test_capacity_level_transitions** — Create tracker with watermarks: high=80%, critical=95%. Update to 79% → Normal. Update to 81% → High. Update to 96% → Critical. Update to 100% → Full. Verify each transition.

18. **test_capacity_eviction_trigger** — Create tracker. Update to usage below high watermark. Verify should_evict() == false. Update to above high watermark. Verify should_evict() == true. (FINDING: eviction trigger at correct threshold).

19. **test_capacity_segment_registration** — Create tracker. Register 3 segments. Verify stats().tracked_segments == 3. Mark one as s3_confirmed. Verify s3_confirmed_segments == 1.

20. **test_capacity_eviction_candidates** — Create tracker. Register segments with different last_access times. Request eviction candidates. Verify oldest/least recently accessed segments returned first. Verify only s3_confirmed segments are candidates.

### Category 5: Config Defaults & Edge Cases (5 tests)

21. **test_io_scheduler_config_defaults** — Create IoSchedulerConfig::default(). Verify max_queue_depth > 0. Verify max_inflight > 0. Verify starvation_threshold_ms > 0. Verify critical_reservation > 0.0.

22. **test_watermark_config_defaults** — Create WatermarkConfig::default(). Verify high_watermark_pct < critical_watermark_pct. Verify low_watermark_pct < high_watermark_pct. Verify all within 0-100.

23. **test_capacity_tracker_zero_total** — Create CapacityTracker with total_capacity=0. Update usage to 0. Verify level() returns Normal or Full (document behavior). Verify usage_pct() doesn't panic. (FINDING: zero capacity edge case).

24. **test_io_scheduler_empty_dequeue** — Create scheduler. Call dequeue() on empty scheduler. Verify returns None (no panic). Verify is_empty() returns true.

25. **test_qos_enforcer_reset_stats** — Create enforcer. Process some requests. Call reset_stats(). Verify all stats counters are 0.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-STOR-QOS-XX: description`
- If a type is not public, skip that test and add an alternative
- DO NOT use any async code — all tests are synchronous
- Use `assert!`, `assert_eq!`, `matches!`
- For IoRequest: IoRequest { tenant_id: "t1".into(), class: WorkloadClass::Interactive, op_type: IoType::Read, bytes: 4096, timestamp_ns: 0 }
- For ScheduledIo: use ScheduledIo::new(id, priority, op_type, block_ref, time_ns)
- For SegmentTracker: SegmentTracker { segment_id: 1, size_bytes: 1024, created_at_secs: 0, last_access_secs: 0, s3_confirmed: false, tier_override: TierOverride::Auto }

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
