//! Adaptive timeout module that tracks latency histograms and automatically
//! tunes timeout/deadline values based on observed p50/p95/p99 latencies.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Configuration for adaptive timeout tuning.
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    pub initial_timeout_ms: u64,
    pub min_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub percentile_target: f64,
    pub safety_margin: f64,
    pub window_size: usize,
    pub adjustment_interval_ms: u64,
    pub enabled: bool,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            initial_timeout_ms: 5000,
            min_timeout_ms: 100,
            max_timeout_ms: 30000,
            percentile_target: 0.99,
            safety_margin: 1.5,
            window_size: 1000,
            adjustment_interval_ms: 5000,
            enabled: true,
        }
    }
}

/// Latency histogram with sliding window.
pub struct LatencyHistogram {
    samples: Mutex<LatencyWindow>,
}

struct LatencyWindow {
    values: Vec<u64>,
    head: usize,
    count: usize,
    capacity: usize,
}

impl LatencyHistogram {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(LatencyWindow {
                values: vec![0; capacity],
                head: 0,
                count: 0,
                capacity,
            }),
        }
    }

    pub fn record(&self, latency_us: u64) {
        let mut window = self.samples.lock().unwrap();
        let head = window.head;
        let capacity = window.capacity;
        window.values[head] = latency_us;
        window.head = (head + 1) % capacity;
        if window.count < capacity {
            window.count += 1;
        }
    }

    pub fn percentile(&self, p: f64) -> u64 {
        let window = self.samples.lock().unwrap();
        if window.count == 0 {
            return 0;
        }

        let mut sorted: Vec<u64> = window.values[..window.count].to_vec();
        sorted.sort_unstable();

        let n = sorted.len();
        if n == 1 {
            return sorted[0];
        }

        let raw_idx = p * (n - 1) as f64;
        let lower_idx = raw_idx.floor() as usize;
        let upper_idx = (lower_idx + 1).min(n - 1);
        let fraction = raw_idx - lower_idx as f64;

        if lower_idx == upper_idx {
            sorted[lower_idx]
        } else {
            let lower = sorted[lower_idx];
            let upper = sorted[upper_idx];
            (lower as f64 + fraction * (upper as f64 - lower as f64)) as u64
        }
    }

    pub fn snapshot(&self) -> PercentileSnapshot {
        let window = self.samples.lock().unwrap();
        if window.count == 0 {
            return PercentileSnapshot {
                p50: 0,
                p90: 0,
                p95: 0,
                p99: 0,
                p999: 0,
                min: 0,
                max: 0,
                mean: 0,
                sample_count: 0,
            };
        }

        let mut sorted: Vec<u64> = window.values[..window.count].to_vec();
        sorted.sort_unstable();

        let n = sorted.len();
        let sum: u64 = sorted.iter().sum();
        let mean = sum / n as u64;

        PercentileSnapshot {
            p50: sorted[(0.50 * (n - 1) as f64).round() as usize],
            p90: sorted[(0.90 * (n - 1) as f64).round() as usize],
            p95: sorted[(0.95 * (n - 1) as f64).round() as usize],
            p99: sorted[(0.99 * (n - 1) as f64).round() as usize],
            p999: sorted[(0.999 * (n - 1) as f64).round() as usize],
            min: sorted[0],
            max: sorted[n - 1],
            mean,
            sample_count: n,
        }
    }

    pub fn sample_count(&self) -> usize {
        let window = self.samples.lock().unwrap();
        window.count
    }

    pub fn reset(&self) {
        let mut window = self.samples.lock().unwrap();
        window.head = 0;
        window.count = 0;
    }
}

/// Percentile results from the histogram.
#[derive(Debug, Clone)]
pub struct PercentileSnapshot {
    pub p50: u64,
    pub p90: u64,
    pub p95: u64,
    pub p99: u64,
    pub p999: u64,
    pub min: u64,
    pub max: u64,
    pub mean: u64,
    pub sample_count: usize,
}

/// Adaptive timeout manager.
pub struct AdaptiveTimeout {
    config: AdaptiveConfig,
    histogram: LatencyHistogram,
    current_timeout_ms: AtomicU64,
    stats: AdaptiveStats,
}

/// Stats tracking.
pub struct AdaptiveStats {
    samples_recorded: AtomicU64,
    timeout_adjustments: AtomicU64,
    timeouts_hit: AtomicU64,
}

