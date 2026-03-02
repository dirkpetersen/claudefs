//! NFSv4 file delegation management

use rand::thread_rng;
use rand::RngCore;
use std::collections::HashMap;
use thiserror::Error;

/// Delegation type - specifies what operations are delegated to the client.
pub enum DelegationType {
    /// Read delegation allows caching for read operations
    Read,
    /// Write delegation allows caching for read/write operations
    Write,
}

/// Delegation state - current status of the delegation.
pub enum DelegationState {
    /// Delegation has been granted to the client
    Granted,
    /// Client has been asked to recall the delegation
    RecallPending,
    /// Delegation has been returned to the server
    Returned,
    /// Delegation has been revoked by the server
    Revoked,
}

/// Unique identifier for a delegation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelegationId(pub [u8; 16]);

impl DelegationId {
    /// Generates a new random delegation ID.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 16];
        thread_rng().fill_bytes(&mut bytes);
        DelegationId(bytes)
    }

    /// Returns the delegation ID as a hex string.
    pub fn as_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// NFSv4 delegation - grants a client the right to cache file data.
pub struct Delegation {
    /// Unique delegation identifier
    pub id: DelegationId,
    /// File inode number
    pub file_id: u64,
    /// Client identifier
    pub client_id: u64,
    /// Type of delegation (read or write)
    pub delegation_type: DelegationType,
    /// Current state of the delegation
    pub state: DelegationState,
    /// Timestamp when delegation was granted (milliseconds since epoch)
    pub granted_at_ms: u64,
}

impl Delegation {
    /// Creates a new delegation with the given parameters.
    pub fn new(file_id: u64, client_id: u64, delegation_type: DelegationType) -> Self {
        Delegation {
            id: DelegationId::generate(),
            file_id,
            client_id,
            delegation_type,
            state: DelegationState::Granted,
            granted_at_ms: 0,
        }
    }

    /// Returns true if the delegation is currently active (granted).
    pub fn is_active(&self) -> bool {
        matches!(self.state, DelegationState::Granted)
    }

    /// Initiates recall of this delegation, requesting the client to return it.
    pub fn initiate_recall(&mut self) {
        if matches!(self.state, DelegationState::Granted) {
            self.state = DelegationState::RecallPending;
        }
    }

    /// Marks the delegation as returned by the client.
    pub fn mark_returned(&mut self) {
        self.state = DelegationState::Returned;
    }

    /// Revokes the delegation from the client.
    pub fn revoke(&mut self) {
        self.state = DelegationState::Revoked;
    }
}

/// Errors that can occur when managing delegations.
#[derive(Debug, Error)]
pub enum DelegationError {
    /// The requested delegation was not found.
    #[error("delegation not found")]
    NotFound,
    /// A write delegation already exists for this file.
    #[error("write delegation conflict: file {0} already has a write delegation")]
    WriteConflict(u64),
    /// The delegation has already been returned.
    #[error("delegation already returned")]
    AlreadyReturned,
}

/// Manages NFSv4 delegations for all files and clients.
pub struct DelegationManager {
    delegations: HashMap<DelegationId, Delegation>,
}

impl DelegationManager {
    /// Creates a new empty delegation manager.
    pub fn new() -> Self {
        DelegationManager {
            delegations: HashMap::new(),
        }
    }

    /// Grants a delegation to a client for a file.
    /// Returns the delegation ID on success, or an error if a write conflict exists.
    pub fn grant(
        &mut self,
        file_id: u64,
        client_id: u64,
        delegation_type: DelegationType,
    ) -> Result<DelegationId, DelegationError> {
        for del in self.delegations.values() {
            if del.file_id == file_id
                && del.is_active()
                && matches!(del.delegation_type, DelegationType::Write)
            {
                return Err(DelegationError::WriteConflict(file_id));
            }
        }

        let delegation = Delegation::new(file_id, client_id, delegation_type);
        let id = delegation.id.clone();
        self.delegations.insert(id.clone(), delegation);
        Ok(id)
    }

