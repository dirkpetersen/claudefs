//! Prometheus metrics exporter for transport layer.
//!
//! Provides a Prometheus-compatible `/metrics` endpoint that exports transport metrics
//! in the standard Prometheus text format.

use crate::metrics::TransportMetrics;
use crate::reactive_backpressure::{BackpressureLevel, BackpressureStatsSnapshot};
use crate::trace_aggregator::TraceAggregatorStats;
use crate::transport_pooling::PoolStatsSnapshot;
use crate::qos::{QosStats, WorkloadClass};

/// Prometheus-compatible transport metrics exporter.
///
/// This struct wraps references to various transport metrics and provides
/// a `scrape()` method that outputs metrics in Prometheus text format.
pub struct PrometheusTransportMetrics<'a> {
    transport_metrics: &'a TransportMetrics,
    trace_stats: Option<&'a TraceAggregatorStats>,
    pool_stats: Option<&'a PoolStatsSnapshot>,
    backpressure_stats: Option<&'a BackpressureStatsSnapshot>,
    qos_stats: Option<&'a QosStats>,
    current_backpressure: Option<BackpressureLevel>,
}

impl<'a> PrometheusTransportMetrics<'a> {
    /// Creates a new Prometheus transport metrics exporter.
    ///
    /// All parameters are optional except `transport_metrics`.
    /// Optional parameters can be `None` if the corresponding subsystem is not used.
    pub fn new(
        transport_metrics: &'a TransportMetrics,
        trace_stats: Option<&'a TraceAggregatorStats>,
        pool_stats: Option<&'a PoolStatsSnapshot>,
        backpressure_stats: Option<&'a BackpressureStatsSnapshot>,
        qos_stats: Option<&'a QosStats>,
        current_backpressure: Option<BackpressureLevel>,
    ) -> Self {
        Self {
            transport_metrics,
            trace_stats,
            pool_stats,
            backpressure_stats,
            qos_stats,
            current_backpressure,
        }
    }

    /// Scrapes all metrics and returns them in Prometheus text format.
    ///
    /// Output follows the Prometheus exposition format with HELP, TYPE comments
    /// and metric values. Each metric is formatted as:
    /// - `# HELP <name> <description>`
    /// - `# TYPE <name> counter|gauge`
    /// - `<name>{<labels>} <value>`
    pub fn scrape(&self) -> String {
        let mut output = String::new();

        self.write_counters(&mut output);
        self.write_gauges(&mut output);
        self.write_histograms(&mut output);

        output
    }

    fn write_counters(&self, output: &mut String) {
        let snapshot = self.transport_metrics.snapshot();

        Self::write_counter(output, "transport_requests_sent_total", "Total number of requests sent", snapshot.requests_sent);
        Self::write_counter(output, "transport_requests_received_total", "Total number of requests received", snapshot.requests_received);
        Self::write_counter(output, "transport_responses_sent_total", "Total number of responses sent", snapshot.responses_sent);
        Self::write_counter(output, "transport_responses_received_total", "Total number of responses received", snapshot.responses_received);
        Self::write_counter(output, "transport_errors_total", "Total number of errors", snapshot.errors_total);
        Self::write_counter(output, "transport_retries_total", "Total number of retries", snapshot.retries_total);
        Self::write_counter(output, "transport_timeouts_total", "Total number of timeouts", snapshot.timeouts_total);
        Self::write_counter(output, "transport_connections_opened_total", "Total number of connections opened", snapshot.connections_opened);
        Self::write_counter(output, "transport_connections_closed_total", "Total number of connections closed", snapshot.connections_closed);
        Self::write_counter(output, "transport_health_checks_total", "Total number of health checks performed", snapshot.health_checks_total);
        Self::write_counter(output, "transport_health_checks_failed_total", "Total number of health checks that failed", snapshot.health_checks_failed);

        if let Some(stats) = self.backpressure_stats {
            Self::write_counter(output, "transport_backpressure_signals_emitted_total", "Total number of backpressure signals emitted", stats.signals_emitted);
        }

        if let Some(stats) = self.pool_stats {
            Self::write_counter(output, "transport_pool_connections_acquired_total", "Total number of connections acquired from pool", stats.connections_acquired);
        }
    }

