//! Per-user and per-group storage quota management.
//!
//! This module provides quota tracking and enforcement for multi-tenant
//! file systems, supporting both byte and inode limits.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// Quota target — identifies what entity the quota applies to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuotaTarget {
    /// User quota (identified by UID)
    User(u32),
    /// Group quota (identified by GID)
    Group(u32),
}

impl QuotaTarget {
    /// Returns true if this target is a user quota.
    pub fn is_user(&self) -> bool {
        matches!(self, QuotaTarget::User(_))
    }

    /// Returns true if this target is a group quota.
    pub fn is_group(&self) -> bool {
        matches!(self, QuotaTarget::Group(_))
    }
}

/// A storage quota limit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaLimit {
    /// Maximum number of bytes (-1 or u64::MAX for unlimited).
    pub max_bytes: u64,
    /// Maximum number of inodes (-1 or u64::MAX for unlimited).
    pub max_inodes: u64,
}

impl QuotaLimit {
    /// Creates a new quota limit with both bytes and inodes unlimited.
    pub fn unlimited() -> Self {
        Self {
            max_bytes: u64::MAX,
            max_inodes: u64::MAX,
        }
    }

    /// Creates a new quota limit with specified byte and inode limits.
    pub fn new(max_bytes: u64, max_inodes: u64) -> Self {
        Self {
            max_bytes,
            max_inodes,
        }
    }

    /// Returns true if the quota has a byte limit.
    pub fn has_byte_limit(&self) -> bool {
        self.max_bytes != u64::MAX
    }

    /// Returns true if the quota has an inode limit.
    pub fn has_inode_limit(&self) -> bool {
        self.max_inodes != u64::MAX
    }
}

/// Current usage for a quota target.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Current bytes used.
    pub bytes_used: u64,
    /// Current inodes used.
    pub inodes_used: u64,
}

impl QuotaUsage {
    /// Creates a new quota usage with zero values.
    pub fn new() -> Self {
        Self {
            bytes_used: 0,
            inodes_used: 0,
        }
    }

    /// Adds to the current usage.
    pub fn add(&mut self, bytes: i64, inodes: i64) {
        if bytes >= 0 {
            self.bytes_used = self.bytes_used.saturating_add(bytes as u64);
        } else {
            self.bytes_used = self.bytes_used.saturating_sub((-bytes) as u64);
        }
        if inodes >= 0 {
            self.inodes_used = self.inodes_used.saturating_add(inodes as u64);
        } else {
            self.inodes_used = self.inodes_used.saturating_sub((-inodes) as u64);
        }
    }
}

/// A quota entry combining limit and usage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaEntry {
    /// The quota target.
    pub target: QuotaTarget,
    /// The quota limit.
    pub limit: QuotaLimit,
    /// The current usage.
    pub usage: QuotaUsage,
}

impl QuotaEntry {
    /// Creates a new quota entry.
    pub fn new(target: QuotaTarget, limit: QuotaLimit) -> Self {
        Self {
            target,
            limit,
            usage: QuotaUsage::new(),
        }
    }

    /// Checks if the current usage exceeds the quota limit.
    pub fn is_over_quota(&self) -> bool {
        (self.limit.has_byte_limit() && self.usage.bytes_used > self.limit.max_bytes)
            || (self.limit.has_inode_limit() && self.usage.inodes_used > self.limit.max_inodes)
    }
}

/// Manages per-user and per-group storage quotas.
pub struct QuotaManager {
    /// Quota entries indexed by target.
    quotas: RwLock<HashMap<QuotaTarget, QuotaEntry>>,
}

impl QuotaManager {
    /// Creates a new quota manager with no quota entries.
    pub fn new() -> Self {
        Self {
            quotas: RwLock::new(HashMap::new()),
        }
    }

    /// Sets or updates the quota for a target.
    ///
    /// # Arguments
    /// * `target` - The quota target (user or group)
    /// * `limit` - The quota limit to apply
    pub fn set_quota(&self, target: QuotaTarget, limit: QuotaLimit) {
        let mut quotas = self.quotas.write().unwrap();
        let entry = quotas
            .entry(target.clone())
            .or_insert_with(|| QuotaEntry::new(target.clone(), limit.clone()));
        entry.limit = limit.clone();
        tracing::debug!("Set quota for {:?}: {:?}", target, limit);
    }

