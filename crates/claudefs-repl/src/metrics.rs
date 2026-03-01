//! Prometheus-compatible replication metrics.
//!
//! Exposes replication pipeline metrics in Prometheus text exposition format.

use crate::pipeline::PipelineStats;
use std::collections::HashMap;

/// A single Prometheus metric (counter or gauge).
#[derive(Debug, Clone)]
pub struct Metric {
    /// Metric name (e.g., "claudefs_repl_entries_sent_total").
    pub name: String,
    /// Help text for the metric.
    pub help: String,
    /// Metric type ("counter" or "gauge").
    pub metric_type: String,
    /// Labels as key=value pairs (e.g., vec![("site_id".to_string(), "42".to_string())]).
    pub labels: Vec<(String, String)>,
    /// Current value.
    pub value: f64,
}

impl Metric {
    /// Create a new counter metric.
    pub fn counter(name: &str, help: &str, labels: Vec<(String, String)>, value: f64) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
            metric_type: "counter".to_string(),
            labels,
            value,
        }
    }

    /// Create a new gauge metric.
    pub fn gauge(name: &str, help: &str, labels: Vec<(String, String)>, value: f64) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
            metric_type: "gauge".to_string(),
            labels,
            value,
        }
    }

    /// Format this metric as Prometheus text exposition format.
    /// Example output: `HELP claudefs_repl_entries_sent_total Total entries sent` etc.
    pub fn format(&self) -> String {
        let mut output = format!("# HELP {} {}\n", self.name, self.help);
        output.push_str(&format!("# TYPE {} {}\n", self.name, self.metric_type));

        if self.labels.is_empty() {
            output.push_str(&format!("{} {}\n", self.name, self.format_value()));
        } else {
            let label_str = self
                .labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(",");
            output.push_str(&format!(
                "{}{{{}}} {}\n",
                self.name,
                label_str,
                self.format_value()
            ));
        }

        output
    }

    fn format_value(&self) -> String {
        if self.value.fract() == 0.0 && self.value.abs() < 1e15 {
            format!("{}", self.value as i64)
        } else {
            format!("{}", self.value)
        }
    }
}

/// Snapshot of all replication metrics for one pipeline instance.
#[derive(Debug, Clone, Default)]
pub struct ReplMetrics {
    /// Site ID this pipeline belongs to.
    pub site_id: u64,
    /// Total entries tailed from local journal.
    pub entries_tailed: u64,
    /// Entries removed by compaction.
    pub entries_compacted_away: u64,
    /// Batches dispatched to fanout.
    pub batches_dispatched: u64,
    /// Total entries successfully sent to remote sites.
    pub entries_sent: u64,
    /// Total bytes sent to remote sites.
    pub bytes_sent: u64,
    /// Number of times throttling blocked a send.
    pub throttle_stalls: u64,
    /// Number of fanout failures.
    pub fanout_failures: u64,
    /// Current replication lag in entries (per site, summed).
    pub lag_entries: u64,
    /// Whether the pipeline is currently running (1.0) or not (0.0).
    pub pipeline_running: f64,
}

impl ReplMetrics {
    /// Update metrics from a PipelineStats snapshot.
    pub fn update_from_stats(&mut self, stats: &PipelineStats) {
        self.entries_tailed = stats.entries_tailed;
        self.entries_compacted_away = stats.entries_compacted_away;
        self.batches_dispatched = stats.batches_dispatched;
        self.entries_sent = stats.total_entries_sent;
        self.bytes_sent = stats.total_bytes_sent;
        self.throttle_stalls = stats.throttle_stalls;
        self.fanout_failures = stats.fanout_failures;
    }

