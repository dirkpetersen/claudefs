//! Per-tenant storage quota enforcement.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{StorageError, StorageResult};

/// Quota limits for a tenant - defines thresholds for storage and inode limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaLimit {
    /// Hard limit in bytes - writes fail above this threshold.
    pub bytes_hard: u64,
    /// Soft limit in bytes - warnings issued above this threshold.
    pub bytes_soft: u64,
    /// Hard limit for number of inodes.
    pub inodes_hard: u64,
    /// Soft limit for number of inodes.
    pub inodes_soft: u64,
    /// Time allowed above soft limit before enforcement (default 7 days = 604800 seconds).
    pub grace_period_secs: u64,
}

impl Default for QuotaLimit {
    fn default() -> Self {
        Self {
            bytes_hard: u64::MAX,
            bytes_soft: u64::MAX,
            inodes_hard: u64::MAX,
            inodes_soft: u64::MAX,
            grace_period_secs: 604800,
        }
    }
}

/// Current usage statistics for a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Total bytes currently allocated to this tenant.
    pub bytes_used: u64,
    /// Number of inodes currently in use by this tenant.
    pub inodes_used: u64,
    /// Timestamp (in seconds) when soft limit was first exceeded, if applicable.
    pub soft_exceeded_since: Option<u64>,
}

/// Status of a tenant's quota after checking against current usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaStatus {
    /// Tenant is within all limits.
    Ok,
    /// Tenant exceeds soft limit but is within grace period.
    SoftExceeded {
        /// Remaining grace period in seconds.
        grace_remaining_secs: u64,
    },
    /// Grace period has expired, writes should be rejected.
    GraceExpired,
    /// Tenant has exceeded hard limit.
    HardExceeded,
}

/// Complete quota state for a single tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    /// Unique identifier for this tenant.
    pub tenant_id: String,
    /// Quota limits for this tenant.
    pub limit: QuotaLimit,
    /// Current usage statistics.
    pub usage: QuotaUsage,
}

impl TenantQuota {
    /// Creates a new TenantQuota with the given tenant ID and limit.
    pub fn new(tenant_id: String, limit: QuotaLimit) -> Self {
        Self {
            tenant_id,
            limit,
            usage: QuotaUsage::default(),
        }
    }

    /// Checks the current quota status for this tenant.
    pub fn check_status(&self, now_secs: u64) -> QuotaStatus {
        let bytes_exceeded = self.usage.bytes_used > self.limit.bytes_hard;
        if bytes_exceeded {
            return QuotaStatus::HardExceeded;
        }

        let inodes_exceeded = self.usage.inodes_used > self.limit.inodes_hard;
        if inodes_exceeded {
            return QuotaStatus::HardExceeded;
        }

        let bytes_soft_exceeded = self.usage.bytes_used > self.limit.bytes_soft;
        let inodes_soft_exceeded = self.usage.inodes_used > self.limit.inodes_soft;
        let soft_exceeded = bytes_soft_exceeded || inodes_soft_exceeded;

        if soft_exceeded {
            if let Some(exceeded_since) = self.usage.soft_exceeded_since {
                let elapsed = now_secs.saturating_sub(exceeded_since);
                if elapsed >= self.limit.grace_period_secs {
                    return QuotaStatus::GraceExpired;
                }
                let remaining = self.limit.grace_period_secs.saturating_sub(elapsed);
                return QuotaStatus::SoftExceeded {
                    grace_remaining_secs: remaining,
                };
            } else {
                return QuotaStatus::SoftExceeded {
                    grace_remaining_secs: self.limit.grace_period_secs,
                };
            }
        }

        QuotaStatus::Ok
    }

    /// Checks if the given number of bytes can be allocated.
    pub fn can_allocate(&self, bytes: u64, now_secs: u64) -> bool {
        let status = self.check_status(now_secs);
        match status {
            QuotaStatus::Ok | QuotaStatus::SoftExceeded { .. } => {
                let new_bytes = self.usage.bytes_used.saturating_add(bytes);
                new_bytes <= self.limit.bytes_hard
            }
            QuotaStatus::GraceExpired | QuotaStatus::HardExceeded => false,
        }
    }