    /// Removes a quota for a target.
    ///
    /// # Arguments
    /// * `target` - The quota target to remove
    ///
    /// # Returns
    /// True if the quota existed and was removed
    pub fn remove_quota(&self, target: &QuotaTarget) -> bool {
        let mut quotas = self.quotas.write().unwrap();
        let removed = quotas.remove(target).is_some();
        if removed {
            tracing::debug!("Removed quota for {:?}", target);
        }
        removed
    }

    /// Gets the quota entry for a target.
    ///
    /// # Arguments
    /// * `target` - The quota target
    ///
    /// # Returns
    /// The quota entry if it exists
    pub fn get_quota(&self, target: &QuotaTarget) -> Option<QuotaEntry> {
        let quotas = self.quotas.read().unwrap();
        quotas.get(target).cloned()
    }

    /// Checks if adding the given deltas would exceed any quota.
    ///
    /// This checks both the user's quota and the group's quota.
    ///
    /// # Arguments
    /// * `uid` - User ID
    /// * `gid` - Group ID
    /// * `bytes_delta` - Change in bytes (can be negative for removal)
    /// * `inodes_delta` - Change in inode count (can be negative for removal)
    ///
    /// # Returns
    /// Ok(()) if within limits, Err(MetaError::NoSpace) if exceeded
    pub fn check_quota(
        &self,
        uid: u32,
        gid: u32,
        bytes_delta: i64,
        inodes_delta: i64,
    ) -> Result<(), MetaError> {
        let quotas = self.quotas.read().unwrap();

        let user_target = QuotaTarget::User(uid);
        if let Some(entry) = quotas.get(&user_target) {
            let would_be_bytes = entry
                .usage
                .bytes_used
                .saturating_add(bytes_delta.max(0) as u64);
            let would_be_inodes = entry
                .usage
                .inodes_used
                .saturating_add(inodes_delta.max(0) as u64);

            if entry.limit.has_byte_limit() && would_be_bytes > entry.limit.max_bytes {
                tracing::warn!(
                    "User {} would exceed byte quota: {} > {}",
                    uid,
                    would_be_bytes,
                    entry.limit.max_bytes
                );
                return Err(MetaError::NoSpace);
            }
            if entry.limit.has_inode_limit() && would_be_inodes > entry.limit.max_inodes {
                tracing::warn!(
                    "User {} would exceed inode quota: {} > {}",
                    uid,
                    would_be_inodes,
                    entry.limit.max_inodes
                );
                return Err(MetaError::NoSpace);
            }
        }

        let group_target = QuotaTarget::Group(gid);
        if let Some(entry) = quotas.get(&group_target) {
            let would_be_bytes = entry
                .usage
                .bytes_used
                .saturating_add(bytes_delta.max(0) as u64);
            let would_be_inodes = entry
                .usage
                .inodes_used
                .saturating_add(inodes_delta.max(0) as u64);

            if entry.limit.has_byte_limit() && would_be_bytes > entry.limit.max_bytes {
                tracing::warn!(
                    "Group {} would exceed byte quota: {} > {}",
                    gid,
                    would_be_bytes,
                    entry.limit.max_bytes
                );
                return Err(MetaError::NoSpace);
            }
            if entry.limit.has_inode_limit() && would_be_inodes > entry.limit.max_inodes {
                tracing::warn!(
                    "Group {} would exceed inode quota: {} > {}",
                    gid,
                    would_be_inodes,
                    entry.limit.max_inodes
                );
                return Err(MetaError::NoSpace);
            }
        }

        Ok(())
    }

    /// Updates usage counters after a successful operation.
    ///
    /// # Arguments
    /// * `uid` - User ID
    /// * `gid` - Group ID
    /// * `bytes_delta` - Change in bytes (can be negative for removal)
    /// * `inodes_delta` - Change in inode count (can be negative for removal)
    pub fn update_usage(&self, uid: u32, gid: u32, bytes_delta: i64, inodes_delta: i64) {
        let mut quotas = self.quotas.write().unwrap();

        let user_target = QuotaTarget::User(uid);
        if let Some(entry) = quotas.get_mut(&user_target) {
            entry.usage.add(bytes_delta, inodes_delta);
        }

        let group_target = QuotaTarget::Group(gid);
        if let Some(entry) = quotas.get_mut(&group_target) {
            entry.usage.add(bytes_delta, inodes_delta);
        }
    }

