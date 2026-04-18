# A10 Phase 36 Module 1: Storage Background Subsystems Security Tests

Generate ONLY the `storage_background_subsystems_security_tests.rs` file with 30 security tests.

File location: `crates/claudefs-security/src/storage_background_subsystems_security_tests.rs`

Target modules from `claudefs-storage` crate:
- `background_scheduler` — Task scheduling, priority, deadlines, memory bounds
- `device_health_monitor` — SMART metrics, state transitions, concurrency
- `prefetch_engine` — Pattern detection, memory bounds, I/O cancellation
- `wear_leveling` — Block wear tracking, fair distribution
- `node_rebalance` — Segment distribution, failover safety

## Test Structure

```rust
#[cfg(test)]
mod storage_background_subsystems_security_tests {
    use claudefs_storage::background_scheduler::BackgroundScheduler;
    use claudefs_storage::device_health_monitor::{DeviceHealthMonitor, HealthStatus};
    use claudefs_storage::prefetch_engine::PrefetchEngine;
    use claudefs_storage::wear_leveling::WearLevelingManager;
    use claudefs_storage::node_rebalance::NodeRebalancer;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::RwLock;
    use std::time::Duration;

    // 30 test functions

    // BACKGROUND SCHEDULER TESTS (8 tests)
    #[test]
    fn test_background_scheduler_concurrent_submit_no_race() { ... }

    #[test]
    fn test_background_scheduler_priority_enforcement() { ... }

    #[test]
    fn test_background_scheduler_task_deadline_respected() { ... }

    #[test]
    fn test_background_scheduler_memory_bounded() { ... }

    #[test]
    fn test_background_scheduler_graceful_shutdown() { ... }

    #[test]
    fn test_background_scheduler_starvation_prevention() { ... }

    #[test]
    fn test_background_scheduler_reentrant_submission() { ... }

    #[test]
    fn test_background_scheduler_error_recovery() { ... }

    // DEVICE HEALTH MONITOR TESTS (7 tests)
    #[test]
    fn test_device_health_monitor_smart_metrics_overflow_safe() { ... }

    #[test]
    fn test_device_health_monitor_state_transition_rules() { ... }

    #[test]
    fn test_device_health_monitor_concurrent_updates() { ... }

    #[test]
    fn test_device_health_monitor_alert_suppression_no_leak() { ... }

    #[test]
    fn test_device_health_monitor_metric_timestamp_monotonic() { ... }

    #[test]
    fn test_device_health_monitor_health_score_concurrency() { ... }

    #[test]
    fn test_device_health_monitor_dashmap_consistency() { ... }

    // PREFETCH ENGINE TESTS (8 tests)
    #[test]
    fn test_prefetch_engine_pattern_detection_memory_bounded() { ... }

    #[test]
    fn test_prefetch_engine_access_history_lru_eviction() { ... }

    #[test]
    fn test_prefetch_engine_speculative_io_cancellation() { ... }

    #[test]
    fn test_prefetch_engine_priority_over_user_io() { ... }

    #[test]
    fn test_prefetch_engine_prediction_accuracy() { ... }

    #[test]
    fn test_prefetch_engine_concurrent_prefetch_requests() { ... }

    #[test]
    fn test_prefetch_engine_cache_eviction_correctness() { ... }

    #[test]
    fn test_prefetch_engine_io_submission_ordering() { ... }

    // WEAR LEVELING TESTS (5 tests)
    #[test]
    fn test_wear_leveling_block_wear_tracking_overflow_safe() { ... }

    #[test]
    fn test_wear_leveling_erase_count_distribution_fair() { ... }

    #[test]
    fn test_wear_leveling_hot_spot_rebalancing_safe() { ... }

    #[test]
    fn test_wear_leveling_concurrent_wear_updates() { ... }

    #[test]
    fn test_wear_leveling_ssd_type_detection() { ... }

    // NODE REBALANCE TESTS (2 tests)
    #[test]
    fn test_node_rebalance_segment_distribution_fair() { ... }

    #[test]
    fn test_node_rebalance_during_node_failure() { ... }
}
```

## Implementation Notes

1. Use existing test patterns from Phase 35
2. Reference: `crates/claudefs-security/src/storage_io_depth_limiter_security_tests.rs` (35 tests)
3. Each test should be 20-50 lines
4. Use `#[test]` for sync tests, `#[tokio::test]` for async
5. Use `std::thread::spawn()` for concurrent tests
6. Use `Arc<Mutex<T>>` for shared state
7. Use `std::sync::atomic::*` for lock-free tests
8. No panics in assertions — use proper error handling
9. All tests must pass with `cargo test --release`

## Critical Test Patterns to Include

- **Concurrency Tests:** Use `std::sync::Barrier` for thread synchronization
- **Memory Bounds:** Verify no unbounded growth DoS attacks
- **State Machine:** Verify correct state transitions
- **Overflow Protection:** Use `saturating_*` or checked arithmetic
- **Timing:** Verify no race conditions with `Arc<AtomicUsize>`

Generate the complete, ready-to-compile `storage_background_subsystems_security_tests.rs` file.
