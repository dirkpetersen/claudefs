//! Track and analyze write amplification from dedup, compression, and erasure coding.
//!
//! Write amplification is the ratio of physical bytes written to logical bytes
//! submitted by applications. Tracking helps identify inefficiencies.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single write event with amplification details.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WriteEvent {
    /// Logical bytes from application
    pub logical_bytes: u64,
    /// Physical bytes written to storage
    pub physical_bytes: u64,
    /// Bytes saved by deduplication
    pub dedup_bytes_saved: u64,
    /// Bytes saved by compression
    pub compression_bytes_saved: u64,
    /// Erasure coding overhead bytes
    pub ec_overhead_bytes: u64,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
}

/// Configuration for the write amplification tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteAmplificationConfig {
    /// Maximum events to retain (circular buffer)
    pub max_events: usize,
}

impl Default for WriteAmplificationConfig {
    fn default() -> Self {
        Self { max_events: 10000 }
    }
}

/// Statistics aggregated from write events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WriteAmplificationStats {
    /// Total logical bytes from applications
    pub total_logical_bytes: u64,
    /// Total physical bytes written
    pub total_physical_bytes: u64,
    /// Total bytes saved by deduplication
    pub total_dedup_saved: u64,
    /// Total bytes saved by compression
    pub total_compression_saved: u64,
    /// Total erasure coding overhead
    pub total_ec_overhead: u64,
    /// Number of events aggregated
    pub event_count: u64,
}

impl WriteAmplificationStats {
    /// Write amplification factor (physical / logical)
    ///
    /// A value > 1.0 indicates write amplification.
    /// A value < 1.0 indicates net reduction.
    pub fn write_amplification(&self) -> f64 {
        if self.total_logical_bytes == 0 {
            return 1.0;
        }
        self.total_physical_bytes as f64 / self.total_logical_bytes as f64
    }

    /// Effective reduction ratio (logical / physical)
    ///
    /// Higher is better. This is the inverse of write amplification.
    pub fn effective_reduction(&self) -> f64 {
        if self.total_physical_bytes == 0 {
            return 1.0;
        }
        self.total_logical_bytes as f64 / self.total_physical_bytes as f64
    }

    /// Deduplication ratio (bytes saved / logical bytes)
    pub fn dedup_ratio(&self) -> f64 {
        if self.total_logical_bytes == 0 {
            return 0.0;
        }
        self.total_dedup_saved as f64 / self.total_logical_bytes as f64
    }

    /// Compression ratio (bytes saved / bytes after dedup)
    pub fn compression_ratio(&self) -> f64 {
        let post_dedup = self
            .total_logical_bytes
            .saturating_sub(self.total_dedup_saved);
        if post_dedup == 0 {
            return 0.0;
        }
        self.total_compression_saved as f64 / post_dedup as f64
    }

    /// EC overhead as percentage of physical bytes
    pub fn ec_overhead_pct(&self) -> f64 {
        if self.total_physical_bytes == 0 {
            return 0.0;
        }
        self.total_ec_overhead as f64 / self.total_physical_bytes as f64 * 100.0
    }

    /// Merge an event into these stats
    pub fn merge_event(&mut self, event: &WriteEvent) {
        self.total_logical_bytes += event.logical_bytes;
        self.total_physical_bytes += event.physical_bytes;
        self.total_dedup_saved += event.dedup_bytes_saved;
        self.total_compression_saved += event.compression_bytes_saved;
        self.total_ec_overhead += event.ec_overhead_bytes;
        self.event_count += 1;
    }
}

/// Tracker for write amplification across the reduction pipeline.
#[derive(Debug)]
pub struct WriteAmplificationTracker {
    events: VecDeque<WriteEvent>,
    stats: WriteAmplificationStats,
    config: WriteAmplificationConfig,
}

