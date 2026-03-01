//! Prometheus-compatible metrics for the ClaudeFS data reduction pipeline.
//!
//! Provides atomic counters for tracking deduplication, compression, encryption,
//! and garbage collection operations. Integrates with the A8 management crate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The type of metric: counter, gauge, or histogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    /// A monotonically increasing counter.
    Counter,
    /// A gauge that can go up or down.
    Gauge,
    /// A histogram with buckets.
    Histogram,
}

/// The value of a metric.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    /// A counter value.
    Counter(u64),
    /// A gauge value.
    Gauge(f64),
    /// A histogram with sum, count, and bucket boundaries.
    Histogram {
        /// Sum of all observed values.
        sum: f64,
        /// Total count of observations.
        count: u64,
        /// Bucket boundaries and counts.
        buckets: Vec<(f64, u64)>,
    },
}

/// A single metric with metadata and value.
#[derive(Debug, Clone, PartialEq)]
pub struct ReduceMetric {
    /// The metric name.
    pub name: String,
    /// Help text describing the metric.
    pub help: String,
    /// The kind of metric.
    pub kind: MetricKind,
    /// The metric value.
    pub value: MetricValue,
}

/// Thread-safe metrics collection for the reduction pipeline.
///
/// Tracks all data reduction operations atomically using `AtomicU64` for
/// lock-free concurrent access from multiple threads.
pub struct ReductionMetrics {
    /// Total chunks processed through the pipeline.
    chunks_processed: AtomicU64,
    /// Raw bytes entering the pipeline.
    bytes_in: AtomicU64,
    /// Bytes after reduction.
    bytes_out: AtomicU64,
    /// Deduplication cache hits (exact matches).
    dedup_hits: AtomicU64,
    /// Deduplication misses (new unique chunks).
    dedup_misses: AtomicU64,
    /// Bytes sent to compressor.
    compress_bytes_in: AtomicU64,
    /// Bytes after compression.
    compress_bytes_out: AtomicU64,
    /// Encryption operations performed.
    encrypt_ops: AtomicU64,
    /// Garbage collection cycles completed.
    gc_cycles: AtomicU64,
    /// Bytes freed by garbage collection.
    gc_bytes_freed: AtomicU64,
    /// Key rotation events.
    key_rotations: AtomicU64,
}

impl ReductionMetrics {
    /// Create a new ReductionMetrics with all counters initialized to zero.
    #[inline]
    pub fn new() -> Self {
        Self {
            chunks_processed: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            dedup_hits: AtomicU64::new(0),
            dedup_misses: AtomicU64::new(0),
            compress_bytes_in: AtomicU64::new(0),
            compress_bytes_out: AtomicU64::new(0),
            encrypt_ops: AtomicU64::new(0),
            gc_cycles: AtomicU64::new(0),
            gc_bytes_freed: AtomicU64::new(0),
            key_rotations: AtomicU64::new(0),
        }
    }

    /// Record a chunk processed through the pipeline.
    ///
    /// Increments `chunks_processed`, `bytes_in`, and `bytes_out` counters.
    #[inline]
    pub fn record_chunk(&self, bytes_in: u64, bytes_out: u64) {
        self.chunks_processed.fetch_add(1, Ordering::Relaxed);
        self.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
    }

