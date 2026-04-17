//! NFSv4 delegation state machine manager.
//!
//! This module implements NFSv4 delegation grant, recall, and revocation with lease-based
//! expiry and conflict detection.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// Unique NFSv4 delegation identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DelegationId(pub u64);

/// NFSv4 delegation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DelegationType {
    /// OPEN delegation (write for exclusive, read for shared)
    Open,
    /// Read/write delegation
    ReadWrite,
    /// Read-only delegation
    Read,
}

/// NFSv4 delegation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DelegationState {
    /// Delegation has been granted to client
    Granted,
    /// Delegation recall has been initiated
    Recalled,
    /// Delegation has been revoked by server
    Revoked,
}

/// Active NFSv4 delegation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveDelegation {
    /// Unique delegation ID
    pub id: DelegationId,
    /// Client ID that holds this delegation
    pub client_id: u64,
    /// Inode being delegated
    pub inode_id: u64,
    /// Type of delegation
    pub delegation_type: DelegationType,
    /// Current state of delegation
    pub state: DelegationState,
    /// Lease expiry time in milliseconds since epoch
    pub lease_expiry_ms: u64,
    /// Description of conflicting operation that prompted recall
    pub conflicting_op: Option<String>,
}

/// Metrics for delegation tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DelegationMetrics {
    /// Total delegations granted
    pub total_granted: u64,
    /// Total recall operations initiated
    pub total_recalled: u64,
    /// Total delegations revoked
    pub total_revoked: u64,
    /// Currently active (Granted) delegations
    pub active_delegations: u64,
}

/// Errors for delegation operations.
#[derive(Debug, Error)]
pub enum DelegationError {
    /// Delegation lease has expired
    #[error("Delegation expired")]
    Expired,
    /// Lease conflict with another delegation
    #[error("Lease conflict detected")]
    LeaseConflict,
    /// Delegation not found
    #[error("Delegation not found")]
    NotFound,
    /// Invalid delegation state for operation
    #[error("Invalid delegation state")]
    InvalidState,
}

/// NFSv4 delegation manager.
pub struct DelegationManager {
    delegations: Arc<DashMap<DelegationId, ActiveDelegation>>,
    client_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    inode_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    metrics: Arc<parking_lot::Mutex<DelegationMetrics>>,
    next_id: AtomicU64,
}

impl DelegationManager {
    /// Create a new delegation manager.
    pub fn new() -> Self {
        Self {
            delegations: Arc::new(DashMap::new()),
            client_delegations: Arc::new(DashMap::new()),
            inode_delegations: Arc::new(DashMap::new()),
            metrics: Arc::new(parking_lot::Mutex::new(DelegationMetrics {
                total_granted: 0,
                total_recalled: 0,
                total_revoked: 0,
                active_delegations: 0,
            })),
            next_id: AtomicU64::new(1),
        }
    }

    /// Grant a new delegation.
    pub fn grant_delegation(
        &self,
        client_id: u64,
        inode_id: u64,
        delegation_type: DelegationType,
        lease_duration_secs: u64,
    ) -> Result<ActiveDelegation, DelegationError> {
        let id = DelegationId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let lease_expiry_ms = now_ms + (lease_duration_secs * 1000);

        let delegation = ActiveDelegation {
            id,
            client_id,
            inode_id,
            delegation_type,
            state: DelegationState::Granted,
            lease_expiry_ms,
            conflicting_op: None,
        };

        self.delegations.insert(id, delegation.clone());

        // Track by client
        self.client_delegations
            .entry(client_id)
            .or_insert_with(Vec::new)
            .push(id);

        // Track by inode
        self.inode_delegations
            .entry(inode_id)
            .or_insert_with(Vec::new)
            .push(id);

        // Update metrics
        {
            let mut metrics = self.metrics.lock();
            metrics.total_granted += 1;
            metrics.active_delegations += 1;
        }

        Ok(delegation)
    }

    /// Check if a delegation is valid (granted and not expired).
    pub fn is_delegation_valid(&self, delegation_id: DelegationId) -> bool {
        if let Some(delegation) = self.delegations.get(&delegation_id) {
            if delegation.state != DelegationState::Granted {
                return false;
            }
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            delegation.lease_expiry_ms > now_ms
        } else {
            false
        }
    }

    /// Get a delegation by ID.
    pub fn get_delegation(&self, delegation_id: DelegationId) -> Option<ActiveDelegation> {
        self.delegations.get(&delegation_id).map(|d| d.clone())
    }

