//! Distributed lease manager for coordination and locking.
//!
//! This module manages time-limited leases for distributed locks and coordination.
//! Used by A2 (Metadata) to grant client-side metadata caching leases, by A5 (FUSE)
//! for open-file delegation, and by A7 (pNFS) for layout leases.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Unique lease identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseId(pub u64);

/// Type of lease access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseType {
    /// Shared read lease — multiple holders allowed.
    Shared,
    /// Exclusive write lease — only one holder allowed.
    Exclusive,
}

/// State of a lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseState {
    /// Active and valid.
    Active,
    /// Recall requested — holder should release soon.
    Recalled,
    /// Expired (past expiry time).
    Expired,
    /// Revoked by administrator or conflict.
    Revoked,
}

/// A granted lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    /// Unique lease identifier.
    pub id: LeaseId,
    /// Resource identifier this lease is for.
    pub resource_id: String,
    /// Holder identifier (16-byte UUID).
    pub holder_id: [u8; 16],
    /// Type of lease (shared or exclusive).
    pub lease_type: LeaseType,
    /// Current state of the lease.
    pub state: LeaseState,
    /// When the lease was granted (ms since epoch).
    pub granted_at_ms: u64,
    /// When the lease expires (ms since epoch).
    pub expires_at_ms: u64,
    /// When the lease was recalled (ms since epoch), if recalled.
    pub recalled_at_ms: Option<u64>,
}

impl Lease {
    /// Check if the lease has expired.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
            || self.state == LeaseState::Expired
            || self.state == LeaseState::Revoked
    }

    /// Remaining time in milliseconds. Returns negative if expired.
    pub fn remaining_ms(&self, now_ms: u64) -> i64 {
        if self.is_expired(now_ms) {
            return -1;
        }
        self.expires_at_ms.saturating_sub(now_ms) as i64
    }

    /// Check if the lease is still active (not expired, recalled, or revoked).
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.state == LeaseState::Active && !self.is_expired(now_ms)
    }
}

/// Configuration for lease management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConfig {
    /// Default lease duration in ms (default: 30000 = 30 seconds).
    pub default_duration_ms: u64,
    /// Maximum lease duration in ms (default: 300000 = 5 minutes).
    pub max_duration_ms: u64,
    /// Grace period after expiry before forced cleanup in ms (default: 5000).
    pub grace_period_ms: u64,
    /// Maximum leases per resource (shared) (default: 64).
    pub max_shared_leases: usize,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            default_duration_ms: 30000,
            max_duration_ms: 300000,
            grace_period_ms: 5000,
            max_shared_leases: 64,
        }
    }
}

/// Error type for lease operations.
#[derive(Debug, Error)]
pub enum LeaseError {
    /// Lease not found.
    #[error("lease {0:?} not found")]
    NotFound(LeaseId),
    /// Resource is exclusively locked.
    #[error("resource {0:?} is exclusively locked")]
    ExclusiveConflict(String),
    /// Cannot grant exclusive lease while shared leases exist.
    #[error("cannot grant exclusive lease while shared leases exist")]
    SharedConflict,
    /// Too many shared leases for resource.
    #[error("too many shared leases for resource {0:?} (max {1})")]
    TooManyShared(String, usize),
    /// Lease has already expired or been revoked.
    #[error("lease {0:?} has already expired or been revoked")]
    NotActive(LeaseId),
}

/// Statistics for lease operations.
pub struct LeaseStats {
    /// Total leases granted.
    pub leases_granted: AtomicU64,
    /// Total leases released.
    pub leases_released: AtomicU64,
    /// Total leases expired.
    pub leases_expired: AtomicU64,
    /// Total leases revoked.
    pub leases_revoked: AtomicU64,
    /// Total leases recalled.
    pub leases_recalled: AtomicU64,
    /// Total lease conflicts.
    pub lease_conflicts: AtomicU64,
    /// Total renewals.
    pub renewals: AtomicU64,
}

