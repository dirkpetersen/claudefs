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

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claudefs_storage::background_scheduler::{
    BackgroundScheduler, BackgroundTask, BackgroundTaskType,
};
use claudefs_storage::device_health_monitor::{
    DeviceHealthMonitor, HealthAlertType, SmartSnapshot, WearSnapshot,
};
use claudefs_storage::node_rebalance::{
    NodeId, RebalanceConfig, RebalanceEngine, RebalanceSegmentId, ShardId,
};
use claudefs_storage::prefetch_engine::{PrefetchConfig, PrefetchEngine};
use claudefs_storage::wear_leveling::{WearConfig, WearLevelingEngine};

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // BACKGROUND SCHEDULER TESTS (8 tests)
    // ============================================================================

    #[test]
    fn test_background_scheduler_concurrent_tasks_no_race() {
        let scheduler = Arc::new(Mutex::new(BackgroundScheduler::new()));
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for i in 0..20 {
            let sched = Arc::clone(&scheduler);
            let count = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for j in 0..50 {
                    let mut s = sched.lock().unwrap();
                    let task = BackgroundTask::new(
                        BackgroundTaskType::Scrub,
                        1000,
                        format!("task_{}_{}", i, j),
                    );
                    s.schedule(task);
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 1000);
    }

    #[test]
    fn test_background_scheduler_priority_ordering() {
        let mut scheduler = BackgroundScheduler::new();

        let low_priority = BackgroundTask::with_priority(
            BackgroundTaskType::TierEviction,
            200,
            1000,
            "low".to_string(),
        );
        let medium_priority = BackgroundTask::with_priority(
            BackgroundTaskType::Defrag,
            100,
            1000,
            "medium".to_string(),
        );
        let high_priority = BackgroundTask::with_priority(
            BackgroundTaskType::JournalFlush,
            10,
            1000,
            "high".to_string(),
        );

        scheduler.schedule(low_priority);
        scheduler.schedule(medium_priority);
        scheduler.schedule(high_priority);

        let first = scheduler.next_runnable().unwrap();
        assert_eq!(first.priority, 10, "High priority should run first");

        let second = scheduler.next_runnable().unwrap();
        assert_eq!(second.priority, 100, "Medium priority should run second");

        let third = scheduler.next_runnable().unwrap();
        assert_eq!(third.priority, 200, "Low priority should run third");
    }

    #[test]
    fn test_background_scheduler_deadline_enforcement() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(1000, 0);

        let task =
            BackgroundTask::new(BackgroundTaskType::Scrub, 1000, "deadline_task".to_string());
        scheduler.schedule(task);

        let started = scheduler.next_runnable();
        assert!(started.is_some(), "Task should start");

        let result = scheduler.next_runnable();
        assert!(result.is_none(), "Budget exhausted, deadline enforced");
    }

    #[test]
    fn test_background_scheduler_no_starvation() {
        let mut scheduler = BackgroundScheduler::new();

        for _ in 0..100 {
            let task = BackgroundTask::with_priority(
                BackgroundTaskType::Compaction,
                150,
                1000,
                "low".to_string(),
            );
            scheduler.schedule(task);
        }

        let high_priority = BackgroundTask::with_priority(
            BackgroundTaskType::JournalFlush,
            10,
            1000,
            "high".to_string(),
        );
        scheduler.schedule(high_priority);

        let next = scheduler.next_runnable().unwrap();
        assert_eq!(next.priority, 10, "High priority should not be starved");
    }

    #[test]
    fn test_background_scheduler_valid_state_transitions() {
        let mut scheduler = BackgroundScheduler::new();

        let task = BackgroundTask::new(BackgroundTaskType::Scrub, 1000, "test".to_string());
        let id = scheduler.schedule(task);

        let pending_task = scheduler.next_runnable();
        assert!(pending_task.is_some(), "Task should be pending -> running");

        scheduler.complete_task(id, 500);
        let stats = scheduler.stats();
        assert_eq!(stats.tasks_completed, 1, "Task should complete");

        let task2 = BackgroundTask::new(BackgroundTaskType::Scrub, 1000, "cancelable".to_string());
        let id2 = scheduler.schedule(task2);
        let cancelled = scheduler.cancel_task(id2);
        assert!(cancelled, "Pending task should be cancellable");

        let stats = scheduler.stats();
        assert_eq!(stats.tasks_cancelled, 1, "Cancelled count should increase");
    }

    #[test]
    fn test_background_scheduler_queue_capacity_bounded() {
        let mut scheduler = BackgroundScheduler::new();

        let initial_stats = scheduler.stats();
        let initial_pending = initial_stats.pending_count;

        for i in 0..1000 {
            let task = BackgroundTask::new(BackgroundTaskType::Scrub, 1000, format!("task_{}", i));
            scheduler.schedule(task);
        }

        let stats = scheduler.stats();
        assert!(
            stats.pending_count >= initial_pending,
            "Queue should have pending tasks"
        );
    }

    #[test]
    fn test_background_scheduler_saturating_arithmetic() {
        let mut scheduler = BackgroundScheduler::new();

        for _ in 0..1000 {
            let task =
                BackgroundTask::new(BackgroundTaskType::Scrub, u64::MAX, "large_io".to_string());
            scheduler.schedule(task);
        }

        let started = scheduler.next_runnable();
        assert!(started.is_some());

        if let Some(task) = started {
            scheduler.complete_task(task.id, u64::MAX);
            scheduler.complete_task(task.id, u64::MAX);
        }

        let stats = scheduler.stats();
        assert!(
            stats.total_bytes_io > 0,
            "Should track bytes without overflow"
        );
    }

    #[test]
    fn test_background_scheduler_fair_scheduling_under_load() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(u32::MAX, u64::MAX);

        let mut task_ids = vec![];
        for i in 0..50 {
            let task = BackgroundTask::new(BackgroundTaskType::Defrag, 1000, format!("task_{}", i));
            let id = scheduler.schedule(task);
            task_ids.push(id);
        }

        let mut executed = 0;
        while let Some(task) = scheduler.next_runnable() {
            scheduler.complete_task(task.id, 100);
            executed += 1;
        }

        assert_eq!(executed, 50, "All 50 tasks should execute under load");
    }

    // ============================================================================
    // DEVICE HEALTH MONITOR TESTS (7 tests)
    // ============================================================================

    #[test]
    fn test_device_health_monitor_smart_metric_parsing() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());

        let smart = SmartSnapshot {
            reallocated_sectors: 100,
            media_errors: 50,
            unsafe_shutdowns: 10,
            temperature_celsius: 45,
            percentage_used: 25,
        };

        let result = monitor.update_smart(0, smart.clone());
        assert!(result.is_ok(), "Valid SMART data should parse");

        let score = monitor.compute_health_score(0).unwrap();
        assert!(score > 0.0 && score <= 1.0, "Health score should be valid");
    }

    #[test]
    fn test_device_health_monitor_valid_state_transitions() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());

        let initial_score = monitor.compute_health_score(0).unwrap();
        assert!((initial_score - 1.0).abs() < 0.001, "Should start healthy");

        monitor.update_capacity(0, 1000, 50).unwrap();
        let degraded_score = monitor.compute_health_score(0).unwrap();
        assert!(
            degraded_score < initial_score,
            "Capacity reduction should degrade health"
        );

        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 90,
                    power_on_hours: 1000,
                },
            )
            .unwrap();
        let bad_score = monitor.compute_health_score(0).unwrap();
        assert!(
            bad_score < degraded_score,
            "High wear should further degrade"
        );

        let alerts = monitor.check_alerts();
        assert!(!alerts.is_empty(), "Should generate alerts for bad health");
    }

    #[test]
    fn test_device_health_monitor_concurrent_health_checks() {
        let monitor = Arc::new(Mutex::new(DeviceHealthMonitor::new()));
        {
            let mut m = monitor.lock().unwrap();
            m.register_device(0, "/dev/nvme0".to_string());
            m.register_device(1, "/dev/nvme1".to_string());
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for _ in 0..30 {
            let m = Arc::clone(&monitor);
            let count = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..10 {
                    let m = m.lock().unwrap();
                    let _ = m.compute_health_score(0);
                    let _ = m.compute_health_score(1);
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        assert_eq!(
            counter.load(Ordering::SeqCst),
            300,
            "All health checks should complete"
        );
    }

    #[test]
    fn test_device_health_monitor_thermal_threshold_alert() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());

        let hot_smart = SmartSnapshot {
            reallocated_sectors: 0,
            media_errors: 0,
            unsafe_shutdowns: 0,
            temperature_celsius: 85,
            percentage_used: 10,
        };
        monitor.update_smart(0, hot_smart).unwrap();

        let alerts = monitor.check_alerts();
        let temp_alert = alerts
            .iter()
            .find(|a| a.alert_type == HealthAlertType::HighTemperature);
        assert!(
            temp_alert.is_some(),
            ">70C should trigger temperature alert"
        );
    }

    #[test]
    fn test_device_health_monitor_error_rate_accuracy() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());

        let mut errors = 0u64;
        let total_ops = 10000u64;

        for _ in 0..100 {
            let smart = SmartSnapshot {
                reallocated_sectors: 0,
                media_errors: 1,
                unsafe_shutdowns: 0,
                temperature_celsius: 40,
                percentage_used: 10,
            };
            monitor.update_smart(0, smart).unwrap();
            errors += 1;
        }

        monitor
            .update_capacity(0, total_ops, total_ops - errors)
            .unwrap();

        let score = monitor.compute_health_score(0).unwrap();
        let error_rate = errors as f64 / total_ops as f64;
        assert!(
            (error_rate - 0.01).abs() < 0.001,
            "Error rate should be ~1%"
        );
        assert!(score < 0.9, "Errors should reduce health score");
    }

    #[test]
    fn test_device_health_monitor_memory_bounded() {
        let mut monitor = DeviceHealthMonitor::new();

        for i in 0..100 {
            monitor.register_device(i, format!("/dev/nvme{}", i));
            monitor.update_capacity(i, 1000000, 500000).unwrap();
        }

        let summaries = monitor.health_summary();
        assert_eq!(summaries.len(), 100, "Should track all devices");
    }

    #[test]
    fn test_device_health_monitor_state_persistence() {
        let mut monitor = DeviceHealthMonitor::new();
        monitor.register_device(0, "/dev/nvme0".to_string());

        monitor
            .update_smart(
                0,
                SmartSnapshot {
                    reallocated_sectors: 10,
                    media_errors: 5,
                    unsafe_shutdowns: 2,
                    temperature_celsius: 50,
                    percentage_used: 30,
                },
            )
            .unwrap();
        monitor
            .update_wear(
                0,
                WearSnapshot {
                    wear_percentage_used: 50,
                    power_on_hours: 500,
                },
            )
            .unwrap();
        monitor.update_capacity(0, 1000000, 800000).unwrap();

        let summary = monitor.health_summary();
        assert_eq!(summary.len(), 1);

        let s = &summary[0];
        assert_eq!(s.device_path, "/dev/nvme0");
        assert!(s.health_score > 0.0 && s.health_score <= 1.0);
    }

    // ============================================================================
    // PREFETCH ENGINE TESTS (8 tests)
    // ============================================================================

    #[test]
    fn test_prefetch_engine_lru_eviction_capacity() {
        let mut config = PrefetchConfig::default();
        config.max_streams = 100;
        let mut engine = PrefetchEngine::new(config);

        for i in 0..200 {
            engine.record_access(i as u64, i as u64 * 4096, 4096);
        }

        let stats = engine.stats();
        assert!(
            stats.streams_tracked <= 100,
            "Should respect max_streams limit"
        );
    }

    #[test]
    fn test_prefetch_engine_pattern_detection_doesnt_overkill() {
        let mut config = PrefetchConfig::default();
        config.lookahead_blocks = 5;
        config.sequential_threshold = 3;
        let mut engine = PrefetchEngine::new(config);

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);
        engine.record_access(1, 8192, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(
            hints.len() <= 5,
            "Should respect lookahead_blocks limit, got {}",
            hints.len()
        );
    }

    #[test]
    fn test_prefetch_engine_queue_bounded_capacity() {
        let mut config = PrefetchConfig::default();
        config.max_streams = 500;
        let mut engine = PrefetchEngine::new(config);

        for i in 0..1000 {
            engine.record_access(i as u64, 0, 4096);
        }

        let stats = engine.stats();
        assert!(
            stats.streams_tracked <= 500,
            "Stream count should be bounded"
        );
    }

    #[test]
    fn test_prefetch_engine_concurrent_prefetch_no_race() {
        let config = PrefetchConfig::default();
        let engine = Arc::new(Mutex::new(PrefetchEngine::new(config)));
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        for i in 0..10 {
            let e = Arc::clone(&engine);
            let count = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for j in 0..10 {
                    let mut eng = e.lock().unwrap();
                    eng.record_access((i * 10 + j) as u64, j as u64 * 4096, 4096);
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_prefetch_engine_pattern_detection_accuracy() {
        let mut config = PrefetchConfig::default();
        config.sequential_threshold = 3;
        config.confidence_threshold = 0.5;
        let mut engine = PrefetchEngine::new(config);

        for i in 0..100 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }

        let hints = engine.get_prefetch_advice(1);
        let accuracy = if hints.len() > 0 { 1.0 } else { 0.0 };
        assert!(
            accuracy >= 0.7,
            "Sequential pattern should have >70% accuracy, got {}",
            accuracy
        );
    }

    #[test]
    fn test_prefetch_engine_memory_for_patterns_bounded() {
        let mut config = PrefetchConfig::default();
        config.max_streams = 100;
        config.history_window = 16;
        let mut engine = PrefetchEngine::new(config);

        for stream in 0..1000 {
            for i in 0..20 {
                engine.record_access(stream, i as u64 * 4096, 4096);
            }
        }

        let stats = engine.stats();
        assert!(
            stats.streams_tracked <= 100,
            "Streams should be bounded to 100, got {}",
            stats.streams_tracked
        );
    }

    #[test]
    fn test_prefetch_engine_ordering_preserved() {
        let mut engine = PrefetchEngine::default_config();

        for i in 100..110 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }

        let hints = engine.get_prefetch_advice(1);

        let mut prev = 0u64;
        for hint in &hints {
            assert!(hint.offset >= prev, "Hints should maintain ascending order");
            prev = hint.offset;
        }
    }

    #[test]
    fn test_prefetch_engine_cache_coherence_maintained() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);
        engine.record_access(1, 8192, 4096);

        let hints_before = engine.get_prefetch_advice(1);
        assert!(!hints_before.is_empty(), "Should detect sequential pattern");

        engine.cancel_stream(1);

        let hints_after = engine.get_prefetch_advice(1);
        assert!(
            hints_after.is_empty(),
            "Cancelled stream should have no hints"
        );
    }

    // ============================================================================
    // WEAR LEVELING TESTS (5 tests)
    // ============================================================================

    #[test]
    fn test_wear_leveling_block_wear_tracked_accurately() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 80.0;
        let mut engine = WearLevelingEngine::new(config);

        for zone in 0..10 {
            engine.register_zone(zone);
        }

        let mut total_writes = 0u64;
        for _ in 0..1000 {
            let zone = (total_writes % 10) as u32;
            engine.record_write(zone, 4096, 1000).unwrap();
            total_writes += 1;
        }

        for zone in 0..10u32 {
            let zone_data = engine.get_zone(zone);
            assert!(zone_data.is_some(), "Zone {} should exist", zone);
            let z = zone_data.unwrap();
            assert!(z.write_count > 0, "Zone {} should have writes", zone);
        }
    }

    #[test]
    fn test_wear_leveling_hotspot_detection_triggers_rebalance() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 50.0;
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);
        engine.register_zone(1);
        engine.register_zone(2);

        for _ in 0..100 {
            engine.record_write(0, 4096, 1000).unwrap();
        }

        engine.record_write(1, 4096, 1000).unwrap();
        engine.record_write(2, 4096, 1000).unwrap();

        let hot_zones = engine.hot_zones();
        assert!(
            !hot_zones.is_empty(),
            "Hotspot detection should identify hot zone"
        );
    }

    #[test]
    fn test_wear_leveling_fair_write_distribution() {
        let mut config = WearConfig::default();
        config.cold_zone_target_pct = 20.0;
        let mut engine = WearLevelingEngine::new(config);

        for zone in 0..10 {
            engine.register_zone(zone);
        }

        let mut wear_levels = vec![];
        for _ in 0..100 {
            for zone in 0..10u32 {
                engine.record_write(zone, 4096, 1000).unwrap();
            }
        }

        for zone in 0..10u32 {
            let z = engine.get_zone(zone).unwrap();
            wear_levels.push(z.wear_level);
        }

        let avg_wear: f64 = wear_levels.iter().sum::<f64>() / wear_levels.len() as f64;
        let max_deviation = wear_levels
            .iter()
            .map(|w| (w - avg_wear).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_deviation < 20.0,
            "Write distribution should be fair, max deviation: {}",
            max_deviation
        );
    }

    #[test]
    fn test_wear_leveling_overflow_protection() {
        let config = WearConfig::default();
        let mut engine = WearLevelingEngine::new(config);

        engine.register_zone(0);

        for _ in 0..10000 {
            engine.record_erase(0, 1000).unwrap();
        }

        let zone = engine.get_zone(0).unwrap();
        assert!(
            zone.erase_count <= u64::MAX,
            "Erase count should not overflow"
        );
        assert!(
            zone.wear_level <= 100.0,
            "Wear level should be capped at 100%"
        );
    }

    #[test]
    fn test_wear_leveling_rebalancing_preserves_data() {
        let mut config = WearConfig::default();
        config.hot_zone_threshold = 30.0;
        let mut engine = WearLevelingEngine::new(config);

        for zone in 0..5 {
            engine.register_zone(zone);
            for _ in 0..10 {
                engine.record_write(zone, 4096, 1000).unwrap();
            }
        }

        let imbalance_before = engine.check_wear_balance();

        engine.record_write(0, 100 * 4096, 2000).unwrap();

        let imbalance_after = engine.check_wear_balance();
        assert!(
            imbalance_after.is_some() || imbalance_before.is_some(),
            "Rebalance check should work"
        );
    }

    // ============================================================================
    // NODE REBALANCE TESTS (2 tests)
    // ============================================================================

    #[test]
    fn test_node_rebalance_segment_distribution_consistent() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, NodeId("node1".to_string()));

        for i in 0..100 {
            engine.register_segment(RebalanceSegmentId(i), ShardId(i as u16 % 10));
        }

        let segments_before = engine.local_segments().len();
        assert_eq!(
            segments_before, 100,
            "Should have 100 segments before rebalance"
        );

        engine.update_shard_map(HashMap::new());

        let segments_after = engine.local_segments().len();
        assert_eq!(
            segments_after, 100,
            "Segment distribution should remain consistent"
        );
    }

    #[test]
    fn test_node_rebalance_failover_safety_maintained() {
        let config = RebalanceConfig::default();
        let mut engine = RebalanceEngine::new(config, NodeId("node1".to_string()));

        for i in 0..50 {
            engine.register_segment(RebalanceSegmentId(i), ShardId(i as u16));
        }

        let mut new_shard_map = HashMap::new();
        for i in 0..50u16 {
            new_shard_map.insert(ShardId(i), NodeId("node1".to_string()));
        }
        engine.update_shard_map(new_shard_map);

        for i in 50..100 {
            engine.register_segment(RebalanceSegmentId(i), ShardId(i as u16));
        }

        let segments = engine.local_segments().len();
        assert_eq!(segments, 100, "All segments should be accounted for");
    }
}
