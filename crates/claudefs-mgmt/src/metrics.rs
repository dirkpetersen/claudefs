use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Gauge {
    value: Arc<AtomicU64>,
}

impl Gauge {
    pub fn new() -> Self {
        Self {
            value: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set(&self, value: f64) {
        let bits = value.to_bits();
        self.value.store(bits, Ordering::Relaxed);
    }

    pub fn get(&self) -> f64 {
        let bits = self.value.load(Ordering::Relaxed);
        f64::from_bits(bits)
    }

    pub fn inc(&self) {
        let current = f64::from_bits(self.value.load(Ordering::Relaxed));
        self.set(current + 1.0);
    }

    pub fn dec(&self) {
        let current = f64::from_bits(self.value.load(Ordering::Relaxed));
        self.set(current - 1.0);
    }

    pub fn add(&self, delta: f64) {
        let current = f64::from_bits(self.value.load(Ordering::Relaxed));
        self.set(current + delta);
    }

    pub fn sub(&self, delta: f64) {
        let current = f64::from_bits(self.value.load(Ordering::Relaxed));
        self.set(current - delta);
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Gauge {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
        }
    }
}

pub struct Counter {
    value: Arc<AtomicU64>,
    #[allow(dead_code)]
    name: String,
}

impl Counter {
    pub fn new(name: &str) -> Self {
        Self {
            value: Arc::new(AtomicU64::new(0)),
            name: name.to_string(),
        }
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add(&self, delta: u64) {
        self.value.fetch_add(delta, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Clone for Counter {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
            name: self.name.clone(),
        }
    }
}

const BUCKET_BOUNDARIES: &[f64] = &[100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0];

pub struct Histogram {
    buckets: Vec<Arc<AtomicU64>>,
    sum: Arc<AtomicU64>,
    count: Arc<AtomicU64>,
    #[allow(dead_code)]
    name: String,
}

impl Histogram {
    pub fn new(name: &str) -> Self {
        let buckets: Vec<Arc<AtomicU64>> = (0..=BUCKET_BOUNDARIES.len())
            .map(|_| Arc::new(AtomicU64::new(0)))
            .collect();

        Self {
            buckets,
            sum: Arc::new(AtomicU64::new(0)),
            count: Arc::new(AtomicU64::new(0)),
            name: name.to_string(),
        }
    }

    pub fn observe(&self, value: f64) {
        let bits = value.to_bits();
        self.sum.fetch_add(bits, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        for (i, boundary) in BUCKET_BOUNDARIES.iter().enumerate() {
            if value <= *boundary {
                self.buckets[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
        if value > *BUCKET_BOUNDARIES.last().unwrap() {
            self.buckets.last().unwrap().fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn bucket_count(&self, bucket_idx: usize) -> u64 {
        if bucket_idx < self.buckets.len() {
            self.buckets[bucket_idx].load(Ordering::Relaxed)
        } else {
            0
        }
    }

    pub fn sum(&self) -> f64 {
        let bits = self.sum.load(Ordering::Relaxed);
        f64::from_bits(bits)
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

impl Clone for Histogram {
    fn clone(&self) -> Self {
        Self {
            buckets: self.buckets.clone(),
            sum: Arc::clone(&self.sum),
            count: Arc::clone(&self.count),
            name: self.name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeMetricsSnapshot {
    pub node_id: String,
    pub iops_read: u64,
    pub iops_write: u64,
    pub bytes_read: u64,
    pub bytes_write: u64,
    pub latency_read_us_p50: f64,
    pub latency_read_us_p99: f64,
    pub latency_write_us_p50: f64,
    pub latency_write_us_p99: f64,
    pub capacity_total_bytes: u64,
    pub capacity_used_bytes: u64,
    pub is_healthy: bool,
    pub replication_lag_secs: f64,
    pub dedupe_hit_rate: f64,
    pub compression_ratio: f64,
    pub s3_queue_depth: u64,
    pub timestamp: u64,
}

pub struct ClusterMetrics {
    pub iops_read: Counter,
    pub iops_write: Counter,
    pub bytes_read: Counter,
    pub bytes_write: Counter,
    pub latency_read_us: Histogram,
    pub latency_write_us: Histogram,

    pub capacity_total_bytes: Gauge,
    pub capacity_used_bytes: Gauge,
    pub capacity_available_bytes: Gauge,

    pub nodes_total: Gauge,
    pub nodes_healthy: Gauge,
    pub nodes_degraded: Gauge,
    pub nodes_offline: Gauge,

    pub replication_lag_secs: Gauge,
    pub replication_conflicts_total: Counter,

    pub dedupe_hit_rate: Gauge,
    pub compression_ratio: Gauge,

    pub s3_queue_depth: Gauge,
    pub s3_flush_latency_ms: Histogram,
}

impl ClusterMetrics {
    pub fn new() -> Self {
        Self {
            iops_read: Counter::new("claudefs_iops_read_total"),
            iops_write: Counter::new("claudefs_iops_write_total"),
            bytes_read: Counter::new("claudefs_bytes_read_total"),
            bytes_write: Counter::new("claudefs_bytes_write_total"),
            latency_read_us: Histogram::new("claudefs_latency_read_us"),
            latency_write_us: Histogram::new("claudefs_latency_write_us"),

            capacity_total_bytes: Gauge::new(),
            capacity_used_bytes: Gauge::new(),
            capacity_available_bytes: Gauge::new(),

            nodes_total: Gauge::new(),
            nodes_healthy: Gauge::new(),
            nodes_degraded: Gauge::new(),
            nodes_offline: Gauge::new(),

            replication_lag_secs: Gauge::new(),
            replication_conflicts_total: Counter::new("claudefs_replication_conflicts_total"),

            dedupe_hit_rate: Gauge::new(),
            compression_ratio: Gauge::new(),

            s3_queue_depth: Gauge::new(),
            s3_flush_latency_ms: Histogram::new("claudefs_s3_flush_latency_ms"),
        }
    }

    pub fn render_prometheus(&self) -> String {
        let mut output = String::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        output.push_str("# TYPE claudefs_iops_read_total counter\n");
        output.push_str(&format!(
            "claudefs_iops_read_total {} {}\n",
            self.iops_read.get(),
            now
        ));

        output.push_str("# TYPE claudefs_iops_write_total counter\n");
        output.push_str(&format!(
            "claudefs_iops_write_total {} {}\n",
            self.iops_write.get(),
            now
        ));

        output.push_str("# TYPE claudefs_bytes_read_total counter\n");
        output.push_str(&format!(
            "claudefs_bytes_read_total {} {}\n",
            self.bytes_read.get(),
            now
        ));

        output.push_str("# TYPE claudefs_bytes_write_total counter\n");
        output.push_str(&format!(
            "claudefs_bytes_write_total {} {}\n",
            self.bytes_write.get(),
            now
        ));

        output.push_str("# TYPE claudefs_latency_read_us histogram\n");
        for (i, boundary) in BUCKET_BOUNDARIES.iter().enumerate() {
            output.push_str(&format!(
                "claudefs_latency_read_us_bucket{{le={}}} {}\n",
                boundary,
                self.latency_read_us.bucket_count(i)
            ));
        }
        output.push_str(&format!(
            "claudefs_latency_read_us_bucket{{le=\"+Inf\"}} {}\n",
            self.latency_read_us.bucket_count(BUCKET_BOUNDARIES.len())
        ));
        output.push_str(&format!(
            "claudefs_latency_read_us_sum {}\n",
            self.latency_read_us.sum()
        ));
        output.push_str(&format!(
            "claudefs_latency_read_us_count {}\n\n",
            self.latency_read_us.count()
        ));

        output.push_str("# TYPE claudefs_latency_write_us histogram\n");
        for (i, boundary) in BUCKET_BOUNDARIES.iter().enumerate() {
            output.push_str(&format!(
                "claudefs_latency_write_us_bucket{{le={}}} {}\n",
                boundary,
                self.latency_write_us.bucket_count(i)
            ));
        }
        output.push_str(&format!(
            "claudefs_latency_write_us_bucket{{le=\"+Inf\"}} {}\n",
            self.latency_write_us.bucket_count(BUCKET_BOUNDARIES.len())
        ));
        output.push_str(&format!(
            "claudefs_latency_write_us_sum {}\n",
            self.latency_write_us.sum()
        ));
        output.push_str(&format!(
            "claudefs_latency_write_us_count {}\n\n",
            self.latency_write_us.count()
        ));

        output.push_str("# TYPE claudefs_capacity_total_bytes gauge\n");
        output.push_str(&format!(
            "claudefs_capacity_total_bytes {}\n\n",
            self.capacity_total_bytes.get()
        ));

        output.push_str("# TYPE claudefs_capacity_used_bytes gauge\n");
        output.push_str(&format!(
            "claudefs_capacity_used_bytes {}\n\n",
            self.capacity_used_bytes.get()
        ));

        output.push_str("# TYPE claudefs_capacity_available_bytes gauge\n");
        output.push_str(&format!(
            "claudefs_capacity_available_bytes {}\n\n",
            self.capacity_available_bytes.get()
        ));

        output.push_str("# TYPE claudefs_nodes_total gauge\n");
        output.push_str(&format!(
            "claudefs_nodes_total {}\n\n",
            self.nodes_total.get()
        ));

        output.push_str("# TYPE claudefs_nodes_healthy gauge\n");
        output.push_str(&format!(
            "claudefs_nodes_healthy {}\n\n",
            self.nodes_healthy.get()
        ));

        output.push_str("# TYPE claudefs_nodes_degraded gauge\n");
        output.push_str(&format!(
            "claudefs_nodes_degraded {}\n\n",
            self.nodes_degraded.get()
        ));

        output.push_str("# TYPE claudefs_nodes_offline gauge\n");
        output.push_str(&format!(
            "claudefs_nodes_offline {}\n\n",
            self.nodes_offline.get()
        ));

        output.push_str("# TYPE claudefs_replication_lag_secs gauge\n");
        output.push_str(&format!(
            "claudefs_replication_lag_secs {}\n\n",
            self.replication_lag_secs.get()
        ));

        output.push_str("# TYPE claudefs_replication_conflicts_total counter\n");
        output.push_str(&format!(
            "claudefs_replication_conflicts_total {}\n\n",
            self.replication_conflicts_total.get()
        ));

        output.push_str("# TYPE claudefs_dedupe_hit_rate gauge\n");
        output.push_str(&format!(
            "claudefs_dedupe_hit_rate {}\n\n",
            self.dedupe_hit_rate.get()
        ));

        output.push_str("# TYPE claudefs_compression_ratio gauge\n");
        output.push_str(&format!(
            "claudefs_compression_ratio {}\n\n",
            self.compression_ratio.get()
        ));

        output.push_str("# TYPE claudefs_s3_queue_depth gauge\n");
        output.push_str(&format!(
            "claudefs_s3_queue_depth {}\n\n",
            self.s3_queue_depth.get()
        ));

        output.push_str("# TYPE claudefs_s3_flush_latency_ms histogram\n");
        for (i, boundary) in BUCKET_BOUNDARIES.iter().enumerate() {
            output.push_str(&format!(
                "claudefs_s3_flush_latency_ms_bucket{{le={}}} {}\n",
                boundary,
                self.s3_flush_latency_ms.bucket_count(i)
            ));
        }
        output.push_str(&format!(
            "claudefs_s3_flush_latency_ms_bucket{{le=\"+Inf\"}} {}\n",
            self.s3_flush_latency_ms
                .bucket_count(BUCKET_BOUNDARIES.len())
        ));
        output.push_str(&format!(
            "claudefs_s3_flush_latency_ms_sum {}\n",
            self.s3_flush_latency_ms.sum()
        ));
        output.push_str(&format!(
            "claudefs_s3_flush_latency_ms_count {}\n",
            self.s3_flush_latency_ms.count()
        ));

        output
    }

    pub fn update_from_snapshot(&self, snapshot: &NodeMetricsSnapshot) {
        self.iops_read.add(snapshot.iops_read);
        self.iops_write.add(snapshot.iops_write);
        self.bytes_read.add(snapshot.bytes_read);
        self.bytes_write.add(snapshot.bytes_write);

        self.latency_read_us.observe(snapshot.latency_read_us_p50);
        self.latency_read_us.observe(snapshot.latency_read_us_p99);
        self.latency_write_us.observe(snapshot.latency_write_us_p50);
        self.latency_write_us.observe(snapshot.latency_write_us_p99);

        self.capacity_total_bytes
            .add(snapshot.capacity_total_bytes as f64);
        self.capacity_used_bytes
            .add(snapshot.capacity_used_bytes as f64);
        self.capacity_available_bytes.add(
            (snapshot
                .capacity_total_bytes
                .saturating_sub(snapshot.capacity_used_bytes)) as f64,
        );

        self.replication_lag_secs.set(snapshot.replication_lag_secs);
        self.dedupe_hit_rate.set(snapshot.dedupe_hit_rate);
        self.compression_ratio.set(snapshot.compression_ratio);
        self.s3_queue_depth.set(snapshot.s3_queue_depth as f64);
    }
}

impl Default for ClusterMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        let counter = Counter::new("test_counter");
        assert_eq!(counter.get(), 0);
        counter.inc();
        assert_eq!(counter.get(), 1);
        counter.inc();
        assert_eq!(counter.get(), 2);
        counter.add(5);
        assert_eq!(counter.get(), 7);
    }

    #[test]
    fn test_counter_reset() {
        let counter = Counter::new("test_counter");
        counter.add(100);
        assert_eq!(counter.get(), 100);
        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_gauge_set() {
        let gauge = Gauge::new();
        gauge.set(42.5);
        assert_eq!(gauge.get(), 42.5);
        gauge.set(100.0);
        assert_eq!(gauge.get(), 100.0);
    }

    #[test]
    fn test_gauge_get() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0.0);
        gauge.set(123.456);
        assert!((gauge.get() - 123.456).abs() < 0.001);
    }

    #[test]
    fn test_gauge_add() {
        let gauge = Gauge::new();
        gauge.set(10.0);
        gauge.add(5.0);
        assert_eq!(gauge.get(), 15.0);
    }

    #[test]
    fn test_gauge_sub() {
        let gauge = Gauge::new();
        gauge.set(10.0);
        gauge.sub(3.0);
        assert_eq!(gauge.get(), 7.0);
    }

    #[test]
    fn test_histogram_observe() {
        let hist = Histogram::new("test_hist");
        hist.observe(50.0);
        hist.observe(200.0);
        hist.observe(800.0);
        hist.observe(15000.0);

        assert_eq!(hist.count(), 4);
        assert!(hist.sum() > 0.0);
        assert!(hist.bucket_count(0) > 0);
    }

    #[test]
    fn test_histogram_bucket_counts() {
        let hist = Histogram::new("test_hist");
        hist.observe(50.0);
        hist.observe(50.0);
        hist.observe(500.0);
        hist.observe(10000.0);

        assert_eq!(hist.bucket_count(0), 2);
        assert_eq!(hist.bucket_count(1), 1);
        assert_eq!(hist.bucket_count(4), 1);
    }

    #[test]
    fn test_prometheus_text_format() {
        let metrics = ClusterMetrics::new();
        metrics.iops_read.add(100);
        metrics.iops_write.add(50);
        metrics.bytes_read.add(1024);
        metrics.bytes_write.add(512);
        metrics.capacity_total_bytes.set(1000000000.0);
        metrics.capacity_used_bytes.set(500000000.0);
        metrics.nodes_total.set(5.0);
        metrics.nodes_healthy.set(4.0);
        metrics.nodes_degraded.set(1.0);
        metrics.dedupe_hit_rate.set(0.75);
        metrics.compression_ratio.set(2.5);

        let output = metrics.render_prometheus();

        assert!(output.contains("claudefs_iops_read_total"));
        assert!(output.contains("claudefs_iops_write_total"));
        assert!(output.contains("claudefs_capacity_total_bytes"));
        assert!(output.contains("claudefs_nodes_total"));
        assert!(output.contains("claudefs_dedupe_hit_rate"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn test_cluster_metrics_update_from_snapshot() {
        let metrics = ClusterMetrics::new();
        let snapshot = NodeMetricsSnapshot {
            node_id: "node1".to_string(),
            iops_read: 100,
            iops_write: 50,
            bytes_read: 1024,
            bytes_write: 512,
            latency_read_us_p50: 100.0,
            latency_read_us_p99: 500.0,
            latency_write_us_p50: 150.0,
            latency_write_us_p99: 600.0,
            capacity_total_bytes: 1000000000,
            capacity_used_bytes: 500000000,
            is_healthy: true,
            replication_lag_secs: 1.5,
            dedupe_hit_rate: 0.8,
            compression_ratio: 2.0,
            s3_queue_depth: 10,
            timestamp: 1234567890,
        };

        metrics.update_from_snapshot(&snapshot);

        assert_eq!(metrics.iops_read.get(), 100);
        assert_eq!(metrics.iops_write.get(), 50);
        assert!(metrics.capacity_used_bytes.get() > 0.0);
    }

    #[test]
    fn test_render_prometheus_contains_expected_metrics() {
        let metrics = ClusterMetrics::new();
        metrics.latency_read_us.observe(100.0);
        metrics.latency_write_us.observe(200.0);
        metrics.s3_flush_latency_ms.observe(50.0);

        let output = metrics.render_prometheus();

        assert!(output.contains("claudefs_latency_read_us_bucket"));
        assert!(output.contains("claudefs_latency_write_us_bucket"));
        assert!(output.contains("claudefs_s3_flush_latency_ms_bucket"));
        assert!(output.contains("+Inf"));
    }

    #[test]
    fn test_histogram_inf_bucket() {
        let hist = Histogram::new("test");
        hist.observe(100000.0);

        let bucket_count = hist.bucket_count(BUCKET_BOUNDARIES.len());
        assert_eq!(bucket_count, 1);
    }

    #[test]
    fn test_gauge_inc_dec() {
        let gauge = Gauge::new();
        gauge.set(5.0);
        gauge.inc();
        assert_eq!(gauge.get(), 6.0);
        gauge.dec();
        assert_eq!(gauge.get(), 5.0);
    }
}