impl Default for WriteAmplificationTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteAmplificationTracker {
    /// Create a new tracker with default configuration
    pub fn new() -> Self {
        Self::with_config(WriteAmplificationConfig::default())
    }

    /// Create a new tracker with custom configuration
    pub fn with_config(config: WriteAmplificationConfig) -> Self {
        Self {
            events: VecDeque::with_capacity(config.max_events),
            stats: WriteAmplificationStats::default(),
            config,
        }
    }

    /// Record a write event
    pub fn record(&mut self, event: WriteEvent) {
        // If at capacity, remove oldest and adjust stats
        if self.events.len() >= self.config.max_events {
            if let Some(oldest) = self.events.pop_front() {
                self.subtract_event(&oldest);
            }
        }

        self.stats.merge_event(&event);
        self.events.push_back(event);
    }

    /// Subtract an event from stats (for circular buffer eviction)
    fn subtract_event(&mut self, event: &WriteEvent) {
        self.stats.total_logical_bytes = self
            .stats
            .total_logical_bytes
            .saturating_sub(event.logical_bytes);
        self.stats.total_physical_bytes = self
            .stats
            .total_physical_bytes
            .saturating_sub(event.physical_bytes);
        self.stats.total_dedup_saved = self
            .stats
            .total_dedup_saved
            .saturating_sub(event.dedup_bytes_saved);
        self.stats.total_compression_saved = self
            .stats
            .total_compression_saved
            .saturating_sub(event.compression_bytes_saved);
        self.stats.total_ec_overhead = self
            .stats
            .total_ec_overhead
            .saturating_sub(event.ec_overhead_bytes);
        self.stats.event_count = self.stats.event_count.saturating_sub(1);
    }

    /// Get current aggregate statistics
    pub fn stats(&self) -> &WriteAmplificationStats {
        &self.stats
    }

    /// Reset all tracked data
    pub fn reset(&mut self) {
        self.events.clear();
        self.stats = WriteAmplificationStats::default();
    }

    /// Get statistics for the last N events
    pub fn window_stats(&self, last_n: usize) -> WriteAmplificationStats {
        let count = last_n.min(self.events.len());
        let start = self.events.len().saturating_sub(count);

        let mut window_stats = WriteAmplificationStats::default();
        for event in self.events.iter().skip(start) {
            window_stats.merge_event(event);
        }

        window_stats
    }

    /// Number of events currently tracked
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_zero_stats() {
        let tracker = WriteAmplificationTracker::new();
        let stats = tracker.stats();

        assert_eq!(stats.total_logical_bytes, 0);
        assert_eq!(stats.total_physical_bytes, 0);
        assert_eq!(stats.event_count, 0);
    }

