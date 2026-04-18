//! WORM retention policy enforcement with compliance audit logging.
//!
//! Enforces Write-Once-Read-Many policies at the chunk level, prevents
//! deletion under retention, tracks legal holds, and maintains immutable
//! audit trail for compliance.

use crate::error::ReduceError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Retention policy type for a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetentionType {
    /// No retention enforcement.
    None,
    /// Time-based retention until timestamp (seconds since UNIX_EPOCH).
    TimeBasedRetention,
    /// Legal hold (indefinite until explicitly released).
    LegalHold,
    /// Eventual deletion (marked but waiting for GC).
    EventualDelete,
}

/// Represents a legal hold on a chunk or file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComplianceHold {
    /// Unique hold identifier.
    pub hold_id: String,
    /// User who placed the hold.
    pub placed_by: String,
    /// Timestamp when hold was placed (seconds since UNIX_EPOCH).
    pub placed_at: u64,
    /// Reason for the hold.
    pub reason: String,
    /// Optional expiration time (None = indefinite).
    pub expires_at: Option<u64>,
}

impl ComplianceHold {
    /// Check if this hold is still active.
    pub fn is_active(&self, now: u64) -> bool {
        match self.expires_at {
            None => true,
            Some(expiry) => now < expiry,
        }
    }
}

/// Retention policy with expiration time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Policy type.
    pub policy_type: RetentionType,
    /// Expiration time (seconds since UNIX_EPOCH), None for indefinite.
    pub expires_at: Option<u64>,
    /// Associated legal holds (if any).
    pub holds: Vec<ComplianceHold>,
}

impl RetentionPolicy {
    /// Create a no-retention policy.
    pub fn none() -> Self {
        Self {
            policy_type: RetentionType::None,
            expires_at: None,
            holds: Vec::new(),
        }
    }

    /// Create a time-based retention policy.
    pub fn time_based(expires_at: u64) -> Self {
        Self {
            policy_type: RetentionType::TimeBasedRetention,
            expires_at: Some(expires_at),
            holds: Vec::new(),
        }
    }

    /// Create a legal hold policy.
    pub fn legal_hold() -> Self {
        Self {
            policy_type: RetentionType::LegalHold,
            expires_at: None,
            holds: Vec::new(),
        }
    }

    /// Check if policy has expired.
    pub fn is_expired(&self, now: u64) -> bool {
        match self.policy_type {
            RetentionType::None => true,
            RetentionType::LegalHold => false,
            RetentionType::TimeBasedRetention => self.expires_at.map(|e| now > e).unwrap_or(true),
            RetentionType::EventualDelete => false,
        }
    }

    /// Check if any hold is active.
    pub fn has_active_hold(&self, now: u64) -> bool {
        self.holds.iter().any(|h| h.is_active(now))
    }

    /// Add a legal hold.
    pub fn add_hold(&mut self, hold: ComplianceHold) {
        self.holds.push(hold);
    }

    /// Remove a legal hold by ID.
    pub fn remove_hold(&mut self, hold_id: &str) -> bool {
        if let Some(pos) = self.holds.iter().position(|h| h.hold_id == hold_id) {
            self.holds.remove(pos);
            true
        } else {
            false
        }
    }
}

/// Audit log entry for retention policy changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// Entry timestamp (seconds since UNIX_EPOCH).
    pub timestamp: u64,
    /// Type of audit event.
    pub event_type: String,
    /// Resource (chunk_id or inode_id).
    pub resource_id: String,
    /// User performing the action.
    pub user: String,
    /// Details about the change.
    pub details: String,
}

/// Enforces WORM retention policies and compliance holds.
pub struct WormRetentionEnforcer {
    /// Per-chunk retention policies.
    policies: HashMap<u64, RetentionPolicy>,
    /// Immutable audit trail.
    audit_log: Vec<AuditLogEntry>,
    /// Holds by resource ID.
    holds: HashMap<String, Vec<ComplianceHold>>,
}

