//! FUSE layer performance metrics.
//!
//! Tracks operation latencies and throughput for observability.
//! Metrics are exposed for consumption by the management layer (A8).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct OpCounters {
    pub lookups: AtomicU64,
    pub reads: AtomicU64,
    pub writes: AtomicU64,
    pub creates: AtomicU64,
    pub unlinks: AtomicU64,
    pub mkdirs: AtomicU64,
    pub rmdirs: AtomicU64,
    pub renames: AtomicU64,
    pub getattrs: AtomicU64,
    pub setattrs: AtomicU64,
    pub readdirs: AtomicU64,
    pub errors: AtomicU64,
}

#[derive(Default)]
pub struct ByteCounters {
    pub bytes_read: AtomicU64,
    pub bytes_written: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct LatencyHistogram {
    pub buckets: [u64; 6],
    pub total_us: u64,
    pub count: u64,
}

impl LatencyHistogram {
    pub fn record(&mut self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.total_us += us;
        self.count += 1;

        let bucket = match us {
            0..=10 => 0,
            11..=100 => 1,
            101..=1000 => 2,
            1001..=10_000 => 3,
            10_001..=100_000 => 4,
            _ => 5,
        };
        self.buckets[bucket] += 1;
    }

    pub fn p50_us(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        self.total_us / self.count
    }

    pub fn p99_us(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = (self.count * 99) / 100;
        let mut cumulative = 0u64;
        for (i, &count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return match i {
                    0 => 10,
                    1 => 100,
                    2 => 1000,
                    3 => 10_000,
                    4 => 100_000,
                    _ => 200_000,
                };
            }
        }
        200_000
    }

    pub fn mean_us(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        self.total_us / self.count
    }
}

pub struct FuseMetrics {
    pub ops: Arc<OpCounters>,
    pub bytes: Arc<ByteCounters>,
}

impl FuseMetrics {
    pub fn new() -> Self {
        Self {
            ops: Arc::new(OpCounters::default()),
            bytes: Arc::new(ByteCounters::default()),
        }
    }

