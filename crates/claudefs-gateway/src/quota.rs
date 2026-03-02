//! Per-user/group/tenant quota tracking

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Quota subject - identifies what the quota applies to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuotaSubject {
    /// Per-user quota (by UID)
    User(u32),
    /// Per-group quota (by GID)
    Group(u32),
    /// Per-export-path quota
    Export,
}

/// Quota limits for a subject
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuotaLimits {
    /// Hard limit on bytes (0 = unlimited)
    pub bytes_hard: u64,
    /// Soft limit on bytes (0 = unlimited)
    pub bytes_soft: u64,
    /// Hard limit on inodes/files (0 = unlimited)
    pub inodes_hard: u64,
    /// Soft limit on inodes (0 = unlimited)
    pub inodes_soft: u64,
}

impl QuotaLimits {
    /// Create unlimited quotas
    pub fn unlimited() -> Self {
        Self {
            bytes_hard: 0,
            bytes_soft: 0,
            inodes_hard: 0,
            inodes_soft: 0,
        }
    }

    /// Create limits with hard limits only
    pub fn new(bytes_hard: u64, inodes_hard: u64) -> Self {
        Self {
            bytes_hard,
            bytes_soft: 0,
            inodes_hard,
            inodes_soft: 0,
        }
    }

    /// Create limits with both hard and soft limits
    pub fn with_soft(bytes_hard: u64, bytes_soft: u64, inodes_hard: u64, inodes_soft: u64) -> Self {
        Self {
            bytes_hard,
            bytes_soft,
            inodes_hard,
            inodes_soft,
        }
    }
}

/// Current quota usage for a subject
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    pub bytes_used: u64,
    pub inodes_used: u64,
}

impl QuotaUsage {
    pub fn new(bytes_used: u64, inodes_used: u64) -> Self {
        Self {
            bytes_used,
            inodes_used,
        }
    }

    pub fn add_bytes(&self, bytes: u64) -> Self {
        Self {
            bytes_used: self.bytes_used.saturating_add(bytes),
            inodes_used: self.inodes_used,
        }
    }

    pub fn sub_bytes(&self, bytes: u64) -> Self {
        Self {
            bytes_used: self.bytes_used.saturating_sub(bytes),
            inodes_used: self.inodes_used,
        }
    }

    pub fn add_inodes(&self, n: u64) -> Self {
        Self {
            bytes_used: self.bytes_used,
            inodes_used: self.inodes_used.saturating_add(n),
        }
    }

    pub fn sub_inodes(&self, n: u64) -> Self {
        Self {
            bytes_used: self.bytes_used,
            inodes_used: self.inodes_used.saturating_sub(n),
        }
    }
}

/// Quota violation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaViolation {
    /// Hard limit exceeded - operation denied
    HardLimitExceeded,
    /// Soft limit exceeded - warn but allow
    SoftLimitExceeded,
    /// Within limits - no violation
    None,
}

/// Quota manager - tracks usage and enforces limits
pub struct QuotaManager {
    limits: Mutex<HashMap<QuotaSubject, QuotaLimits>>,
    usage: Mutex<HashMap<QuotaSubject, QuotaUsage>>,
}

impl QuotaManager {
    pub fn new() -> Self {
        Self {
            limits: Mutex::new(HashMap::new()),
            usage: Mutex::new(HashMap::new()),
        }
    }

    /// Set quota limits for a subject
    pub fn set_limits(&self, subject: QuotaSubject, limits: QuotaLimits) {
        self.limits.lock().unwrap().insert(subject, limits);
    }

    /// Get quota limits for a subject (None if no limits set = unlimited)
    pub fn get_limits(&self, subject: QuotaSubject) -> Option<QuotaLimits> {
        self.limits.lock().unwrap().get(&subject).copied()
    }

    /// Get current usage for a subject
    pub fn get_usage(&self, subject: QuotaSubject) -> QuotaUsage {
        self.usage
            .lock()
            .unwrap()
            .get(&subject)
            .copied()
            .unwrap_or_default()
    }

    /// Record bytes written for a subject, check against limits
    pub fn record_write(&self, subject: QuotaSubject, bytes: u64) -> QuotaViolation {
        let violation = self.check_write(subject, bytes);
        if violation == QuotaViolation::None {
            let mut guard = self.usage.lock().unwrap();
            let new_val = guard
                .get(&subject)
                .map(|u| u.bytes_used.saturating_add(bytes))
                .unwrap_or(bytes);
            guard.entry(subject).or_default().bytes_used = new_val;
        }
        violation
    }

