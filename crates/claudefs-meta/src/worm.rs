//! WORM (Write Once Read Many) compliance module.
//!
//! Implements immutable file locking, retention policies, and legal holds
//! for regulatory compliance (Priority 2 feature gap). Files under WORM
//! protection cannot be modified or deleted until retention expires and
//! all legal holds are released.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// Retention policy for WORM-protected files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Minimum retention period in seconds.
    pub min_retention_secs: u64,
    /// Maximum retention period in seconds (None for infinite).
    pub max_retention_secs: Option<u64>,
    /// Whether to automatically lock file when closed.
    pub auto_lock_on_close: bool,
}

impl RetentionPolicy {
    /// Creates a new retention policy.
    pub fn new(
        min_retention_secs: u64,
        max_retention_secs: Option<u64>,
        auto_lock_on_close: bool,
    ) -> Self {
        Self {
            min_retention_secs,
            max_retention_secs,
            auto_lock_on_close,
        }
    }

    /// Creates a default retention policy with 1 year min, no max, no auto-lock.
    pub fn default_policy() -> Self {
        Self {
            min_retention_secs: 365 * 24 * 60 * 60, // 1 year
            max_retention_secs: None,
            auto_lock_on_close: false,
        }
    }
}

/// WORM protection state for a file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WormState {
    /// File is not under WORM protection.
    Unlocked,
    /// File is locked with retention period.
    Locked {
        /// Timestamp when file was locked.
        locked_at: Timestamp,
        /// Timestamp until which file is locked.
        locked_until: Timestamp,
    },
    /// File is under legal hold.
    LegalHold {
        /// Unique identifier for this legal hold.
        hold_id: String,
        /// Timestamp when hold was placed.
        held_at: Timestamp,
    },
}

impl WormState {
    /// Returns true if the file is under any WORM protection.
    pub fn is_protected(&self) -> bool {
        !matches!(self, WormState::Unlocked)
    }

    /// Returns true if the file is locked (but not under legal hold).
    pub fn is_locked(&self) -> bool {
        matches!(self, WormState::Locked { .. })
    }

    /// Returns true if the file is under legal hold.
    pub fn is_legal_hold(&self) -> bool {
        matches!(self, WormState::LegalHold { .. })
    }
}

/// Audit event for WORM operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WormAuditEvent {
    /// Timestamp of the event.
    pub timestamp: Timestamp,
    /// Type of event (e.g., "lock", "unlock", "legal_hold", "release").
    pub event_type: String,
    /// User ID of the actor who performed the operation.
    pub actor_uid: u32,
    /// Additional details about the event.
    pub details: String,
}

impl WormAuditEvent {
    /// Creates a new audit event.
    pub fn new(event_type: String, actor_uid: u32, details: String) -> Self {
        Self {
            timestamp: Timestamp::now(),
            event_type,
            actor_uid,
            details,
        }
    }
}

/// WORM entry for a protected file.
#[derive(Clone, Debug)]
pub struct WormEntry {
    /// Inode ID of the protected file.
    pub ino: InodeId,
    /// Current WORM state.
    pub state: WormState,
    /// Retention policy (None if file is unlocked).
    pub retention_policy: Option<RetentionPolicy>,
    /// Audit trail of all WORM operations.
    pub audit_trail: Vec<WormAuditEvent>,
}

impl WormEntry {
    /// Creates a new WORM entry in unlocked state.
    pub fn new(ino: InodeId) -> Self {
        Self {
            ino,
            state: WormState::Unlocked,
            retention_policy: None,
            audit_trail: Vec::new(),
        }
    }

    /// Adds an audit event to the trail.
    pub fn add_audit_event(&mut self, event: WormAuditEvent) {
        self.audit_trail.push(event);
    }
}

/// WORM manager for compliance and immutable file handling.
pub struct WormManager {
    /// Map of inode ID to WORM entry.
    entries: RwLock<HashMap<InodeId, WormEntry>>,
}

