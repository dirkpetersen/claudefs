//! Request deduplication for exactly-once semantics.
//!
//! This module provides a deduplication tracker to prevent duplicate request processing
//! by tracking request IDs and their response hashes with configurable TTL and eviction.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(u64);

#[derive(Debug, Clone)]
pub struct DedupConfig {
    pub max_entries: usize,
    pub ttl_ms: u64,
    pub cleanup_interval_ms: u64,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            ttl_ms: 30_000,
            cleanup_interval_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DedupEntry {
    pub request_id: RequestId,
    pub response_hash: u64,
    pub created_at_ms: u64,
    pub hit_count: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum DedupResult {
    New,
    Duplicate { hit_count: u64 },
    Expired,
}

#[derive(Debug, Clone, Default)]
pub struct DedupStats {
    pub total_checks: u64,
    pub total_duplicates: u64,
    pub total_evictions: u64,
    pub current_entries: usize,
    pub hit_rate: f64,
}

pub struct DedupTracker {
    config: DedupConfig,
    entries: HashMap<RequestId, DedupEntry>,
    total_checks: u64,
    total_duplicates: u64,
    total_evictions: u64,
    current_time_ms: u64,
}

impl DedupTracker {
    pub fn new(config: DedupConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            total_checks: 0,
            total_duplicates: 0,
            total_evictions: 0,
            current_time_ms: 0,
        }
    }

    pub fn check(&mut self, request_id: RequestId) -> DedupResult {
        self.total_checks += 1;

        if let Some(entry) = self.entries.get(&request_id) {
            let is_expired =
                self.current_time_ms.saturating_sub(entry.created_at_ms) >= self.config.ttl_ms;

            if is_expired {
                self.entries.remove(&request_id);
                self.total_evictions += 1;
                return DedupResult::Expired;
            }

            let new_hit_count = entry.hit_count + 1;
            let entry = self.entries.get_mut(&request_id).unwrap();
            entry.hit_count = new_hit_count;
            self.total_duplicates += 1;
            return DedupResult::Duplicate {
                hit_count: new_hit_count,
            };
        }

        if self.entries.len() >= self.config.max_entries {
            if let Some(oldest_key) = self.entries.keys().copied().min_by_key(|k| {
                self.entries
                    .get(k)
                    .map(|e| e.created_at_ms)
                    .unwrap_or(u64::MAX)
            }) {
                self.entries.remove(&oldest_key);
                self.total_evictions += 1;
            }
        }

        let entry = DedupEntry {
            request_id,
            response_hash: 0,
            created_at_ms: self.current_time_ms,
            hit_count: 1,
        };
        self.entries.insert(request_id, entry);
        DedupResult::New
    }

    pub fn record(&mut self, request_id: RequestId, response_hash: u64) {
        if let Some(entry) = self.entries.get_mut(&request_id) {
            entry.response_hash = response_hash;
        } else {
            let entry = DedupEntry {
                request_id,
                response_hash,
                created_at_ms: self.current_time_ms,
                hit_count: 0,
            };
            self.entries.insert(request_id, entry);
        }
    }

    pub fn evict_expired(&mut self) -> usize {
        let cutoff = self.current_time_ms.saturating_sub(self.config.ttl_ms);
        let before = self.entries.len();
        self.entries.retain(|_, entry| entry.created_at_ms > cutoff);
        let evicted = before - self.entries.len();
        self.total_evictions += evicted as u64;
        evicted
    }

    pub fn advance_time(&mut self, ms: u64) {
        self.current_time_ms = self.current_time_ms.saturating_add(ms);
    }

    pub fn set_time(&mut self, ms: u64) {
        self.current_time_ms = ms;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn stats(&self) -> DedupStats {
        let hit_rate = if self.total_checks > 0 {
            self.total_duplicates as f64 / self.total_checks as f64
        } else {
            0.0
        };
        DedupStats {
            total_checks: self.total_checks,
            total_duplicates: self.total_duplicates,
            total_evictions: self.total_evictions,
            current_entries: self.entries.len(),
            hit_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = DedupConfig::default();
        assert_eq!(config.max_entries, 100_000);
        assert_eq!(config.ttl_ms, 30_000);
        assert_eq!(config.cleanup_interval_ms, 5_000);
    }

    #[test]
    fn test_new_request() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        let request_id = RequestId(1);
        let result = tracker.check(request_id);
        assert!(matches!(result, DedupResult::New));
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn test_duplicate_request() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        let request_id = RequestId(1);
        tracker.check(request_id);
        let result = tracker.check(request_id);
        match result {
            DedupResult::Duplicate { hit_count } => assert_eq!(hit_count, 2),
            _ => panic!("Expected Duplicate"),
        }
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn test_expired_request() {
        let config = DedupConfig {
            ttl_ms: 100,
            ..Default::default()
        };
        let mut tracker = DedupTracker::new(config);
        let request_id = RequestId(1);
        tracker.check(request_id);
        tracker.advance_time(150);
        let result = tracker.check(request_id);
        assert!(matches!(result, DedupResult::Expired));
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_record_and_check() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        let request_id = RequestId(1);
        tracker.check(request_id);
        tracker.record(request_id, 12345);
        let entry = tracker.entries.get(&request_id).unwrap();
        assert_eq!(entry.response_hash, 12345);
    }

    #[test]
    fn test_evict_expired() {
        let config = DedupConfig {
            ttl_ms: 100,
            ..Default::default()
        };
        let mut tracker = DedupTracker::new(config);
        tracker.check(RequestId(1));
        tracker.check(RequestId(2));
        assert_eq!(tracker.len(), 2);
        tracker.advance_time(150);
        let evicted = tracker.evict_expired();
        assert_eq!(evicted, 2);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_max_entries_eviction() {
        let config = DedupConfig {
            max_entries: 3,
            ..Default::default()
        };
        let mut tracker = DedupTracker::new(config);
        tracker.check(RequestId(1));
        tracker.check(RequestId(2));
        tracker.check(RequestId(3));
        assert_eq!(tracker.len(), 3);
        tracker.check(RequestId(4));
        assert_eq!(tracker.len(), 3);
    }

    #[test]
    fn test_hit_count_increments() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        let request_id = RequestId(1);
        tracker.check(request_id);
        tracker.check(request_id);
        tracker.check(request_id);
        let entry = tracker.entries.get(&request_id).unwrap();
        assert_eq!(entry.hit_count, 3);
    }

    #[test]
    fn test_stats_snapshot() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        tracker.check(RequestId(1));
        tracker.check(RequestId(1));
        tracker.check(RequestId(2));
        let stats = tracker.stats();
        assert_eq!(stats.total_checks, 3);
        assert_eq!(stats.total_duplicates, 1);
        assert_eq!(stats.current_entries, 2);
        assert!((stats.hit_rate - 0.333333).abs() < 0.01);
    }

    #[test]
    fn test_is_empty() {
        let config = DedupConfig::default();
        let tracker = DedupTracker::new(config);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_advance_time() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        tracker.advance_time(100);
        assert_eq!(tracker.current_time_ms, 100);
    }

    #[test]
    fn test_set_time() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        tracker.set_time(500);
        assert_eq!(tracker.current_time_ms, 500);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let config = DedupConfig::default();
        let mut tracker = DedupTracker::new(config);
        for _ in 0..10 {
            tracker.check(RequestId(1));
        }
        let stats = tracker.stats();
        assert!((stats.hit_rate - 0.9).abs() < 0.01);
    }
}
