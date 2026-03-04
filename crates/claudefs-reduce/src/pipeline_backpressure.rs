//! Backpressure for the reduction pipeline to prevent memory exhaustion.
//!
//! When the pipeline processes faster than it can write, chunks accumulate in memory.
//! Backpressure signals the ingestion layer to slow down.

use serde::{Deserialize, Serialize};

/// Configuration for pipeline backpressure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// High watermark in bytes (default 256MB).
    pub high_watermark_bytes: usize,
    /// Low watermark in bytes (default 64MB).
    pub low_watermark_bytes: usize,
    /// High watermark in chunks (default 10000).
    pub high_watermark_chunks: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            high_watermark_bytes: 256 * 1024 * 1024,
            low_watermark_bytes: 64 * 1024 * 1024,
            high_watermark_chunks: 10000,
        }
    }
}

/// Backpressure state of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackpressureState {
    /// Normal operation, accept all writes.
    Normal,
    /// Warning, approaching limits.
    Warning,
    /// Throttled, should slow down writes.
    Throttled,
    /// Stalled, stop accepting writes.
    Stalled,
}

/// Statistics for backpressure monitoring.
#[derive(Debug, Clone, Default)]
pub struct BackpressureStats {
    /// Current bytes in flight.
    pub current_bytes: usize,
    /// Current chunks in flight.
    pub current_chunks: usize,
    /// Peak bytes observed.
    pub peak_bytes: usize,
    /// Peak chunks observed.
    pub peak_chunks: usize,
    /// Number of throttle events.
    pub throttle_events: u64,
}

/// Pipeline backpressure controller.
#[derive(Debug, Clone)]
pub struct PipelineBackpressure {
    config: BackpressureConfig,
    in_flight_bytes: usize,
    in_flight_chunks: usize,
    peak_bytes: usize,
    peak_chunks: usize,
    throttle_events: u64,
}

impl PipelineBackpressure {
    /// Create a new backpressure controller with the given configuration.
    pub fn new(config: BackpressureConfig) -> Self {
        Self {
            config,
            in_flight_bytes: 0,
            in_flight_chunks: 0,
            peak_bytes: 0,
            peak_chunks: 0,
            throttle_events: 0,
        }
    }

    /// Add bytes to the in-flight counter.
    pub fn add_bytes(&mut self, n: usize) {
        self.in_flight_bytes = self.in_flight_bytes.saturating_add(n);
        if self.in_flight_bytes > self.peak_bytes {
            self.peak_bytes = self.in_flight_bytes;
        }
        self.check_throttle_event();
    }

    /// Remove bytes from the in-flight counter.
    pub fn remove_bytes(&mut self, n: usize) {
        self.in_flight_bytes = self.in_flight_bytes.saturating_sub(n);
    }

    /// Add chunks to the in-flight counter.
    pub fn add_chunks(&mut self, n: usize) {
        self.in_flight_chunks = self.in_flight_chunks.saturating_add(n);
        if self.in_flight_chunks > self.peak_chunks {
            self.peak_chunks = self.in_flight_chunks;
        }
        self.check_throttle_event();
    }

    /// Remove chunks from the in-flight counter.
    pub fn remove_chunks(&mut self, n: usize) {
        self.in_flight_chunks = self.in_flight_chunks.saturating_sub(n);
    }

    /// Get the current backpressure state.
    pub fn state(&self) -> BackpressureState {
        let stalled_bytes = self.config.high_watermark_bytes.saturating_mul(2);
        let stalled_chunks = self.config.high_watermark_chunks.saturating_mul(2);

        if self.in_flight_bytes >= stalled_bytes || self.in_flight_chunks >= stalled_chunks {
            BackpressureState::Stalled
        } else if self.in_flight_bytes >= self.config.high_watermark_bytes
            || self.in_flight_chunks >= self.config.high_watermark_chunks
        {
            BackpressureState::Throttled
        } else if self.in_flight_bytes >= self.config.low_watermark_bytes {
            BackpressureState::Warning
        } else {
            BackpressureState::Normal
        }
    }

    /// Check if the pipeline should accept new writes.
    pub fn should_accept(&self) -> bool {
        self.state() <= BackpressureState::Warning
    }

    /// Get the current in-flight bytes.
    pub fn in_flight_bytes(&self) -> usize {
        self.in_flight_bytes
    }

    /// Get the current in-flight chunks.
    pub fn in_flight_chunks(&self) -> usize {
        self.in_flight_chunks
    }

    /// Get backpressure statistics.
    pub fn stats(&self) -> BackpressureStats {
        BackpressureStats {
            current_bytes: self.in_flight_bytes,
            current_chunks: self.in_flight_chunks,
            peak_bytes: self.peak_bytes,
            peak_chunks: self.peak_chunks,
            throttle_events: self.throttle_events,
        }
    }