    /// Record a deduplication cache hit.
    #[inline]
    pub fn record_dedup_hit(&self) {
        self.dedup_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a deduplication cache miss (new unique chunk).
    #[inline]
    pub fn record_dedup_miss(&self) {
        self.dedup_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record compression operation stats.
    #[inline]
    pub fn record_compress(&self, bytes_in: u64, bytes_out: u64) {
        self.compress_bytes_in
            .fetch_add(bytes_in, Ordering::Relaxed);
        self.compress_bytes_out
            .fetch_add(bytes_out, Ordering::Relaxed);
    }

    /// Record an encryption operation.
    #[inline]
    pub fn record_encrypt(&self) {
        self.encrypt_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a garbage collection cycle.
    #[inline]
    pub fn record_gc_cycle(&self, bytes_freed: u64) {
        self.gc_cycles.fetch_add(1, Ordering::Relaxed);
        self.gc_bytes_freed
            .fetch_add(bytes_freed, Ordering::Relaxed);
    }

    /// Record a key rotation event.
    #[inline]
    pub fn record_key_rotation(&self) {
        self.key_rotations.fetch_add(1, Ordering::Relaxed);
    }

    /// Calculate the deduplication hit ratio.
    ///
    /// Returns 0.0 if no deduplication operations have occurred.
    #[inline]
    pub fn dedup_ratio(&self) -> f64 {
        let hits = self.dedup_hits.load(Ordering::Relaxed);
        let misses = self.dedup_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Calculate the compression ratio.
    ///
    /// Returns 1.0 if no compression operations have occurred.
    #[inline]
    pub fn compression_ratio(&self) -> f64 {
        let bytes_in = self.compress_bytes_in.load(Ordering::Relaxed);
        let bytes_out = self.compress_bytes_out.load(Ordering::Relaxed);
        if bytes_out == 0 {
            1.0
        } else {
            bytes_in as f64 / bytes_out as f64
        }
    }

    /// Calculate the overall data reduction ratio.
    ///
    /// Returns 1.0 if no bytes have been processed (bytes_out == 0).
    #[inline]
    pub fn overall_reduction_ratio(&self) -> f64 {
        let bytes_in = self.bytes_in.load(Ordering::Relaxed);
        let bytes_out = self.bytes_out.load(Ordering::Relaxed);
        if bytes_out == 0 {
            1.0
        } else {
            bytes_in as f64 / bytes_out as f64
        }
    }

    /// Collect all metrics as a vector of ReduceMetric structs.
    ///
    /// Returns metrics with proper names, help text, and values formatted
    /// for Prometheus compatibility.
    pub fn collect(&self) -> Vec<ReduceMetric> {
        let chunks_processed = self.chunks_processed.load(Ordering::Relaxed);
        let bytes_in = self.bytes_in.load(Ordering::Relaxed);
        let bytes_out = self.bytes_out.load(Ordering::Relaxed);
        let dedup_hits = self.dedup_hits.load(Ordering::Relaxed);
        let dedup_misses = self.dedup_misses.load(Ordering::Relaxed);
        let compress_bytes_in = self.compress_bytes_in.load(Ordering::Relaxed);
        let compress_bytes_out = self.compress_bytes_out.load(Ordering::Relaxed);
        let encrypt_ops = self.encrypt_ops.load(Ordering::Relaxed);
        let gc_cycles = self.gc_cycles.load(Ordering::Relaxed);
        let gc_bytes_freed = self.gc_bytes_freed.load(Ordering::Relaxed);
        let key_rotations = self.key_rotations.load(Ordering::Relaxed);

        vec![
            ReduceMetric {
                name: "claudefs_reduce_chunks_processed_total".to_string(),
                help: "Total number of chunks processed through the reduction pipeline".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(chunks_processed),
            },
            ReduceMetric {
                name: "claudefs_reduce_bytes_in_total".to_string(),
                help: "Total raw bytes entering the reduction pipeline".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(bytes_in),
            },
            ReduceMetric {
                name: "claudefs_reduce_bytes_out_total".to_string(),
                help: "Total bytes after reduction (dedupe + compress + encrypt)".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(bytes_out),
            },
            ReduceMetric {
                name: "claudefs_reduce_dedup_hits_total".to_string(),
                help: "Total deduplication cache hits (exact matches)".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(dedup_hits),
            },
            ReduceMetric {
                name: "claudefs_reduce_dedup_misses_total".to_string(),
                help: "Total deduplication cache misses (new unique chunks)".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(dedup_misses),
            },
            ReduceMetric {
                name: "claudefs_reduce_dedup_ratio".to_string(),
                help: "Deduplication hit ratio (hits / (hits + misses))".to_string(),
                kind: MetricKind::Gauge,
                value: MetricValue::Gauge(self.dedup_ratio()),
            },
            ReduceMetric {
                name: "claudefs_reduce_compress_bytes_in_total".to_string(),
                help: "Total bytes fed to the compressor".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(compress_bytes_in),
            },
            ReduceMetric {
                name: "claudefs_reduce_compress_bytes_out_total".to_string(),
                help: "Total bytes after compression".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(compress_bytes_out),
            },
            ReduceMetric {
                name: "claudefs_reduce_compression_ratio".to_string(),
                help: "Compression ratio (bytes_in / bytes_out)".to_string(),
                kind: MetricKind::Gauge,
                value: MetricValue::Gauge(self.compression_ratio()),
            },
            ReduceMetric {
                name: "claudefs_reduce_encrypt_ops_total".to_string(),
                help: "Total encryption operations performed".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(encrypt_ops),
            },
            ReduceMetric {
                name: "claudefs_reduce_gc_cycles_total".to_string(),
                help: "Total garbage collection cycles completed".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(gc_cycles),
            },
            ReduceMetric {
                name: "claudefs_reduce_gc_bytes_freed_total".to_string(),
                help: "Total bytes freed by garbage collection".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(gc_bytes_freed),
            },
            ReduceMetric {
                name: "claudefs_reduce_key_rotations_total".to_string(),
                help: "Total key rotation events".to_string(),
                kind: MetricKind::Counter,
                value: MetricValue::Counter(key_rotations),
            },
            ReduceMetric {
                name: "claudefs_reduce_overall_reduction_ratio".to_string(),
                help: "Overall data reduction ratio (bytes_in / bytes_out)".to_string(),
                kind: MetricKind::Gauge,
                value: MetricValue::Gauge(self.overall_reduction_ratio()),
            },
        ]
    }
}

impl Default for ReductionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle for accessing reduction metrics.
///
/// Provides a shareable reference to `ReductionMetrics` wrapped in an Arc.
pub struct MetricsHandle {
    inner: Arc<ReductionMetrics>,
}

impl MetricsHandle {
    /// Create a new MetricsHandle with fresh metrics.
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ReductionMetrics::new()),
        }
    }

    /// Get a reference to the underlying ReductionMetrics.
    #[inline]
    pub fn metrics(&self) -> Arc<ReductionMetrics> {
        Arc::clone(&self.inner)
    }

    /// Take a point-in-time snapshot of all metrics.
    #[inline]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot::from_metrics(&self.inner)
    }
}