    /// Checks if a new inode can be created.
    pub fn can_create_inode(&self, now_secs: u64) -> bool {
        let status = self.check_status(now_secs);
        match status {
            QuotaStatus::Ok | QuotaStatus::SoftExceeded { .. } => {
                self.usage.inodes_used < self.limit.inodes_hard
            }
            QuotaStatus::GraceExpired | QuotaStatus::HardExceeded => false,
        }
    }

    /// Records bytes allocated to this tenant.
    pub fn record_allocation(&mut self, bytes: u64, now_secs: u64) {
        let was_under_soft = self.usage.bytes_used <= self.limit.bytes_soft
            && self.usage.inodes_used <= self.limit.inodes_soft;

        self.usage.bytes_used = self.usage.bytes_used.saturating_add(bytes);

        let is_over_soft = self.usage.bytes_used > self.limit.bytes_soft
            || self.usage.inodes_used > self.limit.inodes_soft;

        if was_under_soft && is_over_soft {
            self.usage.soft_exceeded_since = Some(now_secs);
            tracing::warn!(
                tenant_id = %self.tenant_id,
                bytes_used = self.usage.bytes_used,
                bytes_soft = self.limit.bytes_soft,
                "Tenant exceeded soft quota limit"
            );
        }
    }

    /// Records bytes freed by this tenant.
    pub fn record_free(&mut self, bytes: u64) {
        self.usage.bytes_used = self.usage.bytes_used.saturating_sub(bytes);

        let now_under_soft = self.usage.bytes_used <= self.limit.bytes_soft
            && self.usage.inodes_used <= self.limit.inodes_soft;

        if now_under_soft {
            self.usage.soft_exceeded_since = None;
        }
    }

    /// Records an inode created by this tenant.
    pub fn record_inode_create(&mut self, now_secs: u64) {
        let was_under_soft = self.usage.bytes_used <= self.limit.bytes_soft
            && self.usage.inodes_used <= self.limit.inodes_soft;

        self.usage.inodes_used = self.usage.inodes_used.saturating_add(1);

        let is_over_soft = self.usage.bytes_used > self.limit.bytes_soft
            || self.usage.inodes_used > self.limit.inodes_soft;

        if was_under_soft && is_over_soft {
            self.usage.soft_exceeded_since = Some(now_secs);
            tracing::warn!(
                tenant_id = %self.tenant_id,
                inodes_used = self.usage.inodes_used,
                inodes_soft = self.limit.inodes_soft,
                "Tenant exceeded soft inode limit"
            );
        }
    }

    /// Records an inode deleted by this tenant.
    pub fn record_inode_delete(&mut self) {
        self.usage.inodes_used = self.usage.inodes_used.saturating_sub(1);

        let now_under_soft = self.usage.bytes_used <= self.limit.bytes_soft
            && self.usage.inodes_used <= self.limit.inodes_soft;

        if now_under_soft {
            self.usage.soft_exceeded_since = None;
        }
    }

    /// Returns the current bytes usage as a percentage of the hard limit.
    pub fn usage_pct(&self) -> f64 {
        if self.limit.bytes_hard == u64::MAX {
            return 0.0;
        }
        if self.limit.bytes_hard == 0 {
            return 0.0;
        }
        (self.usage.bytes_used as f64 / self.limit.bytes_hard as f64) * 100.0
    }

    /// Returns the current inode usage as a percentage of the hard limit.
    pub fn inode_usage_pct(&self) -> f64 {
        if self.limit.inodes_hard == u64::MAX {
            return 0.0;
        }
        if self.limit.inodes_hard == 0 {
            return 0.0;
        }
        (self.usage.inodes_used as f64 / self.limit.inodes_hard as f64) * 100.0
    }
}

