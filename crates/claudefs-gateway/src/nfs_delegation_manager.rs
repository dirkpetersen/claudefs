use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use thiserror::Error;

/// Unique delegation identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DelegationId(pub u64);

/// NFSv4 stateid_other field
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DelegationCookie(pub [u8; 8]);

/// NFSv4 delegation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationType {
    Open,
    ReadWrite,
    Read,
}

/// NFSv4 delegation state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationState {
    Granted(DelegationCookie, u64),
    Recalled(u64),
    Revoked(u64),
}

/// Active NFSv4 delegation
#[derive(Debug, Clone)]
pub struct ActiveDelegation {
    pub id: DelegationId,
    pub client_id: u64,
    pub inode_id: u64,
    pub delegation_type: DelegationType,
    pub state: DelegationState,
    pub lease_expiry_ns: u64,
    pub conflicting_op: Option<String>,
}

/// Delegation metrics
#[derive(Debug, Clone, Default)]
pub struct DelegationMetrics {
    pub total_granted: u64,
    pub total_recalled: u64,
    pub total_revoked: u64,
    pub active_delegations: u64,
    pub recall_latency_ms: Vec<u64>,
}

/// Delegation manager errors
#[derive(Error, Debug)]
pub enum DelegationError {
    #[error("delegation expired")]
    Expired,
    #[error("lease conflict")]
    LeaseConflict,
    #[error("delegation not found")]
    NotFound,
    #[error("invalid state transition")]
    InvalidState,
}

/// Manages NFSv4 delegations
pub struct DelegationManager {
    delegations: Arc<DashMap<DelegationId, ActiveDelegation>>,
    client_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    inode_delegations: Arc<DashMap<u64, Vec<DelegationId>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    metrics: Arc<std::sync::Mutex<DelegationMetrics>>,
}

impl DelegationManager {
    pub fn new() -> Self {
        Self {
            delegations: Arc::new(DashMap::new()),
            client_delegations: Arc::new(DashMap::new()),
            inode_delegations: Arc::new(DashMap::new()),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            metrics: Arc::new(tokio::sync::RwLock::new(DelegationMetrics::default())),
        }
    }

    pub async fn grant_delegation(
        &self,
        client_id: u64,
        inode_id: u64,
        delegation_type: DelegationType,
        lease_duration_secs: u64,
    ) -> Result<ActiveDelegation, DelegationError> {
        let now_ns = current_time_ns();
        let lease_expiry_ns = now_ns + (lease_duration_secs * 1_000_000_000);

        let delegation_id = DelegationId(
            self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );

        let mut cookie = [0u8; 8];
        cookie[..8].copy_from_slice(&delegation_id.0.to_ne_bytes());

        let delegation = ActiveDelegation {
            id: delegation_id,
            client_id,
            inode_id,
            delegation_type,
            state: DelegationState::Granted(DelegationCookie(cookie), now_ns),
            lease_expiry_ns,
            conflicting_op: None,
        };

        self.delegations.insert(delegation_id, delegation.clone());

        self.client_delegations
            .entry(client_id)
            .or_insert_with(Vec::new)
            .push(delegation_id);

        self.inode_delegations
            .entry(inode_id)
            .or_insert_with(Vec::new)
            .push(delegation_id);

        let mut metrics = self.metrics.lock();
        metrics.total_granted += 1;
        metrics.active_delegations += 1;

        Ok(delegation)
    }

    pub fn is_delegation_valid(&self, delegation_id: DelegationId) -> bool {
        if let Some(deleg) = self.delegations.get(&delegation_id) {
            let now_ns = current_time_ns();
            !matches!(deleg.state, DelegationState::Revoked(_)) &&
                deleg.lease_expiry_ns > now_ns
        } else {
            false
        }
    }

    pub fn get_delegation(&self, delegation_id: DelegationId) -> Option<ActiveDelegation> {
        self.delegations.get(&delegation_id).map(|d| d.clone())
    }

