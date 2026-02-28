//! Metadata service metrics collector.
//!
//! Tracks operation counts, latencies, and error rates for monitoring.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Metadata operation type for metrics tracking.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MetricOp {
    /// Lookup operation (path to inode resolution).
    Lookup,
    /// Getattr operation (inode attribute retrieval).
    Getattr,
    /// Setattr operation (inode attribute modification).
    Setattr,
    /// CreateFile operation (new file creation).
    CreateFile,
    /// Mkdir operation (directory creation).
    Mkdir,
    /// Unlink operation (file deletion).
    Unlink,
    /// Rmdir operation (directory deletion).
    Rmdir,
    /// Rename operation.
    Rename,
    /// Symlink operation.
    Symlink,
    /// Link operation (hard link creation).
    Link,
    /// Readlink operation (symlink target read).
    Readlink,
    /// Readdir operation.
    Readdir,
    /// Open operation.
    Open,
    /// Close operation.
    Close,
    /// ReadIndex operation (linearizable read).
    ReadIndex,
}

impl MetricOp {
    /// Returns a string representation of the operation.
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricOp::Lookup => "lookup",
            MetricOp::Getattr => "getattr",
            MetricOp::Setattr => "setattr",
            MetricOp::CreateFile => "create_file",
            MetricOp::Mkdir => "mkdir",
            MetricOp::Unlink => "unlink",
            MetricOp::Rmdir => "rmdir",
            MetricOp::Rename => "rename",
            MetricOp::Symlink => "symlink",
            MetricOp::Link => "link",
            MetricOp::Readlink => "readlink",
            MetricOp::Readdir => "readdir",
            MetricOp::Open => "open",
            MetricOp::Close => "close",
            MetricOp::ReadIndex => "read_index",
        }
    }
}

/// Per-operation metrics.
#[derive(Clone, Debug, Default)]
pub struct OpMetrics {
    /// Number of operations.
    pub count: u64,
    /// Number of errors.
    pub errors: u64,
    /// Total duration in microseconds.
    pub total_duration_us: u64,
    /// Maximum duration in microseconds.
    pub max_duration_us: u64,
}

impl OpMetrics {
    /// Returns the average duration in microseconds.
    pub fn avg_duration_us(&self) -> u64 {
        if self.count > 0 {
            self.total_duration_us / self.count
        } else {
            0
        }
    }

    /// Returns the error rate (errors / total operations).
    pub fn error_rate(&self) -> f64 {
        if self.count > 0 {
            self.errors as f64 / self.count as f64
        } else {
            0.0
        }
    }
}

/// Aggregated metrics for the metadata service.
#[derive(Clone, Debug, Default)]
pub struct MetadataMetrics {
    /// Per-operation metrics.
    pub ops: HashMap<MetricOp, OpMetrics>,
    /// Total number of operations.
    pub total_ops: u64,
    /// Total number of errors.
    pub total_errors: u64,
    /// Number of active leases.
    pub active_leases: u64,
    /// Number of active watches.
    pub active_watches: u64,
    /// Number of active file handles.
    pub active_file_handles: u64,
    /// Number of cache hits.
    pub cache_hits: u64,
    /// Number of cache misses.
    pub cache_misses: u64,
    /// Number of negative cache hits.
    pub negative_cache_hits: u64,
    /// Total inode count.
    pub inode_count: u64,
}

/// Metrics collector for the metadata service.
///
/// Tracks operation counts, latencies, error rates, and cache statistics.
pub struct MetricsCollector {
    op_metrics: RwLock<HashMap<MetricOp, OpMetrics>>,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    negative_cache_hits: AtomicU64,
}

impl MetricsCollector {
    /// Creates a new MetricsCollector.
    pub fn new() -> Self {
        Self {
            op_metrics: RwLock::new(HashMap::new()),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            negative_cache_hits: AtomicU64::new(0),
        }
    }

    /// Records an operation with its duration and success status.
    pub fn record_op(&self, op: MetricOp, duration_us: u64, success: bool) {
        let mut metrics = self.op_metrics.write().unwrap();
        let entry = metrics.entry(op).or_default();
        entry.count += 1;
        if !success {
            entry.errors += 1;
        }
        entry.total_duration_us += duration_us;
        if duration_us > entry.max_duration_us {
            entry.max_duration_us = duration_us;
        }
    }