    /// Retrieves a delegation by its ID.
    pub fn get(&self, id: &DelegationId) -> Option<&Delegation> {
        self.delegations.get(id)
    }

    /// Marks a delegation as returned by the client.
    pub fn return_delegation(&mut self, id: &DelegationId) -> Result<(), DelegationError> {
        let delegation = self
            .delegations
            .get_mut(id)
            .ok_or(DelegationError::NotFound)?;

        if matches!(delegation.state, DelegationState::Returned) {
            return Err(DelegationError::AlreadyReturned);
        }

        delegation.mark_returned();
        Ok(())
    }

    /// Recalls all delegations for a file, returning IDs of recalled delegations.
    pub fn recall_file(&mut self, file_id: u64) -> Vec<DelegationId> {
        let mut result = Vec::new();
        for (id, del) in self.delegations.iter_mut() {
            if del.file_id == file_id && del.is_active() {
                del.initiate_recall();
                result.push(id.clone());
            }
        }
        result
    }

    /// Revokes all delegations held by a client, returning IDs of revoked delegations.
    pub fn revoke_client(&mut self, client_id: u64) -> Vec<DelegationId> {
        let mut result = Vec::new();
        for (id, del) in self.delegations.iter_mut() {
            if del.client_id == client_id && del.is_active() {
                del.revoke();
                result.push(id.clone());
            }
        }
        result
    }

    /// Returns the number of active (granted) delegations.
    pub fn active_count(&self) -> usize {
        self.delegations.values().filter(|d| d.is_active()).count()
    }

    /// Returns the total number of delegations (including returned/revoked).
    pub fn total_count(&self) -> usize {
        self.delegations.len()
    }

    /// Returns all delegations for a file (regardless of state).
    pub fn file_delegations(&self, file_id: u64) -> Vec<&Delegation> {
        self.delegations
            .values()
            .filter(|d| d.file_id == file_id)
            .collect()
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
    fn test_delegation_id_unique() {
        let ids: std::collections::HashSet<_> =
            (0..100).map(|_| DelegationId::generate()).collect();
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn test_delegation_id_hex_length() {
        let id = DelegationId::generate();
        assert_eq!(id.as_hex().len(), 32);
    }

    #[test]
    fn test_delegation_new_creates_granted() {
        let d = Delegation::new(1, 100, DelegationType::Read);
        assert!(matches!(d.state, DelegationState::Granted));
        assert!(d.is_active());
    }

    #[test]
    fn test_delegation_is_active_true_for_granted() {
        let d = Delegation::new(1, 100, DelegationType::Read);
        assert!(d.is_active());
    }

    #[test]
    fn test_delegation_initiate_recall() {
        let mut d = Delegation::new(1, 100, DelegationType::Read);
        d.initiate_recall();
        assert!(matches!(d.state, DelegationState::RecallPending));
    }

    #[test]
    fn test_delegation_mark_returned() {
        let mut d = Delegation::new(1, 100, DelegationType::Read);
        d.mark_returned();
        assert!(matches!(d.state, DelegationState::Returned));
    }

    #[test]
    fn test_delegation_revoke() {
        let mut d = Delegation::new(1, 100, DelegationType::Read);
        d.revoke();
        assert!(matches!(d.state, DelegationState::Revoked));
    }

    #[test]
    fn test_delegation_is_active_false_after_recall() {
        let mut d = Delegation::new(1, 100, DelegationType::Read);
        d.initiate_recall();
        assert!(!d.is_active());
    }

    #[test]
    fn test_delegation_manager_new_empty() {
        let m = DelegationManager::new();
        assert_eq!(m.active_count(), 0);
        assert_eq!(m.total_count(), 0);
    }

    #[test]
    fn test_grant_read_delegation_succeeds() {
        let mut m = DelegationManager::new();
        let id = m.grant(1, 100, DelegationType::Read).unwrap();
        assert!(m.get(&id).is_some());
    }

    #[test]
    fn test_grant_write_delegation_succeeds() {
        let mut m = DelegationManager::new();
        let id = m.grant(1, 100, DelegationType::Write).unwrap();
        assert!(m.get(&id).is_some());
    }

    #[test]
    fn test_grant_second_write_fails() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Write).unwrap();
        let result = m.grant(1, 200, DelegationType::Write);
        assert!(matches!(result, Err(DelegationError::WriteConflict(1))));
    }