    /// Recall all delegations for an inode (conflict detected).
    pub fn recall_by_inode(&self, inode_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();

        if let Some(deleg_ids) = self.inode_delegations.get(&inode_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut delegation) = self.delegations.get_mut(&id) {
                    if delegation.state == DelegationState::Granted {
                        delegation.state = DelegationState::Recalled;
                        recalled.push(id);

                        let mut metrics = self.metrics.lock();
                        metrics.total_recalled += 1;
                    }
                }
            }
        }

        Ok(recalled)
    }

    /// Recall all delegations held by a client (client disconnect).
    pub fn recall_by_client(&self, client_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();

        if let Some(deleg_ids) = self.client_delegations.get(&client_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut delegation) = self.delegations.get_mut(&id) {
                    if delegation.state == DelegationState::Granted {
                        delegation.state = DelegationState::Recalled;
                        recalled.push(id);

                        let mut metrics = self.metrics.lock();
                        metrics.total_recalled += 1;
                    }
                }
            }
        }

        Ok(recalled)
    }

    /// Process a DELEGRETURN from client.
    pub fn process_delegation_return(
        &self,
        delegation_id: DelegationId,
    ) -> Result<(), DelegationError> {
        if let Some(mut delegation) = self.delegations.get_mut(&delegation_id) {
            if delegation.state == DelegationState::Recalled {
                delegation.state = DelegationState::Revoked;

                let mut metrics = self.metrics.lock();
                metrics.total_revoked += 1;
                metrics.active_delegations = metrics.active_delegations.saturating_sub(1);

                Ok(())
            } else {
                Err(DelegationError::InvalidState)
            }
        } else {
            Err(DelegationError::NotFound)
        }
    }

    /// Clean up expired delegations.
    pub fn cleanup_expired(&self) -> Result<usize, DelegationError> {
        let mut cleaned = 0;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.delegations.retain(|_id, delegation| {
            if delegation.lease_expiry_ms < now_ms {
                cleaned += 1;
                false
            } else {
                true
            }
        });

        if cleaned > 0 {
            let mut metrics = self.metrics.lock();
            metrics.active_delegations = metrics.active_delegations.saturating_sub(cleaned);
        }

        Ok(cleaned)
    }

    /// Get current metrics.
    pub fn metrics(&self) -> DelegationMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for DelegationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_delegation() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant should succeed");

        assert_eq!(deleg.client_id, 1);
        assert_eq!(deleg.inode_id, 100);
        assert_eq!(deleg.state, DelegationState::Granted);
    }

    #[test]
    fn test_grant_delegation_multiple() {
        let mgr = DelegationManager::new();

        let d1 = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant 1 succeeds");
        let d2 = mgr
            .grant_delegation(2, 200, DelegationType::Write, 3600)
            .expect("grant 2 succeeds");

        assert_ne!(d1.id, d2.id);
    }

    #[test]
    fn test_is_delegation_valid() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant succeeds");

        assert!(mgr.is_delegation_valid(deleg.id));
    }

    #[test]
    fn test_get_delegation() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant succeeds");

        let retrieved = mgr.get_delegation(deleg.id).expect("delegation exists");
        assert_eq!(retrieved.id, deleg.id);
    }

    #[test]
    fn test_recall_by_inode() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant succeeds");

        let recalled = mgr.recall_by_inode(100).expect("recall succeeds");
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0], deleg.id);

        let deleg2 = mgr
            .get_delegation(deleg.id)
            .expect("delegation still exists");
        assert_eq!(deleg2.state, DelegationState::Recalled);
    }

    #[test]
    fn test_recall_by_client() {
        let mgr = DelegationManager::new();
        let d1 = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant 1");
        let _d2 = mgr
            .grant_delegation(1, 200, DelegationType::Write, 3600)
            .expect("grant 2");

        let recalled = mgr.recall_by_client(1).expect("recall succeeds");
        assert_eq!(recalled.len(), 2);

        let d1_recalled = mgr.get_delegation(d1.id).expect("delegation exists");
        assert_eq!(d1_recalled.state, DelegationState::Recalled);
    }

    #[test]
    fn test_process_delegation_return() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant succeeds");

        let _recalled = mgr.recall_by_inode(100).expect("recall succeeds");
        let result = mgr.process_delegation_return(deleg.id);
        assert!(result.is_ok());

        let deleg_after = mgr.get_delegation(deleg.id).expect("delegation exists");
        assert_eq!(deleg_after.state, DelegationState::Revoked);
    }

    #[test]
    fn test_cleanup_expired() {
        let mgr = DelegationManager::new();
        let _deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 0) // 0 second lease
            .expect("grant succeeds");

        std::thread::sleep(std::time::Duration::from_millis(100));

        let cleaned = mgr.cleanup_expired().expect("cleanup succeeds");
        assert!(cleaned > 0);
    }

    #[test]
    fn test_metrics() {
        let mgr = DelegationManager::new();

        mgr.grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant 1");
        mgr.grant_delegation(2, 200, DelegationType::Write, 3600)
            .expect("grant 2");

        let metrics = mgr.metrics();
        assert_eq!(metrics.total_granted, 2);
        assert_eq!(metrics.active_delegations, 2);
    }

    #[test]
    fn test_invalid_delegation_id() {
        let mgr = DelegationManager::new();
        let fake_id = DelegationId(999);

        assert!(!mgr.is_delegation_valid(fake_id));
        assert!(mgr.get_delegation(fake_id).is_none());
    }

    #[test]
    fn test_already_recalled_state() {
        let mgr = DelegationManager::new();
        let deleg = mgr
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .expect("grant succeeds");

        mgr.recall_by_inode(100).expect("first recall");

        let result = mgr.process_delegation_return(deleg.id);
        assert!(result.is_ok());

        // Can't return twice
        let result2 = mgr.process_delegation_return(deleg.id);
        assert!(matches!(result2, Err(DelegationError::InvalidState)));
    }

    #[test]
    fn test_concurrent_delegations() {
        let mgr = Arc::new(DelegationManager::new());
        let mut handles = vec![];

        for i in 0..10 {
            let mgr_clone = Arc::clone(&mgr);
            let handle = std::thread::spawn(move || {
                mgr_clone.grant_delegation(i, i * 100, DelegationType::Read, 3600)
            });
            handles.push(handle);
        }

        for handle in handles {
            let _result = handle.join().expect("thread succeeds");
        }

        let metrics = mgr.metrics();
        assert_eq!(metrics.total_granted, 10);
        assert_eq!(metrics.active_delegations, 10);
    }

    #[test]
    fn test_recall_empty_inode() {
        let mgr = DelegationManager::new();
        let recalled = mgr.recall_by_inode(999).expect("recall succeeds");
        assert_eq!(recalled.len(), 0);
    }
}
