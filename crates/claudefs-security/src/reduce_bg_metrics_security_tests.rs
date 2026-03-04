//! Reduce background processor and metrics security tests.
//!
//! Part of A10 Phase 29: Reduce background + metrics security audit

#[cfg(test)]
mod tests {
    use claudefs_reduce::background::{BackgroundConfig, BackgroundHandle, BackgroundProcessor, BackgroundStats, BackgroundTask};
    use claudefs_reduce::metrics::{MetricKind, MetricValue, MetricsHandle, MetricsSnapshot, ReduceMetric, ReductionMetrics};
    use claudefs_reduce::dedupe::CasIndex;
    use claudefs_reduce::fingerprint::{ChunkHash, super_features};
    use claudefs_reduce::gc::GcConfig;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn make_test_hash(data: &[u8]) -> ChunkHash {
        ChunkHash(*blake3::hash(data).as_bytes())
    }

    fn get_metric_value(metrics: &ReductionMetrics, name: &str) -> Option<u64> {
        for m in metrics.collect() {
            if m.name == name {
                if let MetricValue::Counter(v) = m.value {
                    return Some(v);
                }
            }
        }
        None
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_send_process_chunk_after_shutdown_errors() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        handle.send(BackgroundTask::Shutdown).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let data = b"post-shutdown data";
        let hash = make_test_hash(data);
        let features = super_features(data);

        let result = handle
            .send(BackgroundTask::ProcessChunk {
                hash,
                features,
                data: data.to_vec(),
            })
            .await;

        assert!(result.is_err(), "Sending ProcessChunk after Shutdown should error");
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_stats_initially_all_zero() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 0);
        assert_eq!(stats.similarity_hits, 0);
        assert_eq!(stats.delta_compressed, 0);
        assert_eq!(stats.gc_cycles, 0);
        assert_eq!(stats.chunks_reclaimed, 0);
        assert_eq!(stats.bytes_saved_delta, 0);
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_multiple_chunks_processed_counter_increments() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        for i in 0..5 {
            let data = format!("chunk data {}", i);
            let hash = make_test_hash(data.as_bytes());
            let features = super_features(data.as_bytes());
            handle
                .send(BackgroundTask::ProcessChunk {
                    hash,
                    features,
                    data: data.into_bytes(),
                })
                .await
                .unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 5, "Counter should increment correctly for 5 chunks");
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_gc_cycle_with_empty_reachable_set() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        handle
            .send(BackgroundTask::RunGc { reachable: vec![] })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = handle.stats();
        assert_eq!(stats.gc_cycles, 1, "GC cycle counter should increment even with empty reachable set");
    }

    #[test]
    fn test_reduce_bm_sec_background_config_defaults_correct() {
        let config = BackgroundConfig::default();
        assert_eq!(config.channel_capacity, 1000);
        assert_eq!(config.delta_compression_level, 3);
        assert_eq!(config.similarity_threshold, 3);
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_channel_capacity_one_still_processes() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig {
            channel_capacity: 1,
            delta_compression_level: 3,
            similarity_threshold: 3,
            gc_config: GcConfig::default(),
        };
        let handle = BackgroundProcessor::start(config, cas);

        let data = b"single capacity test";
        let hash = make_test_hash(data);
        let features = super_features(data);

        handle
            .send(BackgroundTask::ProcessChunk {
                hash,
                features,
                data: data.to_vec(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 1, "Should process chunk even with capacity=1");
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_large_data_chunk_100kb() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        let large_data = vec![0xABu8; 100 * 1024];
        let hash = make_test_hash(&large_data);
        let features = super_features(&large_data);

        handle
            .send(BackgroundTask::ProcessChunk {
                hash,
                features,
                data: large_data.clone(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 1, "Should handle 100KB chunk without panic");
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_rapid_send_50_chunks() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        for i in 0..50 {
            let data = format!("rapid chunk {}", i);
            let hash = make_test_hash(data.as_bytes());
            let features = super_features(data.as_bytes());
            handle
                .send(BackgroundTask::ProcessChunk {
                    hash,
                    features,
                    data: data.into_bytes(),
                })
                .await
                .unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 50, "All 50 rapid chunks should be processed");
    }

    #[tokio::test]
    async fn test_reduce_bm_sec_is_running_true_before_shutdown() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert!(handle.is_running(), "is_running() should be true before shutdown");

        handle.send(BackgroundTask::Shutdown).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(!handle.is_running(), "is_running() should be false after shutdown");
    }

    #[test]
    fn test_reduce_bm_sec_concurrent_record_chunk_four_threads_100_each() {
        let metrics = Arc::new(ReductionMetrics::new());
        let mut handles = Vec::new();

        for _ in 0..4 {
            let m = Arc::clone(&metrics);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    m.record_chunk(1, 1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let chunks = get_metric_value(&metrics, "claudefs_reduce_chunks_processed_total");
        assert_eq!(chunks, Some(400), "chunks_processed should be 400 after 4 threads × 100 each");
    }

    #[test]
    fn test_reduce_bm_sec_record_chunk_zero_bytes_in_zero_bytes_out() {
        let metrics = ReductionMetrics::new();

        metrics.record_chunk(0, 0);

        let chunks = get_metric_value(&metrics, "claudefs_reduce_chunks_processed_total");
        let bytes_in = get_metric_value(&metrics, "claudefs_reduce_bytes_in_total");
        let bytes_out = get_metric_value(&metrics, "claudefs_reduce_bytes_out_total");

        assert_eq!(chunks, Some(1));
        assert_eq!(bytes_in, Some(0));
        assert_eq!(bytes_out, Some(0));
    }

    #[test]
    fn test_reduce_bm_sec_record_chunk_very_large_values() {
        let metrics = ReductionMetrics::new();
        let large_val = u64::MAX / 2;

        metrics.record_chunk(large_val, large_val);

        let bytes_in = get_metric_value(&metrics, "claudefs_reduce_bytes_in_total");
        let bytes_out = get_metric_value(&metrics, "claudefs_reduce_bytes_out_total");

        assert_eq!(bytes_in, Some(large_val));
        assert_eq!(bytes_out, Some(large_val));
    }

    #[test]
    fn test_reduce_bm_sec_record_compress_zero_bytes_in() {
        let metrics = ReductionMetrics::new();

        metrics.record_compress(0, 100);

        let bytes_in = get_metric_value(&metrics, "claudefs_reduce_compress_bytes_in_total");
        let bytes_out = get_metric_value(&metrics, "claudefs_reduce_compress_bytes_out_total");

        assert_eq!(bytes_in, Some(0));
        assert_eq!(bytes_out, Some(100));
    }

    #[test]
    fn test_reduce_bm_sec_record_gc_cycle_bytes_freed_zero() {
        let metrics = ReductionMetrics::new();

        metrics.record_gc_cycle(0);

        let cycles = get_metric_value(&metrics, "claudefs_reduce_gc_cycles_total");
        let freed = get_metric_value(&metrics, "claudefs_reduce_gc_bytes_freed_total");

        assert_eq!(cycles, Some(1));
        assert_eq!(freed, Some(0));
    }

    #[test]
    fn test_reduce_bm_sec_dedup_ratio_only_hits() {
        let metrics = ReductionMetrics::new();

        metrics.record_dedup_hit();
        metrics.record_dedup_hit();
        metrics.record_dedup_hit();

        assert!((metrics.dedup_ratio() - 1.0).abs() < f64::EPSILON, "dedup_ratio with only hits should be 1.0");
    }

    #[test]
    fn test_reduce_bm_sec_dedup_ratio_only_misses() {
        let metrics = ReductionMetrics::new();

        metrics.record_dedup_miss();
        metrics.record_dedup_miss();

        assert!((metrics.dedup_ratio() - 0.0).abs() < f64::EPSILON, "dedup_ratio with only misses should be 0.0");
    }

    #[test]
    fn test_reduce_bm_sec_dedup_ratio_no_operations() {
        let metrics = ReductionMetrics::new();

        assert!((metrics.dedup_ratio() - 0.0).abs() < f64::EPSILON, "dedup_ratio with no ops should be 0.0");
    }

    #[test]
    fn test_reduce_bm_sec_compression_ratio_no_operations() {
        let metrics = ReductionMetrics::new();

        assert!((metrics.compression_ratio() - 1.0).abs() < f64::EPSILON, "compression_ratio with no ops should be 1.0");
    }

    #[test]
    fn test_reduce_bm_sec_overall_reduction_ratio_no_operations() {
        let metrics = ReductionMetrics::new();

        assert!((metrics.overall_reduction_ratio() - 1.0).abs() < f64::EPSILON, "overall_reduction_ratio with no ops should be 1.0");
    }

    #[test]
    fn test_reduce_bm_sec_metrics_handle_clone_shares_underlying_metrics() {
        let handle1 = MetricsHandle::new();
        let handle2 = handle1.clone();

        handle1.metrics().record_chunk(100, 50);
        handle1.metrics().record_dedup_hit();

        let snap1 = handle1.snapshot();
        let snap2 = handle2.snapshot();

        assert_eq!(snap1.chunks_processed, snap2.chunks_processed, "Cloned handles should share metrics");
        assert_eq!(snap1.dedup_hits, snap2.dedup_hits);
    }

    #[test]
    fn test_reduce_bm_sec_snapshot_captures_point_in_time_values() {
        let handle = MetricsHandle::new();

        handle.metrics().record_chunk(1000, 500);
        handle.metrics().record_dedup_hit();
        handle.metrics().record_dedup_miss();

        let snapshot = handle.snapshot();

        handle.metrics().record_chunk(500, 250);

        assert_eq!(snapshot.chunks_processed, 1, "Snapshot should capture point-in-time value");
        assert_eq!(snapshot.bytes_in, 1000);
        assert_eq!(snapshot.bytes_out, 500);
    }

    #[test]
    fn test_reduce_bm_sec_snapshot_ratios_calculated_correctly() {
        let handle = MetricsHandle::new();

        handle.metrics().record_dedup_hit();
        handle.metrics().record_dedup_miss();
        handle.metrics().record_compress(200, 100);
        handle.metrics().record_chunk(1000, 250);

        let snapshot = handle.snapshot();

        assert!((snapshot.dedup_ratio - 0.5).abs() < 0.001, "dedup_ratio should be 0.5");
        assert!((snapshot.compression_ratio - 2.0).abs() < 0.001, "compression_ratio should be 2.0");
        assert!((snapshot.overall_reduction_ratio - 4.0).abs() < 0.001, "overall_reduction_ratio should be 4.0");
    }

    #[test]
    fn test_reduce_bm_sec_collect_returns_14_metrics_with_correct_names() {
        let metrics = ReductionMetrics::new();
        let collected = metrics.collect();

        assert_eq!(collected.len(), 14, "collect() should return 14 metrics");

        let names: std::collections::HashSet<_> = collected.iter().map(|m| m.name.as_str()).collect();

        assert!(names.contains("claudefs_reduce_chunks_processed_total"));
        assert!(names.contains("claudefs_reduce_bytes_in_total"));
        assert!(names.contains("claudefs_reduce_bytes_out_total"));
        assert!(names.contains("claudefs_reduce_dedup_hits_total"));
        assert!(names.contains("claudefs_reduce_dedup_misses_total"));
        assert!(names.contains("claudefs_reduce_dedup_ratio"));
        assert!(names.contains("claudefs_reduce_compress_bytes_in_total"));
        assert!(names.contains("claudefs_reduce_compress_bytes_out_total"));
        assert!(names.contains("claudefs_reduce_compression_ratio"));
        assert!(names.contains("claudefs_reduce_encrypt_ops_total"));
        assert!(names.contains("claudefs_reduce_gc_cycles_total"));
        assert!(names.contains("claudefs_reduce_gc_bytes_freed_total"));
        assert!(names.contains("claudefs_reduce_key_rotations_total"));
        assert!(names.contains("claudefs_reduce_overall_reduction_ratio"));
    }

    #[test]
    fn test_reduce_bm_sec_metric_value_counter_equality() {
        let v1 = MetricValue::Counter(42);
        let v2 = MetricValue::Counter(42);
        let v3 = MetricValue::Counter(100);

        assert_eq!(v1, v2, "Counter values with same number should be equal");
        assert_ne!(v1, v3, "Counter values with different numbers should not be equal");
    }

    #[test]
    fn test_reduce_bm_sec_metric_value_gauge_equality_with_floats() {
        let v1 = MetricValue::Gauge(3.14159);
        let v2 = MetricValue::Gauge(3.14159);
        let v3 = MetricValue::Gauge(2.71828);

        assert_eq!(v1, v2, "Gauge values with same float should be equal");
        assert_ne!(v1, v3, "Gauge values with different floats should not be equal");
    }

    #[test]
    fn test_reduce_bm_sec_metric_value_histogram_fields_accessible() {
        let v = MetricValue::Histogram {
            sum: 150.0,
            count: 15,
            buckets: vec![(1.0, 3), (10.0, 8), (100.0, 15)],
        };

        if let MetricValue::Histogram { sum, count, buckets } = v {
            assert!((sum - 150.0).abs() < f64::EPSILON);
            assert_eq!(count, 15);
            assert_eq!(buckets.len(), 3);
            assert_eq!(buckets[0], (1.0, 3));
        } else {
            panic!("Expected Histogram variant");
        }
    }

    #[test]
    fn test_reduce_bm_sec_metric_kind_variants_distinct() {
        assert_eq!(MetricKind::Counter, MetricKind::Counter);
        assert_eq!(MetricKind::Gauge, MetricKind::Gauge);
        assert_eq!(MetricKind::Histogram, MetricKind::Histogram);
        assert_ne!(MetricKind::Counter, MetricKind::Gauge);
        assert_ne!(MetricKind::Gauge, MetricKind::Histogram);
        assert_ne!(MetricKind::Histogram, MetricKind::Counter);
    }

    #[test]
    fn test_reduce_bm_sec_reduce_metric_fields_accessible() {
        let metric = ReduceMetric {
            name: "test_metric_name".to_string(),
            help: "Test help text".to_string(),
            kind: MetricKind::Counter,
            value: MetricValue::Counter(12345),
        };

        assert_eq!(metric.name, "test_metric_name");
        assert_eq!(metric.help, "Test help text");
        assert_eq!(metric.kind, MetricKind::Counter);
        assert_eq!(metric.value, MetricValue::Counter(12345));
    }
}