impl LeaseStats {
    /// Create new lease statistics.
    pub fn new() -> Self {
        Self {
            leases_granted: AtomicU64::new(0),
            leases_released: AtomicU64::new(0),
            leases_expired: AtomicU64::new(0),
            leases_revoked: AtomicU64::new(0),
            leases_recalled: AtomicU64::new(0),
            lease_conflicts: AtomicU64::new(0),
            renewals: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self, active_leases: usize) -> LeaseStatsSnapshot {
        LeaseStatsSnapshot {
            leases_granted: self.leases_granted.load(Ordering::Relaxed),
            leases_released: self.leases_released.load(Ordering::Relaxed),
            leases_expired: self.leases_expired.load(Ordering::Relaxed),
            leases_revoked: self.leases_revoked.load(Ordering::Relaxed),
            leases_recalled: self.leases_recalled.load(Ordering::Relaxed),
            lease_conflicts: self.lease_conflicts.load(Ordering::Relaxed),
            renewals: self.renewals.load(Ordering::Relaxed),
            active_leases,
        }
    }
}

impl Default for LeaseStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of lease statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct LeaseStatsSnapshot {
    /// Total leases granted.
    pub leases_granted: u64,
    /// Total leases released.
    pub leases_released: u64,
    /// Total leases expired.
    pub leases_expired: u64,
    /// Total leases revoked.
    pub leases_revoked: u64,
    /// Total leases recalled.
    pub leases_recalled: u64,
    /// Total lease conflicts.
    pub lease_conflicts: u64,
    /// Total renewals.
    pub renewals: u64,
    /// Number of active leases.
    pub active_leases: usize,
}

/// Manages distributed leases for resources.
pub struct LeaseManager {
    config: LeaseConfig,
    next_id: AtomicU64,
    leases: RwLock<HashMap<String, Vec<Lease>>>,
    stats: Arc<LeaseStats>,
}

impl LeaseManager {
    /// Create a new lease manager.
    pub fn new(config: LeaseConfig) -> Self {
        Self {
            config,
            next_id: AtomicU64::new(1),
            leases: RwLock::new(HashMap::new()),
            stats: Arc::new(LeaseStats::new()),
        }
    }

