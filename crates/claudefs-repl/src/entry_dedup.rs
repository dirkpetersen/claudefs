//! Duplicate journal entry detection for cross-site replication.
//!
//! Implements a ring buffer-based deduplication detector to prevent double-apply
//! of journal entries when they are retransmitted after interrupted replication sessions.

use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use thiserror::Error;

/// Configuration for the entry deduplication ring buffer.
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Size of the ring buffer (number of recent entries to remember).
    /// Must be power of 2. Default: 4096.
    pub ring_size: usize,
    /// TTL for entries in the ring buffer (milliseconds). Default: 300_000 (5 min).
    pub entry_ttl_ms: u64,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            ring_size: 4096,
            entry_ttl_ms: 300_000,
        }
    }
}

/// A dedup entry stored in the ring buffer.
#[derive(Debug, Clone)]
pub struct DedupEntry {
    /// Entry fingerprint (8 bytes from SHA-256 of entry payload).
    pub fingerprint: u64,
    /// Sequence number of the entry.
    pub seq: u64,
    /// Time when this entry was recorded (Unix ms).
    pub recorded_at_ms: u64,
}

/// Statistics for dedup operations.
#[derive(Debug, Clone, Default)]
pub struct DedupStats {
    /// Total number of entries checked.
    pub total_checked: u64,
    /// Total number of duplicates detected.
    pub total_duplicates: u64,
    /// Total number of entries evicted.
    pub total_evictions: u64,
    /// Ring buffer fill ratio (0.0-1.0).
    pub ring_fill_ratio: f64,
}

/// Error type for dedup operations.
#[derive(Debug, Error)]
pub enum DedupError {
    /// Ring buffer is full and cannot accept new entries.
    #[error("ring buffer is full")]
    RingFull,
    /// Invalid fingerprint data.
    #[error("invalid fingerprint data")]
    InvalidFingerprint,
}

/// Deduplication detector using a ring buffer of recent entry fingerprints.
#[derive(Debug)]
pub struct EntryDedup {
    config: DedupConfig,
    ring: VecDeque<DedupEntry>,
    stats: DedupStats,
}

impl EntryDedup {
    /// Creates a new EntryDedup with the given configuration.
    pub fn new(config: DedupConfig) -> Self {
        let ring_size = config.ring_size;
        Self {
            config,
            ring: VecDeque::with_capacity(ring_size),
            stats: DedupStats::default(),
        }
    }

    /// Record an entry as seen. Returns true if this is a NEW entry (not a duplicate).
    /// Returns false if the entry was already seen (duplicate — caller should skip it).
    pub fn record(&mut self, fingerprint: u64, seq: u64, now_ms: u64) -> bool {
        self.stats.total_checked += 1;

        if self.is_duplicate_internal(fingerprint, seq) {
            self.stats.total_duplicates += 1;
            return false;
        }

        if self.ring.len() >= self.config.ring_size {
            if let Some(evicted) = self.ring.pop_front() {
                self.stats.total_evictions += 1;
                tracing::trace!(
                    fingerprint = evicted.fingerprint,
                    seq = evicted.seq,
                    "evicted old dedup entry"
                );
            }
        }

        self.ring.push_back(DedupEntry {
            fingerprint,
            seq,
            recorded_at_ms: now_ms,
        });

        self.update_fill_ratio();
        true
    }

    /// Check if an entry is a duplicate WITHOUT recording it.
    pub fn is_duplicate(&self, fingerprint: u64, seq: u64) -> bool {
        self.is_duplicate_internal(fingerprint, seq)
    }

    fn is_duplicate_internal(&self, fingerprint: u64, seq: u64) -> bool {
        self.ring
            .iter()
            .any(|e| e.fingerprint == fingerprint && e.seq == seq)
    }