    #[test]
    fn test_grant_read_after_write_fails() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Write).unwrap();
        let result = m.grant(1, 200, DelegationType::Read);
        assert!(matches!(result, Err(DelegationError::WriteConflict(1))));
    }

    #[test]
    fn test_grant_write_after_read_succeeds() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        let id = m.grant(1, 200, DelegationType::Write).unwrap();
        assert!(m.get(&id).is_some());
    }

    #[test]
    fn test_multiple_read_delegations() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        m.grant(1, 200, DelegationType::Read).unwrap();
        m.grant(1, 300, DelegationType::Read).unwrap();
        assert_eq!(m.file_delegations(1).len(), 3);
    }

    #[test]
    fn test_return_delegation_success() {
        let mut m = DelegationManager::new();
        let id = m.grant(1, 100, DelegationType::Read).unwrap();
        m.return_delegation(&id).unwrap();
        assert!(!m.get(&id).unwrap().is_active());
    }

    #[test]
    fn test_return_delegation_not_found() {
        let mut m = DelegationManager::new();
        let id = DelegationId::generate();
        let result = m.return_delegation(&id);
        assert!(matches!(result, Err(DelegationError::NotFound)));
    }

    #[test]
    fn test_return_delegation_already_returned() {
        let mut m = DelegationManager::new();
        let id = m.grant(1, 100, DelegationType::Read).unwrap();
        m.return_delegation(&id).unwrap();
        let result = m.return_delegation(&id);
        assert!(matches!(result, Err(DelegationError::AlreadyReturned)));
    }

    #[test]
    fn test_recall_file_returns_ids() {
        let mut m = DelegationManager::new();
        let id1 = m.grant(1, 100, DelegationType::Read).unwrap();
        let id2 = m.grant(1, 200, DelegationType::Read).unwrap();
        let ids = m.recall_file(1);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_recall_file_no_delegations() {
        let mut m = DelegationManager::new();
        let ids = m.recall_file(999);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_recall_file_sets_recall_pending() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        m.recall_file(1);
        let delegations = m.file_delegations(1);
        let del = delegations.first().unwrap();
        assert!(matches!(del.state, DelegationState::RecallPending));
    }

    #[test]
    fn test_revoke_client() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        m.grant(2, 100, DelegationType::Read).unwrap();
        m.grant(3, 200, DelegationType::Read).unwrap();
        let ids = m.revoke_client(100);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_revoke_client_sets_revoked() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        m.revoke_client(100);
        let delegations = m.file_delegations(1);
        let del = delegations.first().unwrap();
        assert!(matches!(del.state, DelegationState::Revoked));
    }

    #[test]
    fn test_active_count_only_granted() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        let id2 = m.grant(2, 100, DelegationType::Read).unwrap();
        m.return_delegation(&id2).unwrap();
        assert_eq!(m.active_count(), 1);
    }

    #[test]
    fn test_file_delegations() {
        let mut m = DelegationManager::new();
        m.grant(1, 100, DelegationType::Read).unwrap();
        m.grant(1, 200, DelegationType::Read).unwrap();
        m.grant(2, 300, DelegationType::Read).unwrap();
        let delegations = m.file_delegations(1);
        assert_eq!(delegations.len(), 2);
    }

    #[test]
    fn test_total_count_all_states() {
        let mut m = DelegationManager::new();
        let id = m.grant(1, 100, DelegationType::Read).unwrap();
        m.return_delegation(&id).unwrap();
        m.grant(2, 100, DelegationType::Read).unwrap();
        assert_eq!(m.total_count(), 2);
    }
}
