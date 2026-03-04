use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadEvent {
    pub logical_bytes: u64,
    pub physical_bytes: u64,
    pub io_count: u32,
    pub cache_hit: bool,
}

#[derive(Debug, Clone)]
pub struct ReadAmplificationConfig {
    pub alert_threshold: f64,
    pub window_size: usize,
}

impl Default for ReadAmplificationConfig {
    fn default() -> Self {
        Self {
            alert_threshold: 3.0,
            window_size: 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReadAmplificationStats {
    pub total_events: u64,
    pub total_logical_bytes: u64,
    pub total_physical_bytes: u64,
    pub total_io_count: u64,
    pub cache_hit_events: u64,
    pub alert_count: u64,
}

impl ReadAmplificationStats {
    pub fn amplification_ratio(&self) -> f64 {
        if self.total_logical_bytes == 0 {
            return 1.0;
        }
        self.total_physical_bytes as f64 / self.total_logical_bytes as f64
    }

    pub fn avg_io_per_event(&self) -> f64 {
        if self.total_events == 0 {
            return 0.0;
        }
        self.total_io_count as f64 / self.total_events as f64
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_events == 0 {
            return 0.0;
        }
        self.cache_hit_events as f64 / self.total_events as f64
    }
}

pub struct ReadAmplificationTracker {
    config: ReadAmplificationConfig,
    stats: ReadAmplificationStats,
    window: std::collections::VecDeque<f64>,
}

impl ReadAmplificationTracker {
    pub fn new(config: ReadAmplificationConfig) -> Self {
        Self {
            config,
            stats: ReadAmplificationStats::default(),
            window: std::collections::VecDeque::new(),
        }
    }

    pub fn record(&mut self, event: ReadEvent) -> bool {
        self.stats.total_events += 1;
        self.stats.total_logical_bytes += event.logical_bytes;
        self.stats.total_physical_bytes += event.physical_bytes;
        self.stats.total_io_count += event.io_count as u64;
        if event.cache_hit {
            self.stats.cache_hit_events += 1;
        }

        let ratio = if event.logical_bytes == 0 {
            1.0
        } else {
            event.physical_bytes as f64 / event.logical_bytes as f64
        };
        if self.window.len() >= self.config.window_size {
            self.window.pop_front();
        }
        self.window.push_back(ratio);

        let alert = ratio > self.config.alert_threshold;
        if alert {
            self.stats.alert_count += 1;
        }
        alert
    }

    pub fn rolling_avg_amplification(&self) -> f64 {
        if self.window.is_empty() {
            return 1.0;
        }
        let sum: f64 = self.window.iter().sum();
        sum / self.window.len() as f64
    }

    pub fn window_max(&self) -> f64 {
        self.window
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .max(1.0)
    }

    pub fn stats(&self) -> &ReadAmplificationStats {
        &self.stats
    }
    pub fn window_size(&self) -> usize {
        self.window.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_event_fields() {
        let event = ReadEvent {
            logical_bytes: 4096,
            physical_bytes: 8192,
            io_count: 2,
            cache_hit: true,
        };
        assert_eq!(event.logical_bytes, 4096);
        assert_eq!(event.physical_bytes, 8192);
        assert_eq!(event.io_count, 2);
        assert!(event.cache_hit);
    }

    #[test]
    fn config_default() {
        let config = ReadAmplificationConfig::default();
        assert_eq!(config.alert_threshold, 3.0);
        assert_eq!(config.window_size, 1024);
    }

    #[test]
    fn new_tracker_empty() {
        let tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        assert_eq!(tracker.stats().total_events, 0);
        assert_eq!(tracker.window_size(), 0);
    }

    #[test]
    fn record_increments_events() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().total_events, 1);
    }

    #[test]
    fn record_accumulates_logical_bytes() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 200,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().total_logical_bytes, 300);
    }

    #[test]
    fn record_accumulates_physical_bytes() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 300,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().total_physical_bytes, 500);
    }

    #[test]
    fn record_accumulates_io_count() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 2,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 3,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().total_io_count, 5);
    }

    #[test]
    fn record_cache_hit_increments_cache_hits() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 0,
            io_count: 0,
            cache_hit: true,
        });
        assert_eq!(tracker.stats().cache_hit_events, 1);
    }

    #[test]
    fn record_no_cache_hit() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().cache_hit_events, 0);
    }

    #[test]
    fn amplification_ratio_1x() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        assert!((tracker.stats().amplification_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn amplification_ratio_2x() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        assert!((tracker.stats().amplification_ratio() - 2.0).abs() < 0.001);
    }

    #[test]
    fn amplification_ratio_zero_logical() {
        let stats = ReadAmplificationStats::default();
        assert!((stats.amplification_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn avg_io_per_event() {
        let mut stats = ReadAmplificationStats::default();
        stats.total_events = 4;
        stats.total_io_count = 8;
        assert!((stats.avg_io_per_event() - 2.0).abs() < 0.001);
    }

    #[test]
    fn avg_io_zero_when_no_events() {
        let stats = ReadAmplificationStats::default();
        assert!((stats.avg_io_per_event() - 0.0).abs() < 0.001);
    }

    #[test]
    fn cache_hit_rate_100pct() {
        let mut stats = ReadAmplificationStats::default();
        stats.total_events = 10;
        stats.cache_hit_events = 10;
        assert!((stats.cache_hit_rate() - 1.0).abs() < 0.001);
    }

    #[test]
    fn cache_hit_rate_zero() {
        let stats = ReadAmplificationStats::default();
        assert!((stats.cache_hit_rate() - 0.0).abs() < 0.001);
    }

    #[test]
    fn record_returns_true_when_threshold_exceeded() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig {
            alert_threshold: 2.0,
            window_size: 1024,
        });
        let result = tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 300,
            io_count: 1,
            cache_hit: false,
        });
        assert!(result);
    }

    #[test]
    fn record_returns_false_below_threshold() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig {
            alert_threshold: 3.0,
            window_size: 1024,
        });
        let result = tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 150,
            io_count: 1,
            cache_hit: false,
        });
        assert!(!result);
    }

    #[test]
    fn alert_count_increments() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig {
            alert_threshold: 2.0,
            window_size: 1024,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 300,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.stats().alert_count, 1);
    }

    #[test]
    fn rolling_avg_amplification_single() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 250,
            io_count: 1,
            cache_hit: false,
        });
        assert!((tracker.rolling_avg_amplification() - 2.5).abs() < 0.001);
    }

    #[test]
    fn rolling_avg_amplification_multiple() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 400,
            io_count: 1,
            cache_hit: false,
        });
        assert!((tracker.rolling_avg_amplification() - 3.0).abs() < 0.001);
    }

    #[test]
    fn window_evicts_oldest_at_capacity() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig {
            alert_threshold: 3.0,
            window_size: 2,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 300,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.window_size(), 2);
    }

    #[test]
    fn window_max_returns_highest() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig::default());
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 500,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 200,
            io_count: 1,
            cache_hit: false,
        });
        assert!((tracker.window_max() - 5.0).abs() < 0.001);
    }

    #[test]
    fn window_size_tracks_events() {
        let mut tracker = ReadAmplificationTracker::new(ReadAmplificationConfig {
            alert_threshold: 3.0,
            window_size: 10,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        tracker.record(ReadEvent {
            logical_bytes: 100,
            physical_bytes: 100,
            io_count: 1,
            cache_hit: false,
        });
        assert_eq!(tracker.window_size(), 3);
    }
}
