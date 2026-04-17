//! WORM enforcement for compliance and legal hold support.
//!
//! Provides enforcement of immutability, legal holds, and retention policies
//! for files in the FUSE filesystem.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Immutability level for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImmutabilityLevel {
    /// No immutability restrictions.
    None,
    /// Temporarily immutable until the specified timestamp.
    Temporary { until_ns: u64 },
    /// Permanently immutable (cannot be changed).
    Permanent,
}

impl ImmutabilityLevel {
    /// Checks if this immutability level blocks writes at the current time.
    pub fn blocks_writes(&self) -> bool {
        match self {
            ImmutabilityLevel::None => false,
            ImmutabilityLevel::Temporary { until_ns } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;
                now < *until_ns
            }
            ImmutabilityLevel::Permanent => true,
        }
    }

    /// Checks if this immutability level blocks deletes at the current time.
    pub fn blocks_deletes(&self) -> bool {
        self.blocks_writes()
    }

    /// Returns true if the temporary immutability has expired.
    pub fn is_expired(&self) -> bool {
        match self {
            ImmutabilityLevel::Temporary { until_ns } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;
                now >= *until_ns
            }
            ImmutabilityLevel::Permanent => false,
            ImmutabilityLevel::None => true,
        }
    }
}

/// Legal hold type for compliance and litigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegalHoldType {
    /// Default compliance hold.
    Compliance,
    /// Litigation hold.
    Litigation,
    /// Custom vendor-specific hold.
    Custom(u8),
}

impl LegalHoldType {
    /// Returns a display name for the hold type.
    pub fn display_name(&self) -> &'static str {
        match self {
            LegalHoldType::Compliance => "Compliance",
            LegalHoldType::Litigation => "Litigation",
            LegalHoldType::Custom(_) => "Custom",
        }
    }
}

/// Legal hold on a file.
#[derive(Debug, Clone)]
pub struct LegalHold {
    /// Type of legal hold.
    pub hold_type: LegalHoldType,
    /// User ID who initiated the hold.
    pub initiated_by: String,
    /// Timestamp when hold was created (ns since epoch).
    pub created_ns: u64,
    /// Reason for the hold.
    pub reason: String,
}

/// Retention policy for time-based WORM protection.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Retention period in years.
    pub retention_years: u16,
    /// Timestamp when retention ends (ns since epoch).
    pub retain_until_ns: u64,
    /// Grace period in days after retention expires.
    pub grace_period_days: u16,
}

impl RetentionPolicy {
    /// Creates a new retention policy with the specified years.
    pub fn new(retention_years: u16) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let retention_ns = (retention_years as u64) * 365 * 24 * 60 * 60 * 1_000_000_000;
        Self {
            retention_years,
            retain_until_ns: now + retention_ns,
            grace_period_days: 0,
        }
    }

    /// Checks if the retention policy is active.
    pub fn is_active(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        now < self.retain_until_ns
    }

    /// Returns the remaining retention time in seconds.
    pub fn remaining_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        self.retain_until_ns
            .saturating_sub(now)
            .saturating_div(1_000_000_000)
    }
}

/// WORM state for a file.
#[derive(Debug, Clone)]
pub struct WormState {
    /// Current immutability level.
    pub immutability: ImmutabilityLevel,
    /// Active legal holds on this file.
    pub legal_holds: Vec<LegalHold>,
    /// Retention policy (if any).
    pub retention_policy: Option<RetentionPolicy>,
    /// Last modification timestamp (ns).
    pub last_modified_ns: u64,
}

impl Default for WormState {
    fn default() -> Self {
        Self {
            immutability: ImmutabilityLevel::None,
            legal_holds: Vec::new(),
            retention_policy: None,
            last_modified_ns: 0,
        }
    }
}

impl WormState {
    /// Creates a new WORM state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks if any operation is blocked.
    pub fn is_any_blocked(&self) -> bool {
        self.immutability.blocks_writes()
            || !self.legal_holds.is_empty()
            || self
                .retention_policy
                .as_ref()
                .map(|p| p.is_active())
                .unwrap_or(false)
    }
}

