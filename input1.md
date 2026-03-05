Implement device_timeout_handler.rs in /home/cfs/claudefs/crates/claudefs-storage/src/

Follow io_depth_limiter.rs code style exactly - use tokio, dashmap, thiserror, serde::Serialize.

API:
- QueuePairId(u64)
- CommandType { Read, Write, Flush, Deallocate }
- OpMetadata { cmd_id, submitted_at: Instant, retry_count, op_type }
- TimedOutOp { op_id, metadata, elapsed_ms }
- TimeoutStats { pending_ops, timeout_count, retry_count, degraded_count, p99_latency_ms, avg_latency_ms }
- TimeoutConfig { timeout_ms:5000, max_retries:3, retry_backoff_ms: vec![50,100,200,500], degradation_threshold:3, degradation_window_s:60 }
- TimeoutHandler with new(), track(), complete(), check_timeouts(), get_backoff_delay(), is_degraded(), stats(), reset()
- TimeoutError enum

Tests needed (30 tests):
- test_track_operation, test_complete_operation, test_timeout_detection
- test_retry_logic, test_exponential_backoff, test_max_retries_exceeded
- test_degradation_threshold
- test_concurrent_ops_tracking (5 variants)
- test_histogram_accuracy, test_latency_p99_calculation
- test_recovery_after_timeout, test_multiple_devices_independent
- test_backpressure_on_high_timeout_rate
- test_default_config, test_pending_ops_tracking, test_stats_reset
- test_pending_count, test_clear_old_timeouts

All tests use #[tokio::test]. Include module docs (!/).