//! Sequential access pattern detection and prefetch planning.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Detected access pattern for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Random or unknown access pattern.
    Random,
    /// Sequential forward access — client reading chunks in order.
    Sequential,
    /// Stride pattern — regular gaps between accesses.
    Stride {
        /// The stride size in bytes.
        stride_bytes: u64,
    },
}

/// Configuration for the prefetch tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    /// Number of recent accesses to track per file handle.
    pub history_len: usize,
    /// Number of chunks to prefetch ahead when sequential pattern detected.
    pub prefetch_depth: usize,
    /// Minimum confidence (fraction of history matching pattern) to declare sequential.
    pub sequential_confidence: f64,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            history_len: 8,
            prefetch_depth: 4,
            sequential_confidence: 0.75,
        }
    }
}

/// A prefetch hint: the file handle and offset range to prefetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchHint {
    /// File handle identifier.
    pub file_id: u64,
    /// Start byte offset for prefetch.
    pub start_offset: u64,
    /// Length in bytes to prefetch.
    pub length: u64,
}

/// History of recent accesses for a single file.
pub struct AccessHistory {
    recent: VecDeque<u64>,
    capacity: usize,
}

impl AccessHistory {
    /// Create a new access history with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            recent: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a new access offset into the history.
    pub fn push(&mut self, offset: u64) {
        if self.recent.len() >= self.capacity {
            self.recent.pop_front();
        }
        self.recent.push_back(offset);
    }

    /// Number of accesses in history.
    pub fn len(&self) -> usize {
        self.recent.len()
    }

    /// True if history is empty.
    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }

    /// Detect pattern from recent accesses.
    ///
    /// Returns `Random` if insufficient history.
    pub fn detect_pattern(&self, chunk_size: u64) -> AccessPattern {
        if self.recent.len() < 3 {
            return AccessPattern::Random;
        }

        // Compute differences between consecutive offsets
        let diffs: Vec<u64> = self
            .recent
            .iter()
            .skip(1)
            .zip(self.recent.iter())
            .filter_map(
                |(&curr, &prev)| {
                    if curr > prev {
                        Some(curr - prev)
                    } else {
                        None
                    }
                },
            )
            .collect();

        if diffs.is_empty() {
            return AccessPattern::Random;
        }

        // Check for sequential pattern (all diffs approximately chunk_size)
        let tolerance = (chunk_size as f64 * 0.1) as u64;
        let sequential_matches = diffs
            .iter()
            .filter(|&&d| {
                d >= chunk_size.saturating_sub(tolerance)
                    && d <= chunk_size.saturating_add(tolerance)
            })
            .count();

        let sequential_fraction = sequential_matches as f64 / diffs.len() as f64;

        // Check for stride pattern (all diffs approximately equal to some constant)
        let first_diff = diffs[0];
        let stride_matches = diffs
            .iter()
            .filter(|&&d| {
                let stride_tolerance = (first_diff as f64 * 0.1) as u64;
                d >= first_diff.saturating_sub(stride_tolerance)
                    && d <= first_diff.saturating_add(stride_tolerance)
            })
            .count();

        let stride_fraction = stride_matches as f64 / diffs.len() as f64;

        if sequential_fraction >= 0.75 {
            AccessPattern::Sequential
        } else if stride_fraction >= 0.75 && first_diff != chunk_size {
            AccessPattern::Stride {
                stride_bytes: first_diff,
            }
        } else {
            AccessPattern::Random
        }
    }
}

/// Tracks access patterns and generates prefetch hints.
pub struct PrefetchTracker {
    config: PrefetchConfig,
    history: HashMap<u64, AccessHistory>,
}

impl PrefetchTracker {
    /// Create a new prefetch tracker with the given configuration.
    pub fn new(config: PrefetchConfig) -> Self {
        Self {
            config,
            history: HashMap::new(),
        }
    }

    /// Record a read access and return any prefetch hints.
    ///
    /// - `file_id`: opaque handle ID
    /// - `offset`: byte offset accessed
    /// - `chunk_size`: hint for stride detection
    pub fn record_access(
        &mut self,
        file_id: u64,
        offset: u64,
        chunk_size: u64,
    ) -> Vec<PrefetchHint> {
        let history = self
            .history
            .entry(file_id)
            .or_insert_with(|| AccessHistory::new(self.config.history_len));

        history.push(offset);

        let pattern = history.detect_pattern(chunk_size);
        self.generate_hints(file_id, offset, chunk_size, pattern)
    }

    /// Get the detected pattern for a file.
    pub fn get_pattern(&self, file_id: u64) -> AccessPattern {
        let chunk_size = 4096u64;
        self.history
            .get(&file_id)
            .map(|h| h.detect_pattern(chunk_size))
            .unwrap_or(AccessPattern::Random)
    }

    /// Remove tracking state for a file (called on file close).
    pub fn forget(&mut self, file_id: u64) {
        self.history.remove(&file_id);
    }

    /// How many files are currently being tracked.
    pub fn tracked_files(&self) -> usize {
        self.history.len()
    }

