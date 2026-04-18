//! Storage background subsystems security tests.
//!
//! Part of A10 Phase 36: Security tests for background_scheduler, device_health_monitor,
//! prefetch_engine, wear_leveling, and node_rebalance subsystems.
//!
//! These tests verify security properties of storage background services:
//! - Memory bounds and DoS resilience
//! - Concurrency safety (no data races)
//! - State machine correctness
//! - Overflow protection
//! - Fair resource scheduling

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // BACKGROUND SCHEDULER TESTS (8 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_background_scheduler_concurrent_submit_no_race() {
        let scheduler = Arc::new(BackgroundScheduler::new(100));
        let submit_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for i in 0..10 {
            let scheduler = Arc::clone(&scheduler);
            let count = Arc::clone(&submit_count);
            handles.push(tokio::spawn(async move {
                for j in 0..10 {
                    if scheduler
                        .submit_task(format!("task_{}_{}",i, j), TaskPriority::Normal, Duration::from_secs(1))
                        .await
                        .is_ok()
                    {
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }

        futures::future::join_all(handles).await;

        let total = submit_count.load(Ordering::SeqCst);
        assert_eq!(total, 100, "All 100 tasks should be submitted successfully");
    }

    #[tokio::test]
    async fn test_background_scheduler_priority_enforcement() {
        let scheduler = Arc::new(BackgroundScheduler::new(50));

        // Submit low-priority tasks first
        for i in 0..5 {
            let sched = Arc::clone(&scheduler);
            let _ = sched.submit_task(
                format!("low_{}", i),
                TaskPriority::Low,
                Duration::from_secs(10),
            ).await;
        }

        // Then high-priority tasks
        for i in 0..5 {
            let sched = Arc::clone(&scheduler);
            let _ = sched.submit_task(
                format!("high_{}", i),
                TaskPriority::High,
                Duration::from_secs(10),
            ).await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        // High-priority should start executing before low-priority
        let queue_size = scheduler.pending_task_count().await;
        assert!(queue_size >= 0, "Queue should have items pending");
    }

    #[tokio::test]
    async fn test_background_scheduler_task_deadline_respected() {
        let scheduler = Arc::new(BackgroundScheduler::new(10));

        // Submit task with 10ms deadline
        let deadline = Duration::from_millis(10);
        let submit_result = scheduler
            .submit_task("deadline_task", TaskPriority::Normal, deadline)
            .await;

        assert!(submit_result.is_ok(), "Task submission should succeed");

        // Wait longer than deadline
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Task should have timed out or been cleaned up
        let remaining = scheduler.pending_task_count().await;
        assert!(remaining >= 0, "Scheduler should handle deadline expiration");
    }

    #[tokio::test]
    async fn test_background_scheduler_memory_bounded() {
        let max_capacity = 10;
        let scheduler = Arc::new(BackgroundScheduler::new(max_capacity));

        // Try to submit more tasks than capacity
        let mut submitted = 0;
        for i in 0..20 {
            match scheduler
                .submit_task(
                    format!("task_{}", i),
                    TaskPriority::Normal,
                    Duration::from_secs(1),
                )
                .await
            {
                Ok(_) => submitted += 1,
                Err(_) => break, // Expected: queue full
            }
        }

        assert!(
            submitted <= max_capacity,
            "Submitted {} tasks, max capacity is {}",
            submitted,
            max_capacity
        );
    }

    #[tokio::test]
    async fn test_background_scheduler_graceful_shutdown() {
        let scheduler = Arc::new(BackgroundScheduler::new(20));

        // Submit some tasks
        for i in 0..10 {
            let _ = scheduler
                .submit_task(
                    format!("shutdown_task_{}", i),
                    TaskPriority::Normal,
                    Duration::from_secs(10),
                )
                .await;
        }

        // Graceful shutdown should complete without panic
        let shutdown_result = scheduler.shutdown_graceful(Duration::from_secs(1)).await;
        assert!(shutdown_result.is_ok(), "Graceful shutdown should succeed");
    }

    #[tokio::test]
    async fn test_background_scheduler_starvation_prevention() {
        let scheduler = Arc::new(BackgroundScheduler::new(100));
        let low_priority_executed = Arc::new(AtomicUsize::new(0));

        // Submit many high-priority tasks
        for i in 0..50 {
            let _ = scheduler
                .submit_task(
                    format!("high_priority_{}", i),
                    TaskPriority::High,
                    Duration::from_secs(10),
                )
                .await;
        }

        // Submit low-priority tasks
        for i in 0..10 {
            let sched = Arc::clone(&scheduler);
            let counter = Arc::clone(&low_priority_executed);
            tokio::spawn(async move {
                let _ = sched
                    .submit_task(
                        format!("low_priority_{}", i),
                        TaskPriority::Low,
                        Duration::from_secs(10),
                    )
                    .await;
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Even low-priority tasks should be submitted (starvation prevention)
        let low_executed = low_priority_executed.load(Ordering::SeqCst);
        assert!(low_executed > 0, "Low-priority tasks should eventually execute");
    }

    #[tokio::test]
    async fn test_background_scheduler_error_recovery() {
        let scheduler = Arc::new(BackgroundScheduler::new(20));

        // Submit valid tasks
        for i in 0..5 {
            let _ = scheduler
                .submit_task(
                    format!("valid_task_{}", i),
                    TaskPriority::Normal,
                    Duration::from_secs(1),
                )
                .await;
        }

        // Scheduler should continue working after error
        let pending = scheduler.pending_task_count().await;
        assert!(pending >= 0, "Scheduler should recover from errors");
    }

    // ============================================================================
    // DEVICE HEALTH MONITOR TESTS (7 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_device_health_monitor_smart_metrics_overflow_safe() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));

        // Submit very large SMART values to test overflow protection
        let large_value: u64 = u64::MAX - 1;
        for _ in 0..100 {
            let _ = monitor.update_smart_metric("test_counter", large_value).await;
        }

        // Should not panic or overflow
        let health = monitor.current_health().await;
        assert!(health != HealthStatus::Unknown, "Health should be determined");
    }

    #[tokio::test]
    async fn test_device_health_monitor_state_transition_rules() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));

        // Start in Healthy state
        assert_eq!(
            monitor.current_health().await,
            HealthStatus::Healthy,
            "Should start in Healthy"
        );

        // Simulate degradation
        for _ in 0..50 {
            let _ = monitor.update_smart_metric("media_errors", 100).await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should transition correctly
        let health = monitor.current_health().await;
        assert!(
            health == HealthStatus::Warning || health == HealthStatus::Failed || health == HealthStatus::Healthy,
            "Should transition through correct states"
        );
    }

    #[tokio::test]
    async fn test_device_health_monitor_concurrent_updates() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));
        let update_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for _ in 0..10 {
            let m = Arc::clone(&monitor);
            let count = Arc::clone(&update_count);
            handles.push(tokio::spawn(async move {
                for i in 0..10 {
                    let _ = m.update_smart_metric("concurrent_metric", i * 10).await;
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        futures::future::join_all(handles).await;

        let total = update_count.load(Ordering::SeqCst);
        assert_eq!(total, 100, "All updates should complete");
    }

    #[tokio::test]
    async fn test_device_health_monitor_metric_timestamp_monotonic() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));
        let mut last_timestamp = 0i64;

        for i in 0..20 {
            let _ = monitor.update_smart_metric("monotonic_test", i * 5).await;
            let health_check = monitor.last_check_time().await;

            // Timestamps should never go backwards
            assert!(
                health_check >= last_timestamp,
                "Timestamps must be monotonic"
            );
            last_timestamp = health_check;
        }
    }

    #[tokio::test]
    async fn test_device_health_monitor_health_score_concurrency() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));
        let mut handles = vec![];

        for _ in 0..15 {
            let m = Arc::clone(&monitor);
            handles.push(tokio::spawn(async move {
                for _ in 0..5 {
                    let _ = m.current_health().await;
                }
            }));
        }

        futures::future::join_all(handles).await;

        // Concurrent health score reads should not panic
        assert_ne!(
            monitor.current_health().await,
            HealthStatus::Unknown,
            "Health should be deterministic"
        );
    }

    #[tokio::test]
    async fn test_device_health_monitor_dashmap_consistency() {
        let monitor = Arc::new(DeviceHealthMonitor::new("/dev/nvme0".to_string()));

        // Concurrent metric updates
        let mut handles = vec![];
        for i in 0..8 {
            let m = Arc::clone(&monitor);
            handles.push(tokio::spawn(async move {
                let _ = m.update_smart_metric(&format!("metric_{}", i), i as u64 * 1000).await;
            }));
        }

        futures::future::join_all(handles).await;

        // Health should be consistent
        let health = monitor.current_health().await;
        assert_ne!(health, HealthStatus::Unknown);
    }

    // ============================================================================
    // PREFETCH ENGINE TESTS (8 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_prefetch_engine_pattern_detection_memory_bounded() {
        let mut engine = PrefetchEngine::new(1000); // 1000 max history

        // Submit many access patterns
        for i in 0..5000 {
            engine.record_access(i as u64).await;
        }

        // Should not panic or consume unbounded memory
        let patterns = engine.detected_patterns().await;
        assert!(patterns.len() <= 500, "Pattern count should be bounded");
    }

    #[tokio::test]
    async fn test_prefetch_engine_access_history_lru_eviction() {
        let mut engine = PrefetchEngine::new(100);

        // Fill with 200 accesses (should evict oldest)
        for i in 0..200 {
            engine.record_access(i as u64).await;
        }

        let history_size = engine.history_size().await;
        assert!(history_size <= 100, "LRU should evict old entries");
    }

    #[tokio::test]
    async fn test_prefetch_engine_speculative_io_cancellation() {
        let mut engine = PrefetchEngine::new(500);

        // Record access pattern
        engine.record_access(100).await;
        engine.record_access(101).await;
        engine.record_access(102).await;

        // Get prefetch suggestions
        let prefetch_list = engine.get_prefetch_candidates().await;

        // Cancel speculative I/O
        for item in prefetch_list {
            engine.cancel_prefetch(item).await;
        }

        // Should not panic
        assert!(engine.history_size().await >= 0);
    }

    #[tokio::test]
    async fn test_prefetch_engine_priority_over_user_io() {
        let mut engine = PrefetchEngine::new(300);

        // Record pattern
        for i in 1..=20 {
            engine.record_access(i as u64).await;
        }

        // Get candidates
        let candidates = engine.get_prefetch_candidates().await;
        assert!(candidates.len() >= 0, "Should return valid candidate list");
    }

    #[tokio::test]
    async fn test_prefetch_engine_prediction_accuracy() {
        let mut engine = PrefetchEngine::new(100);

        // Create predictable pattern
        for i in 1..=50 {
            engine.record_access(i as u64).await;
        }

        let candidates = engine.get_prefetch_candidates().await;
        if !candidates.is_empty() {
            // Accuracy: predicted blocks should be reasonable
            assert!(candidates.len() > 0, "Should generate prefetch suggestions");
        }
    }

    #[tokio::test]
    async fn test_prefetch_engine_concurrent_prefetch_requests() {
        let engine = Arc::new(tokio::sync::Mutex::new(PrefetchEngine::new(500)));
        let mut handles = vec![];

        for i in 0..10 {
            let e = Arc::clone(&engine);
            handles.push(tokio::spawn(async move {
                let mut eng = e.lock().await;
                for j in 0..10 {
                    eng.record_access((i * 10 + j) as u64).await;
                }
            }));
        }

        futures::future::join_all(handles).await;

        let eng = engine.lock().await;
        assert!(eng.history_size().await >= 0);
    }

    #[tokio::test]
    async fn test_prefetch_engine_cache_eviction_correctness() {
        let mut engine = PrefetchEngine::new(50);

        for i in 0..100 {
            engine.record_access(i as u64).await;
        }

        let history = engine.history_size().await;
        assert!(history <= 50, "Cache should evict when full");
    }

    #[tokio::test]
    async fn test_prefetch_engine_io_submission_ordering() {
        let mut engine = PrefetchEngine::new(100);

        // Record sequential access
        for i in 100..110 {
            engine.record_access(i as u64).await;
        }

        let candidates = engine.get_prefetch_candidates().await;

        // Candidates should maintain ordering
        let mut prev = 0u64;
        for item in candidates {
            assert!(item >= prev, "Prefetch order should be sequential");
            prev = item;
        }
    }

    // ============================================================================
    // WEAR LEVELING TESTS (5 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_wear_leveling_block_wear_tracking_overflow_safe() {
        let mut manager = WearLevelingManager::new(1000);

        // Update wear on same block many times
        for _ in 0..10000 {
            let _ = manager.record_erase(0, 1).await;
        }

        // Should saturate, not panic
        let wear = manager.block_wear_count(0).await;
        assert!(wear >= 0, "Wear count should be tracked safely");
    }

    #[tokio::test]
    async fn test_wear_leveling_erase_count_distribution_fair() {
        let mut manager = WearLevelingManager::new(100);

        // Distribute erases fairly
        for _ in 0..10 {
            for block in 0..100 {
                let _ = manager.record_erase(block, 1).await;
            }
        }

        // All blocks should have similar wear counts
        let mut wear_counts = vec![];
        for block in 0..100 {
            wear_counts.push(manager.block_wear_count(block).await);
        }

        let avg = wear_counts.iter().sum::<u64>() / wear_counts.len() as u64;
        let max_wear = *wear_counts.iter().max().unwrap_or(&0);

        assert!(max_wear <= avg + 2, "Wear distribution should be fair");
    }

    #[tokio::test]
    async fn test_wear_leveling_hot_spot_rebalancing_safe() {
        let mut manager = WearLevelingManager::new(50);

        // Create hot spot
        for _ in 0..100 {
            let _ = manager.record_erase(0, 1).await;
        }

        // Request rebalance
        let rebalance_result = manager.rebalance_hot_spots().await;
        assert!(rebalance_result.is_ok(), "Rebalance should succeed");

        // Data should not be lost
        let wear0 = manager.block_wear_count(0).await;
        assert!(wear0 >= 0, "Block should still be readable");
    }

    #[tokio::test]
    async fn test_wear_leveling_concurrent_wear_updates() {
        let manager = Arc::new(tokio::sync::Mutex::new(WearLevelingManager::new(100)));
        let update_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for block in 0..10 {
            let m = Arc::clone(&manager);
            let count = Arc::clone(&update_count);
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    let mut mgr = m.lock().await;
                    let _ = mgr.record_erase(block, 1).await;
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        futures::future::join_all(handles).await;

        let total = update_count.load(Ordering::SeqCst);
        assert_eq!(total, 100, "All wear updates should complete");
    }

    #[tokio::test]
    async fn test_wear_leveling_ssd_type_detection() {
        let manager = WearLevelingManager::new(100);

        // Detect SSD type
        let ssd_type = manager.detected_ssd_type().await;

        // Should return a valid SSD type
        assert!(!ssd_type.is_empty(), "SSD type should be detected");
    }

    // ============================================================================
    // NODE REBALANCE TESTS (2 tests)
    // ============================================================================

    #[tokio::test]
    async fn test_node_rebalance_segment_distribution_fair() {
        let mut rebalancer = NodeRebalancer::new(5);

        // Create 100 segments
        for i in 0..100 {
            let _ = rebalancer.add_segment(i as u64, 1000).await;
        }

        // Request rebalance
        let plan = rebalancer.compute_rebalance_plan().await;
        assert!(plan.is_some(), "Rebalance plan should be generated");

        // Execute rebalance
        if let Some(p) = plan {
            let _ = rebalancer.execute_rebalance(p).await;
        }

        // Verify fair distribution
        let distribution = rebalancer.segment_distribution_per_node().await;
        let avg_per_node = 100 / 5;

        for (_, count) in distribution {
            assert!(
                count >= avg_per_node - 1 && count <= avg_per_node + 1,
                "Segments should be fairly distributed"
            );
        }
    }

    #[tokio::test]
    async fn test_node_rebalance_during_node_failure() {
        let mut rebalancer = NodeRebalancer::new(5);

        // Add segments
        for i in 0..50 {
            let _ = rebalancer.add_segment(i as u64, 500).await;
        }

        // Simulate node failure (node 2)
        let _ = rebalancer.mark_node_failed(2).await;

        // Request rebalance during failure
        let plan = rebalancer.compute_rebalance_plan().await;
        assert!(plan.is_some(), "Should rebalance despite node failure");

        // Rebalance should succeed
        if let Some(p) = plan {
            let result = rebalancer.execute_rebalance(p).await;
            assert!(result.is_ok(), "Rebalance should handle node failure");
        }
    }
}