/// Snapshot of WORM state for audit/logging purposes.
#[derive(Debug, Clone)]
pub struct WormStateSnapshot {
    /// Inode number.
    pub inode: u64,
    /// Current immutability level.
    pub immutability_level: ImmutabilityLevel,
    /// Number of active legal holds.
    pub legal_hold_count: usize,
    /// Whether retention policy is active.
    pub retention_policy_active: bool,
    /// Last modification timestamp.
    pub last_modified_ns: u64,
}

/// WORM enforcement engine for FUSE operations.
pub struct WormEnforcer {
    /// Map of inode to WORM state.
    immutable_files: Arc<DashMap<u64, WormState>>,
}

impl WormEnforcer {
    /// Creates a new WORM enforcer.
    pub fn new() -> Self {
        Self {
            immutable_files: Arc::new(DashMap::new()),
        }
    }

    /// Gets the current timestamp in nanoseconds.
    fn now_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Checks if a file is currently immutable.
    pub fn is_immutable(&self, inode: u64) -> bool {
        self.immutable_files
            .get(&inode)
            .map(|state| state.is_any_blocked())
            .unwrap_or(false)
    }

    /// Sets temporary immutability on a file.
    pub fn set_temporary_immutable(&self, inode: u64, duration_ns: u64) -> Result<(), String> {
        let now = Self::now_ns();
        let until_ns = now.saturating_add(duration_ns);

        let mut state = self
            .immutable_files
            .entry(inode)
            .or_insert_with(WormState::new);
        state.immutability = ImmutabilityLevel::Temporary { until_ns };
        state.last_modified_ns = now;

        Ok(())
    }

    /// Sets permanent immutability on a file.
    pub fn set_permanent_immutable(&self, inode: u64) -> Result<(), String> {
        let now = Self::now_ns();

        let mut state = self
            .immutable_files
            .entry(inode)
            .or_insert_with(WormState::new);
        state.immutability = ImmutabilityLevel::Permanent;
        state.last_modified_ns = now;

        Ok(())
    }

    /// Adds a legal hold to a file.
    pub fn add_legal_hold(&self, inode: u64, hold: LegalHold) -> Result<(), String> {
        let now = Self::now_ns();

        let mut state = self
            .immutable_files
            .entry(inode)
            .or_insert_with(WormState::new);
        state.legal_holds.push(hold);
        state.last_modified_ns = now;

        Ok(())
    }

    /// Removes a legal hold from a file.
    /// The user_id must match the initiator for security.
    pub fn remove_legal_hold(
        &self,
        inode: u64,
        hold_type: LegalHoldType,
        user_id: &str,
    ) -> Result<(), String> {
        let state = self
            .immutable_files
            .get(&inode)
            .ok_or_else(|| format!("no WORM state for inode {}", inode))?;

        let _hold = state
            .legal_holds
            .iter()
            .find(|h| h.hold_type == hold_type && h.initiated_by == user_id)
            .ok_or_else(|| format!("no matching legal hold found for user {}", user_id))?;

        drop(state);

        let mut state = self
            .immutable_files
            .get_mut(&inode)
            .ok_or_else(|| format!("no WORM state for inode {}", inode))?;

        state
            .legal_holds
            .retain(|h| !(h.hold_type == hold_type && h.initiated_by == user_id));
        state.last_modified_ns = Self::now_ns();

        Ok(())
    }

    /// Sets a retention policy on a file.
    pub fn set_retention(&self, inode: u64, policy: RetentionPolicy) -> Result<(), String> {
        let now = Self::now_ns();

        let mut state = self
            .immutable_files
            .entry(inode)
            .or_insert_with(WormState::new);
        state.retention_policy = Some(policy);
        state.last_modified_ns = now;

        Ok(())
    }

    /// Enforces write operation - returns error if blocked.
    pub fn enforce_write(&self, inode: u64, operation: &str) -> Result<(), String> {
        let state = self
            .immutable_files
            .get(&inode)
            .ok_or_else(|| format!("inode {} not found", inode))?;

        if state.immutability.blocks_writes() {
            let reason = match state.immutability {
                ImmutabilityLevel::Permanent => "file is permanently immutable".to_string(),
                ImmutabilityLevel::Temporary { until_ns } => {
                    format!("file is temporarily immutable until {}", until_ns)
                }
                ImmutabilityLevel::None => "write blocked for unknown reason".to_string(),
            };
            return Err(format!(
                "write operation '{}' denied: {}",
                operation, reason
            ));
        }

        if !state.legal_holds.is_empty() {
            return Err(format!(
                "write operation '{}' denied: file has {} active legal hold(s)",
                operation,
                state.legal_holds.len()
            ));
        }

        if let Some(ref policy) = state.retention_policy {
            if policy.is_active() {
                return Err(format!(
                    "write operation '{}' denied: file under retention until {}",
                    operation, policy.retain_until_ns
                ));
            }
        }

        Ok(())
    }