    fn check_throttle_event(&mut self) {
        if self.state() >= BackpressureState::Throttled && self.throttle_events == 0
            || (self.throttle_events > 0 && self.state() == BackpressureState::Stalled)
        {
            self.throttle_events = self.throttle_events.saturating_add(1);
        }
    }
}

impl Default for PipelineBackpressure {
    fn default() -> Self {
        Self::new(BackpressureConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = BackpressureConfig::default();
        assert_eq!(config.high_watermark_bytes, 256 * 1024 * 1024);
        assert_eq!(config.low_watermark_bytes, 64 * 1024 * 1024);
        assert_eq!(config.high_watermark_chunks, 10000);
    }

    #[test]
    fn initial_state_normal() {
        let bp = PipelineBackpressure::default();
        assert_eq!(bp.state(), BackpressureState::Normal);
    }

    #[test]
    fn add_bytes_increments() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(1024);
        assert_eq!(bp.in_flight_bytes(), 1024);
    }

    #[test]
    fn remove_bytes_decrements() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(2048);
        bp.remove_bytes(1024);
        assert_eq!(bp.in_flight_bytes(), 1024);
    }

    #[test]
    fn remove_bytes_min_zero() {
        let mut bp = PipelineBackpressure::default();
        bp.remove_bytes(1000);
        assert_eq!(bp.in_flight_bytes(), 0);
    }

    #[test]
    fn state_normal() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(32 * 1024 * 1024);
        assert_eq!(bp.state(), BackpressureState::Normal);
    }

    #[test]
    fn state_warning() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(64 * 1024 * 1024);
        assert_eq!(bp.state(), BackpressureState::Warning);
    }

    #[test]
    fn state_throttled() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(256 * 1024 * 1024);
        assert_eq!(bp.state(), BackpressureState::Throttled);
    }

    #[test]
    fn state_stalled() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(512 * 1024 * 1024);
        assert_eq!(bp.state(), BackpressureState::Stalled);
    }

    #[test]
    fn should_accept_normal() {
        let bp = PipelineBackpressure::default();
        assert!(bp.should_accept());
    }

    #[test]
    fn should_accept_warning() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(64 * 1024 * 1024);
        assert!(bp.should_accept());
    }

    #[test]
    fn should_not_accept_throttled() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(256 * 1024 * 1024);
        assert!(!bp.should_accept());
    }

    #[test]
    fn should_not_accept_stalled() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(512 * 1024 * 1024);
        assert!(!bp.should_accept());
    }

    #[test]
    fn in_flight_bytes() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(100);
        assert_eq!(bp.in_flight_bytes(), 100);
    }

    #[test]
    fn in_flight_chunks() {
        let mut bp = PipelineBackpressure::default();
        bp.add_chunks(50);
        assert_eq!(bp.in_flight_chunks(), 50);
    }

    #[test]
    fn peak_bytes_tracked() {
        let mut bp = PipelineBackpressure::default();
        bp.add_bytes(1000);
        bp.add_bytes(500);
        bp.remove_bytes(1000);

        let stats = bp.stats();
        assert_eq!(stats.peak_bytes, 1500);
    }

    #[test]
    fn peak_chunks_tracked() {
        let mut bp = PipelineBackpressure::default();
        bp.add_chunks(100);
        bp.add_chunks(50);
        bp.remove_chunks(100);

        let stats = bp.stats();
        assert_eq!(stats.peak_chunks, 150);
    }

    #[test]
    fn state_throttled_by_chunks() {
        let mut bp = PipelineBackpressure::default();
        bp.add_chunks(10000);
        assert_eq!(bp.state(), BackpressureState::Throttled);
    }

    #[test]
    fn state_stalled_by_chunks() {
        let mut bp = PipelineBackpressure::default();
        bp.add_chunks(20000);
        assert_eq!(bp.state(), BackpressureState::Stalled);
    }

    #[test]
    fn remove_chunks_min_zero() {
        let mut bp = PipelineBackpressure::default();
        bp.remove_chunks(100);
        assert_eq!(bp.in_flight_chunks(), 0);
    }

    #[test]
    fn backpressure_state_ordering() {
        assert!(BackpressureState::Normal < BackpressureState::Warning);
        assert!(BackpressureState::Warning < BackpressureState::Throttled);
        assert!(BackpressureState::Throttled < BackpressureState::Stalled);
    }

    #[test]
    fn stats_default() {
        let stats = BackpressureStats::default();
        assert_eq!(stats.current_bytes, 0);
        assert_eq!(stats.current_chunks, 0);
        assert_eq!(stats.peak_bytes, 0);
        assert_eq!(stats.peak_chunks, 0);
        assert_eq!(stats.throttle_events, 0);
    }
}