    /// Produce the full list of Prometheus metrics.
    pub fn to_metrics(&self) -> Vec<Metric> {
        let site_labels = vec![("site_id".to_string(), self.site_id.to_string())];

        vec![
            Metric::counter(
                "claudefs_repl_entries_tailed_total",
                "Total entries tailed from local journal",
                site_labels.clone(),
                self.entries_tailed as f64,
            ),
            Metric::counter(
                "claudefs_repl_entries_compacted_total",
                "Entries removed by compaction",
                site_labels.clone(),
                self.entries_compacted_away as f64,
            ),
            Metric::counter(
                "claudefs_repl_batches_dispatched_total",
                "Total batches dispatched to fanout",
                site_labels.clone(),
                self.batches_dispatched as f64,
            ),
            Metric::counter(
                "claudefs_repl_entries_sent_total",
                "Total entries successfully sent to remote sites",
                site_labels.clone(),
                self.entries_sent as f64,
            ),
            Metric::counter(
                "claudefs_repl_bytes_sent_total",
                "Total bytes sent to remote sites",
                site_labels.clone(),
                self.bytes_sent as f64,
            ),
            Metric::counter(
                "claudefs_repl_throttle_stalls_total",
                "Number of times throttling blocked a send",
                site_labels.clone(),
                self.throttle_stalls as f64,
            ),
            Metric::counter(
                "claudefs_repl_fanout_failures_total",
                "Number of fanout failures",
                site_labels.clone(),
                self.fanout_failures as f64,
            ),
            Metric::gauge(
                "claudefs_repl_lag_entries",
                "Current replication lag in entries",
                site_labels.clone(),
                self.lag_entries as f64,
            ),
            Metric::gauge(
                "claudefs_repl_pipeline_running",
                "Whether the pipeline is currently running (1.0) or not (0.0)",
                site_labels,
                self.pipeline_running,
            ),
        ]
    }

    /// Format all metrics as Prometheus text exposition format.
    /// This is the format Prometheus scrapes via HTTP.
    pub fn format_prometheus(&self) -> String {
        self.to_metrics()
            .iter()
            .map(|m| m.format())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Returns the compaction rate (entries compacted / entries tailed), or 0.0 if no entries.
    pub fn compaction_rate(&self) -> f64 {
        if self.entries_tailed == 0 {
            return 0.0;
        }
        self.entries_compacted_away as f64 / self.entries_tailed as f64
    }

    /// Returns the fanout failure rate (failures / batches_dispatched), or 0.0 if no batches.
    pub fn fanout_failure_rate(&self) -> f64 {
        if self.batches_dispatched == 0 {
            return 0.0;
        }
        self.fanout_failures as f64 / self.batches_dispatched as f64
    }
}

/// Aggregates metrics across multiple pipeline instances (multi-site).
#[derive(Debug, Default)]
pub struct MetricsAggregator {
    per_site: HashMap<u64, ReplMetrics>,
}

impl MetricsAggregator {
    /// Create a new aggregator.
    pub fn new() -> Self {
        Self {
            per_site: HashMap::new(),
        }
    }

    /// Update or insert metrics for a site.
    pub fn update(&mut self, metrics: ReplMetrics) {
        self.per_site.insert(metrics.site_id, metrics);
    }

    /// Remove a site.
    pub fn remove(&mut self, site_id: u64) {
        self.per_site.remove(&site_id);
    }

    /// Get metrics for a site.
    pub fn get(&self, site_id: u64) -> Option<&ReplMetrics> {
        self.per_site.get(&site_id)
    }

    /// Aggregate all sites into a combined format_prometheus string.
    pub fn format_all(&self) -> String {
        let mut output = String::new();
        for metrics in self.per_site.values() {
            output.push_str(&metrics.format_prometheus());
        }
        output
    }

    /// Total entries sent across all sites.
    pub fn total_entries_sent(&self) -> u64 {
        self.per_site.values().map(|m| m.entries_sent).sum()
    }

    /// Total bytes sent across all sites.
    pub fn total_bytes_sent(&self) -> u64 {
        self.per_site.values().map(|m| m.bytes_sent).sum()
    }