impl WormRetentionEnforcer {
    /// Create a new WORM retention enforcer.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            audit_log: Vec::new(),
            holds: HashMap::new(),
        }
    }

    /// Set a retention policy for a chunk.
    pub fn set_policy(
        &mut self,
        chunk_id: u64,
        policy: RetentionPolicy,
        user: &str,
    ) -> Result<(), ReduceError> {
        self.policies.insert(chunk_id, policy.clone());

        // Log the policy change
        self.audit_log.push(AuditLogEntry {
            timestamp: Self::now(),
            event_type: "policy_set".to_string(),
            resource_id: format!("chunk_{}", chunk_id),
            user: user.to_string(),
            details: format!("{:?}", policy.policy_type),
        });

        Ok(())
    }

    /// Check if a chunk can be deleted.
    pub fn can_delete(&self, chunk_id: u64) -> bool {
        let now = Self::now();
        match self.policies.get(&chunk_id) {
            None => true,
            Some(policy) => {
                if policy.has_active_hold(now) {
                    false
                } else {
                    policy.is_expired(now)
                }
            }
        }
    }

    /// Place a legal hold on a resource.
    pub fn place_hold(
        &mut self,
        resource_id: &str,
        hold: ComplianceHold,
        user: &str,
    ) -> Result<(), ReduceError> {
        self.holds.entry(resource_id.to_string()).or_default().push(hold.clone());

        self.audit_log.push(AuditLogEntry {
            timestamp: Self::now(),
            event_type: "hold_placed".to_string(),
            resource_id: resource_id.to_string(),
            user: user.to_string(),
            details: format!("Hold ID: {}", hold.hold_id),
        });

        Ok(())
    }

    /// Release a legal hold.
    pub fn release_hold(
        &mut self,
        resource_id: &str,
        hold_id: &str,
        user: &str,
    ) -> Result<(), ReduceError> {
        if let Some(holds) = self.holds.get_mut(resource_id) {
            if let Some(pos) = holds.iter().position(|h| h.hold_id == hold_id) {
                holds.remove(pos);

                self.audit_log.push(AuditLogEntry {
                    timestamp: Self::now(),
                    event_type: "hold_released".to_string(),
                    resource_id: resource_id.to_string(),
                    user: user.to_string(),
                    details: format!("Hold ID: {}", hold_id),
                });

                return Ok(());
            }
        }

        Err(ReduceError::NotFound(format!("Hold not found: {}", hold_id)))
    }

    /// Get the audit log (immutable reference).
    pub fn audit_log(&self) -> &[AuditLogEntry] {
        &self.audit_log
    }

    /// Check if modification is allowed for a chunk.
    pub fn can_modify(&self, chunk_id: u64) -> bool {
        let now = Self::now();
        match self.policies.get(&chunk_id) {
            None => true,
            Some(policy) => {
                // Cannot modify if under retention or hold
                !policy.has_active_hold(now)
                    && (policy.is_expired(now) || policy.policy_type == RetentionType::None)
            }
        }
    }

    /// Get the number of chunks under retention.
    pub fn chunks_under_retention(&self) -> usize {
        let now = Self::now();
        self.policies
            .values()
            .filter(|p| !p.is_expired(now) || p.has_active_hold(now))
            .count()
    }

    /// Get all active holds for a resource.
    pub fn get_holds(&self, resource_id: &str) -> Option<&[ComplianceHold]> {
        self.holds.get(resource_id).map(|h| h.as_slice())
    }

    /// Clear expired holds and policies (maintenance).
    pub fn cleanup_expired(&mut self) -> Result<usize, ReduceError> {
        let now = Self::now();
        let mut removed = 0;

        // Remove expired policies without active holds
        self.policies.retain(|_, p| {
            if p.is_expired(now) && !p.has_active_hold(now) {
                removed += 1;
                false
            } else {
                true
            }
        });

        // Remove expired holds
        for holds in self.holds.values_mut() {
            let before_len = holds.len();
            holds.retain(|h| h.is_active(now));
            removed += before_len - holds.len();
        }

        Ok(removed)
    }

    /// Get current system time as UNIX timestamp.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Get the retention policy for a chunk.
    pub fn get_policy(&self, chunk_id: u64) -> Option<&RetentionPolicy> {
        self.policies.get(&chunk_id)
    }

    /// Count total audit log entries.
    pub fn audit_log_len(&self) -> usize {
        self.audit_log.len()
    }
}

