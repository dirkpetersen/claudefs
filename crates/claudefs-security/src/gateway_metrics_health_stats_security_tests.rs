//! Gateway metrics, health checking, and statistics security tests.
//!
//! Part of A10 Phase 21: Gateway metrics/health/stats security audit

use claudefs_gateway::gateway_metrics::{
    GatewayMetrics, LatencyHistogram, MetricOperation, MetricProtocol, OperationMetrics,
};
use claudefs_gateway::health::{CheckResult, HealthChecker, HealthReport, HealthStatus};
use claudefs_gateway::stats::{GatewayStats, ProtocolStats};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_histogram() -> LatencyHistogram {
        LatencyHistogram::new()
    }

    fn make_operation_metrics() -> OperationMetrics {
        OperationMetrics::new()
    }

    fn make_gateway_metrics() -> GatewayMetrics {
        GatewayMetrics::new()
    }

    fn make_health_checker() -> HealthChecker {
        HealthChecker::new()
    }

    fn make_protocol_stats() -> ProtocolStats {
        ProtocolStats::new()
    }

    fn make_gateway_stats() -> GatewayStats {
        GatewayStats::new()
    }

    // Category 1: Latency Histogram (5 tests)

    #[test]
    fn test_histogram_observe_and_count() {
        let mut hist = make_histogram();
        hist.observe(50);
        hist.observe(200);
        hist.observe(800);
        hist.observe(5000);

        assert_eq!(hist.count, 4);
        assert_eq!(hist.sum_us, 6050);
    }

    #[test]
    fn test_histogram_percentiles() {
        let mut hist = make_histogram();

        for _ in 0..100 {
            hist.observe(100);
        }

        assert_eq!(hist.p50_us(), 100);
        assert_eq!(hist.p99_us(), 100);

        for _ in 0..100 {
            hist.observe(100000);
        }
        assert!(hist.p99_us() > 100);
    }

    #[test]
    fn test_histogram_empty_percentiles() {
        let hist = make_histogram();

        assert_eq!(hist.p50_us(), 0);
        assert_eq!(hist.p99_us(), 0);
        assert_eq!(hist.p999_us(), 0);
        assert_eq!(hist.mean_us(), 0);
    }

    #[test]
    fn test_histogram_mean() {
        let mut hist = make_histogram();
        hist.observe(100);
        hist.observe(200);
        hist.observe(300);

        assert_eq!(hist.mean_us(), 200);
    }

    #[test]
    fn test_histogram_reset() {
        let mut hist = make_histogram();
        hist.observe(100);
        hist.observe(200);

        hist.reset();

        assert_eq!(hist.count, 0);
        assert_eq!(hist.sum_us, 0);
    }

    // Category 2: Operation Metrics (5 tests)

    #[test]
    fn test_operation_metrics_record_success() {
        let mut metrics = make_operation_metrics();
        metrics.record_success(100, 4096, 0);
        metrics.record_success(200, 8192, 0);

        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_count, 2);
        assert_eq!(metrics.error_count, 0);
        assert_eq!(metrics.bytes_read, 12288);
    }

    #[test]
    fn test_operation_metrics_record_error() {
        let mut metrics = make_operation_metrics();
        metrics.record_error(100);
        metrics.record_error(200);

        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.error_count, 2);
        assert_eq!(metrics.success_count, 0);
    }

    #[test]
    fn test_operation_metrics_error_rate() {
        let mut metrics = make_operation_metrics();
        metrics.record_success(100, 0, 0);
        metrics.record_success(100, 0, 0);
        metrics.record_error(100);

        let error_rate = metrics.error_rate();
        assert!((error_rate - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_operation_metrics_empty_error_rate() {
        let metrics = make_operation_metrics();

        assert_eq!(metrics.error_rate(), 0.0);
    }

    #[test]
    fn test_gateway_metrics_record_and_query() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            200,
            0,
            0,
            false,
        );

        let op_metrics = metrics.get_op_metrics(&MetricProtocol::Nfs3, &MetricOperation::Read);
        assert!(op_metrics.is_some());
        let m = op_metrics.unwrap();
        assert_eq!(m.total_requests, 2);
        assert_eq!(m.error_count, 1);

        let no_metrics = metrics.get_op_metrics(&MetricProtocol::Nfs4, &MetricOperation::Read);
        assert!(no_metrics.is_none());
    }

    // Category 3: Gateway Metrics Aggregation (5 tests)

    #[test]
    fn test_gateway_metrics_total_requests() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.record_op(
            MetricProtocol::Nfs4,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        assert_eq!(metrics.total_requests(), 2);
    }

    #[test]
    fn test_gateway_metrics_total_errors() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            false,
        );
        metrics.record_op(
            MetricProtocol::S3,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        assert_eq!(metrics.total_errors(), 1);
    }

    #[test]
    fn test_gateway_metrics_overall_error_rate() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            false,
        );
        metrics.record_op(
            MetricProtocol::Nfs4,
            MetricOperation::Write,
            200,
            0,
            4096,
            true,
        );

        let rate = metrics.overall_error_rate();
        assert!((rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gateway_metrics_export_text() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );

        let text = metrics.export_text();
        assert!(text.contains("gateway_requests_total"));
        assert!(text.contains("nfs3"));
        assert!(text.contains("read"));
    }

    #[test]
    fn test_gateway_metrics_reset() {
        let mut metrics = make_gateway_metrics();

        metrics.record_op(
            MetricProtocol::Nfs3,
            MetricOperation::Read,
            100,
            4096,
            0,
            true,
        );
        metrics.active_connections.insert("nfs3".to_string(), 5);

        metrics.reset();

        assert_eq!(metrics.total_requests(), 0);
        assert!(metrics.active_connections.is_empty());
    }

    // Category 4: Health Checker & Report (5 tests)

    #[test]
    fn test_health_status_is_ok() {
        assert!(HealthStatus::Healthy.is_ok());
        assert!(HealthStatus::Degraded.is_ok());
        assert!(!HealthStatus::Unhealthy.is_ok());
        assert!(!HealthStatus::Starting.is_ok());
    }

    #[test]
    fn test_health_report_worst_wins() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::unhealthy("check2", "failed", 20),
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.overall, HealthStatus::Unhealthy);

        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
        ];
        let report = HealthReport::new(checks, 1000);
        assert_eq!(report.overall, HealthStatus::Degraded);

        let report = HealthReport::new(vec![], 1000);
        assert_eq!(report.overall, HealthStatus::Starting);
    }

    #[test]
    fn test_health_checker_register_and_report() {
        let checker = make_health_checker();
        checker.register_result(CheckResult::ok("transport", 10));
        checker.register_result(CheckResult::ok("storage", 20));

        let report = checker.report(1000);

        assert_eq!(report.checks.len(), 2);
        assert_eq!(report.overall, HealthStatus::Healthy);
        assert!(report.is_ready());
    }

    #[test]
    fn test_health_checker_update_result() {
        let checker = make_health_checker();
        checker.register_result(CheckResult::ok("transport", 10));

        let updated =
            checker.update_result("transport", HealthStatus::Unhealthy, "connection lost");
        assert!(updated);
        assert!(!checker.is_healthy());

        let not_found = checker.update_result("nonexistent", HealthStatus::Healthy, "test");
        assert!(!not_found);
    }

    #[test]
    fn test_health_report_passed_failed_counts() {
        let checks = vec![
            CheckResult::ok("check1", 10),
            CheckResult::degraded("check2", "slow", 20),
            CheckResult::unhealthy("check3", "failed", 30),
        ];
        let report = HealthReport::new(checks, 1000);

        assert_eq!(report.passed_count(), 1);
        assert_eq!(report.failed_count(), 1);
    }

    // Category 5: Protocol Stats & Edge Cases (5 tests)

    #[test]
    fn test_protocol_stats_record_request() {
        let stats = make_protocol_stats();
        stats.record_request(100, 200, 50);

        assert_eq!(stats.requests(), 1);
        assert_eq!(stats.bytes_in(), 100);
        assert_eq!(stats.bytes_out(), 200);
        assert_eq!(stats.avg_latency_us(), 50);
    }

    #[test]
    fn test_protocol_stats_error_rate() {
        let stats = make_protocol_stats();
        stats.record_request(100, 200, 50);
        stats.record_request(100, 200, 50);
        stats.record_error(30);

        assert_eq!(stats.error_rate(), 0.5);
        assert_eq!(stats.errors(), 1);

        let empty_stats = make_protocol_stats();
        assert_eq!(empty_stats.error_rate(), 0.0);
    }

    #[test]
    fn test_gateway_stats_aggregation() {
        let stats = make_gateway_stats();
        stats.nfs3.record_request(100, 100, 50);
        stats.s3.record_request(200, 200, 50);
        stats.smb3.record_request(300, 300, 50);

        assert_eq!(stats.total_requests(), 3);
        assert_eq!(stats.total_bytes_in(), 600);
        assert_eq!(stats.total_bytes_out(), 600);
    }

    #[test]
    fn test_gateway_stats_prometheus() {
        let stats = make_gateway_stats();
        stats.nfs3.record_request(100, 200, 50);

        let output = stats.to_prometheus();

        assert!(output.contains("cfs_gateway_requests_total"));
        assert!(output.contains("protocol=\"nfs3\""));
    }

    #[test]
    fn test_metric_operation_from_str() {
        assert!(matches!(
            MetricOperation::from("read"),
            MetricOperation::Read
        ));
        assert!(matches!(
            MetricOperation::from("write"),
            MetricOperation::Write
        ));
        assert!(matches!(
            MetricOperation::from("custom_op"),
            MetricOperation::Custom(_)
        ));

        assert_eq!(MetricProtocol::Nfs3.as_str(), "nfs3");
    }
}