    /// Enforces delete operation - returns error if blocked.
    pub fn enforce_delete(&self, inode: u64) -> Result<(), String> {
        let state = self
            .immutable_files
            .get(&inode)
            .ok_or_else(|| format!("inode {} not found", inode))?;

        if state.immutability.blocks_deletes() {
            return Err("delete operation denied: file is immutable".to_string());
        }

        if !state.legal_holds.is_empty() {
            return Err(format!(
                "delete operation denied: file has {} active legal hold(s)",
                state.legal_holds.len()
            ));
        }

        if let Some(ref policy) = state.retention_policy {
            if policy.is_active() {
                return Err("delete operation denied: file under retention".to_string());
            }
        }

        Ok(())
    }

    /// Gets the WORM state snapshot for audit logging.
    pub fn get_worm_state(&self, inode: u64) -> Option<WormStateSnapshot> {
        let state = self.immutable_files.get(&inode)?;

        Some(WormStateSnapshot {
            inode,
            immutability_level: state.immutability,
            legal_hold_count: state.legal_holds.len(),
            retention_policy_active: state
                .retention_policy
                .as_ref()
                .map(|p| p.is_active())
                .unwrap_or(false),
            last_modified_ns: state.last_modified_ns,
        })
    }

    /// Removes WORM state for an inode.
    pub fn clear(&self, inode: u64) {
        self.immutable_files.remove(&inode);
    }
}