impl WormManager {
    /// Creates a new WORM manager with no protected files.
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Sets the retention policy for a file (does not lock it yet).
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    /// * `policy` - The retention policy
    /// * `actor_uid` - User performing the operation
    pub fn set_retention_policy(&self, ino: InodeId, policy: RetentionPolicy, actor_uid: u32) {
        let min_retention = policy.min_retention_secs;
        let mut entries = self.entries.write().expect("lock poisoned");
        let entry = entries.entry(ino).or_insert_with(|| WormEntry::new(ino));
        let policy_clone = policy.clone();
        entry.retention_policy = Some(policy_clone);
        entry.add_audit_event(WormAuditEvent::new(
            "set_retention_policy".to_string(),
            actor_uid,
            format!("Set retention policy: min={}s", min_retention),
        ));
        tracing::info!("Set retention policy for inode {}: {}s", ino, min_retention);
    }

    /// Locks a file with the configured retention policy.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    /// * `actor_uid` - User performing the operation
    ///
    /// # Returns
    /// Ok(()) if successful, Err(MetaError) if no retention policy is set
    pub fn lock_file(&self, ino: InodeId, actor_uid: u32) -> Result<(), MetaError> {
        let mut entries = self.entries.write().expect("lock poisoned");
        let entry = entries.entry(ino).or_insert_with(|| WormEntry::new(ino));

        let policy = entry
            .retention_policy
            .clone()
            .ok_or(MetaError::PermissionDenied)?;

        let locked_at = Timestamp::now();
        let locked_until = Timestamp {
            secs: locked_at.secs + policy.min_retention_secs,
            nanos: locked_at.nanos,
        };

        entry.state = WormState::Locked {
            locked_at,
            locked_until,
        };
        entry.add_audit_event(WormAuditEvent::new(
            "lock_file".to_string(),
            actor_uid,
            format!("File locked until {}", locked_until.secs),
        ));
        tracing::info!("Locked inode {} until {}", ino, locked_until.secs);
        Ok(())
    }

    /// Unlocks a file if retention period has expired and no legal holds exist.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    /// * `actor_uid` - User performing the operation
    ///
    /// # Returns
    /// Ok(()) if successful, Err(MetaError) if still protected
    pub fn unlock_file(&self, ino: InodeId, actor_uid: u32) -> Result<(), MetaError> {
        let mut entries = self.entries.write().expect("lock poisoned");
        let not_found = MetaError::InodeNotFound(ino);
        let entry = entries.get_mut(&ino).ok_or(not_found)?;

        match &entry.state {
            WormState::Unlocked => {
                return Ok(());
            }
            WormState::LegalHold { hold_id, .. } => {
                tracing::warn!("Cannot unlock inode {}: legal hold {} active", ino, hold_id);
                return Err(MetaError::PermissionDenied);
            }
            WormState::Locked { locked_until, .. } => {
                let now = Timestamp::now();
                if now.secs < locked_until.secs {
                    tracing::warn!(
                        "Cannot unlock inode {}: retention until {}",
                        ino,
                        locked_until.secs
                    );
                    return Err(MetaError::PermissionDenied);
                }
            }
        }

        entry.state = WormState::Unlocked;
        entry.add_audit_event(WormAuditEvent::new(
            "unlock_file".to_string(),
            actor_uid,
            "File unlocked".to_string(),
        ));
        tracing::info!("Unlocked inode {}", ino);
        Ok(())
    }

    /// Places a legal hold on a file (cannot be released until explicitly cleared).
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    /// * `hold_id` - Unique identifier for the legal hold
    /// * `actor_uid` - User performing the operation
    pub fn place_legal_hold(&self, ino: InodeId, hold_id: String, actor_uid: u32) {
        let mut entries = self.entries.write().expect("lock poisoned");
        let entry = entries.entry(ino).or_insert_with(|| WormEntry::new(ino));
        entry.state = WormState::LegalHold {
            hold_id: hold_id.clone(),
            held_at: Timestamp::now(),
        };
        entry.add_audit_event(WormAuditEvent::new(
            "place_legal_hold".to_string(),
            actor_uid,
            format!("Legal hold placed: {}", hold_id),
        ));
        tracing::info!("Placed legal hold {} on inode {}", hold_id, ino);
    }

