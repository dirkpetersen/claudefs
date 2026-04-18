//! Prometheus metrics exporter for replication subsystem.
//!
//! Exposes replication health and performance metrics in Prometheus text exposition format.
//! Thread-safe via Arc and atomic operations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::topology::SiteId;

/// Histogram bucket for latency tracking.
/// Uses exponential-style buckets for microsecond latencies.
#[derive(Debug)]
pub struct Histogram {
    name: String,
    help: String,
    buckets: Vec<f64>,
    counts: Vec<AtomicU64>,
    sum: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    /// Create a new histogram with the given name, help text, and bucket boundaries.
    pub fn new(name: &str, help: &str, buckets: Vec<f64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            name: name.to_string(),
            help: help.to_string(),
            buckets,
            counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record a value in microseconds.
    pub fn record(&self, value_micros: u64) {
        self.sum.fetch_add(value_micros, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        for (i, boundary) in self.buckets.iter().enumerate() {
            if (*boundary as u64) >= value_micros {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Format this histogram as Prometheus text exposition format.
    pub fn to_prometheus(&self) -> Vec<String> {
        let mut lines = Vec::new();

        lines.push(format!("# HELP {} {}", self.name, self.help));
        lines.push(format!("# TYPE {} histogram", self.name));

        let sum = self.sum.load(Ordering::Relaxed);
        let count = self.count.load(Ordering::Relaxed);

        let mut cumulative = 0u64;
        for (i, boundary) in self.buckets.iter().enumerate() {
            cumulative += self.counts[i].load(Ordering::Relaxed);
            let le_label = if i == self.buckets.len() - 1 {
                "+Inf".to_string()
            } else {
                boundary.to_string()
            };
            lines.push(format!(
                "{}_bucket{{{}}} {}",
                self.name, le_label, cumulative
            ));
        }

        lines.push(format!("{}_sum {}", self.name, sum));
        lines.push(format!("{} {}", self.name, count));

        lines
    }
}

/// Atomic counter for thread-safe incrementing.
#[derive(Debug)]
pub struct Counter {
    name: String,
    help: String,
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter with the given name and help text.
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
            value: AtomicU64::new(0),
        }
    }

    /// Increment the counter by 1.
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the current value of the counter.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Atomic gauge for thread-safe updates.
/// Stores f64 bits in an AtomicU64 for lock-free operations.
#[derive(Debug)]
pub struct Gauge {
    name: String,
    help: String,
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge with the given name and help text.
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
            value: AtomicU64::new(0),
        }
    }

    /// Set the gauge to a new value.
    pub fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Get the current value of the gauge.
    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }
}

/// Main metrics exporter for replication metrics.
/// Thread-safe via Arc.
pub struct ReplMetricsExporter {
    quorum_write_latency_micros: Arc<Histogram>,
    split_brain_events_total: Arc<Counter>,
    split_brain_resolution_time_secs: Arc<Gauge>,
    repair_actions_triggered_total: Arc<Counter>,
    repair_actions_successful_total: Arc<Counter>,
    replication_lag_secs: Mutex<HashMap<SiteId, Arc<Gauge>>>,
    connected_sites_count: Arc<Gauge>,
    quorum_writes_total: Arc<Counter>,
    quorum_writes_failed_total: Arc<Counter>,
    local_writes_total: Arc<Counter>,
    remote_writes_received_total: Arc<Counter>,
    split_brain_active: Mutex<bool>,
}

impl ReplMetricsExporter {
    /// Create a new metrics exporter with default buckets.
    pub fn new() -> Self {
        let buckets = vec![
            100.0,
            500.0,
            1000.0,
            5000.0,
            10000.0,
            50000.0,
            100000.0,
            f64::MAX,
        ];

        Self {
            quorum_write_latency_micros: Arc::new(Histogram::new(
                "claudefs_repl_quorum_write_latency_micros",
                "Latency of quorum writes in microseconds",
                buckets,
            )),
            split_brain_events_total: Arc::new(Counter::new(
                "claudefs_repl_split_brain_events_total",
                "Total number of split-brain events detected",
            )),
            split_brain_resolution_time_secs: Arc::new(Gauge::new(
                "claudefs_repl_split_brain_resolution_time_secs",
                "Time taken to resolve split-brain in seconds",
            )),
            repair_actions_triggered_total: Arc::new(Counter::new(
                "claudefs_repl_repair_actions_triggered_total",
                "Total number of repair actions triggered",
            )),
            repair_actions_successful_total: Arc::new(Counter::new(
                "claudefs_repl_repair_actions_successful_total",
                "Total number of successful repair actions",
            )),
            replication_lag_secs: Mutex::new(HashMap::new()),
            connected_sites_count: Arc::new(Gauge::new(
                "claudefs_repl_connected_sites_count",
                "Number of currently connected remote sites",
            )),
            quorum_writes_total: Arc::new(Counter::new(
                "claudefs_repl_quorum_writes_total",
                "Total number of quorum writes completed",
            )),
            quorum_writes_failed_total: Arc::new(Counter::new(
                "claudefs_repl_quorum_writes_failed_total",
                "Total number of quorum writes that failed",
            )),
            local_writes_total: Arc::new(Counter::new(
                "claudefs_repl_local_writes_total",
                "Total number of local writes processed",
            )),
            remote_writes_received_total: Arc::new(Counter::new(
                "claudefs_repl_remote_writes_received_total",
                "Total number of remote writes received",
            )),
            split_brain_active: Mutex::new(false),
        }
    }