/// Statistics tracking for quota operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaStats {
    /// Total number of allocation requests that were allowed.
    pub allocations_allowed: u64,
    /// Total number of allocation requests that were denied.
    pub allocations_denied: u64,
    /// Number of times a soft limit was exceeded (warning level).
    pub soft_warnings: u64,
    /// Number of times a grace period expired.
    pub grace_expirations: u64,
}

/// Manages quotas for all tenants in the system.
pub struct QuotaManager {
    /// Map of tenant ID to tenant quota state.
    tenants: HashMap<String, TenantQuota>,
    /// Default limit applied to new tenants.
    default_limit: QuotaLimit,
    /// Statistics for quota operations.
    stats: QuotaStats,
}

impl QuotaManager {
    /// Creates a new QuotaManager with the given default limit.
    pub fn new(default_limit: QuotaLimit) -> Self {
        Self {
            tenants: HashMap::new(),
            default_limit,
            stats: QuotaStats::default(),
        }
    }

    /// Adds a new tenant with the specified quota limit.
    pub fn add_tenant(&mut self, tenant_id: &str, limit: QuotaLimit) {
        let quota = TenantQuota::new(tenant_id.to_string(), limit);
        self.tenants.insert(tenant_id.to_string(), quota);
        tracing::info!(tenant_id = %tenant_id, "Tenant added with quota");
    }

    /// Removes a tenant from the quota manager.
    pub fn remove_tenant(&mut self, tenant_id: &str) -> Option<TenantQuota> {
        self.tenants.remove(tenant_id)
    }

    /// Gets a reference to a tenant's quota state.
    pub fn get_tenant(&self, tenant_id: &str) -> Option<&TenantQuota> {
        self.tenants.get(tenant_id)
    }

    /// Checks if an allocation would be allowed without recording it.
    pub fn check_allocation(
        &self,
        tenant_id: &str,
        bytes: u64,
        now_secs: u64,
    ) -> StorageResult<()> {
        let tenant = self
            .tenants
            .get(tenant_id)
            .ok_or(StorageError::OutOfSpace)?;

        if tenant.can_allocate(bytes, now_secs) {
            Ok(())
        } else {
            Err(StorageError::OutOfSpace)
        }
    }

    /// Checks and records an allocation for a tenant.
    pub fn record_allocation(
        &mut self,
        tenant_id: &str,
        bytes: u64,
        now_secs: u64,
    ) -> StorageResult<()> {
        let tenant = self
            .tenants
            .get(tenant_id)
            .ok_or(StorageError::OutOfSpace)?;

        let old_status = tenant.check_status(now_secs);
        let can_alloc = tenant.can_allocate(bytes, now_secs);

        if can_alloc {
            self.stats.allocations_allowed += 1;

            let new_status = tenant.check_status(now_secs);
            if old_status == QuotaStatus::Ok && new_status != QuotaStatus::Ok {
                self.stats.soft_warnings += 1;
            }

            drop(tenant);
            self.tenants
                .get_mut(tenant_id)
                .unwrap()
                .record_allocation(bytes, now_secs);
            Ok(())
        } else {
            self.stats.allocations_denied += 1;

            let new_status = tenant.check_status(now_secs);
            if new_status == QuotaStatus::GraceExpired {
                self.stats.grace_expirations += 1;
            }

            tracing::warn!(
                tenant_id = %tenant_id,
                bytes_requested = bytes,
                bytes_used = tenant.usage.bytes_used,
                bytes_hard = tenant.limit.bytes_hard,
                "Allocation denied: quota exceeded"
            );
            Err(StorageError::OutOfSpace)
        }
    }

    /// Records bytes freed for a tenant.
    pub fn record_free(&mut self, tenant_id: &str, bytes: u64) {
        if let Some(tenant) = self.tenants.get_mut(tenant_id) {
            tenant.record_free(bytes);
            tracing::debug!(
                tenant_id = %tenant_id,
                bytes_freed = bytes,
                bytes_used = tenant.usage.bytes_used,
                "Bytes freed for tenant"
            );
        }
    }

