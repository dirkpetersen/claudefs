//! Open File Delegation
//!
//! File delegation/lease management for caching optimization.
//!
//! This module implements NFSv4-style open file delegation, allowing clients
//! to cache file data locally with the server's blessing. Delegations grant
//! exclusive or shared access rights and can be recalled when conflicting
//! operations arrive.

use std::collections::HashMap;
use thiserror::Error;

/// Type of delegation granted to a client.
///
/// Determines whether the client has exclusive write access or shared read access
/// to the delegated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegType {
    /// Read delegation: allows multiple clients to cache reads simultaneously.
    /// Other read delegations can coexist, but write delegations are blocked.
    Read,
    /// Write delegation: exclusive access for both reads and writes.
    /// No other delegations (read or write) can exist on the same inode.
    Write,
}

/// Lifecycle state of a delegation.
///
/// Tracks the delegation through grant, recall, return, and revocation phases.
#[derive(Debug, Clone, PartialEq)]
pub enum DelegState {
    /// Delegation is active and the client holds valid caching rights.
    Active,
    /// Server has requested the delegation be returned.
    /// Client must respond within the recall timeout or face revocation.
    Recalled {
        /// Unix timestamp (seconds) when the recall was initiated.
        recalled_at_secs: u64,
    },
    /// Client has voluntarily returned the delegation.
    Returned {
        /// Unix timestamp (seconds) when the delegation was returned.
        returned_at_secs: u64,
    },
    /// Delegation was forcibly revoked by the server.
    /// Occurs on timeout, client crash recovery, or conflicting operations.
    Revoked {
        /// Unix timestamp (seconds) when the delegation was revoked.
        revoked_at_secs: u64,
    },
}

/// Represents an active or historical file delegation.
///
/// A delegation grants a client the right to cache file data locally
/// for a specified lease duration. The server may recall delegations
/// when conflicting operations arrive.
#[derive(Debug, Clone)]
pub struct Delegation {
    /// Unique identifier for this delegation instance.
    pub id: u64,
    /// Inode number of the delegated file.
    pub ino: u64,
    /// Type of delegation (read or write).
    pub deleg_type: DelegType,
    /// Identifier of the client holding this delegation.
    pub client_id: u64,
    /// Unix timestamp (seconds) when the delegation was granted.
    pub granted_at_secs: u64,
    /// Duration of the lease in seconds from grant time.
    pub lease_duration_secs: u64,
    /// Current state of the delegation lifecycle.
    pub state: DelegState,
}

impl Delegation {
    /// Creates a new active delegation.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique delegation identifier.
    /// * `ino` - Inode number of the file to delegate.
    /// * `deleg_type` - Read or write delegation type.
    /// * `client_id` - Identifier of the client receiving the delegation.
    /// * `now_secs` - Current Unix timestamp in seconds.
    /// * `lease_secs` - Lease duration in seconds.
    ///
    /// # Returns
    ///
    /// A new `Delegation` in the `Active` state.
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

    /// Returns `true` if the delegation is currently active.
    pub fn is_active(&self) -> bool {
        matches!(self.state, DelegState::Active)
    }

    /// Returns `true` if the delegation can be returned by the client.
    ///
    /// Both `Active` and `Recalled` delegations are returnable.
    pub fn is_returnable(&self) -> bool {
        matches!(self.state, DelegState::Active | DelegState::Recalled { .. })
    }

    /// Checks if the delegation lease has expired.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Returns
    ///
    /// `true` if the current time is at or past the lease expiry time.
    pub fn is_expired(&self, now_secs: u64) -> bool {
        let expiry = self
            .granted_at_secs
            .saturating_add(self.lease_duration_secs);
        now_secs >= expiry
    }

    /// Computes the absolute lease expiry timestamp.
    ///
    /// # Returns
    ///
    /// Unix timestamp (seconds) when the delegation expires.
    pub fn expires_at(&self) -> u64 {
        self.granted_at_secs
            .saturating_add(self.lease_duration_secs)
    }

    /// Transitions the delegation from `Active` to `Recalled`.
    ///
    /// If the delegation is not `Active`, this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds (recorded as recall time).
    pub fn recall(&mut self, now_secs: u64) {
        if let DelegState::Active = self.state {
            self.state = DelegState::Recalled {
                recalled_at_secs: now_secs,
            };
        }
    }

    /// Transitions the delegation from `Recalled` to `Returned`.
    ///
    /// If the delegation is not `Recalled`, this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds (recorded as return time).
    pub fn returned(&mut self, now_secs: u64) {
        if let DelegState::Recalled { .. } = self.state {
            self.state = DelegState::Returned {
                returned_at_secs: now_secs,
            };
        }
    }

    /// Forcibly revokes the delegation regardless of current state.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds (recorded as revocation time).
    pub fn revoke(&mut self, now_secs: u64) {
        self.state = DelegState::Revoked {
            revoked_at_secs: now_secs,
        };
    }

