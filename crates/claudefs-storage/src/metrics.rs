//! Prometheus-compatible storage engine metrics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::IoOpType;

/// Type of metric for Prometheus compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// Monotonically increasing value
    Counter,
    /// Value that can go up or down
    Gauge,
    /// Distribution of values in buckets
    Histogram,
}

/// Value of a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    /// Counter value (u64)
    Counter(u64),
    /// Gauge value (f64)
    Gauge(f64),
    /// Histogram with sum, count, and buckets
    Histogram {
        /// Sum of all values
        sum: f64,
        /// Count of values
        count: u64,
        /// Buckets as (upper_bound, cumulative_count)
        buckets: Vec<(f64, u64)>,
    },
}

/// A single metric with metadata and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name
    pub name: String,
    /// Help text
    pub help: String,
    /// Type of metric
    pub metric_type: MetricType,
    /// Current value
    pub value: MetricValue,
    /// Label key-value pairs
    pub labels: Vec<(String, String)>,
}

/// Storage engine metrics collector.
/// Provides counters, gauges, and histograms for Prometheus export.
pub struct StorageMetrics {
    /// Total I/O operations by type
    io_ops_total: HashMap<String, u64>,
    /// Total I/O bytes by type
    io_bytes_total: HashMap<String, u64>,
    /// Total I/O errors
    io_errors_total: u64,
    /// Raw latency samples in microseconds (ring buffer of last 1024)
    io_latency_us: Vec<u64>,
    /// Current index in the latency ring buffer
    latency_index: usize,
    /// Total blocks allocated
    blocks_allocated: u64,
    /// Total blocks freed
    blocks_freed: u64,
    /// Used capacity in bytes
    capacity_used_bytes: u64,
    /// Total capacity in bytes
    capacity_total_bytes: u64,
    /// Cache hits
    cache_hits: u64,
    /// Cache misses
    cache_misses: u64,
    /// Journal entries appended
    journal_entries: u64,
    /// Journal commits
    journal_commits: u64,
}

impl StorageMetrics {
    /// Create a new StorageMetrics instance.
    pub fn new() -> Self {
        Self {
            io_ops_total: HashMap::new(),
            io_bytes_total: HashMap::new(),
            io_errors_total: 0,
            io_latency_us: Vec::with_capacity(1024),
            latency_index: 0,
            blocks_allocated: 0,
            blocks_freed: 0,
            capacity_used_bytes: 0,
            capacity_total_bytes: 0,
            cache_hits: 0,
            cache_misses: 0,
            journal_entries: 0,
            journal_commits: 0,
        }
    }

    /// Record an I/O operation.
    pub fn record_io(&mut self, op: IoOpType, bytes: u64, latency_us: u64) {
        let op_key = match op {
            IoOpType::Read => "read",
            IoOpType::Write => "write",
            IoOpType::Flush => "flush",
            IoOpType::Discard => "discard",
        };

        *self.io_ops_total.entry(op_key.to_string()).or_insert(0) += 1;
        *self.io_bytes_total.entry(op_key.to_string()).or_insert(0) += bytes;

        if self.io_latency_us.len() < 1024 {
            self.io_latency_us.push(latency_us);
        } else {
            self.io_latency_us[self.latency_index] = latency_us;
            self.latency_index = (self.latency_index + 1) % 1024;
        }

        debug!(
            "Recorded I/O: op={}, bytes={}, latency_us={}",
            op_key, bytes, latency_us
        );
    }

    /// Record an I/O error.
    pub fn record_io_error(&mut self) {
        self.io_errors_total += 1;
        debug!("Recorded I/O error");
    }

    /// Record block allocations.
    pub fn record_allocation(&mut self, count: u64) {
        self.blocks_allocated += count;
        debug!("Recorded {} block allocations", count);
    }

    /// Record block frees.
    pub fn record_free(&mut self, count: u64) {
        self.blocks_freed += count;
        debug!("Recorded {} block frees", count);
    }

