use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressionBucket {
    pub window_id: u64,
    pub samples: u64,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    pub total_latency_us: u64,
}

impl CompressionBucket {
    pub fn ratio(&self) -> f64 {
        if self.total_output_bytes == 0 {
            return 1.0;
        }
        self.total_input_bytes as f64 / self.total_output_bytes as f64
    }

    pub fn throughput_mbps(&self) -> f64 {
        if self.total_latency_us == 0 {
            return 0.0;
        }
        (self.total_input_bytes as f64 / (self.total_latency_us as f64 / 1_000_000.0)) / 1_000_000.0
    }
}

#[derive(Debug, Clone)]
pub struct CompressionStatsConfig {
    pub bucket_count: usize,
}

impl Default for CompressionStatsConfig {
    fn default() -> Self {
        Self { bucket_count: 60 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedCompressionStats {
    pub total_samples: u64,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    pub avg_ratio: f64,
    pub avg_throughput_mbps: f64,
    pub bucket_count: usize,
}

pub struct CompressionStats {
    config: CompressionStatsConfig,
    buckets: std::collections::VecDeque<CompressionBucket>,
    current_bucket: CompressionBucket,
    current_window: u64,
}

impl CompressionStats {
    pub fn new(config: CompressionStatsConfig) -> Self {
        Self {
            config,
            buckets: std::collections::VecDeque::new(),
            current_bucket: CompressionBucket::default(),
            current_window: 0,
        }
    }

    pub fn record(&mut self, window_id: u64, input_bytes: u64, output_bytes: u64, latency_us: u64) {
        if window_id != self.current_window {
            let old = std::mem::replace(
                &mut self.current_bucket,
                CompressionBucket {
                    window_id,
                    ..Default::default()
                },
            );
            if old.samples > 0 {
                self.buckets.push_back(old);
                while self.buckets.len() > self.config.bucket_count {
                    self.buckets.pop_front();
                }
            }
            self.current_window = window_id;
        }
        self.current_bucket.window_id = window_id;
        self.current_bucket.samples += 1;
        self.current_bucket.total_input_bytes += input_bytes;
        self.current_bucket.total_output_bytes += output_bytes;
        self.current_bucket.total_latency_us += latency_us;
    }

    pub fn aggregate(&self) -> AggregatedCompressionStats {
        let mut agg = AggregatedCompressionStats {
            bucket_count: self.buckets.len(),
            ..Default::default()
        };
        for b in &self.buckets {
            agg.total_samples += b.samples;
            agg.total_input_bytes += b.total_input_bytes;
            agg.total_output_bytes += b.total_output_bytes;
        }
        if agg.total_output_bytes > 0 {
            agg.avg_ratio = agg.total_input_bytes as f64 / agg.total_output_bytes as f64;
        } else {
            agg.avg_ratio = 1.0;
        }
        agg
    }

    pub fn current_bucket(&self) -> &CompressionBucket {
        &self.current_bucket
    }
    pub fn sealed_bucket_count(&self) -> usize {
        self.buckets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_stats_config_default() {
        let config = CompressionStatsConfig::default();
        assert_eq!(config.bucket_count, 60);
    }

    #[test]
    fn compression_bucket_default() {
        let bucket = CompressionBucket::default();
        assert_eq!(bucket.window_id, 0);
        assert_eq!(bucket.samples, 0);
        assert_eq!(bucket.total_input_bytes, 0);
        assert_eq!(bucket.total_output_bytes, 0);
        assert_eq!(bucket.total_latency_us, 0);
    }

    #[test]
    fn bucket_ratio_no_output() {
        let bucket = CompressionBucket {
            total_input_bytes: 1000,
            total_output_bytes: 0,
            ..Default::default()
        };
        assert_eq!(bucket.ratio(), 1.0);
    }

    #[test]
    fn bucket_ratio_2x() {
        let bucket = CompressionBucket {
            total_input_bytes: 1000,
            total_output_bytes: 500,
            ..Default::default()
        };
        assert!((bucket.ratio() - 2.0).abs() < 0.001);
    }

    #[test]
    fn bucket_throughput_zero_latency() {
        let bucket = CompressionBucket {
            total_input_bytes: 1000000,
            total_latency_us: 0,
            ..Default::default()
        };
        assert_eq!(bucket.throughput_mbps(), 0.0);
    }

    #[test]
    fn bucket_throughput_nonzero() {
        let bucket = CompressionBucket {
            total_input_bytes: 1_000_000,
            total_latency_us: 1_000_000,
            ..Default::default()
        };
        assert!(bucket.throughput_mbps() > 0.0);
    }

    #[test]
    fn record_first_event() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        assert_eq!(stats.current_bucket().samples, 1);
    }

    #[test]
    fn record_increments_samples() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(1, 100, 50, 1000);
        assert_eq!(stats.current_bucket().samples, 2);
    }

    #[test]
    fn record_same_window() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(1, 200, 100, 2000);
        assert_eq!(stats.sealed_bucket_count(), 0);
    }

    #[test]
    fn record_new_window_seals_old() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 200, 100, 2000);
        assert_eq!(stats.sealed_bucket_count(), 1);
    }

    #[test]
    fn sealed_bucket_count_after_window_change() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 200, 100, 2000);
        assert_eq!(stats.sealed_bucket_count(), 1);
    }

    #[test]
    fn sealed_bucket_count_zero_initially() {
        let stats = CompressionStats::new(CompressionStatsConfig::default());
        assert_eq!(stats.sealed_bucket_count(), 0);
    }

    #[test]
    fn bucket_eviction_at_capacity() {
        let mut stats = CompressionStats::new(CompressionStatsConfig { bucket_count: 2 });
        for i in 1..=3 {
            stats.record(i, 100, 50, 1000);
            stats.record(i + 1, 100, 50, 1000);
        }
        assert_eq!(stats.sealed_bucket_count(), 2);
    }

    #[test]
    fn aggregate_empty_buckets() {
        let stats = CompressionStats::new(CompressionStatsConfig::default());
        let agg = stats.aggregate();
        assert_eq!(agg.total_samples, 0);
        assert_eq!(agg.total_input_bytes, 0);
        assert_eq!(agg.total_output_bytes, 0);
    }

    #[test]
    fn aggregate_total_samples() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 100, 50, 1000);
        let agg = stats.aggregate();
        assert_eq!(agg.total_samples, 1);
    }

    #[test]
    fn aggregate_total_bytes() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 200, 100, 2000);
        let agg = stats.aggregate();
        assert_eq!(agg.total_input_bytes, 100);
    }

    #[test]
    fn aggregate_avg_ratio() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 1000, 500, 1000);
        stats.record(2, 1000, 500, 1000);
        let agg = stats.aggregate();
        assert!((agg.avg_ratio - 2.0).abs() < 0.001);
    }

    #[test]
    fn aggregate_ratio_one_when_no_output() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 1000, 0, 1000);
        stats.record(2, 1000, 0, 1000);
        let agg = stats.aggregate();
        assert_eq!(agg.avg_ratio, 1.0);
    }

    #[test]
    fn current_bucket_window_matches() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(42, 100, 50, 1000);
        assert_eq!(stats.current_bucket().window_id, 42);
    }

    #[test]
    fn record_skips_empty_old_bucket() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 0, 0, 0);
        stats.record(1, 100, 50, 1000);
        stats.record(2, 100, 50, 1000);
        assert_eq!(stats.sealed_bucket_count(), 1);
    }

    #[test]
    fn multiple_window_changes() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 100, 50, 1000);
        stats.record(3, 100, 50, 1000);
        stats.record(4, 100, 50, 1000);
        assert_eq!(stats.sealed_bucket_count(), 3);
    }

    #[test]
    fn aggregated_stats_bucket_count() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 100, 50, 1000);
        let agg = stats.aggregate();
        assert_eq!(agg.bucket_count, 1);
    }

    #[test]
    fn record_accumulates_bytes() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(1, 200, 100, 2000);
        assert_eq!(stats.current_bucket().total_input_bytes, 300);
    }

    #[test]
    fn sealed_bucket_not_included_in_current() {
        let mut stats = CompressionStats::new(CompressionStatsConfig::default());
        stats.record(1, 100, 50, 1000);
        stats.record(2, 200, 100, 2000);
        let agg = stats.aggregate();
        assert_eq!(stats.current_bucket().samples, 1);
        assert_eq!(agg.total_samples, 1);
    }
}