    /// Computes remaining lease time, which may be negative if expired.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Returns
    ///
    /// Seconds remaining until expiry (negative if already expired).
    pub fn time_remaining_secs(&self, now_secs: u64) -> i64 {
        let expiry = self.expires_at() as i64;
        expiry - now_secs as i64
    }
}

/// Manages all active file delegations for the filesystem.
///
/// Tracks delegations by ID and inode, enforces conflict rules,
/// handles recall/return/revoke lifecycle, and expires stale delegations.
pub struct DelegationManager {
    delegations: HashMap<u64, Delegation>,
    ino_to_deleg: HashMap<u64, Vec<u64>>,
    next_id: u64,
    default_lease_secs: u64,
}

impl DelegationManager {
    /// Creates a new delegation manager.
    ///
    /// # Arguments
    ///
    /// * `default_lease_secs` - Default lease duration for new delegations.
    pub fn new(default_lease_secs: u64) -> Self {
        Self {
            delegations: HashMap::new(),
            ino_to_deleg: HashMap::new(),
            next_id: 1,
            default_lease_secs,
        }
    }

    /// Grants a new delegation for an inode to a client.
    ///
    /// # Arguments
    ///
    /// * `ino` - Inode number to delegate.
    /// * `deleg_type` - Read or write delegation.
    /// * `client_id` - Client receiving the delegation.
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Errors
    ///
    /// * `DelegError::ConflictingRead` - Write delegation requested but read delegations exist.
    /// * `DelegError::ConflictingWrite` - Read delegation requested but a write delegation exists.
    ///
    /// # Returns
    ///
    /// The unique delegation ID on success.
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

    /// Retrieves a delegation by its ID.
    ///
    /// # Returns
    ///
    /// `Some(&Delegation)` if found, `None` otherwise.
    pub fn get(&self, id: u64) -> Option<&Delegation> {
        self.delegations.get(&id)
    }

    /// Returns all active delegations for a given inode.
    ///
    /// # Arguments
    ///
    /// * `ino` - Inode number to query.
    ///
    /// # Returns
    ///
    /// Vector of references to active delegations (empty if none).
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

    /// Recalls all active delegations for an inode.
    ///
    /// Called when a conflicting operation (e.g., write when read delegations exist)
    /// requires clients to flush their caches.
    ///
    /// # Arguments
    ///
    /// * `ino` - Inode number whose delegations should be recalled.
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Returns
    ///
    /// Vector of delegation IDs that were recalled.
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

    /// Processes a client's voluntary return of a delegation.
    ///
    /// # Arguments
    ///
    /// * `id` - Delegation ID being returned.
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Errors
    ///
    /// * `DelegError::NotFound` - No delegation with the given ID.
    /// * `DelegError::NotActive` - Delegation is not in a returnable state.
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

    /// Revokes all expired active delegations.
    ///
    /// Should be called periodically to clean up delegations whose
    /// leases have elapsed without client return.
    ///
    /// # Arguments
    ///
    /// * `now_secs` - Current Unix timestamp in seconds.
    ///
    /// # Returns
    ///
    /// Number of delegations revoked.
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

    /// Returns the count of currently active delegations.
    pub fn active_count(&self) -> usize {
        self.delegations.values().filter(|d| d.is_active()).count()
    }

    /// Checks if a write delegation can be granted for the given inode.
    ///
    /// Write delegations require exclusive access; no other delegations
    /// (read or write) may exist.
    ///
    /// # Arguments
    ///
    /// * `ino` - Inode number to check.
    ///
    /// # Returns
    ///
    /// `true` if no active delegations exist for this inode.
    pub fn can_grant_write(&self, ino: u64) -> bool {
        self.delegations_for_ino(ino).is_empty()
    }

    /// Checks if a read delegation can be granted for the given inode.
    ///
    /// Read delegations can coexist with other read delegations, but not
    /// with write delegations.
    ///
    /// # Arguments
    ///
    /// * `ino` - Inode number to check.
    ///
    /// # Returns
    ///
    /// `true` if no active write delegation exists for this inode.
    pub fn can_grant_read(&self, ino: u64) -> bool {
        !self
            .delegations_for_ino(ino)
            .iter()
            .any(|d| d.deleg_type == DelegType::Write)
    }
}

/// Errors that can occur during delegation operations.
#[derive(Debug, Error)]
pub enum DelegError {
    /// The requested delegation does not exist.
    #[error("Delegation not found: {0}")]
    NotFound(u64),
    /// A write delegation already exists, blocking a read delegation request.
    #[error("Conflicting write delegation exists for inode {0}")]
    ConflictingWrite(u64),
    /// Read delegations already exist, blocking a write delegation request.
    #[error("Cannot grant write delegation: read delegations exist for inode {0}")]
    ConflictingRead(u64),
    /// The delegation is not in an active or returnable state.
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

        let _id = mgr.grant(1, DelegType::Read, 100, 1000).unwrap();
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
