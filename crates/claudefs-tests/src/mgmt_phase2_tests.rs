//! Tests for MetricsCollector and ClusterMetrics in claudefs-mgmt

use claudefs_mgmt::metrics::ClusterMetrics;
use claudefs_mgmt::metrics_collector::MetricsCollector;
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_metrics_new_creates_without_panic() {
        let metrics = ClusterMetrics::new();
        let output = metrics.render_prometheus();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_render_prometheus_contains_metric_names() {
        let metrics = ClusterMetrics::new();
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_iops_read_total"));
        assert!(output.contains("claudefs_iops_write_total"));
        assert!(output.contains("claudefs_capacity_total_bytes"));
    }

    #[test]
    fn test_counter_increment_iops_read() {
        let metrics = ClusterMetrics::new();
        metrics.iops_read.add(100);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_iops_read_total 100"));
    }

    #[test]
    fn test_counter_increment_iops_write() {
        let metrics = ClusterMetrics::new();
        metrics.iops_write.add(50);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_iops_write_total 50"));
    }

    #[test]
    fn test_counter_increment_bytes_read() {
        let metrics = ClusterMetrics::new();
        metrics.bytes_read.add(1024);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_bytes_read_total 1024"));
    }

    #[test]
    fn test_counter_increment_bytes_write() {
        let metrics = ClusterMetrics::new();
        metrics.bytes_write.add(512);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_bytes_write_total 512"));
    }

    #[test]
    fn test_gauge_set_nodes_total() {
        let metrics = ClusterMetrics::new();
        metrics.nodes_total.set(10.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_nodes_total 10"));
    }

    #[test]
    fn test_gauge_set_nodes_healthy() {
        let metrics = ClusterMetrics::new();
        metrics.nodes_healthy.set(8.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_nodes_healthy 8"));
    }

    #[test]
    fn test_gauge_set_capacity_values() {
        let metrics = ClusterMetrics::new();
        metrics.capacity_total_bytes.set(1_000_000_000.0);
        metrics.capacity_used_bytes.set(500_000_000.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_capacity_total_bytes 1000000000"));
        assert!(output.contains("claudefs_capacity_used_bytes 500000000"));
    }

    #[test]
    fn test_histogram_observe_read_latency() {
        let metrics = ClusterMetrics::new();
        metrics.latency_read_us.observe(100.0);
        metrics.latency_read_us.observe(500.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_latency_read_us_bucket"));
        assert!(output.contains("claudefs_latency_read_us_count 2"));
    }

    #[test]
    fn test_histogram_observe_write_latency() {
        let metrics = ClusterMetrics::new();
        metrics.latency_write_us.observe(200.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_latency_write_us_bucket"));
    }

    #[test]
    fn test_metrics_collector_new_creates() {
        let metrics = Arc::new(ClusterMetrics::new());
        let collector = MetricsCollector::new(metrics, 5);
        let _ = collector;
    }

    #[tokio::test]
    async fn test_metrics_collector_start_sets_running() {
        let metrics = Arc::new(ClusterMetrics::new());
        let collector = MetricsCollector::new(metrics.clone(), 1);
        let _handle = collector.start();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        collector.stop();
    }

    #[tokio::test]
    async fn test_metrics_collector_stop_clears_running() {
        let metrics = Arc::new(ClusterMetrics::new());
        let collector = MetricsCollector::new(metrics.clone(), 1);
        let _handle = collector.start();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        collector.stop();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_metrics_collector_start_returns_handle() {
        let metrics = Arc::new(ClusterMetrics::new());
        let collector = MetricsCollector::new(metrics.clone(), 1);
        let handle = collector.start();
        collector.stop();
        handle.await.unwrap();
    }

    #[test]
    fn test_prometheus_output_format_metric_prefix() {
        let metrics = ClusterMetrics::new();
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_"));
    }

    #[test]
    fn test_histogram_multiple_observes() {
        let metrics = ClusterMetrics::new();
        for i in 0..100 {
            metrics.latency_read_us.observe(i as f64);
        }
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_latency_read_us_count 100"));
    }

    #[test]
    fn test_gauge_set_s3_queue_depth() {
        let metrics = ClusterMetrics::new();
        metrics.s3_queue_depth.set(42.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_s3_queue_depth 42"));
    }

    #[test]
    fn test_gauge_set_dedupe_hit_rate() {
        let metrics = ClusterMetrics::new();
        metrics.dedupe_hit_rate.set(0.75);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_dedupe_hit_rate 0.75"));
    }

    #[test]
    fn test_gauge_set_compression_ratio() {
        let metrics = ClusterMetrics::new();
        metrics.compression_ratio.set(2.5);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_compression_ratio 2.5"));
    }

    #[test]
    fn test_gauge_set_replication_lag() {
        let metrics = ClusterMetrics::new();
        metrics.replication_lag_secs.set(1.5);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_replication_lag_secs 1.5"));
    }

    #[test]
    fn test_concurrent_access_metrics_collector() {
        let metrics = Arc::new(ClusterMetrics::new());
        let metrics_clone1 = metrics.clone();
        let metrics_clone2 = metrics.clone();

        let handle1 = std::thread::spawn(move || {
            for _ in 0..1000 {
                metrics_clone1.iops_read.inc();
            }
        });

        let handle2 = std::thread::spawn(move || {
            for _ in 0..1000 {
                metrics_clone2.iops_write.inc();
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        assert_eq!(metrics.iops_read.get(), 1000);
    }

    #[test]
    fn test_nodes_degraded_gauge() {
        let metrics = ClusterMetrics::new();
        metrics.nodes_degraded.set(2.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_nodes_degraded 2"));
    }

    #[test]
    fn test_nodes_offline_gauge() {
        let metrics = ClusterMetrics::new();
        metrics.nodes_offline.set(1.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_nodes_offline 1"));
    }

    #[test]
    fn test_replication_conflicts_counter() {
        let metrics = ClusterMetrics::new();
        metrics.replication_conflicts_total.add(5);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_replication_conflicts_total 5"));
    }

    #[test]
    fn test_capacity_available_bytes() {
        let metrics = ClusterMetrics::new();
        metrics.capacity_available_bytes.set(500_000_000.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_capacity_available_bytes 500000000"));
    }

    #[test]
    fn test_s3_flush_latency_histogram() {
        let metrics = ClusterMetrics::new();
        metrics.s3_flush_latency_ms.observe(100.0);
        let output = metrics.render_prometheus();
        assert!(output.contains("claudefs_s3_flush_latency_ms_bucket"));
    }

    #[tokio::test]
    async fn test_metrics_collector_interval_set() {
        let metrics = Arc::new(ClusterMetrics::new());
        let collector = MetricsCollector::new(metrics, 10);
        let _handle = collector.start();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        collector.stop();
    }
}

mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_histogram_observe_any_positive_f64(val in 0.1f64..1e9) {
            let metrics = ClusterMetrics::new();
            metrics.latency_read_us.observe(val);
            assert!(metrics.latency_read_us.count() == 1);
        }

        #[test]
        fn prop_histogram_observe_small_values(val in 0.0f64..100.0) {
            let metrics = ClusterMetrics::new();
            metrics.latency_read_us.observe(val);
            assert!(metrics.latency_read_us.count() == 1);
        }

        #[test]
        fn prop_histogram_observe_large_values(val in 1e4f64..1e7) {
            let metrics = ClusterMetrics::new();
            metrics.latency_write_us.observe(val);
            assert!(metrics.latency_write_us.count() == 1);
        }

        #[test]
        fn prop_gauge_set_any_f64(val in 0.0f64..1e12) {
            let metrics = ClusterMetrics::new();
            metrics.capacity_total_bytes.set(val);
            assert!((metrics.capacity_total_bytes.get() - val).abs() < 0.001);
        }
    }
}