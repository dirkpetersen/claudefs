# MODULE 1: Storage Background Subsystems Security Tests

Create the file `crates/claudefs-security/src/storage_background_subsystems_security_tests.rs` with 30 comprehensive security tests.

## Target Modules from claudefs-storage crate

- `background_scheduler` - Background task scheduler
- `device_health_monitor` - Device health monitoring
- `prefetch_engine` - Prefetch engine  
- `wear_leveling` - Wear leveling
- `node_rebalance` - Node rebalancing

## Import Requirements

```rust
use claudefs_storage::background_scheduler::{BackgroundScheduler, BackgroundTask, BackgroundTaskType, BackgroundTaskId};
use claudefs_storage::device_health_monitor::{DeviceHealthMonitor, HealthStatus, SmartSnapshot};
use claudefs_storage::prefetch_engine::{PrefetchEngine, PrefetchConfig};
use claudefs_storage::wear_leveling::{WearLevelingEngine, WearConfig, ZoneWear};
use claudefs_storage::node_rebalance::{RebalanceEngine, RebalanceConfig, NodeId, RebalanceSegmentId};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
```

## Tests to Implement (30 total)

### Background Scheduler (8 tests):

1. `test_background_scheduler_concurrent_submit_no_race` - Concurrent task submission from multiple threads without lost updates or duplicates. Use Arc<Mutex<HashMap>> to track submitted tasks and verify no duplicates or lost updates.

2. `test_background_scheduler_priority_enforcement` - High-priority tasks execute before normal/low-priority tasks. Create tasks with different priorities and verify execution order.

3. `test_background_scheduler_task_deadline_respected` - Tasks with deadlines abort if not executed by deadline. Set short deadlines and verify expired tasks are cancelled.

4. `test_background_scheduler_memory_bounded` - Task queue has maximum size; exceeding it returns error. Submit tasks until queue full, verify subsequent submits return error.

5. `test_background_scheduler_graceful_shutdown` - Shutdown cancels pending tasks and waits for in-flight tasks to complete. Call shutdown and verify clean termination.

6. `test_background_scheduler_starvation_prevention` - Low-priority tasks eventually execute. Submit many high-priority tasks, verify low-priority eventually runs.

7. `test_background_scheduler_reentrant_submission` - Task can submit subtasks without deadlock. A task submits another task, verify no deadlock.

8. `test_background_scheduler_error_recovery` - Failed task doesn't crash scheduler; errors are propagated. Task returns error, verify scheduler continues.

### Device Health Monitor (7 tests):

9. `test_device_health_monitor_smart_metrics_overflow_safe` - Large SMART values don't overflow internal counters. Use saturating arithmetic for large values.

10. `test_device_health_monitor_state_transition_rules` - Health transitions correctly Good→Warning→Failed. Verify state machine only allows valid transitions.

11. `test_device_health_monitor_concurrent_updates` - Concurrent metric updates without lost updates. Use Arc<RwLock> and concurrent updates, verify final state correct.

12. `test_device_health_monitor_alert_suppression_no_leak` - Alert suppression doesn't leak timing information. Alerts should be suppressed consistently regardless of timing.

13. `test_device_health_monitor_metric_timestamp_monotonic` - Metric timestamps never go backwards. Track timestamps and verify monotonic increase.

14. `test_device_health_monitor_health_score_concurrency` - Concurrent score calculations without race conditions. Multiple threads compute scores simultaneously.

15. `test_device_health_monitor_dashmap_consistency` - DashMap updates are atomic and visible to all readers. Concurrent read/write consistency.

### Prefetch Engine (8 tests):

16. `test_prefetch_engine_pattern_detection_memory_bounded` - Pattern detection uses bounded memory. Verify memory usage stays within limits.

17. `test_prefetch_engine_access_history_lru_eviction` - Old access history properly evicted. Track oldest entries are evicted first.

18. `test_prefetch_engine_speculative_io_cancellation` - Speculative I/O properly cancelled when user I/O arrives. Cancel prefetch when user request arrives.

19. `test_prefetch_engine_priority_over_user_io` - User I/O gets priority over prefetch requests. Verify user I/O is processed first.

20. `test_prefetch_engine_prediction_accuracy` - Prediction accuracy within expected bounds. Measure hit rate of predictions.

21. `test_prefetch_engine_concurrent_prefetch_requests` - Concurrent prefetch requests handled without interference. Multiple threads submit prefetch.

22. `test_prefetch_engine_cache_eviction_correctness` - Least-recently-used entries evicted first. Verify LRU eviction order.

23. `test_prefetch_engine_io_submission_ordering` - Prefetch I/Os submitted in correct order to storage. Verify ordering preserved.

### Wear Leveling (5 tests):

24. `test_wear_leveling_block_wear_tracking_overflow_safe` - Wear counts don't overflow. Use saturating arithmetic for wear counters.

25. `test_wear_leveling_erase_count_distribution_fair` - Wear distribution is fair. Check variance/entropy of erase counts across blocks.

26. `test_wear_leveling_hot_spot_rebalancing_safe` - Rebalancing doesn't lose data. Verify all blocks readable after rebalancing.

27. `test_wear_leveling_concurrent_wear_updates` - Concurrent wear updates use atomic operations. No lost updates with concurrent writes.

28. `test_wear_leveling_ssd_type_detection` - Detects SSD type and applies correct tier-specific wear policy. Different SSD types get different policies.

### Node Rebalance (2 tests):

29. `test_node_rebalance_segment_distribution_fair` - Segments distributed fairly across nodes. Verify no node overloaded.

30. `test_node_rebalance_during_node_failure` - Rebalance continues correctly if node fails mid-operation. Handle node failure gracefully.

## Code Patterns

Reference: `crates/claudefs-security/src/storage_io_depth_limiter_security_tests.rs`

- Use `#[tokio::test]` for async, `#[test]` for sync
- Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state
- Use `futures::future::join_all(handles).await` for concurrent execution
- Use `assert!()`, `assert_eq!()` for assertions
- Wrap in `mod tests { use super::*; ... }`

Write to: `crates/claudefs-security/src/storage_background_subsystems_security_tests.rs`