    /// Releases a legal hold on a file.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    /// * `hold_id` - The hold ID to release
    /// * `actor_uid` - User performing the operation
    ///
    /// # Returns
    /// Ok(()) if successful, Err(MetaError) if hold not found
    pub fn release_legal_hold(
        &self,
        ino: InodeId,
        hold_id: &str,
        actor_uid: u32,
    ) -> Result<(), MetaError> {
        let mut entries = self.entries.write().expect("lock poisoned");
        let not_found = MetaError::InodeNotFound(ino);
        let entry = entries.get_mut(&ino).ok_or(not_found)?;

        if let WormState::LegalHold {
            hold_id: current_hold_id,
            ..
        } = &entry.state
        {
            if current_hold_id == hold_id {
                entry.state = WormState::Unlocked;
                entry.add_audit_event(WormAuditEvent::new(
                    "release_legal_hold".to_string(),
                    actor_uid,
                    format!("Legal hold released: {}", hold_id),
                ));
                tracing::info!("Released legal hold {} on inode {}", hold_id, ino);
                return Ok(());
            }
        }
        Err(MetaError::PermissionDenied)
    }

    /// Checks if a file is immutable (cannot be modified).
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    ///
    /// # Returns
    /// true if file is immutable
    pub fn is_immutable(&self, ino: InodeId) -> bool {
        let entries = self.entries.read().expect("lock poisoned");
        entries
            .get(&ino)
            .map(|e| e.state.is_protected())
            .unwrap_or(false)
    }

    /// Checks if a file can be deleted.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    ///
    /// # Returns
    /// true if file can be deleted
    pub fn can_delete(&self, ino: InodeId) -> bool {
        let entries = self.entries.read().expect("lock poisoned");
        match entries.get(&ino) {
            Some(entry) => !entry.state.is_protected(),
            None => true,
        }
    }

    /// Checks if a file can be modified.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    ///
    /// # Returns
    /// true if file can be modified
    pub fn can_modify(&self, ino: InodeId) -> bool {
        self.can_delete(ino)
    }

    /// Gets the current WORM state for a file.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    ///
    /// # Returns
    /// The WORM state if file exists, None otherwise
    pub fn get_state(&self, ino: InodeId) -> Option<WormState> {
        let entries = self.entries.read().expect("lock poisoned");
        entries.get(&ino).map(|e| e.state.clone())
    }

    /// Gets the audit trail for a file.
    ///
    /// # Arguments
    /// * `ino` - The inode ID
    ///
    /// # Returns
    /// Vector of audit events (empty if file not found)
    pub fn audit_trail(&self, ino: InodeId) -> Vec<WormAuditEvent> {
        let entries = self.entries.read().expect("lock poisoned");
        entries
            .get(&ino)
            .map(|e| e.audit_trail.clone())
            .unwrap_or_default()
    }

    /// Returns the number of WORM-protected files.
    pub fn worm_count(&self) -> usize {
        let entries = self.entries.read().expect("lock poisoned");
        entries.values().filter(|e| e.state.is_protected()).count()
    }
}