    /// Record a cache hit.
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
        debug!("Recorded cache hit");
    }

    /// Record a cache miss.
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
        debug!("Recorded cache miss");
    }

    /// Record a journal append.
    pub fn record_journal_append(&mut self) {
        self.journal_entries += 1;
        debug!("Recorded journal append");
    }

    /// Record a journal commit.
    pub fn record_journal_commit(&mut self) {
        self.journal_commits += 1;
        debug!("Recorded journal commit");
    }

    /// Update capacity gauge.
    pub fn set_capacity(&mut self, used: u64, total: u64) {
        self.capacity_used_bytes = used;
        self.capacity_total_bytes = total;
        debug!("Set capacity: used={}, total={}", used, total);
    }

    /// Export all metrics in Prometheus format.
    pub fn export(&self) -> Vec<Metric> {
        let mut metrics = Vec::new();

        for (op_type, count) in &self.io_ops_total {
            metrics.push(Metric {
                name: "claudefs_storage_io_ops_total".to_string(),
                help: "Total number of I/O operations".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(*count),
                labels: vec![("op".to_string(), op_type.clone())],
            });
        }

        for (op_type, bytes) in &self.io_bytes_total {
            metrics.push(Metric {
                name: "claudefs_storage_io_bytes_total".to_string(),
                help: "Total number of I/O bytes".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(*bytes),
                labels: vec![("op".to_string(), op_type.clone())],
            });
        }

        if self.io_errors_total > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_io_errors_total".to_string(),
                help: "Total number of I/O errors".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.io_errors_total),
                labels: vec![],
            });
        }

        if self.blocks_allocated > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_blocks_allocated".to_string(),
                help: "Total number of blocks allocated".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.blocks_allocated),
                labels: vec![],
            });
        }

        if self.blocks_freed > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_blocks_freed".to_string(),
                help: "Total number of blocks freed".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.blocks_freed),
                labels: vec![],
            });
        }

        if self.capacity_total_bytes > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_capacity_used_bytes".to_string(),
                help: "Used capacity in bytes".to_string(),
                metric_type: MetricType::Gauge,
                value: MetricValue::Gauge(self.capacity_used_bytes as f64),
                labels: vec![],
            });

            metrics.push(Metric {
                name: "claudefs_storage_capacity_total_bytes".to_string(),
                help: "Total capacity in bytes".to_string(),
                metric_type: MetricType::Gauge,
                value: MetricValue::Gauge(self.capacity_total_bytes as f64),
                labels: vec![],
            });
        }

        if self.cache_hits > 0 || self.cache_misses > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_cache_hits_total".to_string(),
                help: "Total number of cache hits".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.cache_hits),
                labels: vec![],
            });

            metrics.push(Metric {
                name: "claudefs_storage_cache_misses_total".to_string(),
                help: "Total number of cache misses".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.cache_misses),
                labels: vec![],
            });
        }

        if self.journal_entries > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_journal_entries_total".to_string(),
                help: "Total number of journal entries".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.journal_entries),
                labels: vec![],
            });
        }

        if self.journal_commits > 0 {
            metrics.push(Metric {
                name: "claudefs_storage_journal_commits_total".to_string(),
                help: "Total number of journal commits".to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(self.journal_commits),
                labels: vec![],
            });
        }

        if !self.io_latency_us.is_empty() {
            let avg = self.avg_latency_us();
            metrics.push(Metric {
                name: "claudefs_storage_io_latency_avg_us".to_string(),
                help: "Average I/O latency in microseconds".to_string(),
                metric_type: MetricType::Gauge,
                value: MetricValue::Gauge(avg),
                labels: vec![],
            });

            let p99 = self.p99_latency_us();
            metrics.push(Metric {
                name: "claudefs_storage_io_latency_p99_us".to_string(),
                help: "P99 I/O latency in microseconds".to_string(),
                metric_type: MetricType::Gauge,
                value: MetricValue::Gauge(p99 as f64),
                labels: vec![],
            });
        }

        metrics
    }

    /// Calculate average I/O latency from ring buffer.
    pub fn avg_latency_us(&self) -> f64 {
        if self.io_latency_us.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.io_latency_us.iter().sum();
        sum as f64 / self.io_latency_us.len() as f64
    }

    /// Calculate P99 latency from ring buffer.
    pub fn p99_latency_us(&self) -> u64 {
        if self.io_latency_us.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = self.io_latency_us.clone();
        sorted.sort_unstable();
        let idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1);
        sorted[idx]
    }

    /// Calculate cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }
        (self.cache_hits as f64) / (total as f64)
    }

    /// Reset all counters (for testing).
    pub fn reset(&mut self) {
        self.io_ops_total.clear();
        self.io_bytes_total.clear();
        self.io_errors_total = 0;
        self.io_latency_us.clear();
        self.latency_index = 0;
        self.blocks_allocated = 0;
        self.blocks_freed = 0;
        self.capacity_used_bytes = 0;
        self.capacity_total_bytes = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.journal_entries = 0;
        self.journal_commits = 0;
        debug!("Reset all metrics");
    }
}

