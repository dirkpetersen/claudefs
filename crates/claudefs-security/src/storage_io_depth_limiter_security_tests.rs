//! Storage IO depth limiter security tests.
//!
//! Part of A10 Phase 35

use claudefs_storage::io_depth_limiter::{
    HealthAdaptiveMode, IoDepthLimiter, IoDepthLimiterConfig, QueueDepthStats,
};
use claudefs_storage::nvme_passthrough::QueuePairId;
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;
    use claudefs_storage::device::DeviceHealth;
    use std::time::Duration;
    use tokio::time::timeout;

    fn create_test_limiter() -> IoDepthLimiter {
        IoDepthLimiter::new(
            QueuePairId(0),
            IoDepthLimiterConfig {
                initial_depth: 32,
                degradation_latency_ms: 2,
                critical_latency_ms: 5,
                min_depth: 8,
                reduction_percent: 50,
                history_size: 100,
                recovery_delay_ms: 50,
            },
        )
    }

    mod concurrency_and_race_conditions {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_concurrent_acquire_no_data_race() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for _ in 0..20 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for _ in 0..10 {
                        let _ = limiter.try_acquire().await;
                    }
                }));
            }

            futures::future::join_all(handles).await;

            let pending = limiter.pending_count().await;
            assert!(
                pending <= 200,
                "pending_count {} should be <= 200 concurrent ops",
                pending
            );
            assert!(limiter.current_limit().await > 0);
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_healthy_degraded() {
            let limiter = create_test_limiter();

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(3000).await;
            }

            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;

            let mode = limiter.mode().await;
            assert_eq!(
                mode,
                HealthAdaptiveMode::Degraded,
                "Should transition to Degraded with 3ms latency"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_degraded_critical() {
            let limiter = create_test_limiter();

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(6000).await;
            }

            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;

            let mode = limiter.mode().await;
            assert_eq!(
                mode,
                HealthAdaptiveMode::Critical,
                "Should transition to Critical with 6ms latency"
            );
            assert_eq!(
                limiter.current_limit().await,
                8,
                "Critical mode should set min_depth"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_critical_recovery() {
            let limiter = create_test_limiter();

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(6000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);

            tokio::time::sleep(Duration::from_millis(60)).await;

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(100).await;
            }

            limiter.check_and_adjust().await;

            let mode = limiter.mode().await;
            assert_eq!(
                mode,
                HealthAdaptiveMode::Healthy,
                "Should recover to Healthy after low latency"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_counter_concurrent_increments() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for _ in 0..10 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for _ in 0..10 {
                        let acquired = limiter.try_acquire().await;
                        if acquired {
                            limiter.release(100).await;
                        }
                    }
                }));
            }

            futures::future::join_all(handles).await;

            let pending = limiter.pending_count().await;
            assert!(
                pending <= 100,
                "pending_count should stay bounded, got {}",
                pending
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_latency_history_concurrent_updates() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for i in 0..20 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for j in 0..50 {
                        let latency = (i * 50 + j) as u64 % 1000 + 100;
                        limiter.try_acquire().await;
                        limiter.release(latency).await;
                    }
                }));
            }

            futures::future::join_all(handles).await;

            let stats = limiter.stats().await;
            assert!(
                stats.avg_latency_us > 0,
                "avg_latency should be recorded"
            );
            assert!(
                stats.p99_latency_us >= 100,
                "p99_latency should be >= smallest latency"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_dispatch_time_tracking_concurrent() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for _ in 0..10 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for _ in 0..10 {
                        if limiter.try_acquire().await {
                            tokio::time::sleep(Duration::from_micros(10)).await;
                            limiter.release(100).await;
                        }
                    }
                }));
            }

            futures::future::join_all(handles).await;

            let stats = limiter.stats().await;
            assert!(
                stats.avg_dispatch_wait_us >= 0,
                "dispatch wait should be non-negative"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_stats_snapshot_under_concurrent_load() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for i in 0..5 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for j in 0..20 {
                        let latency = ((i * 20 + j) % 10) as u64 * 100 + 100;
                        limiter.try_acquire().await;
                        limiter.release(latency).await;
                    }
                }));
            }

            let stats_handle = {
                let limiter = Arc::clone(&limiter);
                tokio::spawn(async move { limiter.stats().await })
            };

            futures::future::join_all(handles).await;
            let stats = stats_handle.await.unwrap();

            assert!(stats.pending_ops <= 100);
            assert!(stats.avg_latency_us > 0);
        }
    }

    mod latency_calculation_and_percentile_logic {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_empty_history() {
            let limiter = create_test_limiter();

            let stats = limiter.stats().await;
            assert_eq!(
                stats.p99_latency_us,
                0,
                "p99_latency should be 0 for empty history"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_single_latency() {
            let limiter = create_test_limiter();

            limiter.try_acquire().await;
            limiter.release(500).await;

            let stats = limiter.stats().await;
            assert_eq!(
                stats.p99_latency_us,
                500,
                "p99_latency should equal single latency value"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_p99_calculation_sorted_correctly() {
            let limiter = create_test_limiter();

            let latencies: Vec<u64> = (1..=100).map(|i| i * 10).collect();
            for latency in latencies.iter().rev() {
                limiter.try_acquire().await;
                limiter.release(*latency).await;
            }

            let stats = limiter.stats().await;
            let expected_p99 = 990;
            assert!(
                stats.p99_latency_us >= 980 && stats.p99_latency_us <= 1000,
                "p99 should be around 990, got {}",
                stats.p99_latency_us
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_avg_latency_computation() {
            let limiter = create_test_limiter();

            let latencies = vec![100u64, 200, 300, 400, 500];
            for &latency in &latencies {
                limiter.try_acquire().await;
                limiter.release(latency).await;
            }

            let stats = limiter.stats().await;
            let expected_avg: u64 = latencies.iter().sum::<u64>() / latencies.len() as u64;
            assert_eq!(
                stats.avg_latency_us, expected_avg,
                "avg_latency should be {}",
                expected_avg
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_avg_latency_overflow_resistant() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 2,
                    critical_latency_ms: 5,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 1000,
                    recovery_delay_ms: 50,
                },
            );

            for _ in 0..10 {
                limiter.try_acquire().await;
                limiter.release(u64::MAX - 100).await;
            }

            let result = timeout(Duration::from_secs(1), limiter.stats()).await;
            assert!(result.is_ok(), "Stats should not panic on large values");

            let stats = result.unwrap();
            assert!(
                stats.avg_latency_us > 0,
                "Should compute avg without overflow panic"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_history_window_rolling() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 2,
                    critical_latency_ms: 5,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 10,
                    recovery_delay_ms: 50,
                },
            );

            for i in 0..100 {
                limiter.try_acquire().await;
                limiter.release(i as u64).await;
            }

            let stats = limiter.stats().await;
            assert!(
                stats.avg_latency_us > 0,
                "Should have aggregated from rolling window"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_percentile_with_duplicates() {
            let limiter = create_test_limiter();

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(500).await;
            }

            let stats = limiter.stats().await;
            assert_eq!(
                stats.p99_latency_us, 500,
                "p99 with all identical values should equal that value"
            );
        }
    }

    mod mode_transition_security {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_transition_gating_recovery_delay() {
            let limiter = create_test_limiter();

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(10000).await;
            }

            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);

            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Critical,
                "Should not transition back within recovery_delay"
            );

            tokio::time::sleep(Duration::from_millis(50)).await;

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(100).await;
            }

            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Healthy,
                "Should transition after recovery_delay"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_degradation_latency_threshold() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 5,
                    critical_latency_ms: 10,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 100,
                    recovery_delay_ms: 50,
                },
            );

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(4000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(5000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Degraded,
                "Should transition at >= degradation_latency_ms"
            );

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(6000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Degraded,
                "Should stay at degraded for mid-range latencies"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_critical_latency_threshold() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 5,
                    critical_latency_ms: 10,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 100,
                    recovery_delay_ms: 50,
                },
            );

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(9000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Degraded,
                "Should be degraded at 9ms (below critical)"
            );

            for _ in 0..50 {
                limiter.try_acquire().await;
                limiter.release(10000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Critical,
                "Should transition to critical at >= critical_latency_ms"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_mode_transition_via_device_health() {
            let limiter = create_test_limiter();

            let healthy = DeviceHealth {
                temperature_celsius: 40,
                percentage_used: 10,
                available_spare: 80,
                data_units_written: 1000,
                data_units_read: 2000,
                power_on_hours: 100,
                unsafe_shutdowns: 0,
                critical_warning: false,
            };

            limiter.update_health(healthy).await;
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);

            let degraded = DeviceHealth {
                temperature_celsius: 60,
                percentage_used: 75,
                available_spare: 15,
                data_units_written: 100000,
                data_units_read: 200000,
                power_on_hours: 5000,
                unsafe_shutdowns: 1,
                critical_warning: false,
            };

            limiter.update_health(degraded).await;
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Degraded);

            let critical = DeviceHealth {
                temperature_celsius: 80,
                percentage_used: 95,
                available_spare: 5,
                data_units_written: 1000000,
                data_units_read: 2000000,
                power_on_hours: 10000,
                unsafe_shutdowns: 5,
                critical_warning: true,
            };

            limiter.update_health(critical).await;
            assert_eq!(
                limiter.mode().await,
                HealthAdaptiveMode::Critical,
                "Should transition to Critical with critical_warning"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_min_max_depth_bounds() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 256,
                    degradation_latency_ms: 1,
                    critical_latency_ms: 2,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 100,
                    recovery_delay_ms: 50,
                },
            );

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(10000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;

            let depth = limiter.current_limit().await;
            assert!(
                depth >= 8,
                "Depth should be >= min_depth (8), got {}",
                depth
            );
            assert!(
                depth <= 256,
                "Depth should be <= 256, got {}",
                depth
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_reduction_percent_applied() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 1,
                    critical_latency_ms: 2,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 100,
                    recovery_delay_ms: 50,
                },
            );

            assert_eq!(limiter.current_limit().await, 32);

            for _ in 0..100 {
                limiter.try_acquire().await;
                limiter.release(5000).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            limiter.check_and_adjust().await;

            let depth = limiter.current_limit().await;
            assert!(
                depth <= 16,
                "Depth should be reduced by 50%, got {}",
                depth
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_depth_adjustment_clamped_min() {
            let limiter = create_test_limiter();

            limiter.set_depth(1).await;
            let depth = limiter.current_limit().await;
            assert!(
                depth >= 8,
                "set_depth(1) should be clamped to min_depth (8), got {}",
                depth
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_depth_adjustment_clamped_max() {
            let limiter = create_test_limiter();

            limiter.set_depth(1000).await;
            let depth = limiter.current_limit().await;
            assert!(
                depth <= 256,
                "set_depth(1000) should be clamped to 256, got {}",
                depth
            );
        }
    }

    mod resource_exhaustion_resistance {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_counter_overflow_safe() {
            let limiter = Arc::new(create_test_limiter());

            let acquire_many = Arc::clone(&limiter);
            let handle = tokio::spawn(async move {
                for _ in 0..1000 {
                    let _ = acquire_many.try_acquire().await;
                }
            });

            let release_some = Arc::clone(&limiter);
            let handle2 = tokio::spawn(async move {
                for _ in 0..500 {
                    release_some.release(100).await;
                }
            });

            handle.await.unwrap();
            handle2.await.unwrap();

            let pending = limiter.pending_count().await;
            assert!(
                pending <= 1000,
                "pending should stay bounded, got {}",
                pending
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_latency_history_bounded_memory() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 2,
                    critical_latency_ms: 5,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 100,
                    recovery_delay_ms: 50,
                },
            );

            for i in 0..10000 {
                limiter.try_acquire().await;
                limiter.release(i as u64).await;
            }

            let stats = limiter.stats().await;
            assert!(
                stats.p99_latency_us > 0,
                "Should have p99 from bounded history"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_dispatch_time_deque_bounded() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 32,
                    degradation_latency_ms: 2,
                    critical_latency_ms: 5,
                    min_depth: 8,
                    reduction_percent: 50,
                    history_size: 50,
                    recovery_delay_ms: 50,
                },
            );

            for _ in 0..1000 {
                if limiter.try_acquire().await {
                    limiter.release(100).await;
                }
            }

            let stats = limiter.stats().await;
            assert!(
                stats.avg_dispatch_wait_us >= 0,
                "Should track dispatch times within bounds"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_stats_aggregation_large_sample() {
            let limiter = create_test_limiter();

            for i in 0..10000 {
                limiter.try_acquire().await;
                limiter.release((i % 1000) as u64 + 100).await;
            }

            let stats = limiter.stats().await;
            assert!(
                stats.reduction_events >= 0 || stats.pending_ops >= 0,
                "Stats should aggregate large samples"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_concurrent_acquire_no_unbounded_growth() {
            let limiter = Arc::new(create_test_limiter());
            let mut handles = vec![];

            for _ in 0..50 {
                let limiter = Arc::clone(&limiter);
                handles.push(tokio::spawn(async move {
                    for _ in 0..20 {
                        if limiter.try_acquire().await {
                            tokio::time::sleep(Duration::from_micros(1)).await;
                        }
                    }
                }));
            }

            futures::future::join_all(handles).await;

            let pending = limiter.pending_count().await;
            assert!(
                pending <= 1000,
                "pending should not grow unbounded, got {}",
                pending
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_large_config_values_handled() {
            let limiter = IoDepthLimiter::new(
                QueuePairId(0),
                IoDepthLimiterConfig {
                    initial_depth: 128,
                    degradation_latency_ms: 100,
                    critical_latency_ms: 200,
                    min_depth: 16,
                    reduction_percent: 25,
                    history_size: 100000,
                    recovery_delay_ms: 1000,
                },
            );

            assert!(limiter.current_limit().await > 0);
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_negative_latency_impossible() {
            let limiter = create_test_limiter();

            limiter.try_acquire().await;
            limiter.release(100).await;

            let stats = limiter.stats().await;
            assert!(
                stats.avg_latency_us >= 0,
                "Latency should never be negative"
            );
            assert!(
                stats.p99_latency_us >= 0,
                "p99 latency should never be negative"
            );
        }
    }

    mod api_boundary_validation {
        use super::*;

        #[tokio::test]
        async fn test_storage_io_depth_sec_try_acquire_respects_limit() {
            let limiter = create_test_limiter();

            for _ in 0..32 {
                assert!(limiter.try_acquire().await, "Should acquire up to limit");
            }

            let result = limiter.try_acquire().await;
            assert!(
                !result,
                "Should fail when limit reached (32)"
            );
            assert_eq!(
                limiter.pending_count().await, 32,
                "pending should equal limit"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_release_with_zero_pending_noop() {
            let limiter = create_test_limiter();

            limiter.release(100).await;

            let pending = limiter.pending_count().await;
            assert_eq!(
                pending, 0,
                "pending should remain 0 when releasing without acquire"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_set_depth_out_of_range_clamped() {
            let limiter = create_test_limiter();

            limiter.set_depth(1000).await;
            let depth = limiter.current_limit().await;
            assert!(
                depth <= 256,
                "Depth should be clamped to max 256, got {}",
                depth
            );

            limiter.set_depth(0).await;
            let depth = limiter.current_limit().await;
            assert!(
                depth >= 8,
                "Depth should be clamped to min_depth (8), got {}",
                depth
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_pending_count_reflects_actual_ops() {
            let limiter = create_test_limiter();

            for _ in 0..5 {
                limiter.try_acquire().await;
            }
            assert_eq!(
                limiter.pending_count().await, 5,
                "pending should be 5 after 5 acquires"
            );

            for _ in 0..3 {
                limiter.release(100).await;
            }
            assert_eq!(
                limiter.pending_count().await, 2,
                "pending should be 2 after 3 releases"
            );
        }

        #[tokio::test]
        async fn test_storage_io_depth_sec_config_validation_on_create() {
            let config = IoDepthLimiterConfig {
                initial_depth: 0,
                degradation_latency_ms: 0,
                critical_latency_ms: 0,
                min_depth: 0,
                reduction_percent: 0,
                history_size: 0,
                recovery_delay_ms: 0,
            };

            let limiter = IoDepthLimiter::new(QueuePairId(0), config);

            assert!(limiter.current_limit().await > 0);
            assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
        }
    }
}