    /// Number of registered sites.
    pub fn site_count(&self) -> usize {
        self.per_site.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_stats() -> PipelineStats {
        PipelineStats {
            entries_tailed: 1000,
            entries_compacted_away: 200,
            batches_dispatched: 10,
            total_entries_sent: 800,
            total_bytes_sent: 50000,
            throttle_stalls: 5,
            fanout_failures: 1,
        }
    }

    #[test]
    fn metric_counter_type() {
        let metric = Metric::counter("test_counter", "A test counter", vec![], 42.0);
        assert_eq!(metric.metric_type, "counter");
    }

    #[test]
    fn metric_gauge_type() {
        let metric = Metric::gauge("test_gauge", "A test gauge", vec![], 42.0);
        assert_eq!(metric.metric_type, "gauge");
    }

    #[test]
    fn metric_format_no_labels() {
        let metric = Metric::counter("test_counter", "A test counter", vec![], 42.0);
        let output = metric.format();
        assert!(output.contains("test_counter 42"));
    }

    #[test]
    fn metric_format_with_labels() {
        let metric = Metric::counter(
            "test_counter",
            "A test counter",
            vec![("site_id".to_string(), "1".to_string())],
            42.0,
        );
        let output = metric.format();
        assert!(output.contains("test_counter{site_id=\"1\"} 42"));
    }

    #[test]
    fn metric_format_contains_help_and_type() {
        let metric = Metric::counter("test_counter", "Help text", vec![], 42.0);
        let output = metric.format();
        assert!(output.contains("# HELP test_counter Help text"));
        assert!(output.contains("# TYPE test_counter counter"));
    }

    #[test]
    fn repl_metrics_default() {
        let metrics = ReplMetrics::default();
        assert_eq!(metrics.site_id, 0);
        assert_eq!(metrics.entries_tailed, 0);
    }

    #[test]
    fn repl_metrics_update_from_stats() {
        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;

        let stats = create_test_stats();
        metrics.update_from_stats(&stats);

        assert_eq!(metrics.entries_tailed, 1000);
        assert_eq!(metrics.entries_compacted_away, 200);
        assert_eq!(metrics.batches_dispatched, 10);
        assert_eq!(metrics.entries_sent, 800);
    }

    #[test]
    fn repl_metrics_to_metrics_count() {
        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;
        metrics.entries_tailed = 100;
        metrics.entries_compacted_away = 10;
        metrics.batches_dispatched = 5;
        metrics.entries_sent = 90;
        metrics.bytes_sent = 1000;
        metrics.throttle_stalls = 1;
        metrics.fanout_failures = 0;

        let result = metrics.to_metrics();
        assert!(result.len() >= 5);
    }

    #[test]
    fn repl_metrics_format_prometheus_nonempty() {
        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;
        metrics.entries_tailed = 100;

        let output = metrics.format_prometheus();
        assert!(!output.is_empty());
        assert!(output.contains("claudefs_repl_entries_tailed_total"));
    }

    #[test]
    fn repl_metrics_compaction_rate_zero_when_no_entries() {
        let metrics = ReplMetrics::default();
        assert_eq!(metrics.compaction_rate(), 0.0);
    }

    #[test]
    fn repl_metrics_compaction_rate_nonzero() {
        let mut metrics = ReplMetrics::default();
        metrics.entries_tailed = 1000;
        metrics.entries_compacted_away = 200;

        let rate = metrics.compaction_rate();
        assert!((rate - 0.2).abs() < 0.001);
    }

    #[test]
    fn repl_metrics_fanout_failure_rate_zero() {
        let metrics = ReplMetrics::default();
        assert_eq!(metrics.fanout_failure_rate(), 0.0);
    }

    #[test]
    fn repl_metrics_fanout_failure_rate_nonzero() {
        let mut metrics = ReplMetrics::default();
        metrics.batches_dispatched = 100;
        metrics.fanout_failures = 5;

        let rate = metrics.fanout_failure_rate();
        assert!((rate - 0.05).abs() < 0.001);
    }

    #[test]
    fn aggregator_new_empty() {
        let aggregator = MetricsAggregator::new();
        assert_eq!(aggregator.site_count(), 0);
    }

    #[test]
    fn aggregator_update_and_get() {
        let mut aggregator = MetricsAggregator::new();

        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;
        metrics.entries_tailed = 100;

        aggregator.update(metrics);

        let retrieved = aggregator.get(1);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().entries_tailed, 100);
    }

    #[test]
    fn aggregator_remove() {
        let mut aggregator = MetricsAggregator::new();

        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;
        aggregator.update(metrics);

        aggregator.remove(1);

        assert!(aggregator.get(1).is_none());
    }

    #[test]
    fn aggregator_format_all_nonempty() {
        let mut aggregator = MetricsAggregator::new();

        let mut metrics = ReplMetrics::default();
        metrics.site_id = 1;
        metrics.entries_sent = 100;
        aggregator.update(metrics);

        let output = aggregator.format_all();
        assert!(!output.is_empty());
    }

    #[test]
    fn aggregator_totals() {
        let mut aggregator = MetricsAggregator::new();

        let mut m1 = ReplMetrics::default();
        m1.site_id = 1;
        m1.entries_sent = 100;
        m1.bytes_sent = 1000;
        aggregator.update(m1);

        let mut m2 = ReplMetrics::default();
        m2.site_id = 2;
        m2.entries_sent = 200;
        m2.bytes_sent = 2000;
        aggregator.update(m2);

        assert_eq!(aggregator.total_entries_sent(), 300);
        assert_eq!(aggregator.total_bytes_sent(), 3000);
        assert_eq!(aggregator.site_count(), 2);
    }
}
