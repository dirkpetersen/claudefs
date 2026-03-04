//! Dedup analytics for capacity planning and reporting.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single dedup sample point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupSample {
    /// Timestamp of the sample in milliseconds.
    pub timestamp_ms: u64,
    /// Total logical bytes (before dedup).
    pub total_logical_bytes: u64,
    /// Total physical bytes (after dedup).
    pub total_physical_bytes: u64,
    /// Number of unique chunks.
    pub unique_chunks: u64,
    /// Dedup ratio (logical / physical).
    pub dedup_ratio: f64,
}

/// Trend direction for dedup ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupTrend {
    /// Dedup ratio is improving (increasing).
    Improving,
    /// Dedup ratio is stable.
    Stable,
    /// Dedup ratio is degrading (decreasing).
    Degrading,
}

/// Analytics engine for dedup metrics with rolling window.
pub struct DedupAnalytics {
    samples: VecDeque<DedupSample>,
    window_size: usize,
}

impl DedupAnalytics {
    /// Creates a new analytics engine with the given window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Records a new sample, evicting the oldest if window is full.
    pub fn record_sample(&mut self, sample: DedupSample) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Returns the current (most recent) dedup ratio.
    pub fn current_ratio(&self) -> Option<f64> {
        self.samples.back().map(|s| s.dedup_ratio)
    }

    /// Returns the average dedup ratio across all samples.
    pub fn average_ratio(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }

        let sum: f64 = self.samples.iter().map(|s| s.dedup_ratio).sum();
        Some(sum / self.samples.len() as f64)
    }

    /// Determines the trend by comparing first half vs second half average.
    pub fn trend(&self) -> DedupTrend {
        if self.samples.len() < 2 {
            return DedupTrend::Stable;
        }

        let mid = self.samples.len() / 2;
        let first_half_avg: f64 = self
            .samples
            .iter()
            .take(mid)
            .map(|s| s.dedup_ratio)
            .sum::<f64>()
            / mid.max(1) as f64;
        let second_half_avg: f64 = self
            .samples
            .iter()
            .skip(mid)
            .map(|s| s.dedup_ratio)
            .sum::<f64>()
            / (self.samples.len() - mid).max(1) as f64;

        if second_half_avg > first_half_avg + 0.05 {
            DedupTrend::Improving
        } else if second_half_avg < first_half_avg - 0.05 {
            DedupTrend::Degrading
        } else {
            DedupTrend::Stable
        }
    }

    /// Returns the peak (maximum) dedup ratio seen.
    pub fn peak_ratio(&self) -> Option<f64> {
        self.samples
            .iter()
            .map(|s| s.dedup_ratio)
            .fold(None, |max, r| Some(max.map_or(r, |m: f64| m.max(r))))
    }

    /// Returns the storage savings in bytes (logical - physical) from the last sample.
    pub fn savings_bytes(&self) -> Option<u64> {
        self.samples
            .back()
            .map(|s| s.total_logical_bytes.saturating_sub(s.total_physical_bytes))
    }

    /// Returns the number of samples in the window.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Estimates future physical capacity using linear extrapolation.
    ///
    /// Uses the last 2 samples to project physical bytes growth.
    pub fn estimate_future_capacity(&self, months_ahead: u32) -> Option<u64> {
        if self.samples.len() < 2 {
            return None;
        }

        let samples: Vec<_> = self.samples.iter().rev().take(2).collect();
        let s1 = samples[0];
        let s2 = samples[1];

        let time_diff_months = ((s1.timestamp_ms as f64 - s2.timestamp_ms as f64)
            / (30.0 * 24.0 * 3600.0 * 1000.0))
            .max(1.0);
        let bytes_diff = s1.total_physical_bytes as f64 - s2.total_physical_bytes as f64;
        let bytes_per_month = bytes_diff / time_diff_months;

        let estimated = s1.total_physical_bytes as f64 + bytes_per_month * months_ahead as f64;
        Some(estimated.max(0.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample(ts: u64, logical: u64, physical: u64, ratio: f64) -> DedupSample {
        DedupSample {
            timestamp_ms: ts,
            total_logical_bytes: logical,
            total_physical_bytes: physical,
            unique_chunks: 100,
            dedup_ratio: ratio,
        }
    }

    #[test]
    fn new_analytics_empty() {
        let analytics = DedupAnalytics::new(10);
        assert_eq!(analytics.sample_count(), 0);
        assert!(analytics.current_ratio().is_none());
    }

    #[test]
    fn record_sample_adds_entry() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));

        assert_eq!(analytics.sample_count(), 1);
        assert_eq!(analytics.current_ratio(), Some(2.0));
    }

    #[test]
    fn record_sample_rolling_window() {
        let mut analytics = DedupAnalytics::new(3);

        for i in 0..5 {
            analytics.record_sample(make_sample(i * 1000, 1000, 500, i as f64 + 1.0));
        }

        assert_eq!(analytics.sample_count(), 3);
        assert_eq!(analytics.current_ratio(), Some(5.0));
    }

    #[test]
    fn current_ratio_empty() {
        let analytics = DedupAnalytics::new(10);
        assert!(analytics.current_ratio().is_none());
    }

    #[test]
    fn current_ratio_with_sample() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.5));

        assert_eq!(analytics.current_ratio(), Some(2.5));
    }

    #[test]
    fn average_ratio_single_sample() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 3.0));

        assert_eq!(analytics.average_ratio(), Some(3.0));
    }

    #[test]
    fn average_ratio_multiple() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 4.0));

        let avg = analytics.average_ratio().unwrap();
        assert!((avg - 3.0).abs() < 0.001);
    }

    #[test]
    fn trend_empty_returns_stable() {
        let analytics = DedupAnalytics::new(10);
        assert_eq!(analytics.trend(), DedupTrend::Stable);
    }

    #[test]
    fn trend_improving() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 1.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 2.0));

        assert_eq!(analytics.trend(), DedupTrend::Improving);
    }

    #[test]
    fn trend_degrading() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 3.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 1.0));

        assert_eq!(analytics.trend(), DedupTrend::Degrading);
    }

    #[test]
    fn trend_stable() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 2.02));

        assert_eq!(analytics.trend(), DedupTrend::Stable);
    }

    #[test]
    fn peak_ratio() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 5.0));
        analytics.record_sample(make_sample(3000, 1000, 500, 3.0));

        assert_eq!(analytics.peak_ratio(), Some(5.0));
    }

    #[test]
    fn savings_bytes() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 300, 3.33));

        assert_eq!(analytics.savings_bytes(), Some(700));
    }

    #[test]
    fn estimate_future_capacity_empty() {
        let analytics = DedupAnalytics::new(10);
        assert!(analytics.estimate_future_capacity(6).is_none());
    }

    #[test]
    fn sample_count() {
        let mut analytics = DedupAnalytics::new(10);
        assert_eq!(analytics.sample_count(), 0);

        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        assert_eq!(analytics.sample_count(), 1);

        analytics.record_sample(make_sample(2000, 1000, 500, 2.0));
        assert_eq!(analytics.sample_count(), 2);
    }

    #[test]
    fn estimate_future_capacity_with_samples() {
        let mut analytics = DedupAnalytics::new(10);
        let one_month_ms = 30 * 24 * 3600 * 1000;

        analytics.record_sample(DedupSample {
            timestamp_ms: 1000,
            total_logical_bytes: 1000,
            total_physical_bytes: 500,
            unique_chunks: 100,
            dedup_ratio: 2.0,
        });
        analytics.record_sample(DedupSample {
            timestamp_ms: 1000 + one_month_ms,
            total_logical_bytes: 1000,
            total_physical_bytes: 600,
            unique_chunks: 100,
            dedup_ratio: 1.67,
        });

        let result = analytics.estimate_future_capacity(1);
        assert!(result.is_some());
        assert!(result.unwrap() > 600);
    }

    #[test]
    fn dedup_sample_clone() {
        let sample = make_sample(1000, 1000, 500, 2.0);
        let cloned = sample.clone();

        assert_eq!(sample.timestamp_ms, cloned.timestamp_ms);
        assert_eq!(sample.dedup_ratio, cloned.dedup_ratio);
    }

    #[test]
    fn trend_with_many_samples() {
        let mut analytics = DedupAnalytics::new(10);

        for i in 0..8 {
            analytics.record_sample(make_sample(i * 1000, 1000, 500, 1.0 + i as f64 * 0.1));
        }

        assert_eq!(analytics.trend(), DedupTrend::Improving);
    }

    #[test]
    fn peak_ratio_empty() {
        let analytics = DedupAnalytics::new(10);
        assert!(analytics.peak_ratio().is_none());
    }

    #[test]
    fn savings_bytes_empty() {
        let analytics = DedupAnalytics::new(10);
        assert!(analytics.savings_bytes().is_none());
    }

    #[test]
    fn average_ratio_empty() {
        let analytics = DedupAnalytics::new(10);
        assert!(analytics.average_ratio().is_none());
    }

    #[test]
    fn rolling_window_exact_size() {
        let mut analytics = DedupAnalytics::new(5);

        for i in 0..5 {
            analytics.record_sample(make_sample(i * 1000, 1000, 500, i as f64));
        }

        assert_eq!(analytics.sample_count(), 5);
    }

    #[test]
    fn rolling_window_evicts_oldest() {
        let mut analytics = DedupAnalytics::new(2);

        analytics.record_sample(make_sample(1000, 1000, 500, 1.0));
        analytics.record_sample(make_sample(2000, 1000, 500, 2.0));
        analytics.record_sample(make_sample(3000, 1000, 500, 3.0));

        assert_eq!(analytics.sample_count(), 2);
        assert_eq!(analytics.current_ratio(), Some(3.0));
    }

    #[test]
    fn trend_single_sample_stable() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));

        assert_eq!(analytics.trend(), DedupTrend::Stable);
    }

    #[test]
    fn estimate_future_zero_months() {
        let mut analytics = DedupAnalytics::new(10);
        analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        analytics.record_sample(make_sample(2000, 1000, 600, 1.67));

        let result = analytics.estimate_future_capacity(0);
        assert!(result.is_some());
    }

    #[test]
    fn dedup_trend_variants() {
        assert_eq!(DedupTrend::Improving, DedupTrend::Improving);
        assert_ne!(DedupTrend::Improving, DedupTrend::Degrading);
        assert_ne!(DedupTrend::Stable, DedupTrend::Improving);
    }

    #[test]
    fn trend_with_equal_halves() {
        let mut analytics = DedupAnalytics::new(10);

        for _ in 0..4 {
            analytics.record_sample(make_sample(1000, 1000, 500, 2.0));
        }

        assert_eq!(analytics.trend(), DedupTrend::Stable);
    }
}