impl Default for WormEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_immutable_false_by_default() {
        let enforcer = WormEnforcer::new();
        assert!(!enforcer.is_immutable(1));
    }

    #[test]
    fn test_set_temporary_immutable_succeeds() {
        let enforcer = WormEnforcer::new();
        let result = enforcer.set_temporary_immutable(1, 1_000_000_000);
        assert!(result.is_ok());
        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_set_permanent_immutable_succeeds() {
        let enforcer = WormEnforcer::new();
        let result = enforcer.set_permanent_immutable(1);
        assert!(result.is_ok());
        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_temporary_immutability_expires() {
        let enforcer = WormEnforcer::new();
        enforcer.set_temporary_immutable(1, 0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(!enforcer.is_immutable(1));
    }

    #[test]
    fn test_permanent_immutable_never_expires() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_enforce_write_denied_on_immutable() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();
        let result = enforcer.enforce_write(1, "write");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_enforce_write_denied_on_temporary() {
        let enforcer = WormEnforcer::new();
        enforcer.set_temporary_immutable(1, 1_000_000_000).unwrap();
        let result = enforcer.enforce_write(1, "write");
        assert!(result.is_err());
    }

    #[test]
    fn test_enforce_delete_denied_on_immutable() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();
        let result = enforcer.enforce_delete(1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_add_legal_hold_succeeds() {
        let enforcer = WormEnforcer::new();
        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        let result = enforcer.add_legal_hold(1, hold);
        assert!(result.is_ok());
        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_remove_legal_hold_succeeds_with_auth() {
        let enforcer = WormEnforcer::new();
        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();
        let result = enforcer.remove_legal_hold(1, LegalHoldType::Compliance, "user1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_legal_hold_fails_without_auth() {
        let enforcer = WormEnforcer::new();
        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();
        let result = enforcer.remove_legal_hold(1, LegalHoldType::Compliance, "user2");
        assert!(result.is_err());
    }

    #[test]
    fn test_legal_hold_prevents_deletion() {
        let enforcer = WormEnforcer::new();
        let hold = LegalHold {
            hold_type: LegalHoldType::Litigation,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "lawsuit".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();
        let result = enforcer.enforce_delete(1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("legal hold"));
    }

    #[test]
    fn test_set_retention_policy_succeeds() {
        let enforcer = WormEnforcer::new();
        let policy = RetentionPolicy::new(1);
        let result = enforcer.set_retention(1, policy);
        assert!(result.is_ok());
        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_retention_expiry_calculated_correctly() {
        let policy = RetentionPolicy::new(1);
        assert!(policy.is_active());
        assert!(policy.remaining_secs() > 0);
    }

    #[test]
    fn test_multiple_legal_holds_independent() {
        let enforcer = WormEnforcer::new();

        let hold1 = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        let hold2 = LegalHold {
            hold_type: LegalHoldType::Litigation,
            initiated_by: "user2".to_string(),
            created_ns: 2000,
            reason: "lawsuit".to_string(),
        };

        enforcer.add_legal_hold(1, hold1).unwrap();
        enforcer.add_legal_hold(1, hold2).unwrap();

        let snapshot = enforcer.get_worm_state(1).unwrap();
        assert_eq!(snapshot.legal_hold_count, 2);
    }

    #[test]
    fn test_temporary_immutability_vs_legal_hold_union() {
        let enforcer = WormEnforcer::new();
        enforcer.set_temporary_immutable(1, 1_000_000_000).unwrap();

        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();

        assert!(enforcer.is_immutable(1));
    }

    #[test]
    fn test_worm_state_snapshot_accurate() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();

        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 1000,
            reason: "regulatory".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();

        let snapshot = enforcer.get_worm_state(1).unwrap();
        assert_eq!(snapshot.inode, 1);
        assert!(matches!(
            snapshot.immutability_level,
            ImmutabilityLevel::Permanent
        ));
        assert_eq!(snapshot.legal_hold_count, 1);
    }

    #[test]
    fn test_enforce_write_error_message_detailed() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();
        let result = enforcer.enforce_write(1, "pwrite");
        let err = result.unwrap_err();
        assert!(err.contains("pwrite"));
        assert!(err.contains("immutable"));
    }

    #[test]
    fn test_enforce_delete_error_message_includes_reason() {
        let enforcer = WormEnforcer::new();
        enforcer.set_permanent_immutable(1).unwrap();
        let result = enforcer.enforce_delete(1);
        let err = result.unwrap_err();
        assert!(err.contains("delete"));
        assert!(err.contains("immutable"));
    }

    #[test]
    fn test_legal_hold_create_timestamp_recorded() {
        let enforcer = WormEnforcer::new();
        let before = WormEnforcer::now_ns();

        let hold = LegalHold {
            hold_type: LegalHoldType::Compliance,
            initiated_by: "user1".to_string(),
            created_ns: 0,
            reason: "test".to_string(),
        };
        enforcer.add_legal_hold(1, hold).unwrap();

        let after = WormEnforcer::now_ns();
        let state = enforcer.get_worm_state(1).unwrap();
        assert!(state.last_modified_ns >= before);
        assert!(state.last_modified_ns <= after);
    }

    #[test]
    fn test_worm_state_thread_safe_concurrent_ops() {
        use std::sync::Arc;
        use std::thread;

        let enforcer = Arc::new(WormEnforcer::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let enforcer = Arc::clone(&enforcer);
            let handle = thread::spawn(move || {
                let hold = LegalHold {
                    hold_type: LegalHoldType::Compliance,
                    initiated_by: format!("user{}", i),
                    created_ns: 1000,
                    reason: "test".to_string(),
                };
                let _ = enforcer.add_legal_hold(i + 1, hold);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(enforcer.immutable_files.len(), 10);
    }

    #[test]
    fn test_immutability_level_blocks_writes() {
        assert!(ImmutabilityLevel::Permanent.blocks_writes());
        assert!(!ImmutabilityLevel::None.blocks_writes());
    }

    #[test]
    fn test_retention_policy_with_grace_period() {
        let mut policy = RetentionPolicy::new(1);
        policy.grace_period_days = 30;
        assert_eq!(policy.grace_period_days, 30);
    }

    #[test]
    fn test_legal_hold_type_display_name() {
        assert_eq!(LegalHoldType::Compliance.display_name(), "Compliance");
        assert_eq!(LegalHoldType::Litigation.display_name(), "Litigation");
    }
}
