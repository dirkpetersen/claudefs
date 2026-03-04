//! Transient Node Blacklist.
//!
//! Manages a transient blacklist of nodes that have recently failed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlacklistReason {
    ConnectionFailed,
    ErrorResponse(String),
    LatencyThreshold,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub node_id: [u8; 16],
    pub reason: BlacklistReason,
    pub added_at_ms: u64,
    pub expires_at_ms: u64,
    pub failure_count: u32,
}

impl BlacklistEntry {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistConfig {
    pub base_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub exponential: bool,
    pub max_entries: usize,
}

impl Default for BlacklistConfig {
    fn default() -> Self {
        Self {
            base_backoff_ms: 5000,
            max_backoff_ms: 300000,
            exponential: true,
            max_entries: 128,
        }
    }
}

pub struct NodeBlacklist {
    config: BlacklistConfig,
    entries: RwLock<HashMap<[u8; 16], BlacklistEntry>>,
    stats: Arc<BlacklistStats>,
}

impl NodeBlacklist {
    pub fn new(config: BlacklistConfig) -> Self {
        Self {
            config,
            entries: RwLock::new(HashMap::new()),
            stats: Arc::new(BlacklistStats::new()),
        }
    }

    pub fn blacklist(&self, node_id: [u8; 16], reason: BlacklistReason, now_ms: u64) {
        let mut entries = self.entries.write().unwrap();

        let (failure_count, added_at_ms) = if let Some(existing) = entries.get(&node_id) {
            (existing.failure_count + 1, existing.added_at_ms)
        } else {
            (1, now_ms)
        };

        let backoff_ms = if self.config.exponential {
            let exponential =
                self.config.base_backoff_ms * (2u64.saturating_pow(failure_count - 1));
            exponential.min(self.config.max_backoff_ms)
        } else {
            self.config.base_backoff_ms
        };

        let expires_at_ms = now_ms.saturating_add(backoff_ms);

        let entry = BlacklistEntry {
            node_id,
            reason,
            added_at_ms,
            expires_at_ms,
            failure_count,
        };

        if entries.len() >= self.config.max_entries && !entries.contains_key(&node_id) {
            return;
        }

        entries.insert(node_id, entry);
        self.stats.nodes_blacklisted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn remove(&self, node_id: &[u8; 16]) {
        let mut entries = self.entries.write().unwrap();
        if entries.remove(node_id).is_some() {
            self.stats.nodes_removed.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn is_blacklisted(&self, node_id: &[u8; 16], now_ms: u64) -> bool {
        self.stats.blacklist_checks.fetch_add(1, Ordering::Relaxed);

        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(node_id) {
            if !entry.is_expired(now_ms) {
                self.stats.checks_hit.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    pub fn expire(&self, now_ms: u64) -> usize {
        let mut entries = self.entries.write().unwrap();
        let before = entries.len();
        entries.retain(|_, entry| !entry.is_expired(now_ms));
        let removed = before - entries.len();
        if removed > 0 {
            self.stats
                .nodes_expired
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        removed
    }

    pub fn entry(&self, node_id: &[u8; 16], now_ms: u64) -> Option<BlacklistEntry> {
        let entries = self.entries.read().unwrap();
        entries
            .get(node_id)
            .filter(|e| !e.is_expired(now_ms))
            .cloned()
    }

    pub fn active_entries(&self, now_ms: u64) -> Vec<BlacklistEntry> {
        let entries = self.entries.read().unwrap();
        entries
            .values()
            .filter(|e| !e.is_expired(now_ms))
            .cloned()
            .collect()
    }

    pub fn filter_available<'a>(&self, nodes: &'a [[u8; 16]], now_ms: u64) -> Vec<&'a [u8; 16]> {
        nodes
            .iter()
            .filter(|id| !self.is_blacklisted(id, now_ms))
            .collect()
    }

    pub fn active_count(&self, now_ms: u64) -> usize {
        let entries = self.entries.read().unwrap();
        entries.values().filter(|e| !e.is_expired(now_ms)).count()
    }

    pub fn stats(&self) -> Arc<BlacklistStats> {
        Arc::clone(&self.stats)
    }
}

pub struct BlacklistStats {
    pub nodes_blacklisted: AtomicU64,
    pub nodes_removed: AtomicU64,
    pub nodes_expired: AtomicU64,
    pub blacklist_checks: AtomicU64,
    pub checks_hit: AtomicU64,
}

impl BlacklistStats {
    pub fn new() -> Self {
        Self {
            nodes_blacklisted: AtomicU64::new(0),
            nodes_removed: AtomicU64::new(0),
            nodes_expired: AtomicU64::new(0),
            blacklist_checks: AtomicU64::new(0),
            checks_hit: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self, active_count: usize) -> BlacklistStatsSnapshot {
        BlacklistStatsSnapshot {
            nodes_blacklisted: self.nodes_blacklisted.load(Ordering::Relaxed),
            nodes_removed: self.nodes_removed.load(Ordering::Relaxed),
            nodes_expired: self.nodes_expired.load(Ordering::Relaxed),
            blacklist_checks: self.blacklist_checks.load(Ordering::Relaxed),
            checks_hit: self.checks_hit.load(Ordering::Relaxed),
            active_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistStatsSnapshot {
    pub nodes_blacklisted: u64,
    pub nodes_removed: u64,
    pub nodes_expired: u64,
    pub blacklist_checks: u64,
    pub checks_hit: u64,
    pub active_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(idx: u8) -> [u8; 16] {
        [idx; 16]
    }

    #[test]
    fn test_blacklist_node() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        assert!(blacklist.is_blacklisted(&make_node_id(1), 1000));
    }

    #[test]
    fn test_not_blacklisted() {
        let blacklist = NodeBlacklist::new(Default::default());

        assert!(!blacklist.is_blacklisted(&make_node_id(1), 1000));
    }

    #[test]
    fn test_blacklist_expired() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        assert!(!blacklist.is_blacklisted(&make_node_id(1), 20000));
    }

    #[test]
    fn test_blacklist_not_expired() {
        let blacklist = NodeBlacklist::new(BlacklistConfig {
            base_backoff_ms: 10000,
            ..Default::default()
        });

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        assert!(blacklist.is_blacklisted(&make_node_id(1), 5000));
    }

    #[test]
    fn test_blacklist_increments_failure_count() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 2000);

        let entry = blacklist.entry(&make_node_id(1), 2000);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().failure_count, 2);
    }

    #[test]
    fn test_exponential_backoff() {
        let config = BlacklistConfig {
            base_backoff_ms: 1000,
            max_backoff_ms: 100000,
            exponential: true,
            max_entries: 128,
        };
        let blacklist = NodeBlacklist::new(config);

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        let entry1 = blacklist.entry(&make_node_id(1), 1000).unwrap();

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 2000);
        let entry2 = blacklist.entry(&make_node_id(1), 2000).unwrap();

        assert!(entry2.expires_at_ms - entry1.expires_at_ms > 0);
    }

    #[test]
    fn test_max_backoff() {
        let config = BlacklistConfig {
            base_backoff_ms: 1000,
            max_backoff_ms: 5000,
            exponential: true,
            max_entries: 128,
        };
        let blacklist = NodeBlacklist::new(config);

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        let entry1 = blacklist.entry(&make_node_id(1), 1000).unwrap();

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 2000);
        let entry2 = blacklist.entry(&make_node_id(1), 2000).unwrap();

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 3000);
        let entry3 = blacklist.entry(&make_node_id(1), 3000).unwrap();

        assert!(entry2.expires_at_ms - entry1.expires_at_ms > 0);

        let backoff2 = entry2.expires_at_ms - 2000;
        let backoff3 = entry3.expires_at_ms - 3000;

        assert!(backoff3 <= 5000);
    }

    #[test]
    fn test_remove_explicit() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        blacklist.remove(&make_node_id(1));

        assert!(!blacklist.is_blacklisted(&make_node_id(1), 1000));
    }