    #[test]
    fn test_record_single_event() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 500,
            dedup_bytes_saved: 200,
            compression_bytes_saved: 200,
            ec_overhead_bytes: 50,
            timestamp_ms: 1000,
        });

        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 1000);
        assert_eq!(stats.total_physical_bytes, 500);
        assert_eq!(stats.total_dedup_saved, 200);
        assert_eq!(stats.event_count, 1);
    }

    #[test]
    fn test_write_amplification_calculation() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 1000,
            total_physical_bytes: 500,
            ..Default::default()
        };

        assert_eq!(stats.write_amplification(), 0.5);
    }

    #[test]
    fn test_effective_reduction_calculation() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 1000,
            total_physical_bytes: 500,
            ..Default::default()
        };

        assert_eq!(stats.effective_reduction(), 2.0);
    }

    #[test]
    fn test_dedup_ratio_calculation() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 1000,
            total_dedup_saved: 300,
            ..Default::default()
        };

        assert_eq!(stats.dedup_ratio(), 0.3);
    }

    #[test]
    fn test_compression_ratio_calculation() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 1000,
            total_dedup_saved: 200,
            total_compression_saved: 400,
            ..Default::default()
        };

        // post_dedup = 1000 - 200 = 800
        // compression_ratio = 400 / 800 = 0.5
        assert_eq!(stats.compression_ratio(), 0.5);
    }

    #[test]
    fn test_ec_overhead_pct_calculation() {
        let stats = WriteAmplificationStats {
            total_physical_bytes: 1000,
            total_ec_overhead: 100,
            ..Default::default()
        };

        assert_eq!(stats.ec_overhead_pct(), 10.0);
    }

    #[test]
    fn test_multiple_events_accumulate() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 500,
            ..Default::default()
        });

        tracker.record(WriteEvent {
            logical_bytes: 2000,
            physical_bytes: 800,
            ..Default::default()
        });

        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 3000);
        assert_eq!(stats.total_physical_bytes, 1300);
        assert_eq!(stats.event_count, 2);
    }

    #[test]
    fn test_reset_clears_stats() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 500,
            ..Default::default()
        });

        tracker.reset();

        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 0);
        assert_eq!(stats.event_count, 0);
        assert_eq!(tracker.event_count(), 0);
    }

    #[test]
    fn test_window_stats_fewer_than_window() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 100,
            ..Default::default()
        });

        let window = tracker.window_stats(10);
        assert_eq!(window.total_logical_bytes, 100);
        assert_eq!(window.event_count, 1);
    }

    #[test]
    fn test_window_stats_exact_window() {
        let mut tracker = WriteAmplificationTracker::new();

        for i in 0..5 {
            tracker.record(WriteEvent {
                logical_bytes: (i + 1) as u64 * 100,
                ..Default::default()
            });
        }

        let window = tracker.window_stats(5);
        assert_eq!(window.total_logical_bytes, 100 + 200 + 300 + 400 + 500);
        assert_eq!(window.event_count, 5);
    }

    #[test]
    fn test_window_stats_more_than_window() {
        let mut tracker = WriteAmplificationTracker::new();

        for i in 0..10 {
            tracker.record(WriteEvent {
                logical_bytes: (i + 1) as u64 * 100,
                ..Default::default()
            });
        }

        // Window of 3 should get last 3 events: 800, 900, 1000
        let window = tracker.window_stats(3);
        assert_eq!(window.total_logical_bytes, 800 + 900 + 1000);
        assert_eq!(window.event_count, 3);
    }

    #[test]
    fn test_circular_buffer_eviction() {
        let mut tracker =
            WriteAmplificationTracker::with_config(WriteAmplificationConfig { max_events: 3 });

        for i in 0..5 {
            tracker.record(WriteEvent {
                logical_bytes: (i + 1) as u64 * 100,
                ..Default::default()
            });
        }

        // Only last 3 events should remain: 300, 400, 500
        assert_eq!(tracker.event_count(), 3);
        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 300 + 400 + 500);
    }

    #[test]
    fn test_zero_logical_bytes_edge_case() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 0,
            total_physical_bytes: 100,
            ..Default::default()
        };

        // Should return 1.0 to avoid division by zero
        assert_eq!(stats.write_amplification(), 1.0);
        assert_eq!(stats.dedup_ratio(), 0.0);
    }

    #[test]
    fn test_event_no_dedup_or_compression() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 1000, // No reduction
            dedup_bytes_saved: 0,
            compression_bytes_saved: 0,
            ec_overhead_bytes: 0,
            timestamp_ms: 0,
        });

        let stats = tracker.stats();
        assert_eq!(stats.write_amplification(), 1.0);
        assert_eq!(stats.effective_reduction(), 1.0);
    }

    #[test]
    fn test_event_full_dedup() {
        let mut tracker = WriteAmplificationTracker::new();

        // All logical bytes deduped - nothing stored
        tracker.record(WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 0, // All deduped away
            dedup_bytes_saved: 1000,
            compression_bytes_saved: 0,
            ec_overhead_bytes: 0,
            timestamp_ms: 0,
        });

        let stats = tracker.stats();
        assert_eq!(stats.dedup_ratio(), 1.0);
        // compression_ratio = saved / (logical - dedup_saved) = 0 / 0 = 0
        assert_eq!(stats.compression_ratio(), 0.0);
    }

    #[test]
    fn test_config_default_values() {
        let config = WriteAmplificationConfig::default();
        assert_eq!(config.max_events, 10000);
    }

    #[test]
    fn test_stats_merge_event() {
        let mut stats = WriteAmplificationStats::default();
        let event = WriteEvent {
            logical_bytes: 1000,
            physical_bytes: 500,
            dedup_bytes_saved: 200,
            compression_bytes_saved: 150,
            ec_overhead_bytes: 50,
            timestamp_ms: 1000,
        };
        stats.merge_event(&event);
        assert_eq!(stats.total_logical_bytes, 1000);
        assert_eq!(stats.total_physical_bytes, 500);
        assert_eq!(stats.total_dedup_saved, 200);
        assert_eq!(stats.total_compression_saved, 150);
        assert_eq!(stats.total_ec_overhead, 50);
        assert_eq!(stats.event_count, 1);
    }

    #[test]
    fn test_write_event_default() {
        let event = WriteEvent::default();
        assert_eq!(event.logical_bytes, 0);
        assert_eq!(event.physical_bytes, 0);
        assert_eq!(event.dedup_bytes_saved, 0);
        assert_eq!(event.compression_bytes_saved, 0);
        assert_eq!(event.ec_overhead_bytes, 0);
        assert_eq!(event.timestamp_ms, 0);
    }

    #[test]
    fn test_zero_physical_bytes_edge_case() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 1000,
            total_physical_bytes: 0,
            ..Default::default()
        };
        assert_eq!(stats.effective_reduction(), 1.0);
        assert_eq!(stats.ec_overhead_pct(), 0.0);
    }

    #[test]
    fn test_high_write_amplification() {
        let stats = WriteAmplificationStats {
            total_logical_bytes: 100,
            total_physical_bytes: 500,
            ..Default::default()
        };
        assert_eq!(stats.write_amplification(), 5.0);
    }

    #[test]
    fn test_circular_buffer_exact_capacity() {
        let mut tracker =
            WriteAmplificationTracker::with_config(WriteAmplificationConfig { max_events: 5 });

        for i in 0..5 {
            tracker.record(WriteEvent {
                logical_bytes: (i + 1) as u64 * 100,
                ..Default::default()
            });
        }

        assert_eq!(tracker.event_count(), 5);
        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 100 + 200 + 300 + 400 + 500);
    }

    #[test]
    fn test_window_stats_empty_tracker() {
        let tracker = WriteAmplificationTracker::new();
        let window = tracker.window_stats(5);
        assert_eq!(window.event_count, 0);
        assert_eq!(window.total_logical_bytes, 0);
    }

    #[test]
    fn test_record_with_all_fields() {
        let mut tracker = WriteAmplificationTracker::new();

        tracker.record(WriteEvent {
            logical_bytes: 2000,
            physical_bytes: 800,
            dedup_bytes_saved: 600,
            compression_bytes_saved: 400,
            ec_overhead_bytes: 100,
            timestamp_ms: 5000,
        });

        let stats = tracker.stats();
        assert_eq!(stats.total_logical_bytes, 2000);
        assert_eq!(stats.total_physical_bytes, 800);
        assert_eq!(stats.total_dedup_saved, 600);
        assert_eq!(stats.total_compression_saved, 400);
        assert_eq!(stats.total_ec_overhead, 100);
        assert!((stats.dedup_ratio() - 0.3).abs() < 1e-10);
    }

    #[test]
    fn test_stats_event_count_increment() {
        let mut stats = WriteAmplificationStats::default();
        for _ in 0..5 {
            stats.merge_event(&WriteEvent::default());
        }
        assert_eq!(stats.event_count, 5);
    }
}