    fn generate_hints(
        &self,
        file_id: u64,
        offset: u64,
        chunk_size: u64,
        pattern: AccessPattern,
    ) -> Vec<PrefetchHint> {
        match pattern {
            AccessPattern::Sequential => (0..self.config.prefetch_depth)
                .map(|i| PrefetchHint {
                    file_id,
                    start_offset: offset + chunk_size * (i as u64 + 1),
                    length: chunk_size,
                })
                .collect(),
            AccessPattern::Stride { stride_bytes } => (0..self.config.prefetch_depth)
                .map(|i| PrefetchHint {
                    file_id,
                    start_offset: offset + stride_bytes * (i as u64 + 1),
                    length: chunk_size,
                })
                .collect(),
            AccessPattern::Random => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_history_is_random() {
        let history = AccessHistory::new(8);
        assert_eq!(history.detect_pattern(4096), AccessPattern::Random);
    }

    #[test]
    fn test_one_access_is_random() {
        let mut history = AccessHistory::new(8);
        history.push(0);
        assert_eq!(history.detect_pattern(4096), AccessPattern::Random);
    }

    #[test]
    fn test_sequential_detected() {
        let mut history = AccessHistory::new(8);
        for offset in [0, 4096, 8192, 12288, 16384] {
            history.push(offset);
        }
        assert_eq!(history.detect_pattern(4096), AccessPattern::Sequential);
    }

    #[test]
    fn test_sequential_generates_hints() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());
        for offset in [0u64, 4096, 8192, 12288] {
            tracker.record_access(1, offset, 4096);
        }
        let hints = tracker.record_access(1, 16384, 4096);
        assert!(!hints.is_empty());
        assert_eq!(hints[0].start_offset, 20480);
    }

    #[test]
    fn test_random_no_hints() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());
        tracker.record_access(1, 0, 4096);
        tracker.record_access(1, 50000, 4096);
        tracker.record_access(1, 100, 4096);
        let hints = tracker.record_access(1, 99999, 4096);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_hints_start_after_last_access() {
        let config = PrefetchConfig {
            prefetch_depth: 4,
            ..Default::default()
        };
        let mut tracker = PrefetchTracker::new(config);
        for offset in [0u64, 4096, 8192, 12288] {
            tracker.record_access(1, offset, 4096);
        }
        let hints = tracker.record_access(1, 16384, 4096);
        assert_eq!(hints[0].start_offset, 16384 + 4096);
    }

    #[test]
    fn test_prefetch_depth_respected() {
        let config = PrefetchConfig {
            prefetch_depth: 4,
            ..Default::default()
        };
        let mut tracker = PrefetchTracker::new(config);
        for offset in [0u64, 4096, 8192, 12288] {
            tracker.record_access(1, offset, 4096);
        }
        let hints = tracker.record_access(1, 16384, 4096);
        assert_eq!(hints.len(), 4);
    }

    #[test]
    fn test_stride_pattern_detected() {
        let mut history = AccessHistory::new(8);
        for offset in [0u64, 8192, 16384, 24576] {
            history.push(offset);
        }
        let pattern = history.detect_pattern(4096);
        assert!(matches!(
            pattern,
            AccessPattern::Stride { stride_bytes: 8192 }
        ));
    }

    #[test]
    fn test_forget_removes_state() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());
        tracker.record_access(1, 0, 4096);
        assert_eq!(tracker.tracked_files(), 1);
        tracker.forget(1);
        assert_eq!(tracker.tracked_files(), 0);
    }

    #[test]
    fn test_multiple_files_independent() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());

        // File 1: sequential
        for offset in [0u64, 4096, 8192] {
            tracker.record_access(1, offset, 4096);
        }

        // File 2: random
        tracker.record_access(2, 0, 4096);
        tracker.record_access(2, 50000, 4096);

        assert_eq!(tracker.tracked_files(), 2);
        assert_eq!(tracker.get_pattern(1), AccessPattern::Sequential);
        assert_eq!(tracker.get_pattern(2), AccessPattern::Random);
    }

    #[test]
    fn test_history_bounded_by_capacity() {
        let config = PrefetchConfig {
            history_len: 4,
            ..Default::default()
        };
        let mut tracker = PrefetchTracker::new(config);

        for offset in 0..10 {
            tracker.record_access(1, offset * 4096, 4096);
        }

        let history = tracker.history.get(&1).unwrap();
        assert!(history.len() <= 4);
    }

    #[test]
    fn test_pattern_stabilizes_after_enough_history() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());

        // Start with random
        tracker.record_access(1, 0, 4096);
        tracker.record_access(1, 99999, 4096);

        // Then switch to sequential
        for offset in [0u64, 4096, 8192, 12288, 16384, 20480] {
            tracker.record_access(1, offset, 4096);
        }

        assert_eq!(tracker.get_pattern(1), AccessPattern::Sequential);
    }

    #[test]
    fn test_access_at_zero_offset() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());
        let hints = tracker.record_access(1, 0, 4096);
        assert!(hints.is_empty()); // Not enough history yet
    }

    #[test]
    fn test_tracked_files_count() {
        let mut tracker = PrefetchTracker::new(PrefetchConfig::default());
        assert_eq!(tracker.tracked_files(), 0);

        tracker.record_access(1, 0, 4096);
        assert_eq!(tracker.tracked_files(), 1);

        tracker.record_access(2, 0, 4096);
        assert_eq!(tracker.tracked_files(), 2);

        tracker.record_access(1, 4096, 4096);
        assert_eq!(tracker.tracked_files(), 2); // Still 2 files
    }
}