    /// Evict entries older than TTL to reclaim ring buffer space.
    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let ttl = self.config.entry_ttl_ms;
        let initial_len = self.ring.len();
        self.ring
            .retain(|e| now_ms.saturating_sub(e.recorded_at_ms) <= ttl);
        let evicted = initial_len - self.ring.len();
        self.stats.total_evictions += evicted as u64;
        self.update_fill_ratio();
        evicted
    }

    /// Get current statistics.
    pub fn stats(&self) -> &DedupStats {
        &self.stats
    }

    /// Reset the dedup state (e.g., after a full resync).
    pub fn reset(&mut self) {
        self.ring.clear();
        self.stats = DedupStats::default();
    }

    /// Current number of entries in the ring buffer.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// True if the ring buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    fn update_fill_ratio(&mut self) {
        self.stats.ring_fill_ratio = self.ring.len() as f64 / self.config.ring_size as f64;
    }
}

/// Compute a 64-bit fingerprint from entry bytes (first 8 bytes of SHA-256).
pub fn compute_fingerprint(data: &[u8]) -> u64 {
    let hash = Sha256::digest(data);
    u64::from_le_bytes([
        hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_new_entry_returns_true_not_duplicate() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let result = dedup.record(0x12345678, 1, now_ms());
        assert!(result, "New entry should return true");
        assert_eq!(dedup.len(), 1);
    }

    #[test]
    fn test_same_entry_returns_false_duplicate_detected() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();
        dedup.record(0x12345678, 1, now);
        let result = dedup.record(0x12345678, 1, now + 100);
        assert!(!result, "Duplicate entry should return false");
        assert_eq!(dedup.len(), 1);
    }

    #[test]
    fn test_different_entries_all_return_true() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();
        assert!(dedup.record(0x11111111, 1, now));
        assert!(dedup.record(0x22222222, 2, now + 1));
        assert!(dedup.record(0x33333333, 3, now + 2));
        assert_eq!(dedup.len(), 3);
    }

    #[test]
    fn test_ring_wraps_correctly_at_capacity() {
        let config = DedupConfig {
            ring_size: 3,
            entry_ttl_ms: 300_000,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        for i in 0..5 {
            let result = dedup.record(i as u64, i, now + i as u64);
            assert!(result, "Entry {} should be new", i);
        }

        assert_eq!(dedup.len(), 3);
        let stats = dedup.stats();
        assert_eq!(stats.total_evictions, 2);
    }

    #[test]
    fn test_evict_expired_entries() {
        let config = DedupConfig {
            ring_size: 100,
            entry_ttl_ms: 100,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        dedup.record(1, 1, now);
        dedup.record(2, 2, now);
        dedup.record(3, 3, now + 50);

        let evicted = dedup.evict_expired(now + 200);
        assert!(evicted >= 2);
    }

    #[test]
    fn test_evict_clears_old_entries_but_keeps_fresh_ones() {
        let config = DedupConfig {
            ring_size: 100,
            entry_ttl_ms: 100,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        dedup.record(1, 1, now);
        dedup.record(2, 2, now + 120);
        dedup.record(3, 3, now + 150);

        dedup.evict_expired(now + 200);

        assert!(dedup.is_duplicate(2, 2), "Entry 2 should still be present");
        assert!(!dedup.is_duplicate(1, 1), "Entry 1 should be evicted");
    }

    #[test]
    fn test_reset_clears_all_state() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();

        dedup.record(1, 1, now);
        dedup.record(2, 2, now);
        assert!(!dedup.is_empty());

        dedup.reset();

        assert!(dedup.is_empty());
        assert_eq!(dedup.stats().total_checked, 0);
    }

    #[test]
    fn test_stats_track_correctly() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();

        dedup.record(1, 1, now);
        dedup.record(1, 1, now);
        dedup.record(2, 2, now);

        let stats = dedup.stats();
        assert_eq!(stats.total_checked, 3);
        assert_eq!(stats.total_duplicates, 1);
    }

    #[test]
    fn test_is_duplicate_without_recording() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();

        dedup.record(1, 1, now);
        let stats_before = dedup.stats().total_checked;

        let is_dup = dedup.is_duplicate(1, 1);
        assert!(is_dup, "Should detect as duplicate without recording");

        let stats = dedup.stats();
        assert_eq!(
            stats.total_checked, stats_before,
            "is_duplicate should not increment checked"
        );
        assert_eq!(dedup.len(), 1);
    }

    #[test]
    fn test_fingerprint_computation_is_deterministic() {
        let data = b"test entry data";
        let fp1 = compute_fingerprint(data);
        let fp2 = compute_fingerprint(data);
        assert_eq!(fp1, fp2, "Fingerprint should be deterministic");
    }

    #[test]
    fn test_empty_state_after_reset() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        dedup.reset();
        assert!(dedup.is_empty());
        assert_eq!(dedup.len(), 0);
    }

    #[test]
    fn test_multiple_duplicates_in_sequence() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();

        assert!(dedup.record(1, 1, now));
        assert!(!dedup.record(1, 1, now));
        assert!(!dedup.record(1, 1, now));
        assert!(dedup.record(2, 2, now));
        assert!(!dedup.record(1, 1, now));
        assert!(!dedup.record(2, 2, now));

        let stats = dedup.stats();
        assert_eq!(stats.total_checked, 6);
        assert_eq!(stats.total_duplicates, 4);
    }

    #[test]
    fn test_ttl_of_0_evicts_everything() {
        let config = DedupConfig {
            ring_size: 100,
            entry_ttl_ms: 0,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        dedup.record(1, 1, now);
        dedup.record(2, 2, now);

        let evicted = dedup.evict_expired(now + 1);
        assert_eq!(dedup.len(), 0);
    }

    #[test]
    fn test_large_ring_buffer_handles_many_entries() {
        let config = DedupConfig {
            ring_size: 10000,
            entry_ttl_ms: 300_000,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        for i in 0..20000u64 {
            dedup.record(i, i as u64, now + i);
        }

        assert_eq!(dedup.len(), 10000);
        let stats = dedup.stats();
        assert_eq!(stats.total_evictions, 10000);
    }

    #[test]
    fn test_fingerprint_different_for_different_data() {
        let data1 = b"hello world";
        let data2 = b"hello worl";
        let data3 = b"different";

        let fp1 = compute_fingerprint(data1);
        let fp2 = compute_fingerprint(data2);
        let fp3 = compute_fingerprint(data3);

        assert_ne!(fp1, fp2);
        assert_ne!(fp1, fp3);
        assert_ne!(fp2, fp3);
    }

    #[test]
    fn test_different_seq_same_fingerprint_not_duplicate() {
        let mut dedup = EntryDedup::new(DedupConfig::default());
        let now = now_ms();

        let data1 = b"same content";
        let data2 = b"same content";
        let fp1 = compute_fingerprint(data1);
        let fp2 = compute_fingerprint(data2);

        assert_eq!(fp1, fp2);

        assert!(dedup.record(fp1, 1, now));
        assert!(
            dedup.is_duplicate(fp1, 1),
            "Same seq is duplicate after recording"
        );
        assert!(
            !dedup.is_duplicate(fp1, 2),
            "Different seq is not duplicate"
        );
    }

    #[test]
    fn test_ring_fill_ratio() {
        let config = DedupConfig {
            ring_size: 10,
            entry_ttl_ms: 300_000,
        };
        let mut dedup = EntryDedup::new(config);
        let now = now_ms();

        assert!((dedup.stats().ring_fill_ratio - 0.0).abs() < 0.001);

        for i in 0..5 {
            dedup.record(i, i, now + i);
        }

        assert!((dedup.stats().ring_fill_ratio - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_default_config_values() {
        let config = DedupConfig::default();
        assert_eq!(config.ring_size, 4096);
        assert_eq!(config.entry_ttl_ms, 300_000);
    }

    #[test]
    fn test_dedup_debug_format() {
        let dedup = EntryDedup::new(DedupConfig::default());
        let debug_str = format!("{:?}", dedup);
        assert!(debug_str.contains("EntryDedup"));
    }

    #[test]
    fn test_proptest_random_fingerprints_no_false_positives() {
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn test_random_fingerprints_never_false_positive(
                fp1 in any::<u64>(),
                fp2 in any::<u64>(),
                seq1 in any::<u64>(),
                seq2 in any::<u64>()
            ) {
                let mut dedup = EntryDedup::new(DedupConfig::default());
                let now = now_ms();

                if fp1 != fp2 || seq1 != seq2 {
                    let r1 = dedup.record(fp1, seq1, now);
                    let r2 = dedup.record(fp2, seq2, now + 1);
                    assert!(r1);
                    assert!(r2);
                }
            }
        }
    }
}