    /// Record bytes freed for a subject
    pub fn record_delete(&self, subject: QuotaSubject, bytes: u64) {
        let mut guard = self.usage.lock().unwrap();
        let new_val = guard
            .get(&subject)
            .map(|u| u.bytes_used.saturating_sub(bytes))
            .unwrap_or(0);
        guard.entry(subject).or_default().bytes_used = new_val;
    }

    /// Record inode creation
    pub fn record_create(&self, subject: QuotaSubject) -> QuotaViolation {
        let limits = self.get_limits(subject);
        let mut usage_guard = self.usage.lock().unwrap();
        let usage = usage_guard.entry(subject).or_default();

        if let Some(limits) = limits {
            if limits.inodes_hard > 0 && usage.inodes_used >= limits.inodes_hard {
                return QuotaViolation::HardLimitExceeded;
            }
            if limits.inodes_soft > 0 && usage.inodes_used >= limits.inodes_soft {
                return QuotaViolation::SoftLimitExceeded;
            }
        }

        usage.inodes_used = usage.inodes_used.saturating_add(1);
        QuotaViolation::None
    }

    /// Record inode deletion
    pub fn record_destroy(&self, subject: QuotaSubject) {
        let mut guard = self.usage.lock().unwrap();
        let new_val = guard
            .get(&subject)
            .map(|u| u.inodes_used.saturating_sub(1))
            .unwrap_or(0);
        guard.entry(subject).or_default().inodes_used = new_val;
    }

    /// Check if a write would exceed limits (without recording)
    pub fn check_write(&self, subject: QuotaSubject, bytes: u64) -> QuotaViolation {
        let limits = match self.get_limits(subject) {
            Some(l) => l,
            None => return QuotaViolation::None,
        };

        let current_usage = self.get_usage(subject);
        let new_bytes = current_usage.bytes_used.saturating_add(bytes);

        if limits.bytes_hard > 0 && new_bytes > limits.bytes_hard {
            return QuotaViolation::HardLimitExceeded;
        }
        if limits.bytes_soft > 0 && new_bytes > limits.bytes_soft {
            return QuotaViolation::SoftLimitExceeded;
        }

        QuotaViolation::None
    }

    /// Reset usage for a subject (e.g., after recalculation)
    pub fn reset_usage(&self, subject: QuotaSubject) {
        self.usage.lock().unwrap().remove(&subject);
    }

    /// List all subjects with configured limits
    pub fn subjects(&self) -> Vec<QuotaSubject> {
        self.limits.lock().unwrap().keys().cloned().collect()
    }

    /// Remove limits for a subject
    pub fn remove_limits(&self, subject: QuotaSubject) -> bool {
        self.limits.lock().unwrap().remove(&subject).is_some()
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
    fn test_quota_limits_unlimited() {
        let limits = QuotaLimits::unlimited();
        assert_eq!(limits.bytes_hard, 0);
        assert_eq!(limits.bytes_soft, 0);
        assert_eq!(limits.inodes_hard, 0);
        assert_eq!(limits.inodes_soft, 0);
    }

    #[test]
    fn test_quota_limits_new() {
        let limits = QuotaLimits::new(1000, 100);
        assert_eq!(limits.bytes_hard, 1000);
        assert_eq!(limits.bytes_soft, 0);
        assert_eq!(limits.inodes_hard, 100);
        assert_eq!(limits.inodes_soft, 0);
    }

    #[test]
    fn test_quota_limits_with_soft() {
        let limits = QuotaLimits::with_soft(1000, 800, 100, 80);
        assert_eq!(limits.bytes_hard, 1000);
        assert_eq!(limits.bytes_soft, 800);
        assert_eq!(limits.inodes_hard, 100);
        assert_eq!(limits.inodes_soft, 80);
    }

    #[test]
    fn test_quota_usage_new() {
        let usage = QuotaUsage::new(500, 10);
        assert_eq!(usage.bytes_used, 500);
        assert_eq!(usage.inodes_used, 10);
    }

    #[test]
    fn test_quota_usage_add_bytes() {
        let usage = QuotaUsage::new(500, 10);
        let new = usage.add_bytes(200);
        assert_eq!(new.bytes_used, 700);
        assert_eq!(new.inodes_used, 10);
    }

    #[test]
    fn test_quota_usage_sub_bytes() {
        let usage = QuotaUsage::new(500, 10);
        let new = usage.sub_bytes(200);
        assert_eq!(new.bytes_used, 300);
        assert_eq!(new.inodes_used, 10);
    }

    #[test]
    fn test_quota_usage_add_inodes() {
        let usage = QuotaUsage::new(500, 10);
        let new = usage.add_inodes(5);
        assert_eq!(new.bytes_used, 500);
        assert_eq!(new.inodes_used, 15);
    }

    #[test]
    fn test_quota_usage_sub_inodes() {
        let usage = QuotaUsage::new(500, 10);
        let new = usage.sub_inodes(3);
        assert_eq!(new.bytes_used, 500);
        assert_eq!(new.inodes_used, 7);
    }

    #[test]
    fn test_set_get_limits() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        let limits = QuotaLimits::new(1000000, 10000);

        manager.set_limits(subject, limits);
        assert_eq!(manager.get_limits(subject), Some(limits));
    }