    /// Grant a lease on a resource. Returns LeaseId.
    pub fn grant(
        &self,
        resource_id: String,
        holder_id: [u8; 16],
        lease_type: LeaseType,
        duration_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<LeaseId, LeaseError> {
        let duration = duration_ms
            .unwrap_or(self.config.default_duration_ms)
            .min(self.config.max_duration_ms);

        match self.leases.write() {
            Ok(mut leases) => {
                let resource_leases = leases.entry(resource_id.clone()).or_insert_with(Vec::new);

                match lease_type {
                    LeaseType::Exclusive => {
                        if resource_leases
                            .iter()
                            .any(|l| l.lease_type == LeaseType::Exclusive && !l.is_expired(now_ms))
                        {
                            self.stats.lease_conflicts.fetch_add(1, Ordering::Relaxed);
                            return Err(LeaseError::ExclusiveConflict(resource_id));
                        }
                        if resource_leases
                            .iter()
                            .any(|l| l.lease_type == LeaseType::Shared && !l.is_expired(now_ms))
                        {
                            self.stats.lease_conflicts.fetch_add(1, Ordering::Relaxed);
                            return Err(LeaseError::SharedConflict);
                        }
                    }
                    LeaseType::Shared => {
                        if resource_leases
                            .iter()
                            .any(|l| l.lease_type == LeaseType::Exclusive && !l.is_expired(now_ms))
                        {
                            self.stats.lease_conflicts.fetch_add(1, Ordering::Relaxed);
                            return Err(LeaseError::ExclusiveConflict(resource_id));
                        }
                        let active_shared = resource_leases
                            .iter()
                            .filter(|l| l.lease_type == LeaseType::Shared && !l.is_expired(now_ms))
                            .count();
                        if active_shared >= self.config.max_shared_leases {
                            self.stats.lease_conflicts.fetch_add(1, Ordering::Relaxed);
                            return Err(LeaseError::TooManyShared(
                                resource_id,
                                self.config.max_shared_leases,
                            ));
                        }
                    }
                }

                let id = LeaseId(self.next_id.fetch_add(1, Ordering::Relaxed));
                let lease = Lease {
                    id,
                    resource_id: resource_id.clone(),
                    holder_id,
                    lease_type,
                    state: LeaseState::Active,
                    granted_at_ms: now_ms,
                    expires_at_ms: now_ms.saturating_add(duration),
                    recalled_at_ms: None,
                };
                resource_leases.push(lease);
                self.stats.leases_granted.fetch_add(1, Ordering::Relaxed);
                Ok(id)
            }
            Err(_) => {
                self.stats.lease_conflicts.fetch_add(1, Ordering::Relaxed);
                Err(LeaseError::ExclusiveConflict(resource_id))
            }
        }
    }

    /// Renew a lease — extend its expiry time.
    pub fn renew(&self, id: LeaseId, duration_ms: u64, now_ms: u64) -> Result<(), LeaseError> {
        let duration = duration_ms.min(self.config.max_duration_ms);

        match self.leases.write() {
            Ok(mut leases) => {
                for resource_leases in leases.values_mut() {
                    for lease in resource_leases.iter_mut() {
                        if lease.id == id {
                            if lease.is_expired(now_ms) {
                                return Err(LeaseError::NotActive(id));
                            }
                            lease.expires_at_ms = now_ms.saturating_add(duration);
                            self.stats.renewals.fetch_add(1, Ordering::Relaxed);
                            return Ok(());
                        }
                    }
                }
                Err(LeaseError::NotFound(id))
            }
            Err(_) => Err(LeaseError::NotFound(id)),
        }
    }

    /// Release a lease voluntarily.
    pub fn release(&self, id: LeaseId) -> Result<(), LeaseError> {
        match self.leases.write() {
            Ok(mut leases) => {
                for resource_leases in leases.values_mut() {
                    let initial_len = resource_leases.len();
                    resource_leases.retain(|l| l.id != id);
                    if resource_leases.len() < initial_len {
                        self.stats.leases_released.fetch_add(1, Ordering::Relaxed);
                        return Ok(());
                    }
                }
                Err(LeaseError::NotFound(id))
            }
            Err(_) => Err(LeaseError::NotFound(id)),
        }
    }

    /// Recall a lease (request holder to release).
    pub fn recall(&self, id: LeaseId, now_ms: u64) -> Result<(), LeaseError> {
        match self.leases.write() {
            Ok(mut leases) => {
                for resource_leases in leases.values_mut() {
                    for lease in resource_leases.iter_mut() {
                        if lease.id == id {
                            if lease.is_expired(now_ms) {
                                return Err(LeaseError::NotActive(id));
                            }
                            lease.state = LeaseState::Recalled;
                            lease.recalled_at_ms = Some(now_ms);
                            self.stats.leases_recalled.fetch_add(1, Ordering::Relaxed);
                            return Ok(());
                        }
                    }
                }
                Err(LeaseError::NotFound(id))
            }
            Err(_) => Err(LeaseError::NotFound(id)),
        }
    }

    /// Revoke a lease (force-remove).
    pub fn revoke(&self, id: LeaseId) -> Result<(), LeaseError> {
        match self.leases.write() {
            Ok(mut leases) => {
                for resource_leases in leases.values_mut() {
                    let initial_len = resource_leases.len();
                    resource_leases.retain(|l| l.id != id);
                    if resource_leases.len() < initial_len {
                        self.stats.leases_revoked.fetch_add(1, Ordering::Relaxed);
                        return Ok(());
                    }
                }
                Err(LeaseError::NotFound(id))
            }
            Err(_) => Err(LeaseError::NotFound(id)),
        }
    }

    /// Check if a holder still has a valid lease on a resource.
    pub fn check_lease(&self, resource_id: &str, holder_id: &[u8; 16], now_ms: u64) -> bool {
        match self.leases.read() {
            Ok(leases) => {
                if let Some(resource_leases) = leases.get(resource_id) {
                    return resource_leases
                        .iter()
                        .any(|l| l.holder_id == *holder_id && l.is_active(now_ms));
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Expire all leases past their expiry time. Returns count removed.
    pub fn expire_leases(&self, now_ms: u64) -> usize {
        let mut count = 0;
        match self.leases.write() {
            Ok(mut leases) => {
                for resource_leases in leases.values_mut() {
                    let initial_len = resource_leases.len();
                    resource_leases.retain(|l| !l.is_expired(now_ms));
                    let removed = initial_len - resource_leases.len();
                    count += removed;
                }
                if count > 0 {
                    self.stats
                        .leases_expired
                        .fetch_add(count as u64, Ordering::Relaxed);
                }
            }
            Err(_) => {}
        }
        count
    }

    /// Get lease info.
    pub fn get(&self, id: LeaseId, now_ms: u64) -> Option<Lease> {
        match self.leases.read() {
            Ok(leases) => {
                for resource_leases in leases.values() {
                    for lease in resource_leases {
                        if lease.id == id {
                            let mut l = lease.clone();
                            if l.is_expired(now_ms) && l.state == LeaseState::Active {
                                l.state = LeaseState::Expired;
                            }
                            return Some(l);
                        }
                    }
                }
                None
            }
            Err(_) => None,
        }
    }

    /// List all active leases for a resource.
    pub fn resource_leases(&self, resource_id: &str, now_ms: u64) -> Vec<Lease> {
        match self.leases.read() {
            Ok(leases) => {
                if let Some(resource_leases) = leases.get(resource_id) {
                    return resource_leases
                        .iter()
                        .filter(|l| !l.is_expired(now_ms))
                        .map(|l| {
                            let mut lease = l.clone();
                            if lease.is_expired(now_ms) && lease.state == LeaseState::Active {
                                lease.state = LeaseState::Expired;
                            }
                            lease
                        })
                        .collect();
                }
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Get statistics.
    pub fn stats(&self) -> Arc<LeaseStats> {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_holder_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    #[test]
    fn test_grant_shared_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let result = manager.grant(
            "resource1".to_string(),
            make_holder_id(1),
            LeaseType::Shared,
            None,
            0,
        );
        assert!(result.is_ok());
        let id = result.unwrap();
        assert!(id.0 > 0);
    }

    #[test]
    fn test_grant_exclusive_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let result = manager.grant(
            "resource1".to_string(),
            make_holder_id(1),
            LeaseType::Exclusive,
            None,
            0,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_grant_exclusive_blocks_shared() {
        let manager = LeaseManager::new(LeaseConfig::default());
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Exclusive,
                None,
                0,
            )
            .unwrap();

        let result = manager.grant(
            "resource1".to_string(),
            make_holder_id(2),
            LeaseType::Shared,
            None,
            0,
        );
        assert!(matches!(result, Err(LeaseError::ExclusiveConflict(_))));
    }

    #[test]
    fn test_grant_shared_blocks_exclusive() {
        let manager = LeaseManager::new(LeaseConfig::default());
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                None,
                0,
            )
            .unwrap();

        let result = manager.grant(
            "resource1".to_string(),
            make_holder_id(2),
            LeaseType::Exclusive,
            None,
            0,
        );
        assert!(matches!(result, Err(LeaseError::SharedConflict)));
    }

    #[test]
    fn test_multiple_shared_leases() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let r1 = manager.grant(
            "resource1".to_string(),
            make_holder_id(1),
            LeaseType::Shared,
            None,
            0,
        );
        let r2 = manager.grant(
            "resource1".to_string(),
            make_holder_id(2),
            LeaseType::Shared,
            None,
            0,
        );
        let r3 = manager.grant(
            "resource1".to_string(),
            make_holder_id(3),
            LeaseType::Shared,
            None,
            0,
        );

        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert!(r3.is_ok());
    }

    #[test]
    fn test_release_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let id = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                None,
                0,
            )
            .unwrap();

        let result = manager.release(id);
        assert!(result.is_ok());

        let lease = manager.get(id, 0);
        assert!(lease.is_none());
    }

    #[test]
    fn test_release_unknown_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let result = manager.release(LeaseId(999));
        assert!(matches!(result, Err(LeaseError::NotFound(_))));
    }

    #[test]
    fn test_renew_extends_expiry() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let id = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();

        let lease_before = manager.get(id, 0).unwrap();
        assert_eq!(lease_before.expires_at_ms, 1000);

        manager.renew(id, 2000, 500).unwrap();

        let lease_after = manager.get(id, 500).unwrap();
        assert_eq!(lease_after.expires_at_ms, 2500);
    }

    #[test]
    fn test_renew_expired_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let id = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(100),
                0,
            )
            .unwrap();

        let result = manager.renew(id, 2000, 200);
        assert!(matches!(result, Err(LeaseError::NotActive(_))));
    }