    fn write_gauges(&self, output: &mut String) {
        let snapshot = self.transport_metrics.snapshot();

        Self::write_gauge(output, "transport_active_connections", "Number of currently active connections", snapshot.active_connections as u64);
        Self::write_gauge(output, "transport_bytes_sent_total", "Total bytes sent", snapshot.bytes_sent);
        Self::write_gauge(output, "transport_bytes_received_total", "Total bytes received", snapshot.bytes_received);

        if let Some(stats) = self.pool_stats {
            Self::write_gauge(output, "transport_pool_connections_idle", "Number of idle connections in pool", stats.idle_connections as u64);
            Self::write_gauge(output, "transport_pool_connections_active", "Number of active connections in pool", stats.active_connections as u64);
            Self::write_gauge(output, "transport_pool_connections_total", "Total number of connections in pool", stats.total_connections as u64);
        }

        let bp_level = self.current_backpressure.map(|l| l.to_numeric()).unwrap_or(0);
        Self::write_gauge(output, "transport_backpressure_level", "Current backpressure level (0=Ok, 1=Slow, 2=Degraded, 3=Overloaded)", bp_level);

        if let Some(stats) = self.qos_stats {
            self.write_qos_stats(output, stats);
        }
    }

    fn write_histograms(&self, output: &mut String) {
        if let Some(stats) = self.trace_stats {
            Self::write_counter(output, "transport_trace_aggregator_traces_recorded_total", "Total number of traces recorded", stats.traces_recorded);
            Self::write_gauge(output, "transport_trace_aggregator_active_traces", "Number of currently active traces", stats.active_traces as u64);
        }
    }

    fn write_qos_stats(&self, output: &mut String, stats: &QosStats) {
        for class in [
            WorkloadClass::RealtimeMeta,
            WorkloadClass::Interactive,
            WorkloadClass::Batch,
            WorkloadClass::Replication,
            WorkloadClass::Management,
        ] {
            let class_stats = stats.get(&class);
            let class_label = format!("{:?}", class).to_lowercase();
            
            Self::write_gauge_with_labels(
                output,
                "transport_qos_requests_admitted_total",
                "Total number of QoS requests admitted",
                class_stats.admitted,
                &[("class", &class_label)],
            );
            
            Self::write_gauge_with_labels(
                output,
                "transport_qos_requests_rejected_total",
                "Total number of QoS requests rejected",
                class_stats.rejected,
                &[("class", &class_label)],
            );
            
            Self::write_gauge_with_labels(
                output,
                "transport_qos_bytes_total",
                "Total bytes admitted through QoS",
                class_stats.total_bytes,
                &[("class", &class_label)],
            );
        }
    }

    fn write_counter(output: &mut String, name: &str, help: &str, value: u64) {
        output.push_str(&format!("# HELP {} {}\n", name, help));
        output.push_str(&format!("# TYPE {} counter\n", name));
        output.push_str(&format!("{} {}\n\n", name, value));
    }

    fn write_gauge(output: &mut String, name: &str, help: &str, value: u64) {
        output.push_str(&format!("# HELP {} {}\n", name, help));
        output.push_str(&format!("# TYPE {} gauge\n", name));
        output.push_str(&format!("{} {}\n\n", name, value));
    }

    fn write_gauge_with_labels(output: &mut String, name: &str, help: &str, value: u64, labels: &[(&str, &str)]) {
        output.push_str(&format!("# HELP {} {}\n", name, help));
        output.push_str(&format!("# TYPE {} gauge\n", name));
        
        let labels_str = labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, v))
            .collect::<Vec<_>>()
            .join(",");
        
        output.push_str(&format!("{}{{{}}} {}\n\n", name, labels_str, value));
    }
}

impl BackpressureLevel {
    fn to_numeric(&self) -> u64 {
        match self {
            BackpressureLevel::Ok => 0,
            BackpressureLevel::Slow => 1,
            BackpressureLevel::Degraded => 2,
            BackpressureLevel::Overloaded => 3,
        }
    }
}