    /// Increments the cache hit counter.
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the cache miss counter.
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the negative cache hit counter.
    pub fn record_negative_cache_hit(&self) {
        self.negative_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Gets metrics for a specific operation.
    pub fn get_op_metrics(&self, op: &MetricOp) -> OpMetrics {
        let metrics = self.op_metrics.read().unwrap();
        metrics.get(op).cloned().unwrap_or_default()
    }

    /// Creates a point-in-time snapshot of all metrics.
    pub fn snapshot(
        &self,
        active_leases: u64,
        active_watches: u64,
        active_file_handles: u64,
        inode_count: u64,
    ) -> MetadataMetrics {
        let op_metrics = self.op_metrics.read().unwrap();
        let mut total_ops = 0u64;
        let mut total_errors = 0u64;

        let ops: HashMap<MetricOp, OpMetrics> = op_metrics
            .iter()
            .map(|(k, v)| {
                total_ops += v.count;
                total_errors += v.errors;
                (k.clone(), v.clone())
            })
            .collect();

        MetadataMetrics {
            ops,
            total_ops,
            total_errors,
            active_leases,
            active_watches,
            active_file_handles,
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            negative_cache_hits: self.negative_cache_hits.load(Ordering::Relaxed),
            inode_count,
        }
    }

    /// Resets all counters.
    pub fn reset(&self) {
        let mut metrics = self.op_metrics.write().unwrap();
        metrics.clear();
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.negative_cache_hits.store(0, Ordering::Relaxed);
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_op_success() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Lookup, 100, true);
        collector.record_op(MetricOp::Lookup, 200, true);

        let metrics = collector.get_op_metrics(&MetricOp::Lookup);
        assert_eq!(metrics.count, 2);
        assert_eq!(metrics.errors, 0);
        assert_eq!(metrics.total_duration_us, 300);
    }

    #[test]
    fn test_record_op_error() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Getattr, 50, true);
        collector.record_op(MetricOp::Getattr, 75, false);

        let metrics = collector.get_op_metrics(&MetricOp::Getattr);
        assert_eq!(metrics.count, 2);
        assert_eq!(metrics.errors, 1);
        assert_eq!(metrics.error_rate(), 0.5);
    }

    #[test]
    fn test_record_cache_hit() {
        let collector = MetricsCollector::new();
        collector.record_cache_hit();
        collector.record_cache_hit();
        collector.record_cache_miss();

        let snapshot = collector.snapshot(0, 0, 0, 0);
        assert_eq!(snapshot.cache_hits, 2);
        assert_eq!(snapshot.cache_misses, 1);
    }

    #[test]
    fn test_get_op_metrics() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Mkdir, 150, true);

        let metrics = collector.get_op_metrics(&MetricOp::Mkdir);
        assert_eq!(metrics.count, 1);
        assert_eq!(metrics.avg_duration_us(), 150);

        let empty_metrics = collector.get_op_metrics(&MetricOp::Unlink);
        assert_eq!(empty_metrics.count, 0);
    }

    #[test]
    fn test_snapshot() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Open, 100, true);
        collector.record_op(MetricOp::Close, 50, true);
        collector.record_cache_hit();

        let snapshot = collector.snapshot(10, 20, 30, 1000);
        assert_eq!(snapshot.total_ops, 2);
        assert_eq!(snapshot.total_errors, 0);
        assert_eq!(snapshot.active_leases, 10);
        assert_eq!(snapshot.active_watches, 20);
        assert_eq!(snapshot.active_file_handles, 30);
        assert_eq!(snapshot.inode_count, 1000);
        assert_eq!(snapshot.cache_hits, 1);
    }

    #[test]
    fn test_reset() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Rename, 500, true);
        collector.record_cache_hit();

        collector.reset();

        let metrics = collector.get_op_metrics(&MetricOp::Rename);
        assert_eq!(metrics.count, 0);

        let snapshot = collector.snapshot(0, 0, 0, 0);
        assert_eq!(snapshot.cache_hits, 0);
    }

    #[test]
    fn test_max_duration_tracking() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Readdir, 100, true);
        collector.record_op(MetricOp::Readdir, 500, true);
        collector.record_op(MetricOp::Readdir, 200, true);

        let metrics = collector.get_op_metrics(&MetricOp::Readdir);
        assert_eq!(metrics.max_duration_us, 500);
    }

    #[test]
    fn test_multiple_op_types() {
        let collector = MetricsCollector::new();
        collector.record_op(MetricOp::Lookup, 100, true);
        collector.record_op(MetricOp::Getattr, 50, true);
        collector.record_op(MetricOp::Setattr, 75, false);
        collector.record_op(MetricOp::CreateFile, 200, true);

        let snapshot = collector.snapshot(0, 0, 0, 0);
        assert_eq!(snapshot.total_ops, 4);
        assert_eq!(snapshot.total_errors, 1);
        assert!(snapshot.ops.contains_key(&MetricOp::Lookup));
        assert!(snapshot.ops.contains_key(&MetricOp::Getattr));
        assert!(snapshot.ops.contains_key(&MetricOp::Setattr));
        assert!(snapshot.ops.contains_key(&MetricOp::CreateFile));
    }
}
