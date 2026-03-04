//! Prefetch engine for sequential read-ahead prediction.
//!
//! Detects sequential access patterns and pre-fetches blocks before they are requested.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur in the prefetch engine.
#[derive(Debug, Error)]
pub enum PrefetchError {
    #[error("Stream not found: {0}")]
    StreamNotFound(u64),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Configuration for the prefetch engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    /// Minimum consecutive sequential accesses to trigger prefetch.
    pub sequential_threshold: usize,
    /// Number of blocks to prefetch ahead when pattern detected.
    pub lookahead_blocks: usize,
    /// Maximum number of streams to track simultaneously.
    pub max_streams: usize,
    /// How many accesses to keep in history window per stream.
    pub history_window: usize,
    /// Minimum confidence to issue prefetch (0.0-1.0).
    pub confidence_threshold: f64,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            sequential_threshold: 3,
            lookahead_blocks: 8,
            max_streams: 1024,
            history_window: 16,
            confidence_threshold: 0.6,
        }
    }
}

/// A hint indicating a block to pre-fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchHint {
    /// The offset of the block to prefetch.
    pub offset: u64,
    /// The size of the block to prefetch.
    pub size: u64,
}

/// Statistics for the prefetch engine.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PrefetchStats {
    /// Number of streams currently tracked.
    pub streams_tracked: usize,
    /// Total predictions issued.
    pub predictions_issued: u64,
    /// Streams detected as sequential.
    pub sequential_streams_detected: u64,
    /// Streams detected as random.
    pub random_streams_detected: u64,
    /// Streams that were cancelled.
    pub streams_cancelled: u64,
}

/// A record of a single block access.
#[derive(Debug, Clone)]
struct AccessRecord {
    offset: u64,
    size: u64,
}

/// State tracking for a single stream.
#[derive(Debug)]
struct StreamState {
    history: Vec<AccessRecord>,
    confidence: f64,
    is_sequential: bool,
    last_access_time: u64,
}

impl StreamState {
    fn new() -> Self {
        Self {
            history: Vec::new(),
            confidence: 0.5,
            is_sequential: false,
            last_access_time: 0,
        }
    }

    fn add_access(&mut self, offset: u64, size: u64, current_time: u64, config: &PrefetchConfig) {
        self.last_access_time = current_time;
        self.history.push(AccessRecord { offset, size });
        if self.history.len() > config.history_window {
            self.history.remove(0);
        }
        self.detect_pattern(config);
    }

    fn detect_pattern(&mut self, config: &PrefetchConfig) {
        let history_len = self.history.len();
        if history_len < 2 {
            self.is_sequential = false;
            return;
        }

        let check_window = config.sequential_threshold.min(history_len);
        let start = history_len - check_window;

        let mut is_seq = true;
        for i in (start + 1)..history_len {
            let expected = self.history[i - 1].offset + self.history[i - 1].size;
            if self.history[i].offset != expected {
                is_seq = false;
                break;
            }
        }

        if is_seq && history_len >= config.sequential_threshold {
            if !self.is_sequential {
                self.confidence = (self.confidence + 0.3).min(1.0);
            }
            self.is_sequential = true;
        } else if !is_seq {
            if self.is_sequential {
                self.confidence = (self.confidence - 0.3).max(0.0);
            }
            self.is_sequential = false;
        }
    }

    fn get_next_offsets(&self, config: &PrefetchConfig) -> Vec<PrefetchHint> {
        if !self.is_sequential || self.history.is_empty() {
            return Vec::new();
        }

        let last = self.history.last().unwrap();
        let next_offset = last.offset.saturating_add(last.size);

        (0..config.lookahead_blocks)
            .map(|i| {
                let offset =
                    next_offset.saturating_add(i.saturating_mul(last.size as usize) as u64);
                PrefetchHint {
                    offset: offset.min(u64::MAX - last.size as u64),
                    size: last.size,
                }
            })
            .collect()
    }
}

/// Sequential read-ahead prediction engine.
///
/// Detects sequential access patterns and pre-fetches blocks before they are requested.
#[derive(Debug)]
pub struct PrefetchEngine {
    config: PrefetchConfig,
    streams: HashMap<u64, StreamState>,
    stats: PrefetchStats,
    access_counter: u64,
}