    pub async fn recall_by_inode(&self, inode_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();
        let now_ns = current_time_ns();

        if let Some(deleg_ids) = self.inode_delegations.get(&inode_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut deleg) = self.delegations.get_mut(&id) {
                    if !matches!(deleg.state, DelegationState::Revoked(_)) {
                        deleg.state = DelegationState::Recalled(now_ns);
                        recalled.push(id);
                    }
                }
            }
        }

        let mut metrics = self.metrics.lock();
        metrics.total_recalled += recalled.len() as u64;
        metrics.active_delegations = metrics.active_delegations.saturating_sub(recalled.len() as u64);

        Ok(recalled)
    }

    pub async fn recall_by_client(&self, client_id: u64) -> Result<Vec<DelegationId>, DelegationError> {
        let mut recalled = Vec::new();
        let now_ns = current_time_ns();

        if let Some(deleg_ids) = self.client_delegations.get(&client_id) {
            for &id in deleg_ids.iter() {
                if let Some(mut deleg) = self.delegations.get_mut(&id) {
                    if !matches!(deleg.state, DelegationState::Revoked(_)) {
                        deleg.state = DelegationState::Recalled(now_ns);
                        recalled.push(id);
                    }
                }
            }
        }

        let mut metrics = self.metrics.lock();
        metrics.total_recalled += recalled.len() as u64;
        metrics.active_delegations = metrics.active_delegations.saturating_sub(recalled.len() as u64);

        Ok(recalled)
    }

    pub fn process_delegation_return(&self, delegation_id: DelegationId) -> Result<(), DelegationError> {
        if let Some(mut deleg) = self.delegations.get_mut(&delegation_id) {
            deleg.state = DelegationState::Revoked(current_time_ns());
            let mut metrics = self.metrics.lock();
            metrics.total_revoked += 1;
            metrics.active_delegations = metrics.active_delegations.saturating_sub(1);
            Ok(())
        } else {
            Err(DelegationError::NotFound)
        }
    }

    pub async fn cleanup_expired(&self) -> Result<usize, DelegationError> {
        let mut count = 0;
        let now_ns = current_time_ns();

        let expired_ids: Vec<DelegationId> = self
            .delegations
            .iter()
            .filter(|entry| {
                entry.value().lease_expiry_ns <= now_ns ||
                matches!(entry.value().state, DelegationState::Revoked(_))
            })
            .map(|entry| *entry.key())
            .collect();

        for id in expired_ids {
            self.delegations.remove(&id);
            count += 1;
        }

        let mut metrics = self.metrics.lock();
        metrics.active_delegations = self.delegations.len() as u64;

        Ok(count)
    }

    pub fn metrics(&self) -> DelegationMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for DelegationManager {
    fn default() -> Self {
        Self::new()
    }
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_grant_delegation() {
        let manager = DelegationManager::new();
        let deleg = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();

        assert_eq!(deleg.client_id, 1);
        assert_eq!(deleg.inode_id, 100);
    }

    #[tokio::test]
    async fn test_is_delegation_valid() {
        let manager = DelegationManager::new();
        let deleg = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();

        assert!(manager.is_delegation_valid(deleg.id));
    }

    #[tokio::test]
    async fn test_recall_by_inode() {
        let manager = DelegationManager::new();
        let deleg1 = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();
        let _deleg2 = manager
            .grant_delegation(2, 100, DelegationType::ReadWrite, 3600)
            .await
            .unwrap();

        let recalled = manager.recall_by_inode(100).await.unwrap();
        assert_eq!(recalled.len(), 2);
        assert!(!manager.is_delegation_valid(deleg1.id));
    }

    #[tokio::test]
    async fn test_recall_by_client() {
        let manager = DelegationManager::new();
        let deleg1 = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();
        let _deleg2 = manager
            .grant_delegation(1, 101, DelegationType::ReadWrite, 3600)
            .await
            .unwrap();

        let recalled = manager.recall_by_client(1).await.unwrap();
        assert_eq!(recalled.len(), 2);
        assert!(!manager.is_delegation_valid(deleg1.id));
    }

    #[tokio::test]
    async fn test_delegation_return() {
        let manager = DelegationManager::new();
        let deleg = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();

        manager.process_delegation_return(deleg.id).unwrap();
        assert!(!manager.is_delegation_valid(deleg.id));
    }

    #[tokio::test]
    async fn test_metrics() {
        let manager = DelegationManager::new();
        let _ = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();

        let metrics = manager.metrics();
        assert_eq!(metrics.total_granted, 1);
        assert_eq!(metrics.active_delegations, 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let manager = DelegationManager::new();
        let _ = manager
            .grant_delegation(1, 100, DelegationType::Read, 1)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;

        let count = manager.cleanup_expired().await.unwrap();
        assert!(count > 0);
    }

    #[tokio::test]
    async fn test_get_delegation() {
        let manager = DelegationManager::new();
        let deleg = manager
            .grant_delegation(1, 100, DelegationType::Read, 3600)
            .await
            .unwrap();

        let retrieved = manager.get_delegation(deleg.id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().client_id, 1);
    }
}