    /// Record a quorum write latency.
    pub fn record_quorum_write(&self, latency_micros: u64) {
        self.quorum_write_latency_micros.record(latency_micros);
    }

    /// Record a split-brain event.
    pub fn record_split_brain_event(&self) {
        self.split_brain_events_total.increment();
        *self.split_brain_active.lock().unwrap() = true;
    }

    /// Record a split-brain resolution.
    pub fn record_split_brain_resolved(&self, resolution_time_secs: f64) {
        self.split_brain_resolution_time_secs
            .set(resolution_time_secs);
        *self.split_brain_active.lock().unwrap() = false;
    }

    /// Record a repair action triggered.
    pub fn record_repair_action_triggered(&self) {
        self.repair_actions_triggered_total.increment();
    }

    /// Record a successful repair action.
    pub fn record_repair_action_successful(&self) {
        self.repair_actions_successful_total.increment();
    }

    /// Update the replication lag for a specific site.
    pub fn update_replication_lag(&self, site_id: SiteId, lag_secs: f64) {
        let mut lags = self.replication_lag_secs.lock().unwrap();
        let gauge = lags
            .entry(site_id)
            .or_insert_with(|| Arc::new(Gauge::new("lag", "lag")));
        gauge.set(lag_secs);
    }

    /// Set the number of connected sites.
    pub fn set_connected_sites(&self, count: usize) {
        self.connected_sites_count.set(count as f64);
    }

    /// Get the number of connected sites.
    pub fn connected_sites_count(&self) -> f64 {
        self.connected_sites_count.get()
    }

    /// Increment the quorum writes counter.
    pub fn increment_quorum_writes(&self) {
        self.quorum_writes_total.increment();
    }

    /// Increment the quorum write failures counter.
    pub fn increment_quorum_write_failures(&self) {
        self.quorum_writes_failed_total.increment();
    }

    /// Increment the local writes counter.
    pub fn increment_local_writes(&self) {
        self.local_writes_total.increment();
    }

    /// Increment the remote writes received counter.
    pub fn increment_remote_writes(&self) {
        self.remote_writes_received_total.increment();
    }

    /// Get the current lag for a specific site.
    pub fn get_current_lag(&self, site_id: SiteId) -> Option<f64> {
        let lags = self.replication_lag_secs.lock().unwrap();
        lags.get(&site_id).map(|g| g.get())
    }

    /// Get all current lags as a HashMap.
    pub fn get_all_lags(&self) -> HashMap<SiteId, f64> {
        let lags = self.replication_lag_secs.lock().unwrap();
        lags.iter().map(|(&k, v)| (k, v.get())).collect()
    }