impl PrefetchEngine {
    /// Creates a new prefetch engine with the given configuration.
    pub fn new(config: PrefetchConfig) -> Self {
        Self {
            config,
            streams: HashMap::new(),
            stats: PrefetchStats::default(),
            access_counter: 0,
        }
    }

    /// Creates a new prefetch engine with default configuration.
    pub fn default_config() -> Self {
        Self::new(PrefetchConfig::default())
    }

    /// Records a block access for a stream.
    pub fn record_access(&mut self, stream_id: u64, block_offset: u64, size: u64) {
        self.access_counter += 1;

        if self.streams.len() >= self.config.max_streams && !self.streams.contains_key(&stream_id) {
            self.evict_lru_stream();
        }

        let stream = self
            .streams
            .entry(stream_id)
            .or_insert_with(StreamState::new);
        let was_sequential = stream.is_sequential;
        stream.add_access(block_offset, size, self.access_counter, &self.config);
        let is_now_sequential = stream.is_sequential;
        let history_len = stream.history.len();

        if is_now_sequential && !was_sequential {
            self.stats.sequential_streams_detected += 1;
        } else if !is_now_sequential && was_sequential {
            self.stats.random_streams_detected += 1;
        } else if !is_now_sequential && !was_sequential && history_len == 2 {
            self.stats.random_streams_detected += 1;
        }
    }

    /// Returns prefetch hints for a stream.
    pub fn get_prefetch_advice(&mut self, stream_id: u64) -> Vec<PrefetchHint> {
        let Some(stream) = self.streams.get_mut(&stream_id) else {
            return Vec::new();
        };

        if stream.confidence < self.config.confidence_threshold {
            return Vec::new();
        }

        if stream.history.len() < self.config.sequential_threshold {
            return Vec::new();
        }

        let hints = stream.get_next_offsets(&self.config);
        if !hints.is_empty() {
            self.stats.predictions_issued += 1;
        }
        hints
    }

    /// Cancels tracking for a stream.
    pub fn cancel_stream(&mut self, stream_id: u64) {
        if self.streams.remove(&stream_id).is_some() {
            self.stats.streams_cancelled += 1;
        }
    }

    /// Returns statistics for the prefetch engine.
    pub fn stats(&self) -> PrefetchStats {
        PrefetchStats {
            streams_tracked: self.streams.len(),
            predictions_issued: self.stats.predictions_issued,
            sequential_streams_detected: self.stats.sequential_streams_detected,
            random_streams_detected: self.stats.random_streams_detected,
            streams_cancelled: self.stats.streams_cancelled,
        }
    }

    fn evict_lru_stream(&mut self) {
        if let Some((lru_id, _)) = self.streams.iter().min_by_key(|(_, s)| s.last_access_time) {
            let id = *lru_id;
            self.streams.remove(&id);
            self.stats.streams_cancelled += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = PrefetchConfig::default();
        assert_eq!(config.sequential_threshold, 3);
        assert_eq!(config.lookahead_blocks, 8);
        assert_eq!(config.max_streams, 1024);
        assert_eq!(config.history_window, 16);
        assert_eq!(config.confidence_threshold, 0.6);
    }

    #[test]
    fn test_sequential_pattern_detection() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(hints.is_empty(), "Below threshold, no prefetch");

        engine.record_access(1, 8192, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(!hints.is_empty(), "At threshold, should prefetch");
        assert_eq!(hints[0].offset, 12288);
    }

    #[test]
    fn test_random_access_no_prefetch() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 10000, 4096);
        engine.record_access(1, 500, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(
            hints.is_empty(),
            "Random access should not trigger prefetch"
        );
    }

    #[test]
    fn test_prefetch_offsets_correct() {
        let mut engine = PrefetchEngine::default_config();

        for i in 0..3 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }

        let hints = engine.get_prefetch_advice(1);
        assert_eq!(hints.len(), 8);
        assert_eq!(hints[0].offset, 12288);
        assert_eq!(hints[1].offset, 16384);
    }

    #[test]
    fn test_confidence_increases_on_sequential() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);
        engine.record_access(1, 8192, 4096);

        let stats = engine.stats();
        assert!(stats.sequential_streams_detected >= 1);
    }

    #[test]
    fn test_confidence_drops_on_random() {
        let mut engine = PrefetchEngine::default_config();

        for i in 0..3 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }

        engine.record_access(1, 50000, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(hints.is_empty(), "Confidence should drop below threshold");
    }

    #[test]
    fn test_cancel_stream_removes_tracking() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.cancel_stream(1);

        let hints = engine.get_prefetch_advice(1);
        assert!(hints.is_empty(), "Stream should be removed");

        let stats = engine.stats();
        assert_eq!(stats.streams_cancelled, 1);
    }

    #[test]
    fn test_max_streams_eviction() {
        let mut config = PrefetchConfig::default();
        config.max_streams = 2;
        let mut engine = PrefetchEngine::new(config);

        engine.record_access(1, 0, 4096);
        engine.record_access(2, 0, 4096);
        engine.record_access(3, 0, 4096);

        let stats = engine.stats();
        assert_eq!(stats.streams_tracked, 2);
    }

    #[test]
    fn test_empty_history_no_advice() {
        let mut engine = PrefetchEngine::default_config();

        let hints = engine.get_prefetch_advice(999);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_stats_tracking() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);
        engine.record_access(1, 8192, 4096);

        engine.get_prefetch_advice(1);

        let stats = engine.stats();
        assert!(stats.predictions_issued >= 1);
    }

    #[test]
    fn test_single_access_no_pattern() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_two_sequential_below_threshold() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(hints.is_empty(), "Two accesses below threshold of 3");
    }

    #[test]
    fn test_at_threshold_prefetch() {
        let mut config = PrefetchConfig::default();
        config.sequential_threshold = 2;
        let mut engine = PrefetchEngine::new(config);

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);

        let hints = engine.get_prefetch_advice(1);
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_variable_block_sizes() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 65536);
        engine.record_access(1, 65536, 65536);
        engine.record_access(1, 131072, 65536);

        let hints = engine.get_prefetch_advice(1);
        assert!(!hints.is_empty());
        assert_eq!(hints[0].size, 65536);
    }

    #[test]
    fn test_multiple_independent_streams() {
        let mut engine = PrefetchEngine::default_config();

        for i in 0..3 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }
        for i in 0..3 {
            engine.record_access(2, i as u64 * 8192, 8192);
        }

        let hints1 = engine.get_prefetch_advice(1);
        let hints2 = engine.get_prefetch_advice(2);

        assert!(!hints1.is_empty());
        assert!(!hints2.is_empty());
    }

    #[test]
    fn test_redetection_after_random_break() {
        let mut engine = PrefetchEngine::default_config();

        for i in 0..3 {
            engine.record_access(1, i as u64 * 4096, 4096);
        }

        engine.record_access(1, 100000, 4096);

        for i in 0..5 {
            engine.record_access(1, (200000 + i as u64 * 4096), 4096);
        }

        let hints = engine.get_prefetch_advice(1);
        assert!(!hints.is_empty(), "Pattern should be re-detected");
    }

    #[test]
    fn test_prefetch_hints_max_offset_safety() {
        let mut engine = PrefetchEngine::default_config();

        let large_offset = u64::MAX - 32768;
        engine.record_access(1, large_offset, 4096);
        engine.record_access(1, large_offset + 4096, 4096);
        engine.record_access(1, large_offset + 8192, 4096);

        let hints = engine.get_prefetch_advice(1);
        for hint in &hints {
            assert!(hint.offset <= u64::MAX - 4096, "Offset should not overflow");
        }
    }

    #[test]
    fn test_sequential_streams_detected_counter() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 4096, 4096);
        engine.record_access(1, 8192, 4096);

        let stats = engine.stats();
        assert!(stats.sequential_streams_detected >= 1);
    }

    #[test]
    fn test_random_streams_detected_counter() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 4096);
        engine.record_access(1, 50000, 4096);

        let stats = engine.stats();
        assert!(stats.random_streams_detected >= 1);
    }

    #[test]
    fn test_1mb_block_size() {
        let mut engine = PrefetchEngine::default_config();

        engine.record_access(1, 0, 1048576);
        engine.record_access(1, 1048576, 1048576);
        engine.record_access(1, 2097152, 1048576);

        let hints = engine.get_prefetch_advice(1);
        assert!(!hints.is_empty());
        assert_eq!(hints[0].size, 1048576);
    }

    #[test]
    fn test_custom_config() {
        let config = PrefetchConfig {
            sequential_threshold: 5,
            lookahead_blocks: 16,
            max_streams: 512,
            history_window: 32,
            confidence_threshold: 0.8,
        };

        let engine = PrefetchEngine::new(config.clone());
        assert_eq!(engine.stats().streams_tracked, 0);
    }
}