    #[test]
    fn test_recall_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let id = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(10000),
                0,
            )
            .unwrap();

        manager.recall(id, 100).unwrap();

        let lease = manager.get(id, 100).unwrap();
        assert_eq!(lease.state, LeaseState::Recalled);
        assert_eq!(lease.recalled_at_ms, Some(100));
    }

    #[test]
    fn test_revoke_removes_lease() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let id = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                None,
                0,
            )
            .unwrap();

        manager.revoke(id).unwrap();

        let lease = manager.get(id, 0);
        assert!(lease.is_none());
    }

    #[test]
    fn test_expire_leases() {
        let manager = LeaseManager::new(LeaseConfig::default());
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(100),
                0,
            )
            .unwrap();
        manager
            .grant(
                "resource2".to_string(),
                make_holder_id(2),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();

        let count = manager.expire_leases(500);
        assert_eq!(count, 1);

        let leases1 = manager.resource_leases("resource1", 500);
        assert_eq!(leases1.len(), 0);

        let leases2 = manager.resource_leases("resource2", 500);
        assert_eq!(leases2.len(), 1);
    }

    #[test]
    fn test_check_lease_valid() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let holder = make_holder_id(1);
        manager
            .grant(
                "resource1".to_string(),
                holder,
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();

        assert!(manager.check_lease("resource1", &holder, 100));
    }

    #[test]
    fn test_check_lease_expired() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let holder = make_holder_id(1);
        manager
            .grant(
                "resource1".to_string(),
                holder,
                LeaseType::Shared,
                Some(100),
                0,
            )
            .unwrap();

        assert!(!manager.check_lease("resource1", &holder, 500));
    }

    #[test]
    fn test_resource_leases_list() {
        let manager = LeaseManager::new(LeaseConfig::default());
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(2),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();
        manager
            .grant(
                "resource2".to_string(),
                make_holder_id(3),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();

        let leases = manager.resource_leases("resource1", 0);
        assert_eq!(leases.len(), 2);

        let leases2 = manager.resource_leases("resource2", 0);
        assert_eq!(leases2.len(), 1);
    }

    #[test]
    fn test_stats_counts() {
        let manager = LeaseManager::new(LeaseConfig::default());
        let stats = manager.stats();

        let id1 = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                Some(100),
                0,
            )
            .unwrap();
        let id2 = manager
            .grant(
                "resource1".to_string(),
                make_holder_id(2),
                LeaseType::Shared,
                Some(1000),
                0,
            )
            .unwrap();

        manager.renew(id2, 2000, 100).unwrap();
        manager.recall(id2, 100).unwrap();
        manager.release(id1).unwrap();

        let snapshot = stats.snapshot(0);
        assert_eq!(snapshot.leases_granted, 2);
        assert_eq!(snapshot.leases_released, 1);
        assert_eq!(snapshot.leases_recalled, 1);
        assert_eq!(snapshot.renewals, 1);
    }

    #[test]
    fn test_lease_is_expired() {
        let lease = Lease {
            id: LeaseId(1),
            resource_id: "r".to_string(),
            holder_id: [0u8; 16],
            lease_type: LeaseType::Shared,
            state: LeaseState::Active,
            granted_at_ms: 0,
            expires_at_ms: 100,
            recalled_at_ms: None,
        };

        assert!(!lease.is_expired(50));
        assert!(lease.is_expired(100));
        assert!(lease.is_expired(200));
    }

    #[test]
    fn test_lease_remaining_ms() {
        let lease = Lease {
            id: LeaseId(1),
            resource_id: "r".to_string(),
            holder_id: [0u8; 16],
            lease_type: LeaseType::Shared,
            state: LeaseState::Active,
            granted_at_ms: 0,
            expires_at_ms: 100,
            recalled_at_ms: None,
        };

        assert_eq!(lease.remaining_ms(0), 100);
        assert_eq!(lease.remaining_ms(50), 50);
        assert_eq!(lease.remaining_ms(100), -1);
        assert_eq!(lease.remaining_ms(200), -1);
    }

    #[test]
    fn test_lease_is_active() {
        let mut lease = Lease {
            id: LeaseId(1),
            resource_id: "r".to_string(),
            holder_id: [0u8; 16],
            lease_type: LeaseType::Shared,
            state: LeaseState::Active,
            granted_at_ms: 0,
            expires_at_ms: 100,
            recalled_at_ms: None,
        };

        assert!(lease.is_active(50));
        assert!(!lease.is_active(100));

        lease.state = LeaseState::Recalled;
        assert!(!lease.is_active(50));
    }

    #[test]
    fn test_lease_config_default() {
        let config = LeaseConfig::default();
        assert_eq!(config.default_duration_ms, 30000);
        assert_eq!(config.max_duration_ms, 300000);
        assert_eq!(config.grace_period_ms, 5000);
        assert_eq!(config.max_shared_leases, 64);
    }

    #[test]
    fn test_too_many_shared_leases() {
        let mut config = LeaseConfig::default();
        config.max_shared_leases = 2;

        let manager = LeaseManager::new(config);
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(1),
                LeaseType::Shared,
                None,
                0,
            )
            .unwrap();
        manager
            .grant(
                "resource1".to_string(),
                make_holder_id(2),
                LeaseType::Shared,
                None,
                0,
            )
            .unwrap();

        let result = manager.grant(
            "resource1".to_string(),
            make_holder_id(3),
            LeaseType::Shared,
            None,
            0,
        );
        assert!(matches!(result, Err(LeaseError::TooManyShared(_, 2))));
    }
}