impl<'a> std::fmt::Display for PrometheusTransportMetrics<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.scrape())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_prometheus_transport_metrics_creation() {
        let metrics = TransportMetrics::new();
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_scrape_contains_all_expected_metrics() {
        let metrics = TransportMetrics::new();
        metrics.inc_requests_sent();
        metrics.inc_requests_received();
        metrics.add_bytes_sent(1024);
        metrics.add_bytes_received(2048);
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_requests_sent_total"));
        assert!(output.contains("transport_requests_received_total"));
        assert!(output.contains("transport_bytes_sent_total"));
        assert!(output.contains("transport_bytes_received_total"));
    }

    #[test]
    fn test_scrape_output_format() {
        let metrics = TransportMetrics::new();
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
        
        let lines: Vec<&str> = output.lines().collect();
        let mut has_help = false;
        let mut has_type = false;
        let mut has_value = false;
        
        for line in lines {
            if line.starts_with("# HELP") {
                has_help = true;
            }
            if line.starts_with("# TYPE") {
                has_type = true;
            }
            if !line.starts_with('#') && !line.is_empty() {
                has_value = true;
            }
        }
        
        assert!(has_help, "Output should contain HELP comments");
        assert!(has_type, "Output should contain TYPE comments");
        assert!(has_value, "Output should contain metric values");
    }

    #[test]
    fn test_metrics_values_correct() {
        let metrics = TransportMetrics::new();
        
        metrics.inc_requests_sent();
        metrics.inc_requests_sent();
        metrics.add_bytes_sent(100);
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_requests_sent_total 2"));
        assert!(output.contains("transport_bytes_sent_total 100"));
    }

    #[test]
    fn test_backpressure_level_export() {
        let metrics = TransportMetrics::new();
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            Some(BackpressureLevel::Degraded),
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_backpressure_level"));
        assert!(output.contains("2"));
    }

    #[test]
    fn test_trace_stats_export() {
        let metrics = TransportMetrics::new();
        
        let trace_stats = TraceAggregatorStats {
            traces_recorded: 100,
            traces_completed: 80,
            spans_recorded: 500,
            traces_timed_out: 20,
            active_traces: 5,
        };
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            Some(&trace_stats),
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_trace_aggregator_traces_recorded_total"));
        assert!(output.contains("100"));
    }

    #[test]
    fn test_pool_stats_export() {
        let metrics = TransportMetrics::new();
        
        let pool_stats = PoolStatsSnapshot {
            connections_acquired: 50,
            connections_released: 45,
            connections_created: 10,
            connections_destroyed: 5,
            failures_recorded: 3,
            idle_cleanups: 2,
            total_connections: 5,
            active_connections: 2,
            idle_connections: 3,
        };
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            Some(&pool_stats),
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_pool_connections_idle"));
        assert!(output.contains("transport_pool_connections_active"));
        assert!(output.contains("3"));
        assert!(output.contains("2"));
    }

    #[test]
    fn test_backpressure_stats_export() {
        let metrics = TransportMetrics::new();
        
        let bp_stats = BackpressureStatsSnapshot {
            signals_emitted: 25,
            signals_received: 30,
            backoff_events: 10,
            overloaded_transitions: 5,
        };
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            Some(&bp_stats),
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_backpressure_signals_emitted_total"));
        assert!(output.contains("25"));
    }

    #[test]
    fn test_qos_stats_export() {
        let metrics = TransportMetrics::new();
        
        let mut classes = std::collections::HashMap::new();
        classes.insert(WorkloadClass::Interactive, crate::qos::ClassStats {
            admitted: 100,
            rejected: 10,
            total_bytes: 1024000,
            total_wait_ms: 500,
        });
        
        let qos_stats = QosStats { classes };
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            Some(&qos_stats),
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(output.contains("transport_qos_requests_admitted_total"));
        assert!(output.contains("interactive"));
    }

    #[test]
    fn test_empty_optional_stats() {
        let metrics = TransportMetrics::new();
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = exporter.scrape();
        
        assert!(!output.contains("trace_aggregator"));
        assert!(!output.contains("pool_connections"));
        assert!(!output.contains("backpressure_signals"));
    }

    #[test]
    fn test_display_trait() {
        let metrics = TransportMetrics::new();
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let output = format!("{}", exporter);
        
        assert!(!output.is_empty());
        assert!(output.contains("# HELP"));
    }

    #[tokio::test]
    async fn test_concurrent_metric_updates() {
        use std::sync::Arc;
        
        let metrics = Arc::new(TransportMetrics::new());
        
        let metrics_clone = Arc::clone(&metrics);
        let handle = std::thread::spawn(move || {
            for _ in 0..1000 {
                metrics_clone.inc_requests_sent();
            }
        });
        
        let exporter = PrometheusTransportMetrics::new(
            &metrics,
            None,
            None,
            None,
            None,
            None,
        );
        
        let _ = exporter.scrape();
        
        handle.join().unwrap();
        
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests_sent, 1000);
    }

    #[test]
    fn test_backpressure_level_numeric() {
        assert_eq!(BackpressureLevel::Ok.to_numeric(), 0);
        assert_eq!(BackpressureLevel::Slow.to_numeric(), 1);
        assert_eq!(BackpressureLevel::Degraded.to_numeric(), 2);
        assert_eq!(BackpressureLevel::Overloaded.to_numeric(), 3);
    }
}