impl Default for WormManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retention_policy_default() {
        let policy = RetentionPolicy::default_policy();
        assert_eq!(policy.min_retention_secs, 365 * 24 * 60 * 60);
        assert!(policy.max_retention_secs.is_none());
        assert!(!policy.auto_lock_on_close);
    }

    #[test]
    fn test_retention_policy_new() {
        let policy = RetentionPolicy::new(86400, Some(2592000), true);
        assert_eq!(policy.min_retention_secs, 86400);
        assert_eq!(policy.max_retention_secs, Some(2592000));
        assert!(policy.auto_lock_on_close);
    }

    #[test]
    fn test_worm_state_unlocked() {
        let state = WormState::Unlocked;
        assert!(!state.is_protected());
        assert!(!state.is_locked());
        assert!(!state.is_legal_hold());
    }

    #[test]
    fn test_worm_state_locked() {
        let state = WormState::Locked {
            locked_at: Timestamp::now(),
            locked_until: Timestamp {
                secs: 1000,
                nanos: 0,
            },
        };
        assert!(state.is_protected());
        assert!(state.is_locked());
        assert!(!state.is_legal_hold());
    }

    #[test]
    fn test_worm_state_legal_hold() {
        let state = WormState::LegalHold {
            hold_id: "hold-123".to_string(),
            held_at: Timestamp::now(),
        };
        assert!(state.is_protected());
        assert!(!state.is_locked());
        assert!(state.is_legal_hold());
    }

    #[test]
    fn test_worm_entry_new() {
        let entry = WormEntry::new(InodeId::new(100));
        assert_eq!(entry.ino, InodeId::new(100));
        assert!(matches!(entry.state, WormState::Unlocked));
        assert!(entry.retention_policy.is_none());
        assert!(entry.audit_trail.is_empty());
    }

    #[test]
    fn test_worm_entry_add_audit_event() {
        let mut entry = WormEntry::new(InodeId::new(100));
        entry.add_audit_event(WormAuditEvent::new(
            "test".to_string(),
            0,
            "details".to_string(),
        ));
        assert_eq!(entry.audit_trail.len(), 1);
    }

    #[test]
    fn test_set_retention_policy() {
        let manager = WormManager::new();
        let policy = RetentionPolicy::new(86400, None, false);
        manager.set_retention_policy(InodeId::new(100), policy, 1000);

        let state = manager.get_state(InodeId::new(100));
        assert!(state.is_some());
        assert!(matches!(state.unwrap(), WormState::Unlocked));
    }

    #[test]
    fn test_lock_file() {
        let manager = WormManager::new();
        let policy = RetentionPolicy::new(1, None, false);
        manager.set_retention_policy(InodeId::new(100), policy, 1000);

        manager.lock_file(InodeId::new(100), 1000).unwrap();

        let state = manager.get_state(InodeId::new(100)).unwrap();
        assert!(state.is_locked());
    }

    #[test]
    fn test_lock_file_no_policy() {
        let manager = WormManager::new();
        let result = manager.lock_file(InodeId::new(100), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_unlock_file() {
        let manager = WormManager::new();
        let policy = RetentionPolicy::new(0, None, false);
        manager.set_retention_policy(InodeId::new(100), policy, 1000);
        manager.lock_file(InodeId::new(100), 1000).unwrap();

        manager.unlock_file(InodeId::new(100), 1000).unwrap();

        let state = manager.get_state(InodeId::new(100)).unwrap();
        assert!(matches!(state, WormState::Unlocked));
    }

    #[test]
    fn test_unlock_file_legal_hold_prevents() {
        let manager = WormManager::new();
        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);

        let result = manager.unlock_file(InodeId::new(100), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_place_legal_hold() {
        let manager = WormManager::new();
        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);

        let state = manager.get_state(InodeId::new(100)).unwrap();
        assert!(state.is_legal_hold());
    }

    #[test]
    fn test_release_legal_hold() {
        let manager = WormManager::new();
        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);

        manager
            .release_legal_hold(InodeId::new(100), "hold-1", 1000)
            .unwrap();

        let state = manager.get_state(InodeId::new(100)).unwrap();
        assert!(matches!(state, WormState::Unlocked));
    }

    #[test]
    fn test_release_legal_hold_wrong_id() {
        let manager = WormManager::new();
        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);

        let result = manager.release_legal_hold(InodeId::new(100), "wrong-id", 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_immutable() {
        let manager = WormManager::new();
        assert!(!manager.is_immutable(InodeId::new(100)));

        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);
        assert!(manager.is_immutable(InodeId::new(100)));
    }

    #[test]
    fn test_can_delete() {
        let manager = WormManager::new();
        assert!(manager.can_delete(InodeId::new(100)));

        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);
        assert!(!manager.can_delete(InodeId::new(100)));
    }

    #[test]
    fn test_can_modify() {
        let manager = WormManager::new();
        assert!(manager.can_modify(InodeId::new(100)));

        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);
        assert!(!manager.can_modify(InodeId::new(100)));
    }

    #[test]
    fn test_audit_trail() {
        let manager = WormManager::new();
        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);

        let trail = manager.audit_trail(InodeId::new(100));
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].event_type, "place_legal_hold");
    }

    #[test]
    fn test_worm_count() {
        let manager = WormManager::new();
        assert_eq!(manager.worm_count(), 0);

        manager.place_legal_hold(InodeId::new(100), "hold-1".to_string(), 1000);
        assert_eq!(manager.worm_count(), 1);

        let policy = RetentionPolicy::new(86400, None, false);
        manager.set_retention_policy(InodeId::new(200), policy, 1000);
        manager.lock_file(InodeId::new(200), 1000).unwrap();
        assert_eq!(manager.worm_count(), 2);
    }
}