    /// Returns the number of tenants managed.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    /// Returns the total bytes used across all tenants.
    pub fn total_usage_bytes(&self) -> u64 {
        self.tenants.values().map(|t| t.usage.bytes_used).sum()
    }

    /// Returns a reference to the quota statistics.
    pub fn stats(&self) -> &QuotaStats {
        &self.stats
    }

    /// Returns a list of all tenants.
    pub fn all_tenants(&self) -> Vec<&TenantQuota> {
        self.tenants.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_limit() -> QuotaLimit {
        QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 800,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        }
    }

    #[test]
    fn test_default_quota_is_unlimited() {
        let limit = QuotaLimit::default();
        assert_eq!(limit.bytes_hard, u64::MAX);
        assert_eq!(limit.bytes_soft, u64::MAX);
        assert_eq!(limit.inodes_hard, u64::MAX);
        assert_eq!(limit.inodes_soft, u64::MAX);
        assert_eq!(limit.grace_period_secs, 604800);
    }

    #[test]
    fn test_allocation_within_hard_limit_succeeds() {
        let mut quota = TenantQuota::new("test".to_string(), default_limit());
        assert!(quota.can_allocate(500, 0));
        quota.record_allocation(500, 0);
        assert_eq!(quota.usage.bytes_used, 500);
    }

    #[test]
    fn test_allocation_exceeding_hard_limit_fails() {
        let mut quota = TenantQuota::new("test".to_string(), default_limit());
        assert!(!quota.can_allocate(1001, 0));
    }