impl Default for StorageMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_io_read() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);

        let ops = metrics.io_ops_total.get("read");
        assert_eq!(ops, Some(&1));

        let bytes = metrics.io_bytes_total.get("read");
        assert_eq!(bytes, Some(&4096));
    }

    #[test]
    fn test_record_io_write() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Write, 65536, 200);

        let ops = metrics.io_ops_total.get("write");
        assert_eq!(ops, Some(&1));

        let bytes = metrics.io_bytes_total.get("write");
        assert_eq!(bytes, Some(&65536));
    }

    #[test]
    fn test_record_io_flush() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Flush, 0, 50);

        let ops = metrics.io_ops_total.get("flush");
        assert_eq!(ops, Some(&1));

        let bytes = metrics.io_bytes_total.get("flush");
        assert_eq!(bytes, Some(&0));
    }

    #[test]
    fn test_record_io_discard() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Discard, 4096, 10);

        let ops = metrics.io_ops_total.get("discard");
        assert_eq!(ops, Some(&1));
    }

    #[test]
    fn test_record_io_error() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io_error();
        metrics.record_io_error();
        metrics.record_io_error();

        assert_eq!(metrics.io_errors_total, 3);
    }

    #[test]
    fn test_record_allocation() {
        let mut metrics = StorageMetrics::new();
        metrics.record_allocation(10);
        metrics.record_allocation(5);

        assert_eq!(metrics.blocks_allocated, 15);
    }

    #[test]
    fn test_record_free() {
        let mut metrics = StorageMetrics::new();
        metrics.record_free(8);
        metrics.record_free(4);

        assert_eq!(metrics.blocks_freed, 12);
    }

    #[test]
    fn test_cache_hit_miss() {
        let mut metrics = StorageMetrics::new();
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(metrics.cache_hits, 2);
        assert_eq!(metrics.cache_misses, 1);
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut metrics = StorageMetrics::new();
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();
        metrics.record_cache_miss();

        let rate = metrics.cache_hit_rate();
        assert_eq!(rate, 0.5);
    }

    #[test]
    fn test_cache_hit_rate_zero() {
        let mut metrics = StorageMetrics::new();
        let rate = metrics.cache_hit_rate();
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_journal_stats() {
        let mut metrics = StorageMetrics::new();
        metrics.record_journal_append();
        metrics.record_journal_append();
        metrics.record_journal_append();
        metrics.record_journal_commit();

        assert_eq!(metrics.journal_entries, 3);
        assert_eq!(metrics.journal_commits, 1);
    }

    #[test]
    fn test_capacity_gauge() {
        let mut metrics = StorageMetrics::new();
        metrics.set_capacity(500_000_000_000, 1_000_000_000_000);

        assert_eq!(metrics.capacity_used_bytes, 500_000_000_000);
        assert_eq!(metrics.capacity_total_bytes, 1_000_000_000_000);
    }

    #[test]
    fn test_export_metric_types() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io_error();

        let exported = metrics.export();

        let has_counter = exported
            .iter()
            .any(|m| m.metric_type == MetricType::Counter);
        assert!(has_counter);
    }

    #[test]
    fn test_avg_latency_calculation() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Read, 4096, 200);
        metrics.record_io(IoOpType::Read, 4096, 300);

        let avg = metrics.avg_latency_us();
        assert_eq!(avg, 200.0);
    }

    #[test]
    fn test_p99_latency_calculation() {
        let mut metrics = StorageMetrics::new();
        for i in 1..=100 {
            metrics.record_io(IoOpType::Read, 4096, i);
        }

        let p99 = metrics.p99_latency_us();
        assert_eq!(p99, 100);
    }

    #[test]
    fn test_reset_clears_counters() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io_error();
        metrics.record_allocation(5);
        metrics.record_cache_hit();

        metrics.reset();

        assert_eq!(metrics.io_ops_total.get("read"), None);
        assert_eq!(metrics.io_errors_total, 0);
        assert_eq!(metrics.blocks_allocated, 0);
        assert_eq!(metrics.cache_hits, 0);
    }

    #[test]
    fn test_empty_metrics_export() {
        let metrics = StorageMetrics::new();
        let exported = metrics.export();
        assert!(exported.is_empty());
    }

    #[test]
    fn test_multiple_ops_accumulate() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Write, 8192, 200);

        assert_eq!(metrics.io_ops_total.get("read"), Some(&2));
        assert_eq!(metrics.io_ops_total.get("write"), Some(&1));
        assert_eq!(metrics.io_bytes_total.get("read"), Some(&8192));
        assert_eq!(metrics.io_bytes_total.get("write"), Some(&8192));
    }

    #[test]
    fn test_latency_ring_buffer_wrap() {
        let mut metrics = StorageMetrics::new();
        for _ in 0..2000 {
            metrics.record_io(IoOpType::Read, 4096, 100);
        }

        assert_eq!(metrics.io_latency_us.len(), 1024);
    }

    #[test]
    fn test_export_includes_all_metric_names() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Write, 8192, 200);
        metrics.record_io_error();
        metrics.record_allocation(1);
        metrics.record_free(1);
        metrics.set_capacity(100, 1000);
        metrics.record_cache_hit();
        metrics.record_cache_miss();
        metrics.record_journal_append();
        metrics.record_journal_commit();

        let exported = metrics.export();
        let names: Vec<&str> = exported.iter().map(|m| m.name.as_str()).collect();

        assert!(names.iter().any(|n| n.contains("io_ops_total")));
        assert!(names.iter().any(|n| n.contains("io_bytes_total")));
        assert!(names.iter().any(|n| n.contains("io_errors_total")));
        assert!(names.iter().any(|n| n.contains("blocks_allocated")));
        assert!(names.iter().any(|n| n.contains("blocks_freed")));
        assert!(names.iter().any(|n| n.contains("capacity_used")));
        assert!(names.iter().any(|n| n.contains("capacity_total")));
        assert!(names.iter().any(|n| n.contains("cache_hits")));
        assert!(names.iter().any(|n| n.contains("cache_misses")));
        assert!(names.iter().any(|n| n.contains("journal_entries")));
        assert!(names.iter().any(|n| n.contains("journal_commits")));
        assert!(names.iter().any(|n| n.contains("latency_avg")));
        assert!(names.iter().any(|n| n.contains("latency_p99")));
    }

    #[test]
    fn test_labels_present_in_exported_metrics() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Write, 8192, 200);

        let exported = metrics.export();
        let io_ops = exported.iter().find(|m| m.name.contains("io_ops_total"));

        assert!(io_ops.is_some());
        let op = io_ops.unwrap();
        assert!(!op.labels.is_empty());
        assert!(op.labels.iter().any(|(k, _v)| k == "op"));
    }

    #[test]
    fn test_iotype_mapping_to_string_keys() {
        let mut metrics = StorageMetrics::new();
        metrics.record_io(IoOpType::Read, 4096, 100);
        metrics.record_io(IoOpType::Write, 4096, 100);
        metrics.record_io(IoOpType::Flush, 0, 100);
        metrics.record_io(IoOpType::Discard, 4096, 100);

        assert!(metrics.io_ops_total.contains_key("read"));
        assert!(metrics.io_ops_total.contains_key("write"));
        assert!(metrics.io_ops_total.contains_key("flush"));
        assert!(metrics.io_ops_total.contains_key("discard"));
    }

    #[test]
    fn test_metric_type_variants() {
        use MetricType::*;
        let _ = Counter;
        let _ = Gauge;
        let _ = Histogram;
    }

    #[test]
    fn test_metric_value_variants() {
        use MetricValue::*;
        let _ = Counter(42);
        let _ = Gauge(3.14);
        let _ = Histogram {
            sum: 100.0,
            count: 10,
            buckets: vec![],
        };
    }
}
