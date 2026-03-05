# A10 Phase 35 Part 1: Storage Security Tests (io_depth_limiter + command_queueing)

**Scope:** Create 2 security test modules for A1 Phase 9 storage modules.

---

## Task: Create storage_io_depth_limiter_security_tests.rs + storage_command_queueing_security_tests.rs

You will create **2 comprehensive security test modules** with approximately **67 tests total** for new storage engine modules.

### Module 1: storage_io_depth_limiter_security_tests.rs (~35 tests)

**Located in:** `/home/cfs/claudefs/crates/claudefs-security/src/storage_io_depth_limiter_security_tests.rs`

**Module under test:** `claudefs-storage/src/io_depth_limiter.rs`

**Key types to test:**
```rust
use claudefs_storage::io_depth_limiter::{
    HealthAdaptiveMode, IoDepthLimiter, IoDepthLimiterConfig, QueueDepthStats,
};
use std::sync::Arc;
```

**Test suite structure:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use claudefs_storage::io_depth_limiter::*;
    use claudefs_storage::nvme_passthrough::QueuePairId;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Barrier;

    mod concurrency_and_race_conditions {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_concurrent_acquire_no_data_race() {
            // Spawn 20 concurrent tokio tasks attempting to acquire/release queue slots
            // Verify no panics and pending_count remains valid
            // Use Arc<IoDepthLimiter> and tokio::task::join_all
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_healthy_degraded() {
            // Monitor mode transitions from Healthy→Degraded under sustained latency
            // Verify recovery_delay prevents flip-flop
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_degraded_critical() {
            // Force p99 latency > critical_latency_ms; verify transition to Critical
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_critical_recovery() {
            // In Critical mode, simulate device recovery; verify eventual transition back
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_counter_concurrent_increments() {
            // Arc<AtomicU32> pending counter with 100 concurrent operations
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_latency_history_concurrent_updates() {
            // VecDeque latency history with RwLock; multiple tasks push while reading
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_dispatch_time_tracking_concurrent() {
            // dispatch_times VecDeque updated concurrently
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_stats_snapshot_under_concurrent_load() {
            // Call get_stats() while concurrent tasks update latencies
        }
    }

    mod latency_calculation_and_percentile_logic {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_empty_history() {
            // VecDeque of latencies is empty; p99_latency should be 0, not panic
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_single_latency() {
            // Single latency value in history; p99 should equal that value
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_sorted_correctly() {
            // Insert 100 latencies in random order; compute p99; verify it's at 99th percentile
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_avg_latency_computation() {
            // Sum 50 known latencies; verify avg_latency matches expected
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_avg_latency_overflow_resistant() {
            // Push u64::MAX-100 values; verify no overflow panic
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_history_window_rolling() {
            // history_size=10; push 100 latencies; verify VecDeque never exceeds 10
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_percentile_with_duplicates() {
            // Push 1000 identical latencies; p99 should equal that value
        }
    }

    mod mode_transition_security {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_transition_gating_recovery_delay() {
            // After Healthy→Degraded, immediately attempt back to Healthy
            // Verify recovery_delay prevents it
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_degradation_latency_threshold() {
            // Set degradation_latency_ms=5; simulate p99 at 4, 5, 6 ms
            // Verify transition at ≥5
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_critical_latency_threshold() {
            // Set critical_latency_ms=10; simulate p99 at 9, 10, 11
            // Verify transition at ≥10
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_via_device_health() {
            // Simulate device critical_warning; verify mode→Critical
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_min_max_depth_bounds() {
            // Set min_depth=8, max_depth=256; transition to lower mode
            // Verify new_depth ≥ min_depth
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_reduction_percent_applied() {
            // Healthy depth=32, reduction_percent=50; degrade
            // Verify new depth ≤ 16
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_depth_adjustment_clamped_min() {
            // Force depth below min_depth; verify clamped back
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_depth_adjustment_clamped_max() {
            // Force depth above 256; verify clamped to 256
        }
    }

    mod resource_exhaustion_resistance {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_counter_overflow_safe() {
            // Acquire 2^32-1 operations; verify pending_count doesn't panic
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_latency_history_bounded_memory() {
            // history_size=100; push 1000000 latencies
            // Verify memory stays ~fixed
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_dispatch_time_deque_bounded() {
            // Push dispatch times in loop
            // Verify VecDeque capacity never exceeds history_size
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_stats_aggregation_large_sample() {
            // Aggregate stats from 10000 ops; verify no OOM
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_concurrent_acquire_no_unbounded_growth() {
            // Acquire from 50 concurrent tasks in loop
            // Verify pending_count stays <1000
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_large_config_values_handled() {
            // Set history_size=100000; create limiter
            // Verify no panic
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_negative_latency_impossible() {
            // Ensure end_time < start_time impossible (saturating_sub used)
        }
    }

    mod api_boundary_validation {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_try_acquire_respects_limit() {
            // current_limit=10; call try_acquire() 11 times
            // 11th should fail
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_release_with_zero_pending_noop() {
            // pending_ops=0; release() should not panic, just no-op
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_set_depth_out_of_range_clamped() {
            // Call set_depth(1000) with max=256
            // Verify clamped to 256
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_count_reflects_actual_ops() {
            // Acquire 5 ops; verify pending_count()=5
            // Release 3; verify=2
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_config_validation_on_create() {
            // Create with invalid config
            // Verify handled gracefully
        }
    }
}
```

Write concrete implementations for each test stub above. Key patterns:
- Use `#[tokio::test]` for async tests
- Use proper assertions with descriptive messages
- For concurrent tests, use `Arc<IoDepthLimiter>`, `tokio::spawn()`, and `join_all()`
- Avoid hardcoded sleeps that can flake; use latency checks instead

---

### Module 2: storage_command_queueing_security_tests.rs (~32 tests)

**Located in:** `/home/cfs/claudefs/crates/claudefs-security/src/storage_command_queueing_security_tests.rs`

**Module under test:** `claudefs-storage/src/command_queueing.rs`

**Key types to test:**
```rust
use claudefs_storage::command_queueing::{
    CommandQueue, CommandQueueConfig, CommandQueueStats, CommandType, NvmeCommand,
};
use std::sync::Arc;
```

**Test suite with ~32 tests organized in 5 categories:**

- **Capacity & Backpressure Enforcement** (7 tests)
- **Buffer Lifecycle Safety** (8 tests)
- **Command Ordering & Integrity** (6 tests)
- **Batch Threshold Enforcement** (6 tests)
- **Statistics Accuracy** (5 tests)

Use similar structure to io_depth_limiter tests above. For each category:
1. Create `mod <category> { ... }` section
2. Implement 5-8 test functions with concrete logic (not stubs)
3. Use descriptive test names: `test_storage_cmd_q_sec_<scenario>`
4. Test both happy path and error cases

---

## Code Style Requirements

1. **File header:**
   ```rust
   //! Storage command queueing security tests.
   //!
   //! Part of A10 Phase 35
   ```

2. **Test organization:**
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       use claudefs_storage::command_queueing::*;
       // ... other imports

       mod <category> {
           use super::*;

           #[tokio::test]
           async fn test_storage_cmd_q_sec_<scenario>() {
               // implementation
           }
       }
   }
   ```

3. **Assertions:**
   ```rust
   assert!(condition, "descriptive message with expected vs actual");
   assert_eq!(actual, expected, "why they should match");
   ```

4. **Concurrency patterns:**
   ```rust
   let mut handles = vec![];
   for i in 0..20 {
       let queue = Arc::clone(&queue);
       handles.push(tokio::spawn(async move {
           // concurrent operation
       }));
   }
   futures::future::join_all(handles).await;
   ```

---

## Key Implementation Notes

- **For io_depth_limiter tests:**
  - Test uses IoDepthLimiterConfig defaults where possible
  - Mock DeviceHealth if needed (or test with synthetic latency data)
  - For latency tracking: create vectors of u64 values, simulate them being tracked
  - For mode transitions: manipulate latency history to trigger transitions

- **For command_queueing tests:**
  - Create NvmeCommand structs with realistic field values
  - Test Arc<Vec<u8>> buffer handling explicitly (verify Arc refcounting)
  - For FIFO ordering: track user_data as a sequence and verify dequeue order
  - For batch threshold: use Instant::now() and Duration arithmetic

---

## Compilation & Testing

These files should:
1. Compile without warnings: `cargo build -p claudefs-security`
2. Run all tests: `cargo test -p claudefs-security --lib -- storage_io_depth_sec` (67 tests)
3. No `#[ignore]` markers (all tests should run)

---

**Ready for implementation. Generate both test modules (~67 tests total) fully functional and compile-ready.**