    /// Get the current split-brain status.
    pub fn get_current_split_brain_status(&self) -> bool {
        *self.split_brain_active.lock().unwrap()
    }

    /// Export all metrics in Prometheus format.
    pub fn export_prometheus(&self) -> String {
        let mut lines = Vec::new();

        lines.extend(self.quorum_write_latency_micros.to_prometheus());
        lines.push(String::new());

        let sb_count = self.split_brain_events_total.get();
        lines.push("# HELP claudefs_repl_split_brain_events_total Total number of split-brain events detected".to_string());
        lines.push("# TYPE claudefs_repl_split_brain_events_total counter".to_string());
        lines.push(format!(
            "claudefs_repl_split_brain_events_total {}",
            sb_count
        ));
        lines.push(String::new());

        let sb_resolve = self.split_brain_resolution_time_secs.get();
        lines.push("# HELP claudefs_repl_split_brain_resolution_time_secs Time taken to resolve split-brain in seconds".to_string());
        lines.push("# TYPE claudefs_repl_split_brain_resolution_time_secs gauge".to_string());
        lines.push(format!(
            "claudefs_repl_split_brain_resolution_time_secs {}",
            sb_resolve
        ));
        lines.push(String::new());

        let repair_triggered = self.repair_actions_triggered_total.get();
        lines.push("# HELP claudefs_repl_repair_actions_triggered_total Total number of repair actions triggered".to_string());
        lines.push("# TYPE claudefs_repl_repair_actions_triggered_total counter".to_string());
        lines.push(format!(
            "claudefs_repl_repair_actions_triggered_total {}",
            repair_triggered
        ));
        lines.push(String::new());

        let repair_success = self.repair_actions_successful_total.get();
        lines.push("# HELP claudefs_repl_repair_actions_successful_total Total number of successful repair actions".to_string());
        lines.push("# TYPE claudefs_repl_repair_actions_successful_total counter".to_string());
        lines.push(format!(
            "claudefs_repl_repair_actions_successful_total {}",
            repair_success
        ));
        lines.push(String::new());

        let lag_map = self.replication_lag_secs.lock().unwrap();
        if !lag_map.is_empty() {
            lines.push("# HELP claudefs_repl_replication_lag_secs Current replication lag per site in seconds".to_string());
            lines.push("# TYPE claudefs_repl_replication_lag_secs gauge".to_string());
            for (site_id, gauge) in lag_map.iter() {
                lines.push(format!(
                    "claudefs_repl_replication_lag_secs{{site_id=\"{}\"}} {}",
                    site_id,
                    gauge.get()
                ));
            }
            lines.push(String::new());
        }

        let connected = self.connected_sites_count.get() as u64;
        lines.push(
            "# HELP claudefs_repl_connected_sites_count Number of currently connected remote sites"
                .to_string(),
        );
        lines.push("# TYPE claudefs_repl_connected_sites_count gauge".to_string());
        lines.push(format!("claudefs_repl_connected_sites_count {}", connected));
        lines.push(String::new());

        let qw = self.quorum_writes_total.get();
        lines.push(
            "# HELP claudefs_repl_quorum_writes_total Total number of quorum writes completed"
                .to_string(),
        );
        lines.push("# TYPE claudefs_repl_quorum_writes_total counter".to_string());
        lines.push(format!("claudefs_repl_quorum_writes_total {}", qw));
        lines.push(String::new());

        let qwf = self.quorum_writes_failed_total.get();
        lines.push("# HELP claudefs_repl_quorum_writes_failed_total Total number of quorum writes that failed".to_string());
        lines.push("# TYPE claudefs_repl_quorum_writes_failed_total counter".to_string());
        lines.push(format!("claudefs_repl_quorum_writes_failed_total {}", qwf));
        lines.push(String::new());

        let lw = self.local_writes_total.get();
        lines.push(
            "# HELP claudefs_repl_local_writes_total Total number of local writes processed"
                .to_string(),
        );
        lines.push("# TYPE claudefs_repl_local_writes_total counter".to_string());
        lines.push(format!("claudefs_repl_local_writes_total {}", lw));
        lines.push(String::new());

        let rw = self.remote_writes_received_total.get();
        lines.push("# HELP claudefs_repl_remote_writes_received_total Total number of remote writes received".to_string());
        lines.push("# TYPE claudefs_repl_remote_writes_received_total counter".to_string());
        lines.push(format!("claudefs_repl_remote_writes_received_total {}", rw));
        lines.push(String::new());

        lines.join("\n")
    }
}

impl Default for ReplMetricsExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ReplMetricsExporter {
    fn clone(&self) -> Self {
        Self {
            quorum_write_latency_micros: Arc::new(Histogram::new(
                "claudefs_repl_quorum_write_latency_micros",
                "Latency of quorum writes in microseconds",
                vec![
                    100.0,
                    500.0,
                    1000.0,
                    5000.0,
                    10000.0,
                    50000.0,
                    100000.0,
                    f64::MAX,
                ],
            )),
            split_brain_events_total: Arc::new(Counter::new(
                "claudefs_repl_split_brain_events_total",
                "Total number of split-brain events detected",
            )),
            split_brain_resolution_time_secs: Arc::new(Gauge::new(
                "claudefs_repl_split_brain_resolution_time_secs",
                "Time taken to resolve split-brain in seconds",
            )),
            repair_actions_triggered_total: Arc::new(Counter::new(
                "claudefs_repl_repair_actions_triggered_total",
                "Total number of repair actions triggered",
            )),
            repair_actions_successful_total: Arc::new(Counter::new(
                "claudefs_repl_repair_actions_successful_total",
                "Total number of successful repair actions",
            )),
            replication_lag_secs: Mutex::new(HashMap::new()),
            connected_sites_count: Arc::new(Gauge::new(
                "claudefs_repl_connected_sites_count",
                "Number of currently connected remote sites",
            )),
            quorum_writes_total: Arc::new(Counter::new(
                "claudefs_repl_quorum_writes_total",
                "Total number of quorum writes completed",
            )),
            quorum_writes_failed_total: Arc::new(Counter::new(
                "claudefs_repl_quorum_writes_failed_total",
                "Total number of quorum writes that failed",
            )),
            local_writes_total: Arc::new(Counter::new(
                "claudefs_repl_local_writes_total",
                "Total number of local writes processed",
            )),
            remote_writes_received_total: Arc::new(Counter::new(
                "claudefs_repl_remote_writes_received_total",
                "Total number of remote writes received",
            )),
            split_brain_active: Mutex::new(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_creation() {
        let buckets = vec![100.0, 500.0, 1000.0];
        let histogram = Histogram::new("test_histogram", "Test histogram", buckets.clone());
        assert_eq!(histogram.buckets.len(), buckets.len());
    }

    #[test]
    fn test_histogram_record_single_value() {
        let histogram = Histogram::new("test", "test", vec![100.0, 500.0, 1000.0, f64::MAX]);
        histogram.record(50);
        assert_eq!(histogram.count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_histogram_record_multiple_values() {
        let histogram = Histogram::new("test", "test", vec![100.0, 500.0, 1000.0, f64::MAX]);
        histogram.record(50);
        histogram.record(200);
        histogram.record(600);
        assert_eq!(histogram.count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_histogram_prometheus_format() {
        let histogram = Histogram::new(
            "test_hist",
            "A test histogram",
            vec![100.0, 500.0, f64::MAX],
        );
        histogram.record(50);
        histogram.record(200);

        let lines = histogram.to_prometheus();
        assert!(lines.iter().any(|l| l.contains("# HELP")));
        assert!(lines.iter().any(|l| l.contains("# TYPE")));
        assert!(lines.iter().any(|l| l.contains("_bucket")));
    }

    #[test]
    fn test_counter_increment() {
        let counter = Counter::new("test_counter", "A test counter");
        counter.increment();
        counter.increment();
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_gauge_update_and_read() {
        let gauge = Gauge::new("test_gauge", "A test gauge");
        gauge.set(42.5);
        assert!((gauge.get() - 42.5).abs() < 0.001);
    }

    #[test]
    fn test_multiple_site_lag_tracking() {
        let exporter = ReplMetricsExporter::new();
        exporter.update_replication_lag(1, 10.0);
        exporter.update_replication_lag(2, 20.0);
        exporter.update_replication_lag(3, 30.0);

        assert_eq!(exporter.get_current_lag(1), Some(10.0));
        assert_eq!(exporter.get_current_lag(2), Some(20.0));
        assert_eq!(exporter.get_current_lag(3), Some(30.0));
    }

    #[test]
    fn test_concurrent_metric_updates() {
        let exporter = ReplMetricsExporter::new();
        let exporter_clone = Arc::new(exporter);

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let e = exporter_clone.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        e.increment_local_writes();
                        e.increment_quorum_writes();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(exporter_clone.local_writes_total.get(), 1000);
        assert_eq!(exporter_clone.quorum_writes_total.get(), 1000);
    }

    #[test]
    fn test_split_brain_counter() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_split_brain_event();
        exporter.record_split_brain_event();

        assert_eq!(exporter.split_brain_events_total.get(), 2);
        assert!(exporter.get_current_split_brain_status());

        exporter.record_split_brain_resolved(5.0);
        assert!(!exporter.get_current_split_brain_status());
    }

    #[test]
    fn test_repair_action_tracking() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_repair_action_triggered();
        exporter.record_repair_action_triggered();
        exporter.record_repair_action_successful();

        assert_eq!(exporter.repair_actions_triggered_total.get(), 2);
        assert_eq!(exporter.repair_actions_successful_total.get(), 1);
    }

    #[test]
    fn test_exporter_full_prometheus_output() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_quorum_write(500);
        exporter.increment_quorum_writes();
        exporter.increment_local_writes();
        exporter.set_connected_sites(2);

        let output = exporter.export_prometheus();
        assert!(!output.is_empty());
        assert!(output.contains("claudefs_repl_quorum_write_latency_micros"));
    }

    #[test]
    fn test_exporter_clone_and_thread_safety() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        let exporter_clone = exporter.clone();

        exporter_clone.increment_local_writes();
        assert_eq!(exporter.local_writes_total.get(), 1);
    }

    #[test]
    fn test_exporter_default() {
        let exporter = ReplMetricsExporter::default();
        assert_eq!(exporter.connected_sites_count.get() as u64, 0);
    }

    #[test]
    fn test_gauge_set_negative() {
        let gauge = Gauge::new("test", "test");
        gauge.set(-100.5);
        assert!((gauge.get() - (-100.5)).abs() < 0.001);
    }

    #[test]
    fn test_histogram_bucket_boundaries() {
        let histogram = Histogram::new("test", "test", vec![100.0, 500.0, 1000.0, f64::MAX]);
        histogram.record(100);
        histogram.record(499);
        histogram.record(500);
        histogram.record(1001);
        histogram.record(10000);

        let counts: Vec<u64> = histogram
            .counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect();

        assert_eq!(counts[0], 1);
        assert_eq!(counts[1], 2);
        assert_eq!(counts[2], 0);
        assert_eq!(counts[3], 2);
    }

    #[test]
    fn test_quorum_write_latency_recording() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_quorum_write(150);
        exporter.record_quorum_write(250);
        exporter.record_quorum_write(350);

        let output = exporter.export_prometheus();
        assert!(output.contains("claudefs_repl_quorum_write_latency_micros"));
    }

    #[test]
    fn test_connected_sites_updates() {
        let exporter = ReplMetricsExporter::new();
        exporter.set_connected_sites(3);
        assert_eq!(exporter.connected_sites_count.get() as usize, 3);

        exporter.set_connected_sites(1);
        assert_eq!(exporter.connected_sites_count.get() as usize, 1);
    }

    #[test]
    fn test_write_failure_tracking() {
        let exporter = ReplMetricsExporter::new();
        exporter.increment_quorum_write_failures();
        exporter.increment_quorum_write_failures();

        assert_eq!(exporter.quorum_writes_failed_total.get(), 2);
    }
}