impl Default for WormRetentionEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforcer_creation() {
        let enforcer = WormRetentionEnforcer::new();
        assert_eq!(enforcer.policies.len(), 0);
        assert_eq!(enforcer.audit_log.len(), 0);
    }

    #[test]
    fn test_set_no_retention_policy() {
        let mut enforcer = WormRetentionEnforcer::new();
        let policy = RetentionPolicy::none();

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(enforcer.can_delete(1));
    }

    #[test]
    fn test_time_based_retention_active() {
        let mut enforcer = WormRetentionEnforcer::new();
        let future = WormRetentionEnforcer::now() + 86400;
        let policy = RetentionPolicy::time_based(future);

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(!enforcer.can_delete(1));
    }

    #[test]
    fn test_time_based_retention_expired() {
        let mut enforcer = WormRetentionEnforcer::new();
        let past = WormRetentionEnforcer::now() - 1;
        let policy = RetentionPolicy::time_based(past);

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(enforcer.can_delete(1));
    }

    #[test]
    fn test_legal_hold_prevents_deletion() {
        let mut enforcer = WormRetentionEnforcer::new();
        let mut policy = RetentionPolicy::legal_hold();
        let hold = ComplianceHold {
            hold_id: "hold_1".to_string(),
            placed_by: "user1".to_string(),
            placed_at: WormRetentionEnforcer::now(),
            reason: "Legal investigation".to_string(),
            expires_at: None,
        };

        policy.add_hold(hold);
        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(!enforcer.can_delete(1));
    }

    #[test]
    fn test_place_and_release_hold() {
        let mut enforcer = WormRetentionEnforcer::new();
        let hold = ComplianceHold {
            hold_id: "hold_1".to_string(),
            placed_by: "user1".to_string(),
            placed_at: WormRetentionEnforcer::now(),
            reason: "Compliance".to_string(),
            expires_at: None,
        };

        assert!(enforcer.place_hold("resource_1", hold, "user1").is_ok());
        assert!(enforcer.get_holds("resource_1").is_some());

        assert!(enforcer.release_hold("resource_1", "hold_1", "user1").is_ok());
        assert!(enforcer.get_holds("resource_1").is_none() || enforcer.get_holds("resource_1").unwrap().is_empty());
    }

    #[test]
    fn test_cannot_modify_under_retention() {
        let mut enforcer = WormRetentionEnforcer::new();
        let future = WormRetentionEnforcer::now() + 86400;
        let policy = RetentionPolicy::time_based(future);

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(!enforcer.can_modify(1));
    }

    #[test]
    fn test_can_modify_after_expiration() {
        let mut enforcer = WormRetentionEnforcer::new();
        let past = WormRetentionEnforcer::now() - 1;
        let policy = RetentionPolicy::time_based(past);

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(enforcer.can_modify(1));
    }

    #[test]
    fn test_audit_log_recorded() {
        let mut enforcer = WormRetentionEnforcer::new();
        let policy = RetentionPolicy::none();

        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert_eq!(enforcer.audit_log_len(), 1);

        let entry = &enforcer.audit_log()[0];
        assert_eq!(entry.event_type, "policy_set");
        assert_eq!(entry.user, "user1");
    }

    #[test]
    fn test_chunks_under_retention() {
        let mut enforcer = WormRetentionEnforcer::new();
        let future = WormRetentionEnforcer::now() + 86400;

        for i in 0..3 {
            let policy = RetentionPolicy::time_based(future);
            assert!(enforcer.set_policy(i, policy, "user1").is_ok());
        }

        assert_eq!(enforcer.chunks_under_retention(), 3);
    }

    #[test]
    fn test_cleanup_expired() {
        let mut enforcer = WormRetentionEnforcer::new();
        let past = WormRetentionEnforcer::now() - 1;

        for i in 0..3 {
            let policy = RetentionPolicy::time_based(past);
            assert!(enforcer.set_policy(i, policy, "user1").is_ok());
        }

        assert!(enforcer.cleanup_expired().is_ok());
        assert_eq!(enforcer.policies.len(), 0);
    }

    #[test]
    fn test_compliance_hold_expiration() {
        let now = WormRetentionEnforcer::now();
        let hold1 = ComplianceHold {
            hold_id: "hold_1".to_string(),
            placed_by: "user1".to_string(),
            placed_at: now,
            reason: "Test".to_string(),
            expires_at: Some(now + 100),
        };

        assert!(hold1.is_active(now));
        assert!(!hold1.is_active(now + 200));
    }

    #[test]
    fn test_multiple_holds_on_resource() {
        let mut enforcer = WormRetentionEnforcer::new();
        let hold1 = ComplianceHold {
            hold_id: "hold_1".to_string(),
            placed_by: "user1".to_string(),
            placed_at: WormRetentionEnforcer::now(),
            reason: "Reason 1".to_string(),
            expires_at: None,
        };

        let hold2 = ComplianceHold {
            hold_id: "hold_2".to_string(),
            placed_by: "user2".to_string(),
            placed_at: WormRetentionEnforcer::now(),
            reason: "Reason 2".to_string(),
            expires_at: None,
        };

        assert!(enforcer.place_hold("resource_1", hold1, "user1").is_ok());
        assert!(enforcer.place_hold("resource_1", hold2, "user2").is_ok());

        assert_eq!(enforcer.get_holds("resource_1").map(|h| h.len()), Some(2));
    }

    #[test]
    fn test_cannot_delete_with_active_hold() {
        let mut enforcer = WormRetentionEnforcer::new();
        let mut policy = RetentionPolicy::none();
        let hold = ComplianceHold {
            hold_id: "hold_1".to_string(),
            placed_by: "user1".to_string(),
            placed_at: WormRetentionEnforcer::now(),
            reason: "Hold".to_string(),
            expires_at: None,
        };

        policy.add_hold(hold);
        assert!(enforcer.set_policy(1, policy, "user1").is_ok());
        assert!(!enforcer.can_delete(1));
    }

    #[test]
    fn test_release_nonexistent_hold() {
        let mut enforcer = WormRetentionEnforcer::new();
        assert!(enforcer.release_hold("resource_1", "nonexistent", "user1").is_err());
    }

    #[test]
    fn test_default_enforcer() {
        let enforcer = WormRetentionEnforcer::default();
        assert_eq!(enforcer.policies.len(), 0);
    }
}