impl Default for MetricsHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MetricsHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// A point-in-time snapshot of all reduction metrics.
///
/// All values are captured atomically at construction time using
/// `Ordering::Relaxed` for efficient reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total chunks processed through the pipeline.
    pub chunks_processed: u64,
    /// Raw bytes entering the pipeline.
    pub bytes_in: u64,
    /// Bytes after reduction.
    pub bytes_out: u64,
    /// Deduplication cache hits.
    pub dedup_hits: u64,
    /// Deduplication cache misses.
    pub dedup_misses: u64,
    /// Bytes sent to compressor.
    pub compress_bytes_in: u64,
    /// Bytes after compression.
    pub compress_bytes_out: u64,
    /// Encryption operations performed.
    pub encrypt_ops: u64,
    /// Garbage collection cycles completed.
    pub gc_cycles: u64,
    /// Bytes freed by garbage collection.
    pub gc_bytes_freed: u64,
    /// Key rotation events.
    pub key_rotations: u64,
    /// Deduplication hit ratio.
    pub dedup_ratio: f64,
    /// Compression ratio.
    pub compression_ratio: f64,
    /// Overall data reduction ratio.
    pub overall_reduction_ratio: f64,
}

impl MetricsSnapshot {
    /// Create a snapshot from the given metrics.
    #[inline]
    fn from_metrics(metrics: &ReductionMetrics) -> Self {
        let chunks_processed = metrics.chunks_processed.load(Ordering::Relaxed);
        let bytes_in = metrics.bytes_in.load(Ordering::Relaxed);
        let bytes_out = metrics.bytes_out.load(Ordering::Relaxed);
        let dedup_hits = metrics.dedup_hits.load(Ordering::Relaxed);
        let dedup_misses = metrics.dedup_misses.load(Ordering::Relaxed);
        let compress_bytes_in = metrics.compress_bytes_in.load(Ordering::Relaxed);
        let compress_bytes_out = metrics.compress_bytes_out.load(Ordering::Relaxed);
        let encrypt_ops = metrics.encrypt_ops.load(Ordering::Relaxed);
        let gc_cycles = metrics.gc_cycles.load(Ordering::Relaxed);
        let gc_bytes_freed = metrics.gc_bytes_freed.load(Ordering::Relaxed);
        let key_rotations = metrics.key_rotations.load(Ordering::Relaxed);

        let total_dedup = dedup_hits.saturating_add(dedup_misses);
        let dedup_ratio = if total_dedup == 0 {
            0.0
        } else {
            dedup_hits as f64 / total_dedup as f64
        };

        let compression_ratio = if compress_bytes_out == 0 {
            1.0
        } else {
            compress_bytes_in as f64 / compress_bytes_out as f64
        };

        let overall_reduction_ratio = if bytes_out == 0 {
            1.0
        } else {
            bytes_in as f64 / bytes_out as f64
        };

        Self {
            chunks_processed,
            bytes_in,
            bytes_out,
            dedup_hits,
            dedup_misses,
            compress_bytes_in,
            compress_bytes_out,
            encrypt_ops,
            gc_cycles,
            gc_bytes_freed,
            key_rotations,
            dedup_ratio,
            compression_ratio,
            overall_reduction_ratio,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics() {
        let metrics = ReductionMetrics::new();
        assert_eq!(metrics.chunks_processed.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bytes_in.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bytes_out.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.dedup_hits.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.dedup_misses.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.compress_bytes_in.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.compress_bytes_out.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.encrypt_ops.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.gc_cycles.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.gc_bytes_freed.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.key_rotations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_record_chunk() {
        let metrics = ReductionMetrics::new();
        metrics.record_chunk(1000, 500);

        assert_eq!(metrics.chunks_processed.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.bytes_in.load(Ordering::Relaxed), 1000);
        assert_eq!(metrics.bytes_out.load(Ordering::Relaxed), 500);
    }

    #[test]
    fn test_record_dedup_hit_and_miss() {
        let metrics = ReductionMetrics::new();
        metrics.record_dedup_hit();
        metrics.record_dedup_hit();
        metrics.record_dedup_miss();

        assert_eq!(metrics.dedup_hits.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.dedup_misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_dedup_ratio_no_ops() {
        let metrics = ReductionMetrics::new();
        assert_eq!(metrics.dedup_ratio(), 0.0);
    }

    #[test]
    fn test_dedup_ratio_all_hits() {
        let metrics = ReductionMetrics::new();
        metrics.record_dedup_hit();
        metrics.record_dedup_hit();
        metrics.record_dedup_hit();

        assert_eq!(metrics.dedup_ratio(), 1.0);
    }

    #[test]
    fn test_dedup_ratio_half() {
        let metrics = ReductionMetrics::new();
        metrics.record_dedup_hit();
        metrics.record_dedup_miss();

        assert!((metrics.dedup_ratio() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_record_compress() {
        let metrics = ReductionMetrics::new();
        metrics.record_compress(200, 100);
        metrics.record_compress(300, 150);

        assert_eq!(metrics.compress_bytes_in.load(Ordering::Relaxed), 500);
        assert_eq!(metrics.compress_bytes_out.load(Ordering::Relaxed), 250);
    }

    #[test]
    fn test_compression_ratio_no_ops() {
        let metrics = ReductionMetrics::new();
        assert_eq!(metrics.compression_ratio(), 1.0);
    }

    #[test]
    fn test_compression_ratio_2x() {
        let metrics = ReductionMetrics::new();
        metrics.record_compress(200, 100);

        assert!((metrics.compression_ratio() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_record_encrypt() {
        let metrics = ReductionMetrics::new();
        metrics.record_encrypt();
        metrics.record_encrypt();
        metrics.record_encrypt();

        assert_eq!(metrics.encrypt_ops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_record_gc_cycle() {
        let metrics = ReductionMetrics::new();
        metrics.record_gc_cycle(1000);
        metrics.record_gc_cycle(2000);

        assert_eq!(metrics.gc_cycles.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.gc_bytes_freed.load(Ordering::Relaxed), 3000);
    }

    #[test]
    fn test_record_key_rotation() {
        let metrics = ReductionMetrics::new();
        metrics.record_key_rotation();
        metrics.record_key_rotation();

        assert_eq!(metrics.key_rotations.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_overall_reduction_ratio_no_ops() {
        let metrics = ReductionMetrics::new();
        assert_eq!(metrics.overall_reduction_ratio(), 1.0);
    }

    #[test]
    fn test_overall_reduction_ratio() {
        let metrics = ReductionMetrics::new();
        metrics.record_chunk(1000, 250);

        assert!((metrics.overall_reduction_ratio() - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_collect_returns_metrics() {
        let metrics = ReductionMetrics::new();
        let collected = metrics.collect();

        assert!(!collected.is_empty());
    }

    #[test]
    fn test_collect_metric_names() {
        let metrics = ReductionMetrics::new();
        metrics.record_chunk(100, 50);
        metrics.record_dedup_hit();
        metrics.record_dedup_miss();
        metrics.record_compress(100, 50);
        metrics.record_encrypt();
        metrics.record_gc_cycle(100);
        metrics.record_key_rotation();

        let collected = metrics.collect();
        let names: Vec<_> = collected.iter().map(|m| m.name.clone()).collect();

        assert!(names.contains(&"claudefs_reduce_chunks_processed_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_bytes_in_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_bytes_out_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_dedup_hits_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_dedup_misses_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_compress_bytes_in_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_compress_bytes_out_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_encrypt_ops_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_gc_cycles_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_gc_bytes_freed_total".to_string()));
        assert!(names.contains(&"claudefs_reduce_key_rotations_total".to_string()));
    }

    #[test]
    fn test_metrics_handle_new() {
        let handle = MetricsHandle::new();
        let metrics = handle.metrics();

        assert_eq!(metrics.chunks_processed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = ReductionMetrics::new();
        metrics.record_chunk(1000, 500);
        metrics.record_dedup_hit();
        metrics.record_dedup_miss();
        metrics.record_compress(1000, 500);
        metrics.record_encrypt();
        metrics.record_gc_cycle(100);
        metrics.record_key_rotation();

        let handle = MetricsHandle {
            inner: Arc::new(metrics),
        };
        let snapshot = handle.snapshot();

        assert_eq!(snapshot.chunks_processed, 1);
        assert_eq!(snapshot.bytes_in, 1000);
        assert_eq!(snapshot.bytes_out, 500);
        assert_eq!(snapshot.dedup_hits, 1);
        assert_eq!(snapshot.dedup_misses, 1);
        assert_eq!(snapshot.compress_bytes_in, 1000);
        assert_eq!(snapshot.compress_bytes_out, 500);
        assert_eq!(snapshot.encrypt_ops, 1);
        assert_eq!(snapshot.gc_cycles, 1);
        assert_eq!(snapshot.gc_bytes_freed, 100);
        assert_eq!(snapshot.key_rotations, 1);
    }

    #[test]
    fn test_snapshot_ratios() {
        let metrics = ReductionMetrics::new();
        metrics.record_dedup_hit();
        metrics.record_dedup_miss();
        metrics.record_compress(200, 100);
        metrics.record_chunk(1000, 250);

        let handle = MetricsHandle {
            inner: Arc::new(metrics),
        };
        let snapshot = handle.snapshot();

        assert!((snapshot.dedup_ratio - 0.5).abs() < 0.001);
        assert!((snapshot.compression_ratio - 2.0).abs() < 0.001);
        assert!((snapshot.overall_reduction_ratio - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_thread_safety() {
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

        assert_eq!(metrics.chunks_processed.load(Ordering::Relaxed), 400);
    }
}