    #[test]
    fn test_expire_removes_old() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        let removed = blacklist.expire(20000);
        assert_eq!(removed, 1);
        assert!(!blacklist.is_blacklisted(&make_node_id(1), 20000));
    }

    #[test]
    fn test_expire_keeps_fresh() {
        let blacklist = NodeBlacklist::new(BlacklistConfig {
            base_backoff_ms: 10000,
            ..Default::default()
        });

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        let removed = blacklist.expire(5000);
        assert_eq!(removed, 0);
        assert!(blacklist.is_blacklisted(&make_node_id(1), 5000));
    }

    #[test]
    fn test_filter_available() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        let nodes = vec![make_node_id(1), make_node_id(2), make_node_id(3)];
        let available = blacklist.filter_available(&nodes, 1000);

        assert_eq!(available.len(), 2);
    }

    #[test]
    fn test_filter_all_blacklisted() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        blacklist.blacklist(make_node_id(2), BlacklistReason::ConnectionFailed, 1000);

        let nodes = vec![make_node_id(1), make_node_id(2)];
        let available = blacklist.filter_available(&nodes, 1000);

        assert!(available.is_empty());
    }

    #[test]
    fn test_active_entries() {
        let blacklist = NodeBlacklist::new(BlacklistConfig {
            base_backoff_ms: 10000,
            ..Default::default()
        });

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        blacklist.blacklist(make_node_id(2), BlacklistReason::ConnectionFailed, 1000);

        let active = blacklist.active_entries(5000);
        assert_eq!(active.len(), 2);

        blacklist.expire(20000);
        let active = blacklist.active_entries(20000);
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_active_count() {
        let blacklist = NodeBlacklist::new(BlacklistConfig {
            base_backoff_ms: 10000,
            ..Default::default()
        });

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);

        let count = blacklist.active_count(5000);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_stats_counts() {
        let blacklist = NodeBlacklist::new(Default::default());

        blacklist.blacklist(make_node_id(1), BlacklistReason::ConnectionFailed, 1000);
        blacklist.blacklist(make_node_id(2), BlacklistReason::ConnectionFailed, 1000);

        let _ = blacklist.is_blacklisted(&make_node_id(1), 1000);
        let _ = blacklist.is_blacklisted(&make_node_id(1), 1000);
        let _ = blacklist.is_blacklisted(&make_node_id(2), 1000);

        blacklist.remove(&make_node_id(1));

        let snapshot = blacklist.stats().snapshot(blacklist.active_count(1000));
        assert_eq!(snapshot.nodes_blacklisted, 2);
        assert_eq!(snapshot.nodes_removed, 1);
        assert_eq!(snapshot.blacklist_checks, 3);
        assert_eq!(snapshot.checks_hit, 3);
    }
}
