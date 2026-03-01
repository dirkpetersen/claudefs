//! Open File Delegation
//!
//! File delegation/lease management for caching optimization.

use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegType {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DelegState {
    Active,
    Recalled { recalled_at_secs: u64 },
    Returned { returned_at_secs: u64 },
    Revoked { revoked_at_secs: u64 },
}

#[derive(Debug, Clone)]
pub struct Delegation {
    pub id: u64,
    pub ino: u64,
    pub deleg_type: DelegType,
    pub client_id: u64,
    pub granted_at_secs: u64,
    pub lease_duration_secs: u64,
    pub state: DelegState,
}

impl Delegation {
    pub fn new(
        id: u64,
        ino: u64,
        deleg_type: DelegType,
        client_id: u64,
        now_secs: u64,
        lease_secs: u64,
    ) -> Self {
        Self {
            id,
            ino,
            deleg_type,
            client_id,
            granted_at_secs: now_secs,
            lease_duration_secs: lease_secs,
            state: DelegState::Active,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, DelegState::Active)
    }

    pub fn is_returnable(&self) -> bool {
        matches!(self.state, DelegState::Active | DelegState::Recalled { .. })
    }

    pub fn is_expired(&self, now_secs: u64) -> bool {
        let expiry = self
            .granted_at_secs
            .saturating_add(self.lease_duration_secs);
        now_secs >= expiry
    }

    pub fn expires_at(&self) -> u64 {
        self.granted_at_secs
            .saturating_add(self.lease_duration_secs)
    }

    pub fn recall(&mut self, now_secs: u64) {
        if let DelegState::Active = self.state {
            self.state = DelegState::Recalled {
                recalled_at_secs: now_secs,
            };
        }
    }

    pub fn returned(&mut self, now_secs: u64) {
        if let DelegState::Recalled { .. } = self.state {
            self.state = DelegState::Returned {
                returned_at_secs: now_secs,
            };
        }
    }

    pub fn revoke(&mut self, now_secs: u64) {
        self.state = DelegState::Revoked {
            revoked_at_secs: now_secs,
        };
    }

    pub fn time_remaining_secs(&self, now_secs: u64) -> i64 {
        let expiry = self.expires_at() as i64;
        expiry - now_secs as i64
    }
}

pub struct DelegationManager {
    delegations: HashMap<u64, Delegation>,
    ino_to_deleg: HashMap<u64, Vec<u64>>,
    next_id: u64,
    default_lease_secs: u64,
}

impl DelegationManager {
    pub fn new(default_lease_secs: u64) -> Self {
        Self {
            delegations: HashMap::new(),
            ino_to_deleg: HashMap::new(),
            next_id: 1,
            default_lease_secs,
        }
    }

    pub fn grant(
        &mut self,
        ino: u64,
        deleg_type: DelegType,
        client_id: u64,
        now_secs: u64,
    ) -> std::result::Result<u64, DelegError> {
        match deleg_type {
            DelegType::Write => {
                if !self.can_grant_write(ino) {
                    return Err(DelegError::ConflictingRead(ino));
                }
            }
            DelegType::Read => {
                if !self.can_grant_read(ino) {
                    return Err(DelegError::ConflictingWrite(ino));
                }
            }
        }

        let id = self.next_id;
        self.next_id += 1;

        let deleg = Delegation::new(
            id,
            ino,
            deleg_type,
            client_id,
            now_secs,
            self.default_lease_secs,
        );
        self.delegations.insert(id, deleg);
        self.ino_to_deleg.entry(ino).or_default().push(id);

        Ok(id)
    }

    pub fn get(&self, id: u64) -> Option<&Delegation> {
        self.delegations.get(&id)
    }

    pub fn delegations_for_ino(&self, ino: u64) -> Vec<&Delegation> {
        self.ino_to_deleg
            .get(&ino)
            .map(|ids| {
                ids.iter()
                    .filter_map(|&id| self.delegations.get(&id))
                    .filter(|d| d.is_active())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn recall_for_ino(&mut self, ino: u64, now_secs: u64) -> Vec<u64> {
        let ids = self.ino_to_deleg.get(&ino).cloned().unwrap_or_default();
        let mut recalled = Vec::new();

        for id in ids {
            if let Some(deleg) = self.delegations.get_mut(&id) {
                if deleg.is_active() {
                    deleg.recall(now_secs);
                    recalled.push(id);
                }
            }
        }

        recalled
    }

    pub fn return_deleg(&mut self, id: u64, now_secs: u64) -> std::result::Result<(), DelegError> {
        let deleg = self
            .delegations
            .get_mut(&id)
            .ok_or(DelegError::NotFound(id))?;

        if !deleg.is_returnable() {
            return Err(DelegError::NotActive);
        }

        deleg.returned(now_secs);
        Ok(())
    }

    pub fn revoke_expired(&mut self, now_secs: u64) -> usize {
        let expired_ids: Vec<u64> = self
            .delegations
            .iter()
            .filter(|(_, d)| d.is_expired(now_secs) && d.is_active())
            .map(|(&id, _)| id)
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            if let Some(deleg) = self.delegations.get_mut(&id) {
                deleg.revoke(now_secs);
            }
        }
        count
    }

    pub fn active_count(&self) -> usize {
        self.delegations.values().filter(|d| d.is_active()).count()
    }

    pub fn can_grant_write(&self, ino: u64) -> bool {
        self.delegations_for_ino(ino).is_empty()
    }

    pub fn can_grant_read(&self, ino: u64) -> bool {
        !self
            .delegations_for_ino(ino)
            .iter()
            .any(|d| d.deleg_type == DelegType::Write)
    }
}

#[derive(Debug, Error)]
pub enum DelegError {
    #[error("Delegation not found: {0}")]
    NotFound(u64),
    #[error("Conflicting write delegation exists for inode {0}")]
    ConflictingWrite(u64),
    #[error("Cannot grant write delegation: read delegations exist for inode {0}")]
    ConflictingRead(u64),
    #[error("Delegation is not active")]
    NotActive,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_read_delegation() {
        let mut mgr = DelegationManager::new(300);
        let id = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();

        assert!(mgr.get(id).unwrap().is_active());
    }

    #[test]
    fn test_grant_write_delegation() {
        let mut mgr = DelegationManager::new(300);
        let id = mgr.grant(1, DelegType::Write, 100, 1000).unwrap();

        assert!(mgr.get(id).unwrap().is_active());
    }

    #[test]
    fn test_cannot_grant_write_when_read_exists() {
        let mut mgr = DelegationManager::new(300);
        mgr.grant(1, DelegType::Read, 100, 1000).unwrap();

        let result = mgr.grant(1, DelegType::Write, 200, 1000);
        assert!(matches!(result, Err(DelegError::ConflictingRead(1))));
    }

    #[test]
    fn test_cannot_grant_read_when_write_exists() {
        let mut mgr = DelegationManager::new(300);
        mgr.grant(1, DelegType::Write, 100, 1000).unwrap();

        let result = mgr.grant(1, DelegType::Read, 200, 1000);
        assert!(matches!(result, Err(DelegError::ConflictingWrite(1))));
    }

    #[test]
    fn test_multiple_read_delegations() {
        let mut mgr = DelegationManager::new(300);

        let id1 = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
        let id2 = mgr.grant(1, DelegType::Read, 200, 1000).unwrap();

        assert!(mgr.get(id1).unwrap().is_active());
        assert!(mgr.get(id2).unwrap().is_active());
    }

    #[test]
    fn test_recall_for_ino() {
        let mut mgr = DelegationManager::new(300);

        let id1 = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
        let id2 = mgr.grant(1, DelegType::Read, 200, 1000).unwrap();

        let recalled = mgr.recall_for_ino(1, 1500);

        assert_eq!(recalled.len(), 2);

        assert!(matches!(
            mgr.get(id1).unwrap().state,
            DelegState::Recalled { .. }
        ));
        assert!(matches!(
            mgr.get(id2).unwrap().state,
            DelegState::Recalled { .. }
        ));
    }

    #[test]
    fn test_return_deleg() {
        let mut mgr = DelegationManager::new(300);

        let id = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
        mgr.recall_for_ino(1, 1500);
        mgr.return_deleg(id, 1600).unwrap();

        assert!(matches!(
            mgr.get(id).unwrap().state,
            DelegState::Returned { .. }
        ));
    }

    #[test]
    fn test_return_nonexistent_fails() {
        let mut mgr = DelegationManager::new(300);

        let result = mgr.return_deleg(999, 1000);
        assert!(matches!(result, Err(DelegError::NotFound(999))));
    }

    #[test]
    fn test_revoke_expired() {
        let mut mgr = DelegationManager::new(100);

        mgr.grant(1, DelegType::Read, 100, 1000).unwrap();

        let count = mgr.revoke_expired(1200);

        assert_eq!(count, 1);
    }

    #[test]
    fn test_is_expired() {
        let deleg = Delegation::new(1, 1, DelegType::Read, 100, 1000, 300);

        assert!(!deleg.is_expired(1100));
        assert!(deleg.is_expired(1400));
    }

    #[test]
    fn test_time_remaining_positive() {
        let deleg = Delegation::new(1, 1, DelegType::Read, 100, 1000, 300);

        assert_eq!(deleg.time_remaining_secs(1050), 250);
    }

    #[test]
    fn test_time_remaining_negative() {
        let deleg = Delegation::new(1, 1, DelegType::Read, 100, 1000, 300);

        assert!(deleg.time_remaining_secs(1400) < 0);
    }

    #[test]
    fn test_can_grant_write() {
        let mgr = DelegationManager::new(300);

        assert!(mgr.can_grant_write(1));
    }

    #[test]
    fn test_can_grant_write_blocked_by_read() {
        let mut mgr = DelegationManager::new(300);
        mgr.grant(1, DelegType::Read, 100, 1000).unwrap();

        assert!(!mgr.can_grant_write(1));
    }

    #[test]
    fn test_can_grant_read() {
        let mgr = DelegationManager::new(300);

        assert!(mgr.can_grant_read(1));
    }

    #[test]
    fn test_can_grant_read_blocked_by_write() {
        let mut mgr = DelegationManager::new(300);
        mgr.grant(1, DelegType::Write, 100, 1000).unwrap();

        assert!(!mgr.can_grant_read(1));
    }

    #[test]
    fn test_delegations_for_ino_returns_active_only() {
        let mut mgr = DelegationManager::new(300);

        let id = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
        mgr.recall_for_ino(1, 1500);

        let delegs = mgr.delegations_for_ino(1);
        assert!(delegs.is_empty());
    }

    #[test]
    fn test_active_count_excludes_returned() {
        let mut mgr = DelegationManager::new(300);

        mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
        mgr.grant(2, DelegType::Read, 100, 1000).unwrap();

        let id = mgr.grant(3, DelegType::Read, 100, 1000).unwrap();
        mgr.recall_for_ino(3, 1500);
        mgr.return_deleg(id, 1600).unwrap();

        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn test_delegation_expires_at() {
        let deleg = Delegation::new(1, 1, DelegType::Read, 100, 1000, 300);

        assert_eq!(deleg.expires_at(), 1300);
    }
}
