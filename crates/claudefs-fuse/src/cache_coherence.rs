use crate::{FuseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tracing::{debug, trace, warn};

#[derive(Error, Debug)]
pub enum CoherenceError {
    #[error("Lease not found for inode {0}")]
    LeaseNotFound(u64),
    #[error("Lease {0} has expired")]
    LeaseExpired(LeaseId),
    #[error("Lease {0} is in invalid state")]
    InvalidLeaseState(LeaseId),
    #[error("Invalid version vector: {0}")]
    InvalidVersion(String),
}

pub type CoherenceResult<T> = std::result::Result<T, CoherenceError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeaseId(u64);

impl LeaseId {
    pub fn new(id: u64) -> Self {
        LeaseId(id)
    }
}

impl fmt::Display for LeaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lease:{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LeaseState {
    Active,
    Expired,
    Revoked,
    Renewing,
}

#[derive(Debug, Clone)]
pub struct CacheLease {
    pub lease_id: LeaseId,
    pub inode: u64,
    pub client_id: u64,
    pub granted_at: Instant,
    pub duration: Duration,
    pub state: LeaseState,
}

impl CacheLease {
    pub fn new(lease_id: LeaseId, inode: u64, client_id: u64, duration: Duration) -> Self {
        CacheLease {
            lease_id,
            inode,
            client_id,
            granted_at: Instant::now(),
            duration,
            state: LeaseState::Active,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.state == LeaseState::Active && !self.is_expired()
    }

    pub fn is_expired(&self) -> bool {
        if matches!(self.state, LeaseState::Expired | LeaseState::Revoked) {
            return true;
        }
        self.granted_at.elapsed() >= self.duration
    }

    pub fn time_remaining(&self) -> Duration {
        if self.is_expired() {
            Duration::ZERO
        } else {
            self.duration.saturating_sub(self.granted_at.elapsed())
        }
    }

    pub fn revoke(&mut self) {
        debug!("Revoking lease {}", self.lease_id);
        self.state = LeaseState::Revoked;
    }

    pub fn renew(&mut self, new_duration: Duration) {
        trace!(
            "Renewing lease {} with new duration {:?}",
            self.lease_id,
            new_duration
        );
        self.granted_at = Instant::now();
        self.duration = new_duration;
        self.state = LeaseState::Active;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InvalidationReason {
    LeaseExpired,
    RemoteWrite(u64),
    ConflictDetected,
    ExplicitFlush,
    NodeFailover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheInvalidation {
    pub inode: u64,
    pub reason: InvalidationReason,
    pub version: u64,
    pub timestamp: SystemTime,
}

impl CacheInvalidation {
    pub fn new(inode: u64, reason: InvalidationReason, version: u64) -> Self {
        CacheInvalidation {
            inode,
            reason,
            version,
            timestamp: SystemTime::now(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VersionVector {
    versions: HashMap<u64, u64>,
}

impl VersionVector {
    pub fn new() -> Self {
        VersionVector {
            versions: HashMap::new(),
        }
    }

    pub fn get(&self, inode: u64) -> u64 {
        self.versions.get(&inode).copied().unwrap_or(0)
    }

    pub fn update(&mut self, inode: u64, version: u64) {
        let current = self.versions.get(&inode).copied().unwrap_or(0);
        if version > current {
            trace!(
                "VersionVector updating inode {} from {} to {}",
                inode,
                current,
                version
            );
            self.versions.insert(inode, version);
        }
    }

    pub fn conflicts(&self, other: &VersionVector) -> Vec<u64> {
        let mut conflicted = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let all_inodes: Vec<u64> = self
            .versions
            .keys()
            .chain(other.versions.keys())
            .copied()
            .collect();

        for inode in all_inodes {
            if seen.contains(&inode) {
                continue;
            }
            seen.insert(inode);
            let v1 = self.get(inode);
            let v2 = other.get(inode);
            if v1 != v2 {
                conflicted.push(inode);
            }
        }
        conflicted
    }

    pub fn merge(&mut self, other: &VersionVector) {
        for (&inode, &version) in &other.versions {
            self.update(inode, version);
        }
    }

    pub fn len(&self) -> usize {
        self.versions.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum CoherenceProtocol {
    #[default]
    CloseToOpen,
    SessionBased,
    Strict,
}

pub struct CoherenceManager {
    leases: HashMap<u64, CacheLease>,
    invalidations: Vec<CacheInvalidation>,
    protocol: CoherenceProtocol,
    next_lease_id: u64,
    default_lease_duration: Duration,
}

impl CoherenceManager {
    pub fn new(protocol: CoherenceProtocol) -> Self {
        CoherenceManager {
            leases: HashMap::new(),
            invalidations: Vec::new(),
            protocol,
            next_lease_id: 1,
            default_lease_duration: Duration::from_secs(30),
        }
    }

    pub fn grant_lease(&mut self, inode: u64, client_id: u64) -> CacheLease {
        let lease_id = LeaseId::new(self.next_lease_id);
        self.next_lease_id += 1;

        let lease = CacheLease::new(lease_id, inode, client_id, self.default_lease_duration);

        debug!(
            "Granted lease {} for inode {} to client {}",
            lease_id, inode, client_id
        );
        self.leases.insert(inode, lease.clone());
        lease
    }

    pub fn revoke_lease(&mut self, inode: u64) -> Option<CacheInvalidation> {
        if let Some(lease) = self.leases.get_mut(&inode) {
            lease.revoke();

            let invalidation = CacheInvalidation::new(inode, InvalidationReason::ExplicitFlush, 0);

            trace!("Revoked lease for inode {}", inode);
            self.invalidations.push(invalidation.clone());
            Some(invalidation)
        } else {
            warn!("Attempted to revoke non-existent lease for inode {}", inode);
            None
        }
    }

    pub fn check_lease(&self, inode: u64) -> Option<&CacheLease> {
        self.leases.get(&inode).filter(|lease| lease.is_valid())
    }

    pub fn invalidate(&mut self, inode: u64, reason: InvalidationReason, version: u64) {
        if let Some(lease) = self.leases.get_mut(&inode) {
            lease.revoke();
        }

        let invalidation = CacheInvalidation::new(inode, reason, version);
        debug!("Invalidating inode {} with reason {:?}", inode, reason);
        self.invalidations.push(invalidation);
    }

    pub fn pending_invalidations(&self) -> &[CacheInvalidation] {
        &self.invalidations
    }

    pub fn drain_invalidations(&mut self) -> Vec<CacheInvalidation> {
        trace!("Draining {} invalidations", self.invalidations.len());
        std::mem::take(&mut self.invalidations)
    }

    pub fn active_lease_count(&self) -> usize {
        self.leases.values().filter(|l| l.is_valid()).count()
    }

    pub fn expire_stale_leases(&mut self) -> usize {
        let mut count = 0;
        for lease in self.leases.values_mut() {
            if lease.is_expired() && lease.state == LeaseState::Active {
                trace!("Marking lease {} as expired", lease.lease_id);
                lease.state = LeaseState::Expired;
                count += 1;
            }
        }
        if count > 0 {
            debug!("Expired {} stale leases", count);
        }
        count
    }

    pub fn is_coherent(&self, inode: u64) -> bool {
        self.leases
            .get(&inode)
            .map(|l| l.is_valid())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lease_id_display() {
        let id = LeaseId::new(42);
        assert_eq!(format!("{}", id), "lease:42");
    }

    #[test]
    fn test_lease_id_equality() {
        let id1 = LeaseId::new(1);
        let id2 = LeaseId::new(1);
        let id3 = LeaseId::new(2);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_lease_lifecycle() {
        let lease_id = LeaseId::new(1);
        let mut lease = CacheLease::new(lease_id, 100, 1, Duration::from_secs(30));

        assert!(lease.is_valid());
        assert!(!lease.is_expired());
        assert!(lease.time_remaining() > Duration::ZERO);

        lease.revoke();
        assert_eq!(lease.state, LeaseState::Revoked);
        assert!(!lease.is_valid());

        let mut lease2 = CacheLease::new(LeaseId::new(2), 101, 1, Duration::ZERO);
        assert!(lease2.is_expired());
    }

    #[test]
    fn test_lease_renew() {
        let mut lease = CacheLease::new(LeaseId::new(1), 100, 1, Duration::from_millis(10));

        std::thread::sleep(Duration::from_millis(15));

        lease.renew(Duration::from_secs(60));

        assert_eq!(lease.state, LeaseState::Active);
        assert!(lease.is_valid());
        assert!(lease.time_remaining() > Duration::from_secs(59));
    }

    #[test]
    fn test_cache_invalidation() {
        let inv = CacheInvalidation::new(100, InvalidationReason::RemoteWrite(42), 5);

        assert_eq!(inv.inode, 100);
        assert!(matches!(inv.reason, InvalidationReason::RemoteWrite(42)));
        assert_eq!(inv.version, 5);
    }

    #[test]
    fn test_version_vector_basic() {
        let mut vv = VersionVector::new();

        assert_eq!(vv.get(100), 0);

        vv.update(100, 5);
        assert_eq!(vv.get(100), 5);

        vv.update(100, 3);
        assert_eq!(vv.get(100), 5);

        assert_eq!(vv.len(), 1);
    }

    #[test]
    fn test_version_vector_conflicts() {
        let mut vv1 = VersionVector::new();
        let mut vv2 = VersionVector::new();

        vv1.update(100, 5);
        vv2.update(100, 3);

        vv1.update(200, 10);
        vv2.update(200, 10);

        let conflicts = vv1.conflicts(&vv2);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts.contains(&100));
    }

    #[test]
    fn test_version_vector_merge() {
        let mut vv1 = VersionVector::new();
        let vv2 = VersionVector::new();

        vv1.update(100, 5);
        vv1.update(200, 3);

        vv1.merge(&vv2);
        assert_eq!(vv1.get(100), 5);

        let mut vv3 = VersionVector::new();
        vv3.update(100, 10);
        vv3.update(300, 7);

        vv1.merge(&vv3);
        assert_eq!(vv1.get(100), 10);
        assert_eq!(vv1.get(300), 7);
    }

    #[test]
    fn test_coherence_manager_grant_lease() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        let lease = manager.grant_lease(100, 1);
        assert_eq!(lease.inode, 100);
        assert_eq!(lease.client_id, 1);

        let checked = manager.check_lease(100);
        assert!(checked.is_some());
        assert!(checked.unwrap().is_valid());
    }

    #[test]
    fn test_coherence_manager_revoke_lease() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        manager.grant_lease(100, 1);
        let inv = manager.revoke_lease(100);

        assert!(inv.is_some());
        assert_eq!(inv.unwrap().inode, 100);

        assert!(!manager.is_coherent(100));
    }

    #[test]
    fn test_coherence_manager_invalidate() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::Strict);

        manager.grant_lease(100, 1);
        manager.invalidate(100, InvalidationReason::RemoteWrite(2), 10);

        let invs = manager.pending_invalidations();
        assert_eq!(invs.len(), 1);
        assert!(matches!(invs[0].reason, InvalidationReason::RemoteWrite(2)));
    }

    #[test]
    fn test_coherence_manager_drain_invalidations() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        manager.grant_lease(100, 1);
        manager.grant_lease(200, 1);
        manager.invalidate(100, InvalidationReason::ExplicitFlush, 0);
        manager.invalidate(200, InvalidationReason::LeaseExpired, 0);

        let drained = manager.drain_invalidations();
        assert_eq!(drained.len(), 2);

        let remaining = manager.pending_invalidations();
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_coherence_manager_active_lease_count() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        assert_eq!(manager.active_lease_count(), 0);

        manager.grant_lease(100, 1);
        manager.grant_lease(200, 1);

        assert_eq!(manager.active_lease_count(), 2);

        manager.revoke_lease(100);

        assert_eq!(manager.active_lease_count(), 1);
    }

    #[test]
    fn test_coherence_manager_expire_stale_leases() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        manager.grant_lease(100, 1);
        let mut short_lease = CacheLease::new(LeaseId::new(999), 200, 1, Duration::ZERO);
        manager.leases.insert(200, short_lease);

        std::thread::sleep(Duration::from_millis(5));

        let expired = manager.expire_stale_leases();
        assert!(expired >= 1);

        assert!(!manager.is_coherent(200));
    }

    #[test]
    fn test_coherence_manager_is_coherent() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);

        assert!(!manager.is_coherent(100));

        manager.grant_lease(100, 1);
        assert!(manager.is_coherent(100));

        manager.revoke_lease(100);
        assert!(!manager.is_coherent(100));
    }

    #[test]
    fn test_protocol_default() {
        let protocol = CoherenceProtocol::default();
        assert_eq!(protocol, CoherenceProtocol::CloseToOpen);
    }

    #[test]
    fn test_invalidation_reason_variants() {
        let reasons = vec![
            InvalidationReason::LeaseExpired,
            InvalidationReason::RemoteWrite(42),
            InvalidationReason::ConflictDetected,
            InvalidationReason::ExplicitFlush,
            InvalidationReason::NodeFailover,
        ];

        assert_eq!(reasons.len(), 5);
    }

    #[test]
    fn test_version_vector_empty_conflicts() {
        let vv1 = VersionVector::new();
        let vv2 = VersionVector::new();

        let conflicts = vv1.conflicts(&vv2);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_version_vector_len() {
        let mut vv = VersionVector::new();
        assert_eq!(vv.len(), 0);

        vv.update(100, 1);
        vv.update(200, 2);

        assert_eq!(vv.len(), 2);

        vv.update(100, 5);
        assert_eq!(vv.len(), 2);
    }

    #[test]
    fn test_coherence_protocol_variants() {
        let protocols = vec![
            CoherenceProtocol::CloseToOpen,
            CoherenceProtocol::SessionBased,
            CoherenceProtocol::Strict,
        ];

        assert_eq!(protocols.len(), 3);
    }

    #[test]
    fn test_lease_state_serialize() {
        let states = vec![
            LeaseState::Active,
            LeaseState::Expired,
            LeaseState::Revoked,
            LeaseState::Renewing,
        ];

        for state in states {
            let serialized = serde_json::to_string(&state).unwrap();
            let deserialized: LeaseState = serde_json::from_str(&serialized).unwrap();
            assert_eq!(state, deserialized);
        }
    }
}