    #[test]
    fn test_get_limits_none() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(9999);
        assert_eq!(manager.get_limits(subject), None);
    }

    #[test]
    fn test_record_write_below_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        let violation = manager.record_write(subject, 500);
        assert_eq!(violation, QuotaViolation::None);
        assert_eq!(manager.get_usage(subject).bytes_used, 500);
    }

    #[test]
    fn test_record_write_at_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        let violation = manager.record_write(subject, 1000);
        assert_eq!(violation, QuotaViolation::None);
        assert_eq!(manager.get_usage(subject).bytes_used, 1000);
    }

    #[test]
    fn test_record_write_above_soft_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::with_soft(1000, 500, 100, 50));

        let violation = manager.record_write(subject, 600);
        assert_eq!(violation, QuotaViolation::SoftLimitExceeded);
    }

    #[test]
    fn test_record_write_above_hard_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        let violation = manager.record_write(subject, 1001);
        assert_eq!(violation, QuotaViolation::HardLimitExceeded);
        assert_eq!(manager.get_usage(subject).bytes_used, 0);
    }

    #[test]
    fn test_record_delete() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        manager.record_write(subject, 500);
        manager.record_delete(subject, 200);
        assert_eq!(manager.get_usage(subject).bytes_used, 300);
    }

    #[test]
    fn test_record_create_below_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        let violation = manager.record_create(subject);
        assert_eq!(violation, QuotaViolation::None);
        assert_eq!(manager.get_usage(subject).inodes_used, 1);
    }

    #[test]
    fn test_record_create_at_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 10));

        for _ in 0..10 {
            manager.record_create(subject);
        }
        assert_eq!(manager.get_usage(subject).inodes_used, 10);

        let violation = manager.record_create(subject);
        assert_eq!(violation, QuotaViolation::HardLimitExceeded);
    }

    #[test]
    fn test_check_write() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(500, 100));

        assert_eq!(manager.check_write(subject, 500), QuotaViolation::None);
        assert_eq!(
            manager.check_write(subject, 501),
            QuotaViolation::HardLimitExceeded
        );
    }

    #[test]
    fn test_reset_usage() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        manager.record_write(subject, 500);
        assert_eq!(manager.get_usage(subject).bytes_used, 500);

        manager.reset_usage(subject);
        assert_eq!(manager.get_usage(subject).bytes_used, 0);
    }

    #[test]
    fn test_subjects() {
        let manager = QuotaManager::new();
        manager.set_limits(QuotaSubject::User(1), QuotaLimits::new(1000, 100));
        manager.set_limits(QuotaSubject::Group(1), QuotaLimits::new(2000, 200));
        manager.set_limits(QuotaSubject::Export, QuotaLimits::new(5000, 500));

        let subjects = manager.subjects();
        assert_eq!(subjects.len(), 3);
        assert!(subjects.contains(&QuotaSubject::User(1)));
        assert!(subjects.contains(&QuotaSubject::Group(1)));
        assert!(subjects.contains(&QuotaSubject::Export));
    }

    #[test]
    fn test_remove_limits() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        assert!(manager.remove_limits(subject));
        assert_eq!(manager.get_limits(subject), None);

        assert!(!manager.remove_limits(subject));
    }
}
