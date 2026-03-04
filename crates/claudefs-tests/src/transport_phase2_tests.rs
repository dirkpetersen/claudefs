//! Tests for Transport Phase 2 modules: multipath and flowcontrol.

use claudefs_transport::flowcontrol::{
    FlowControlConfig, FlowControlState, FlowController, FlowPermit, WindowController,
};
use claudefs_transport::multipath::{
    MultipathConfig, MultipathError, MultipathRouter, MultipathStats, PathId, PathInfo,
    PathMetrics, PathSelectionPolicy, PathState,
};
use proptest::prelude::*;

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Section 1: MultipathRouter Tests (25 tests)
    // =========================================================================

    #[test]
    fn test_multipath_new() {
        let config = MultipathConfig::default();
        let router = MultipathRouter::new(config);
        let stats = router.stats();
        assert_eq!(stats.total_paths, 0);
    }

    #[test]
    fn test_add_path_returns_id() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        assert!(id.as_u64() > 0);
    }

    #[test]
    fn test_add_multiple_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        router.add_path("eth0".to_string(), 100, 1);
        router.add_path("eth1".to_string(), 100, 2);
        router.add_path("eth2".to_string(), 100, 3);
        let stats = router.stats();
        assert_eq!(stats.total_paths, 3);
    }

    #[test]
    fn test_select_path_empty() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let result = router.select_path();
        assert!(result.is_none());
    }

    #[test]
    fn test_select_path_single() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let result = router.select_path();
        assert_eq!(result, Some(id));
    }

    #[test]
    fn test_active_paths_empty_initially() {
        let router = MultipathRouter::new(MultipathConfig::default());
        let active = router.active_paths();
        assert!(active.is_empty());
    }

    #[test]
    fn test_active_paths_after_add() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let active = router.active_paths();
        assert!(active.contains(&id));
    }

    #[test]
    fn test_path_info_returns_entry() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let info = router.path_info(id);
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.name, "eth0");
        assert_eq!(info.weight, 100);
    }

    #[test]
    fn test_path_info_missing() {
        let router = MultipathRouter::new(MultipathConfig::default());
        let info = router.path_info(PathId::new(999));
        assert!(info.is_none());
    }

    #[test]
    fn test_remove_path() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let result = router.remove_path(id);
        assert!(result);
    }

    #[test]
    fn test_remove_path_missing() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let result = router.remove_path(PathId::new(999));
        assert!(!result);
    }

    #[test]
    fn test_remove_path_clears_active() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.remove_path(id);
        let active = router.active_paths();
        assert!(!active.contains(&id));
    }

    #[test]
    fn test_record_success_updates_metrics() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.record_success(id, 500, 2048);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.bytes_sent, 2048);
        assert!(info.metrics.latency_us > 0);
    }

    #[test]
    fn test_record_failure_marks_failed() {
        let mut config = MultipathConfig::default();
        config.failure_threshold = 3;
        let mut router = MultipathRouter::new(config);

        let id = router.add_path("eth0".to_string(), 100, 1);
        router.record_failure(id, 1024);
        router.record_failure(id, 1024);
        router.record_failure(id, 1024);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.state, PathState::Failed);
    }

    #[test]
    fn test_mark_failed_removes_from_active() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.mark_failed(id);

        let active = router.active_paths();
        assert!(!active.contains(&id));
    }

    #[test]
    fn test_mark_active_restores_to_active() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);

        router.mark_failed(id);
        assert!(!router.active_paths().contains(&id));

        router.mark_active(id);
        assert!(router.active_paths().contains(&id));
    }

    #[test]
    fn test_stats_total_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        router.add_path("eth0".to_string(), 100, 1);
        router.add_path("eth1".to_string(), 100, 2);

        let stats = router.stats();
        assert_eq!(stats.total_paths, 2);
    }

    #[test]
    fn test_stats_active_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id1 = router.add_path("eth0".to_string(), 100, 1);
        router.add_path("eth1".to_string(), 100, 2);
        router.mark_failed(id1);

        let stats = router.stats();
        assert_eq!(stats.active_paths, 1);
    }

    #[test]
    fn test_stats_failed_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id1 = router.add_path("eth0".to_string(), 100, 1);
        router.mark_failed(id1);

        let stats = router.stats();
        assert_eq!(stats.failed_paths, 1);
    }

    #[test]
    fn test_round_robin_selection() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::RoundRobin,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let _id1 = router.add_path("p1".to_string(), 100, 1);
        let _id2 = router.add_path("p2".to_string(), 100, 2);
        let _id3 = router.add_path("p3".to_string(), 100, 3);

        let mut counts = std::collections::HashMap::new();
        for _ in 0..6 {
            if let Some(id) = router.select_path() {
                *counts.entry(id.as_u64()).or_insert(0) += 1;
            }
        }

        for (_, count) in counts.iter() {
            assert!(
                *count >= 1 && *count <= 3,
                "each path should be selected 1-3 times"
            );
        }
    }

    #[test]
    fn test_path_id_as_u64() {
        let id = PathId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_path_state_default_active() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let info = router.path_info(id).unwrap();
        assert_eq!(info.state, PathState::Active);
    }

    #[test]
    fn test_path_metrics_initial_zero() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.latency_us, 0);
        assert_eq!(info.metrics.bytes_sent, 0);
        assert_eq!(info.metrics.errors, 0);
    }

    #[test]
    fn test_config_defaults() {
        let config = MultipathConfig::default();
        assert_eq!(config.policy, PathSelectionPolicy::LowestLatency);
        assert_eq!(config.max_paths, 8);
        assert_eq!(config.probe_interval_ms, 1000);
        assert_eq!(config.failure_threshold, 3);
    }

    // =========================================================================
    // Section 2: FlowControl (FlowController) Tests (20 tests)
    // =========================================================================

    #[test]
    fn test_flow_control_new() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn test_try_acquire_below_limit() {
        let config = FlowControlConfig {
            max_inflight_bytes: 1000,
            max_inflight_requests: 10,
            ..Default::default()
        };
        let controller = FlowController::new(config);
        let permit = controller.try_acquire(100);
        assert!(permit.is_some());
    }

    #[test]
    fn test_try_acquire_returns_permit_bytes() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        let permit = controller.try_acquire(100).unwrap();
        assert_eq!(permit.bytes(), 100);
    }

    #[test]
    fn test_try_acquire_exceeds_limit() {
        let config = FlowControlConfig {
            max_inflight_bytes: 100,
            max_inflight_requests: 1,
            ..Default::default()
        };
        let controller = FlowController::new(config);

        let _p1 = controller.try_acquire(100).unwrap();
        let p2 = controller.try_acquire(1);
        assert!(p2.is_none());
    }

    #[test]
    fn test_inflight_bytes_after_acquire() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        let _permit = controller.try_acquire(500).unwrap();
        assert_eq!(controller.inflight_bytes(), 500);
    }

    #[test]
    fn test_inflight_requests_after_acquire() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        let _permit = controller.try_acquire(100).unwrap();
        assert_eq!(controller.inflight_requests(), 1);
    }

    #[test]
    fn test_release_decrements_inflight() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        let permit = controller.try_acquire(100).unwrap();
        controller.release(100);
        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
        drop(permit);
    }

    #[test]
    fn test_state_open_initially() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);
        assert_eq!(controller.state(), FlowControlState::Open);
    }

    #[test]
    fn test_state_closed_when_full() {
        let config = FlowControlConfig {
            max_inflight_bytes: 100,
            max_inflight_requests: 1,
            ..Default::default()
        };
        let controller = FlowController::new(config);

        let _permit = controller.try_acquire(100).unwrap();
        assert_eq!(controller.state(), FlowControlState::Blocked);
    }

    #[test]
    fn test_state_throttled_at_high_watermark() {
        let config = FlowControlConfig {
            max_inflight_bytes: 100,
            max_inflight_requests: 10,
            high_watermark_pct: 50,
            ..Default::default()
        };
        let controller = FlowController::new(config);

        for _ in 0..5 {
            let _permit = controller.try_acquire(10).unwrap();
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);
    }

    #[test]
    fn test_config_accessor() {
        let config = FlowControlConfig {
            max_inflight_bytes: 12345,
            max_inflight_requests: 100,
            ..Default::default()
        };
        let controller = FlowController::new(config);
        assert_eq!(controller.config().max_inflight_bytes, 12345);
    }

    #[test]
    fn test_flow_permit_bytes() {
        let controller = FlowController::new(FlowControlConfig::default());
        let permit = controller.try_acquire(1234).unwrap();
        assert_eq!(permit.bytes(), 1234);
    }

    #[test]
    fn test_flow_control_permit_auto_release() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        {
            let _permit = controller.try_acquire(100).unwrap();
            assert_eq!(controller.inflight_requests(), 1);
        }

        assert_eq!(controller.inflight_requests(), 0);
    }

    #[test]
    fn test_concurrent_acquire_release() {
        use std::sync::Arc;
        use std::thread;

        let config = FlowControlConfig {
            max_inflight_requests: 100,
            max_inflight_bytes: 10000,
            ..Default::default()
        };
        let controller = Arc::new(FlowController::new(config));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let c = controller.clone();
                thread::spawn(move || {
                    for _ in 0..50 {
                        if let Some(p) = c.try_acquire(10) {
                            thread::yield_now();
                            drop(p);
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(controller.inflight_requests(), 0);
    }

    #[test]
    fn test_flow_control_config_serialization() {
        let config = FlowControlConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: FlowControlConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.max_inflight_bytes, deserialized.max_inflight_bytes);
    }

    #[test]
    fn test_flow_control_state_serialization() {
        let states = vec![
            FlowControlState::Open,
            FlowControlState::Throttled,
            FlowControlState::Blocked,
        ];
        for state in states {
            let serialized = serde_json::to_string(&state).unwrap();
            let deserialized: FlowControlState = serde_json::from_str(&serialized).unwrap();
            assert_eq!(state, deserialized);
        }
    }

    #[test]
    fn test_permit_drop_order() {
        let controller = FlowController::new(FlowControlConfig::default());
        let _p1 = controller.try_acquire(10).unwrap();
        let p2 = controller.try_acquire(20).unwrap();
        let _p3 = controller.try_acquire(30).unwrap();

        assert_eq!(controller.inflight_bytes(), 60);
        drop(p2);
        assert_eq!(controller.inflight_bytes(), 40);
    }

    #[test]
    fn test_acquire_zero_bytes() {
        let controller = FlowController::new(FlowControlConfig::default());
        let permit = controller.try_acquire(0).unwrap();
        assert_eq!(permit.bytes(), 0);
        assert_eq!(controller.inflight_requests(), 1);
    }

    #[test]
    fn test_multiple_small_acquires() {
        let config = FlowControlConfig {
            max_inflight_bytes: 100,
            max_inflight_requests: 10,
            ..Default::default()
        };
        let controller = FlowController::new(config);

        for _ in 0..10 {
            let _permit = controller.try_acquire(10).unwrap();
        }
        assert_eq!(controller.inflight_bytes(), 100);
        assert_eq!(controller.inflight_requests(), 10);
    }

    // =========================================================================
    // Section 3: SlidingWindow (WindowController) Tests (20 tests)
    // =========================================================================

    #[test]
    fn test_sliding_window_new() {
        let window = WindowController::new(10);
        assert_eq!(window.window_size(), 10);
    }

    #[test]
    fn test_window_size_accessor() {
        let window = WindowController::new(32);
        assert_eq!(window.window_size(), 32);
    }

    #[test]
    fn test_initial_can_send() {
        let window = WindowController::new(10);
        assert!(window.can_send());
    }

    #[test]
    fn test_window_start_initial() {
        let window = WindowController::new(10);
        assert_eq!(window.window_start(), 0);
    }

    #[test]
    fn test_window_end_initial() {
        let window = WindowController::new(10);
        assert_eq!(window.window_end(), 10);
    }

    #[test]
    fn test_advance_first_seq() {
        let window = WindowController::new(10);
        assert!(window.advance(0));
    }

    #[test]
    fn test_advance_within_window() {
        let window = WindowController::new(5);
        assert!(window.advance(0));
        assert!(window.advance(1));
        assert!(window.advance(2));
        assert!(window.advance(3));
        assert!(window.advance(4));
    }

    #[test]
    fn test_advance_beyond_window() {
        let window = WindowController::new(5);
        assert!(window.advance(0));
        assert!(window.advance(4));
        assert!(!window.advance(5));
    }

    #[test]
    fn test_in_flight_after_advance() {
        let window = WindowController::new(5);
        window.advance(0);
        window.advance(1);
        assert_eq!(window.in_flight(), 2);
    }

    #[test]
    fn test_ack_decrements_in_flight() {
        let window = WindowController::new(5);
        window.advance(0);
        window.advance(1);
        assert_eq!(window.in_flight(), 2);

        window.ack(0);
        assert_eq!(window.in_flight(), 1);
    }

    #[test]
    fn test_can_send_false_when_full() {
        let window = WindowController::new(3);
        assert!(window.advance(0));
        assert!(window.advance(1));
        assert!(window.advance(2));
        assert!(!window.can_send());
    }

    #[test]
    fn test_ack_slides_window() {
        let window = WindowController::new(3);
        window.advance(0);
        window.advance(1);
        window.advance(2);

        window.ack(0);
        window.ack(1);

        assert!(window.can_send());
        assert!(window.advance(3));
    }

    #[test]
    fn test_window_boundaries_after_advance() {
        let window = WindowController::new(5);
        window.advance(0);
        window.advance(1);
        window.advance(2);

        assert_eq!(window.window_start(), 0);
        assert_eq!(window.window_end(), 5);
    }

    #[test]
    fn test_multiple_advances() {
        let window = WindowController::new(10);
        for i in 0..5 {
            assert!(window.advance(i));
        }
        assert_eq!(window.in_flight(), 5);
    }

    #[test]
    fn test_ack_out_of_order() {
        let window = WindowController::new(5);
        window.advance(0);
        window.advance(1);
        window.advance(2);

        window.ack(2);
        assert_eq!(window.in_flight(), 0);
    }

    #[test]
    fn test_zero_window_size() {
        let window = WindowController::new(0);
        assert!(!window.can_send());
    }

    #[test]
    fn test_window_size_32() {
        let window = WindowController::new(32);
        assert_eq!(window.window_size(), 32);

        for i in 0..32 {
            assert!(window.advance(i));
        }
        assert!(!window.can_send());
    }

    #[test]
    fn test_in_flight_initial_zero() {
        let window = WindowController::new(10);
        assert_eq!(window.in_flight(), 0);
    }

    #[test]
    fn test_advance_duplicate_seq() {
        let window = WindowController::new(5);
        assert!(window.advance(0));
        assert!(!window.advance(0));
    }

    #[test]
    fn test_window_sliding_after_acks() {
        let window = WindowController::new(3);
        window.advance(0);
        window.advance(1);
        window.advance(2);

        assert_eq!(window.in_flight(), 3);
        window.ack(0);
        assert_eq!(window.window_start(), 1);
        assert!(window.can_send());
    }

    #[test]
    fn test_window_controller_default() {
        let window = WindowController::default();
        assert_eq!(window.window_size(), 256);
    }

    #[test]
    fn test_sequential_send_receive() {
        let window = WindowController::new(5);

        for i in 0u64..10 {
            while !window.can_send() {}
            window.advance(i);

            if i >= 5 {
                window.ack(i - 5);
            }
        }

        assert!(window.window_start() > 0);
    }
}

// =========================================================================
// Proptest Tests
// =========================================================================

#[cfg(test)]
mod proptest_tests {
    use super::*;

    proptest! {
        #[test]
        fn prop_add_remove_paths(n in 1..10u32) {
            let mut router = MultipathRouter::new(MultipathConfig::default());
            let mut ids = Vec::new();

            for _ in 0..n {
                let id = router.add_path("path".to_string(), 100, 1);
                ids.push(id);
            }

            assert_eq!(router.stats().total_paths, n as usize);

            for id in ids {
                router.remove_path(id);
            }

            assert!(router.active_paths().is_empty());
        }

        #[test]
        fn prop_window_invariant(window_size in 1u32..1000) {
            let window = WindowController::new(window_size);

            for _ in 0..window_size {
                if !window.can_send() {
                    break;
                }
                window.advance(0);
            }

            let in_flight = window.in_flight();
            prop_assert!(in_flight <= window_size);
        }

        #[test]
        fn prop_path_id_roundtrip(id in 0u64..10000) {
            let path_id = PathId::new(id);
            prop_assert_eq!(path_id.as_u64(), id);
        }

        #[test]
        fn prop_flow_control_never_negative(config in 1u32..1000, bytes in 1u64..10000) {
            let cfg = FlowControlConfig {
                max_inflight_requests: config,
                max_inflight_bytes: bytes,
                ..Default::default()
            };
            let controller = FlowController::new(cfg);

            if let Some(_permit) = controller.try_acquire(bytes / 2) {
                prop_assert!(controller.inflight_requests() >= 0);
                prop_assert!(controller.inflight_bytes() >= 0);
            }

            prop_assert!(controller.inflight_requests() >= 0);
            prop_assert!(controller.inflight_bytes() >= 0);
        }
    }
}