    #[test]
    fn test_soft_limit_triggers_warning_status() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 500,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_allocation(600, 0);
        let status = quota.check_status(0);
        assert!(matches!(status, QuotaStatus::SoftExceeded { .. }));
    }

    #[test]
    fn test_grace_period_tracking() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 500,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_allocation(600, 0);

        let status = quota.check_status(50);
        assert!(matches!(
            status,
            QuotaStatus::SoftExceeded {
                grace_remaining_secs: 50
            }
        ));

        let status = quota.check_status(101);
        assert!(matches!(status, QuotaStatus::GraceExpired));
    }

    #[test]
    fn test_grace_period_expiration() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 500,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 50,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_allocation(600, 100);
        let status = quota.check_status(151);
        assert!(matches!(status, QuotaStatus::GraceExpired));
        assert!(!quota.can_allocate(100, 151));
    }

    #[test]
    fn test_inode_quota_enforcement() {
        let limit = default_limit();
        let mut quota = TenantQuota::new("test".to_string(), limit);
        for _ in 0..100 {
            assert!(quota.can_create_inode(0));
            quota.record_inode_create(0);
        }
        assert!(!quota.can_create_inode(0));
    }

    #[test]
    fn test_record_allocation_and_free() {
        let mut quota = TenantQuota::new("test".to_string(), default_limit());
        quota.record_allocation(500, 0);
        assert_eq!(quota.usage.bytes_used, 500);
        quota.record_free(500);
        assert_eq!(quota.usage.bytes_used, 0);
    }

    #[test]
    fn test_usage_percentage_calculation() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 800,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_allocation(500, 0);
        let pct = quota.usage_pct();
        assert!((pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_multiple_tenants() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("tenant1", default_limit());
        manager.add_tenant("tenant2", default_limit());

        manager.record_allocation("tenant1", 500, 0).unwrap();
        manager.record_allocation("tenant2", 300, 0).unwrap();

        assert_eq!(manager.tenant_count(), 2);
        assert_eq!(manager.total_usage_bytes(), 800);
    }

    #[test]
    fn test_remove_tenant() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("tenant1", default_limit());
        let removed = manager.remove_tenant("tenant1");
        assert!(removed.is_some());
        assert!(manager.get_tenant("tenant1").is_none());
    }

    #[test]
    fn test_total_usage_across_tenants() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("tenant1", default_limit());
        manager.add_tenant("tenant2", default_limit());

        manager.record_allocation("tenant1", 400, 0).unwrap();
        manager.record_allocation("tenant2", 600, 0).unwrap();

        assert_eq!(manager.total_usage_bytes(), 1000);
    }

    #[test]
    fn test_default_limit_applied_to_new_tenants() {
        let limit = QuotaLimit {
            bytes_hard: 500,
            bytes_soft: 400,
            inodes_hard: 50,
            inodes_soft: 40,
            grace_period_secs: 100,
        };
        let mut manager = QuotaManager::new(limit);
        manager.add_tenant("new_tenant", QuotaLimit::default());

        let tenant = manager.get_tenant("new_tenant").unwrap();
        assert_eq!(tenant.limit.bytes_hard, u64::MAX);
    }

    #[test]
    fn test_stats_tracking_allowed_denied() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("tenant1", default_limit());

        manager.record_allocation("tenant1", 500, 0).unwrap();
        let result = manager.record_allocation("tenant1", 600, 0);
        assert!(result.is_err());

        let stats = manager.stats();
        assert_eq!(stats.allocations_allowed, 1);
        assert_eq!(stats.allocations_denied, 1);
    }

    #[test]
    fn test_check_allocation_without_recording() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("tenant1", default_limit());

        manager.check_allocation("tenant1", 500, 0).unwrap();
        let tenant = manager.get_tenant("tenant1").unwrap();
        assert_eq!(tenant.usage.bytes_used, 0);
    }

    #[test]
    fn test_tenant_not_found_handling() {
        let manager = QuotaManager::new(default_limit());
        let result = manager.check_allocation("nonexistent", 100, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_usage_after_free_equals_allocation() {
        let mut quota = TenantQuota::new("test".to_string(), default_limit());
        quota.record_allocation(500, 0);
        quota.record_free(500);
        assert_eq!(quota.usage.bytes_used, 0);
    }

    #[test]
    fn test_soft_exceeded_resets_when_usage_drops() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 500,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_allocation(600, 0);
        assert!(quota.usage.soft_exceeded_since.is_some());

        quota.record_free(200);
        assert!(quota.usage.soft_exceeded_since.is_none());

        let status = quota.check_status(0);
        assert!(matches!(status, QuotaStatus::Ok));
    }

    #[test]
    fn test_quota_status_equality() {
        let status1 = QuotaStatus::Ok;
        let status2 = QuotaStatus::Ok;
        assert_eq!(status1, status2);

        let status3 = QuotaStatus::SoftExceeded {
            grace_remaining_secs: 50,
        };
        let status4 = QuotaStatus::SoftExceeded {
            grace_remaining_secs: 50,
        };
        assert_eq!(status3, status4);
    }

    #[test]
    fn test_inode_create_and_delete_tracking() {
        let mut quota = TenantQuota::new("test".to_string(), default_limit());
        assert_eq!(quota.usage.inodes_used, 0);

        quota.record_inode_create(0);
        assert_eq!(quota.usage.inodes_used, 1);

        quota.record_inode_create(0);
        assert_eq!(quota.usage.inodes_used, 2);

        quota.record_inode_delete();
        assert_eq!(quota.usage.inodes_used, 1);
    }

    #[test]
    fn test_inode_usage_percentage() {
        let limit = QuotaLimit {
            bytes_hard: 1000,
            bytes_soft: 800,
            inodes_hard: 100,
            inodes_soft: 80,
            grace_period_secs: 100,
        };
        let mut quota = TenantQuota::new("test".to_string(), limit);
        quota.record_inode_create(0);
        quota.record_inode_create(0);

        let pct = quota.inode_usage_pct();
        assert!((pct - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_all_tenants() {
        let mut manager = QuotaManager::new(default_limit());
        manager.add_tenant("t1", default_limit());
        manager.add_tenant("t2", default_limit());
        manager.add_tenant("t3", default_limit());

        let tenants = manager.all_tenants();
        assert_eq!(tenants.len(), 3);
    }
}