    pub fn inc_lookup(&self) {
        self.ops.lookups.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_read(&self, bytes: u64) {
        self.ops.reads.fetch_add(1, Ordering::Relaxed);
        self.bytes.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn inc_write(&self, bytes: u64) {
        self.ops.writes.fetch_add(1, Ordering::Relaxed);
        self.bytes.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn inc_create(&self) {
        self.ops.creates.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_unlink(&self) {
        self.ops.unlinks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_mkdir(&self) {
        self.ops.mkdirs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rmdir(&self) {
        self.ops.rmdirs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rename(&self) {
        self.ops.renames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_getattr(&self) {
        self.ops.getattrs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_setattr(&self) {
        self.ops.setattrs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_readdir(&self) {
        self.ops.readdirs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_error(&self) {
        self.ops.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            lookups: self.ops.lookups.load(Ordering::Relaxed),
            reads: self.ops.reads.load(Ordering::Relaxed),
            writes: self.ops.writes.load(Ordering::Relaxed),
            creates: self.ops.creates.load(Ordering::Relaxed),
            unlinks: self.ops.unlinks.load(Ordering::Relaxed),
            mkdirs: self.ops.mkdirs.load(Ordering::Relaxed),
            rmdirs: self.ops.rmdirs.load(Ordering::Relaxed),
            renames: self.ops.renames.load(Ordering::Relaxed),
            getattrs: self.ops.getattrs.load(Ordering::Relaxed),
            setattrs: self.ops.setattrs.load(Ordering::Relaxed),
            readdirs: self.ops.readdirs.load(Ordering::Relaxed),
            errors: self.ops.errors.load(Ordering::Relaxed),
            bytes_read: self.bytes.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes.bytes_written.load(Ordering::Relaxed),
        }
    }
}

impl Default for FuseMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MetricsSnapshot {
    pub lookups: u64,
    pub reads: u64,
    pub writes: u64,
    pub creates: u64,
    pub unlinks: u64,
    pub mkdirs: u64,
    pub rmdirs: u64,
    pub renames: u64,
    pub getattrs: u64,
    pub setattrs: u64,
    pub readdirs: u64,
    pub errors: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

pub struct OpTimer {
    start: Instant,
}

impl OpTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Default for OpTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuse_metrics_new_creates_zero_counters() {
        let metrics = FuseMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.lookups, 0);
        assert_eq!(snapshot.reads, 0);
        assert_eq!(snapshot.writes, 0);
    }

    #[test]
    fn test_inc_lookup_increments_lookups() {
        let metrics = FuseMetrics::new();
        metrics.inc_lookup();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.lookups, 1);
    }

    #[test]
    fn test_inc_read_increments_reads_and_bytes_read() {
        let metrics = FuseMetrics::new();
        metrics.inc_read(4096);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.reads, 1);
        assert_eq!(snapshot.bytes_read, 4096);
    }

    #[test]
    fn test_inc_write_increments_writes_and_bytes_written() {
        let metrics = FuseMetrics::new();
        metrics.inc_write(2048);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.writes, 1);
        assert_eq!(snapshot.bytes_written, 2048);
    }

    #[test]
    fn test_inc_error_increments_errors() {
        let metrics = FuseMetrics::new();
        metrics.inc_error();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.errors, 1);
    }

    #[test]
    fn test_snapshot_captures_all_counter_values() {
        let metrics = FuseMetrics::new();
        metrics.inc_lookup();
        metrics.inc_create();
        metrics.inc_mkdir();
        metrics.inc_read(100);
        metrics.inc_write(200);

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.lookups, 1);
        assert_eq!(snapshot.creates, 1);
        assert_eq!(snapshot.mkdirs, 1);
        assert_eq!(snapshot.bytes_read, 100);
        assert_eq!(snapshot.bytes_written, 200);
    }

    #[test]
    fn test_latency_histogram_record_bins_correctly() {
        let mut hist = LatencyHistogram::default();

        hist.record(Duration::from_micros(5));
        hist.record(Duration::from_micros(50));
        hist.record(Duration::from_micros(500));
        hist.record(Duration::from_micros(5000));
        hist.record(Duration::from_micros(50000));
        hist.record(Duration::from_micros(500000));

        assert_eq!(hist.buckets[0], 1);
        assert_eq!(hist.buckets[1], 1);
        assert_eq!(hist.buckets[2], 1);
        assert_eq!(hist.buckets[3], 1);
        assert_eq!(hist.buckets[4], 1);
        assert_eq!(hist.buckets[5], 1);
    }

    #[test]
    fn test_latency_histogram_mean_us_with_known_values() {
        let mut hist = LatencyHistogram::default();

        hist.record(Duration::from_micros(100));
        hist.record(Duration::from_micros(200));

        assert_eq!(hist.mean_us(), 150);
    }

    #[test]
    fn test_op_timer_elapsed_us_returns_positive_value() {
        let timer = OpTimer::new();
        std::thread::sleep(Duration::from_micros(100));

        let elapsed = timer.elapsed_us();
        assert!(elapsed > 0);
    }

    #[test]
    fn test_multiple_concurrent_inc_calls() {
        let metrics = FuseMetrics::new();

        metrics.inc_lookup();
        metrics.inc_lookup();
        metrics.inc_read(100);
        metrics.inc_read(200);
        metrics.inc_create();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.lookups, 2);
        assert_eq!(snapshot.reads, 2);
        assert_eq!(snapshot.bytes_read, 300);
        assert_eq!(snapshot.creates, 1);
    }

    #[test]
    fn test_metrics_snapshot_default_is_all_zeros() {
        let snapshot = MetricsSnapshot::default();

        assert_eq!(snapshot.lookups, 0);
        assert_eq!(snapshot.reads, 0);
        assert_eq!(snapshot.writes, 0);
        assert_eq!(snapshot.creates, 0);
        assert_eq!(snapshot.bytes_read, 0);
        assert_eq!(snapshot.bytes_written, 0);
    }

    #[test]
    fn test_snapshot_after_multiple_ops() {
        let metrics = FuseMetrics::new();

        for _ in 0..5 {
            metrics.inc_lookup();
        }
        for _ in 0..3 {
            metrics.inc_create();
        }
        metrics.inc_read(1024);

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.lookups, 5);
        assert_eq!(snapshot.creates, 3);
        assert_eq!(snapshot.reads, 1);
        assert_eq!(snapshot.bytes_read, 1024);
    }

    #[test]
    fn test_inc_unlink_increments_unlinks() {
        let metrics = FuseMetrics::new();
        metrics.inc_unlink();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.unlinks, 1);
    }

    #[test]
    fn test_inc_rmdir_increments_rmdirs() {
        let metrics = FuseMetrics::new();
        metrics.inc_rmdir();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.rmdirs, 1);
    }

    #[test]
    fn test_inc_rename_increments_renames() {
        let metrics = FuseMetrics::new();
        metrics.inc_rename();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.renames, 1);
    }

    #[test]
    fn test_inc_getattr_increments_getattrs() {
        let metrics = FuseMetrics::new();
        metrics.inc_getattr();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.getattrs, 1);
    }

    #[test]
    fn test_inc_setattr_increments_setattrs() {
        let metrics = FuseMetrics::new();
        metrics.inc_setattr();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.setattrs, 1);
    }

    #[test]
    fn test_inc_readdir_increments_readdirs() {
        let metrics = FuseMetrics::new();
        metrics.inc_readdir();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.readdirs, 1);
    }

    #[test]
    fn test_latency_histogram_p99_approximation() {
        let mut hist = LatencyHistogram::default();

        for _ in 0..100 {
            hist.record(Duration::from_micros(5000));
        }

        let p99 = hist.p99_us();
        assert!(p99 > 0);
    }

    #[test]
    fn test_op_timer_elapsed_returns_duration() {
        let timer = OpTimer::new();
        std::thread::sleep(Duration::from_micros(100));

        let elapsed = timer.elapsed();
        assert!(elapsed.as_micros() > 0);
    }
}