impl AdaptiveStats {
    fn new() -> Self {
        Self {
            samples_recorded: AtomicU64::new(0),
            timeout_adjustments: AtomicU64::new(0),
            timeouts_hit: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> AdaptiveStatsSnapshot {
        AdaptiveStatsSnapshot {
            samples_recorded: self.samples_recorded.load(Ordering::Relaxed),
            timeout_adjustments: self.timeout_adjustments.load(Ordering::Relaxed),
            timeouts_hit: self.timeouts_hit.load(Ordering::Relaxed),
            current_timeout_ms: 0,
            current_p99_us: 0,
        }
    }

    fn increment_samples(&self) {
        self.samples_recorded.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_adjustments(&self) {
        self.timeout_adjustments.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_timeouts(&self) {
        self.timeouts_hit.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveStatsSnapshot {
    pub samples_recorded: u64,
    pub timeout_adjustments: u64,
    pub timeouts_hit: u64,
    pub current_timeout_ms: u64,
    pub current_p99_us: u64,
}

impl AdaptiveTimeout {
    #[allow(clippy::if_same_then_else)]
    pub fn new(config: AdaptiveConfig) -> Self {
        let initial = if config.enabled {
            config.initial_timeout_ms
        } else {
            config.initial_timeout_ms
        };

        Self {
            histogram: LatencyHistogram::new(config.window_size),
            current_timeout_ms: AtomicU64::new(initial),
            config,
            stats: AdaptiveStats::new(),
        }
    }

    pub fn record_latency(&self, latency_us: u64) {
        self.histogram.record(latency_us);
        self.stats.increment_samples();
    }

    pub fn record_timeout(&self) {
        self.stats.increment_timeouts();
    }

    pub fn current_timeout_ms(&self) -> u64 {
        if !self.config.enabled {
            return self.config.initial_timeout_ms;
        }
        self.current_timeout_ms.load(Ordering::Relaxed)
    }

    pub fn adjust(&self) {
        if !self.config.enabled {
            return;
        }

        if self.histogram.sample_count() == 0 {
            return;
        }

        let target_percentile = self.histogram.percentile(self.config.percentile_target);
        let new_timeout_us = (target_percentile as f64 * self.config.safety_margin) as u64;
        let min_us = self.config.min_timeout_ms * 1000;
        let max_us = self.config.max_timeout_ms * 1000;
        let clamped = new_timeout_us.clamp(min_us, max_us);
        let new_timeout_ms = clamped / 1000;

        self.current_timeout_ms
            .store(new_timeout_ms, Ordering::Relaxed);
        self.stats.increment_adjustments();
    }

    pub fn percentiles(&self) -> PercentileSnapshot {
        self.histogram.snapshot()
    }

    pub fn stats(&self) -> AdaptiveStatsSnapshot {
        let mut snapshot = self.stats.snapshot();
        snapshot.current_timeout_ms = self.current_timeout_ms.load(Ordering::Relaxed);
        snapshot.current_p99_us = self.histogram.percentile(0.99);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = AdaptiveConfig::default();
        assert_eq!(config.initial_timeout_ms, 5000);
        assert_eq!(config.min_timeout_ms, 100);
        assert_eq!(config.max_timeout_ms, 30000);
        assert!((config.percentile_target - 0.99).abs() < 0.001);
        assert!((config.safety_margin - 1.5).abs() < 0.001);
        assert_eq!(config.window_size, 1000);
        assert_eq!(config.adjustment_interval_ms, 5000);
        assert!(config.enabled);
    }

    #[test]
    fn test_histogram_empty() {
        let hist = LatencyHistogram::new(10);
        assert_eq!(hist.percentile(0.5), 0);
        assert_eq!(hist.percentile(0.99), 0);
        assert_eq!(hist.sample_count(), 0);
    }

    #[test]
    fn test_histogram_single_sample() {
        let hist = LatencyHistogram::new(10);
        hist.record(1000);
        assert_eq!(hist.percentile(0.5), 1000);
        assert_eq!(hist.percentile(0.99), 1000);
        assert_eq!(hist.sample_count(), 1);
    }

    #[test]
    fn test_histogram_percentile_p50() {
        let hist = LatencyHistogram::new(100);
        for i in 1..=100 {
            hist.record(i * 100);
        }
        let p50 = hist.percentile(0.50);
        assert!(p50 >= 5000 && p50 <= 5100);
    }

    #[test]
    fn test_histogram_percentile_p99() {
        let hist = LatencyHistogram::new(100);
        for i in 1..=100 {
            hist.record(i * 100);
        }
        let p99 = hist.percentile(0.99);
        assert!(p99 >= 9000);
    }

    #[test]
    fn test_histogram_window_size() {
        let hist = LatencyHistogram::new(5);
        for i in 1..=10 {
            hist.record(i * 1000);
        }
        assert_eq!(hist.sample_count(), 5);
        let p50 = hist.percentile(0.5);
        assert_eq!(p50, 8000);
    }

    #[test]
    fn test_histogram_reset() {
        let hist = LatencyHistogram::new(10);
        hist.record(1000);
        hist.record(2000);
        hist.reset();
        assert_eq!(hist.sample_count(), 0);
        assert_eq!(hist.percentile(0.5), 0);
    }

    #[test]
    fn test_histogram_snapshot() {
        let hist = LatencyHistogram::new(10);
        hist.record(100);
        hist.record(500);
        hist.record(1000);
        let snapshot = hist.snapshot();
        assert_eq!(snapshot.p50, 500);
        assert!(snapshot.p90 >= snapshot.p50);
        assert!(snapshot.p95 >= snapshot.p90);
        assert!(snapshot.p99 >= snapshot.p95);
        assert!(snapshot.p999 >= snapshot.p99);
        assert_eq!(snapshot.min, 100);
        assert_eq!(snapshot.max, 1000);
        assert_eq!(snapshot.sample_count, 3);
    }

    #[test]
    fn test_histogram_sample_count() {
        let hist = LatencyHistogram::new(10);
        assert_eq!(hist.sample_count(), 0);
        hist.record(100);
        assert_eq!(hist.sample_count(), 1);
        hist.record(200);
        assert_eq!(hist.sample_count(), 2);
    }

    #[test]
    fn test_adaptive_initial_timeout() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 3000,
            enabled: true,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        assert_eq!(timeout.current_timeout_ms(), 3000);
    }

    #[test]
    fn test_adaptive_record_latency() {
        let config = AdaptiveConfig::default();
        let timeout = AdaptiveTimeout::new(config);
        timeout.record_latency(1000);
        timeout.record_latency(2000);
        let stats = timeout.stats();
        assert_eq!(stats.samples_recorded, 2);
    }

    #[test]
    fn test_adaptive_adjust_increases_timeout() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 100,
            min_timeout_ms: 50,
            max_timeout_ms: 10000,
            percentile_target: 0.99,
            safety_margin: 1.5,
            window_size: 100,
            enabled: true,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(4_000_000);
        }
        let before = timeout.current_timeout_ms();
        timeout.adjust();
        let after = timeout.current_timeout_ms();
        assert!(after > before);
    }

    #[test]
    fn test_adaptive_adjust_decreases_timeout() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 5000,
            min_timeout_ms: 100,
            max_timeout_ms: 10000,
            percentile_target: 0.99,
            safety_margin: 1.5,
            window_size: 100,
            enabled: true,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(100);
        }
        let before = timeout.current_timeout_ms();
        timeout.adjust();
        let after = timeout.current_timeout_ms();
        assert!(after < before);
    }

    #[test]
    fn test_adaptive_min_timeout() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 5000,
            min_timeout_ms: 200,
            max_timeout_ms: 10000,
            percentile_target: 0.99,
            safety_margin: 1.0,
            window_size: 100,
            enabled: true,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(10);
        }
        timeout.adjust();
        let p99 = timeout.percentiles().p99;
        let expected = p99 * 1;
        let min_limit = 200 * 1000;
        let actual = timeout.current_timeout_ms();
        assert!(actual >= 200, "timeout {} should be >= min {}", actual, 200);
    }

    #[test]
    fn test_adaptive_max_timeout() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 100,
            min_timeout_ms: 50,
            max_timeout_ms: 500,
            percentile_target: 0.99,
            safety_margin: 1.0,
            window_size: 100,
            enabled: true,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(1000000);
        }
        timeout.adjust();
        assert_eq!(timeout.current_timeout_ms(), 500);
    }

    #[test]
    fn test_adaptive_record_timeout() {
        let config = AdaptiveConfig::default();
        let timeout = AdaptiveTimeout::new(config);
        timeout.record_timeout();
        timeout.record_timeout();
        let stats = timeout.stats();
        assert_eq!(stats.timeouts_hit, 2);
    }

    #[test]
    fn test_adaptive_stats() {
        let config = AdaptiveConfig::default();
        let timeout = AdaptiveTimeout::new(config);
        timeout.record_latency(1000);
        timeout.record_timeout();
        timeout.adjust();
        let stats = timeout.stats();
        assert_eq!(stats.samples_recorded, 1);
        assert_eq!(stats.timeouts_hit, 1);
        assert!(stats.timeout_adjustments >= 1);
    }

    #[test]
    fn test_adaptive_disabled() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 5000,
            enabled: false,
            ..Default::default()
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(100000);
        }
        timeout.adjust();
        assert_eq!(timeout.current_timeout_ms(), 5000);
    }

    #[test]
    fn test_adaptive_safety_margin() {
        let config = AdaptiveConfig {
            initial_timeout_ms: 100,
            min_timeout_ms: 10,
            max_timeout_ms: 10000,
            percentile_target: 0.99,
            safety_margin: 2.0,
            window_size: 100,
            enabled: true,
            adjustment_interval_ms: 5000,
        };
        let timeout = AdaptiveTimeout::new(config);
        for _ in 0..50 {
            timeout.record_latency(10000);
        }
        timeout.adjust();
        let actual = timeout.current_timeout_ms();
        let expected = 10000 * 2 / 1000;
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_percentile_snapshot_ordering() {
        let hist = LatencyHistogram::new(100);
        for i in 1..=100 {
            hist.record(i);
        }
        let snap = hist.snapshot();
        assert!(snap.p50 <= snap.p90);
        assert!(snap.p90 <= snap.p95);
        assert!(snap.p95 <= snap.p99);
        assert!(snap.p99 <= snap.p999);
    }
}