    /// Gets the current usage for a target.
    ///
    /// # Arguments
    /// * `target` - The quota target
    ///
    /// # Returns
    /// The current usage if quota exists
    pub fn get_usage(&self, target: &QuotaTarget) -> Option<QuotaUsage> {
        let quotas = self.quotas.read().unwrap();
        quotas.get(target).map(|e| e.usage.clone())
    }

    /// Lists all quota entries.
    ///
    /// # Returns
    /// Vector of all quota entries
    pub fn list_quotas(&self) -> Vec<QuotaEntry> {
        let quotas = self.quotas.read().unwrap();
        quotas.values().cloned().collect()
    }

    /// Returns targets that are over their quota.
    ///
    /// # Returns
    /// Vector of over-quota targets
    pub fn over_quota_targets(&self) -> Vec<QuotaTarget> {
        let quotas = self.quotas.read().unwrap();
        quotas
            .iter()
            .filter(|(_, entry)| entry.is_over_quota())
            .map(|(target, _)| target.clone())
            .collect()
    }
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_quota() {
        let mgr = QuotaManager::new();
        let target = QuotaTarget::User(1000);
        let limit = QuotaLimit::new(1_000_000, 1000);

        mgr.set_quota(target.clone(), limit.clone());

        let entry = mgr.get_quota(&target).unwrap();
        assert_eq!(entry.limit.max_bytes, 1_000_000);
        assert_eq!(entry.limit.max_inodes, 1000);
    }

    #[test]
    fn test_remove_quota() {
        let mgr = QuotaManager::new();
        let target = QuotaTarget::User(1000);

        mgr.set_quota(target.clone(), QuotaLimit::new(1_000_000, 1000));
        assert!(mgr.remove_quota(&target));
        assert!(mgr.get_quota(&target).is_none());
        assert!(!mgr.remove_quota(&target));
    }

    #[test]
    fn test_check_quota_within_limits() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        mgr.update_usage(1000, 0, 500, 5);

        let result = mgr.check_quota(1000, 0, 100, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_quota_exceeds_bytes() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1000, 100));
        mgr.update_usage(1000, 0, 900, 5);

        let result = mgr.check_quota(1000, 0, 200, 0);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::NoSpace)));
    }

    #[test]
    fn test_check_quota_exceeds_inodes() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 100));
        mgr.update_usage(1000, 0, 0, 95);

        let result = mgr.check_quota(1000, 0, 0, 10);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::NoSpace)));
    }

    #[test]
    fn test_update_usage() {
        let mgr = QuotaManager::new();
        let target = QuotaTarget::User(1000);

        mgr.set_quota(target.clone(), QuotaLimit::new(1_000_000, 1000));
        mgr.update_usage(1000, 0, 1000, 10);

        let usage = mgr.get_usage(&target).unwrap();
        assert_eq!(usage.bytes_used, 1000);
        assert_eq!(usage.inodes_used, 10);
    }

    #[test]
    fn test_check_quota_user_and_group() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 100));
        mgr.set_quota(QuotaTarget::Group(500), QuotaLimit::new(500_000, 50));

        let result = mgr.check_quota(1000, 500, 1000, 5);
        assert!(result.is_ok());

        mgr.update_usage(1000, 500, 900_000, 45);
        let result = mgr.check_quota(1000, 500, 200_000, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_quotas() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 100));
        mgr.set_quota(QuotaTarget::Group(500), QuotaLimit::new(2_000_000, 200));

        let quotas = mgr.list_quotas();
        assert_eq!(quotas.len(), 2);
    }

    #[test]
    fn test_over_quota_targets() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(1000, 100));
        mgr.set_quota(QuotaTarget::User(2000), QuotaLimit::new(2000, 200));

        mgr.update_usage(1000, 0, 500, 50);
        mgr.update_usage(2000, 0, 3000, 300);

        let over = mgr.over_quota_targets();
        assert!(over.contains(&QuotaTarget::User(2000)));
        assert!(!over.contains(&QuotaTarget::User(1000)));
    }

    #[test]
    fn test_unlimited_quota() {
        let mgr = QuotaManager::new();
        let limit = QuotaLimit::unlimited();

        assert!(!limit.has_byte_limit());
        assert!(!limit.has_inode_limit());

        mgr.set_quota(QuotaTarget::User(1000), limit);
        let result = mgr.check_quota(1000, 0, u64::MAX as i64, u64::MAX as i64);
        assert!(result.is_ok());
    }
}
