//! Quota enforcement proxy that sits at the I/O boundary.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::StorageResult;
use crate::io_accounting::{
    IoAccounting, IoAccountingConfig, IoDirection, TenantId, TenantIoStats,
};

/// Quota policy for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaPolicy {
    /// Soft limit in bytes.
    pub soft_limit_bytes: u64,
    /// Hard limit in bytes.
    pub hard_limit_bytes: u64,
    /// Warn threshold percentage of soft limit.
    pub warn_threshold_pct: u8,
}

impl Default for QuotaPolicy {
    fn default() -> Self {
        Self {
            soft_limit_bytes: u64::MAX,
            hard_limit_bytes: u64::MAX,
            warn_threshold_pct: 90,
        }
    }
}

/// Current quota usage for a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Tenant ID.
    pub tenant_id: u64,
    /// Bytes currently used.
    pub bytes_used: u64,
    /// IOPS currently used.
    pub iops_used: u64,
}

/// Quota violation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaViolation {
    /// Tenant that violated quota.
    pub tenant_id: u64,
    /// The limit that was exceeded.
    pub limit_bytes: u64,
    /// Bytes used at time of violation.
    pub used_bytes: u64,
    /// Whether this was a hard limit violation.
    pub is_hard: bool,
}

/// Manages quotas for tenants.
#[derive(Debug)]
pub struct QuotaManager {
    policies: HashMap<u64, QuotaPolicy>,
    usage: HashMap<u64, QuotaUsage>,
}

impl QuotaManager {
    /// Creates a new quota manager.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            usage: HashMap::new(),
        }
    }

    /// Sets a quota policy for a tenant.
    pub fn set_policy(&mut self, tenant_id: u64, policy: QuotaPolicy) {
        self.policies.insert(tenant_id, policy);
        self.usage.entry(tenant_id).or_insert_with(|| QuotaUsage {
            tenant_id,
            ..Default::default()
        });
        let policy = self.policies.get(&tenant_id).unwrap();
        debug!(
            tenant_id,
            soft = policy.soft_limit_bytes,
            hard = policy.hard_limit_bytes,
            "Set quota policy"
        );
    }

    /// Records usage change and returns a violation if limit exceeded.
    pub fn record_usage(&mut self, tenant_id: u64, delta_bytes: i64) -> Option<QuotaViolation> {
        let usage = self.usage.entry(tenant_id).or_insert_with(|| QuotaUsage {
            tenant_id,
            ..Default::default()
        });

        if delta_bytes >= 0 {
            usage.bytes_used = usage.bytes_used.saturating_add(delta_bytes as u64);
        } else {
            usage.bytes_used = usage.bytes_used.saturating_sub((-delta_bytes) as u64);
        }

        let policy = self.policies.get(&tenant_id);
        if let Some(policy) = policy {
            if usage.bytes_used > policy.hard_limit_bytes && policy.hard_limit_bytes > 0 {
                return Some(QuotaViolation {
                    tenant_id,
                    limit_bytes: policy.hard_limit_bytes,
                    used_bytes: usage.bytes_used,
                    is_hard: true,
                });
            }
        }
        None
    }

    /// Returns current usage for a tenant.
    pub fn current_usage(&self, tenant_id: u64) -> QuotaUsage {
        self.usage
            .get(&tenant_id)
            .cloned()
            .unwrap_or_else(|| QuotaUsage {
                tenant_id,
                ..Default::default()
            })
    }

    /// Returns the policy for a tenant.
    pub fn get_policy(&self, tenant_id: u64) -> Option<&QuotaPolicy> {
        self.policies.get(&tenant_id)
    }

    /// Removes a tenant.
    pub fn remove_tenant(&mut self, tenant_id: u64) {
        self.policies.remove(&tenant_id);
        self.usage.remove(&tenant_id);
    }
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of checking a quota before an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuotaCheckResult {
    /// Operation is permitted.
    Allowed,
    /// Operation is permitted but tenant is approaching the limit.
    Warning {
        /// Bytes available before hard limit.
        available_bytes: u64,
        /// Percentage of limit used.
        used_pct: u8,
    },
    /// Operation is rejected - hard limit would be exceeded.
    Rejected {
        /// Human-readable reason.
        reason: String,
        /// Hard limit in bytes.
        limit_bytes: u64,
        /// Current usage in bytes.
        used_bytes: u64,
    },
}

impl QuotaCheckResult {
    /// Returns true if operation is allowed (Allowed or Warning).
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            QuotaCheckResult::Allowed | QuotaCheckResult::Warning { .. }
        )
    }

    /// Returns true if operation was rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(self, QuotaCheckResult::Rejected { .. })
    }
}

/// Configuration for the enforcement proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaEnforcementConfig {
    /// Warn at this percentage of soft limit (default: 90).
    pub warn_threshold_pct: u8,
    /// Whether to track I/O accounting alongside quota (default: true).
    pub track_accounting: bool,
    /// Maximum number of tracked tenants (default: 1024).
    pub max_tenants: usize,
}

impl Default for QuotaEnforcementConfig {
    fn default() -> Self {
        Self {
            warn_threshold_pct: 90,
            track_accounting: true,
            max_tenants: 1024,
        }
    }
}

/// Statistics for the enforcement proxy.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuotaEnforcementStats {
    /// Total checks performed.
    pub checks_performed: u64,
    /// Operations allowed.
    pub allowed_count: u64,
    /// Operations with warning.
    pub warning_count: u64,
    /// Operations rejected.
    pub rejected_count: u64,
    /// Total bytes allowed.
    pub total_bytes_allowed: u64,
    /// Total bytes rejected.
    pub total_bytes_rejected: u64,
}

/// Quota enforcement proxy that sits at the I/O boundary.
pub struct QuotaEnforcementProxy {
    config: QuotaEnforcementConfig,
    quota_manager: QuotaManager,
    accounting: IoAccounting,
    stats: QuotaEnforcementStats,
}

impl QuotaEnforcementProxy {
    /// Creates a new quota enforcement proxy.
    pub fn new(config: QuotaEnforcementConfig) -> Self {
        Self {
            quota_manager: QuotaManager::new(),
            accounting: IoAccounting::new(IoAccountingConfig {
                max_tenants: config.max_tenants,
                ..Default::default()
            }),
            config,
            stats: QuotaEnforcementStats::default(),
        }
    }

    /// Sets a quota policy for a tenant.
    pub fn set_tenant_quota(
        &mut self,
        tenant_id: u64,
        soft_limit_bytes: u64,
        hard_limit_bytes: u64,
    ) {
        let policy = QuotaPolicy {
            soft_limit_bytes,
            hard_limit_bytes,
            warn_threshold_pct: self.config.warn_threshold_pct,
        };
        self.quota_manager.set_policy(tenant_id, policy);
    }

    /// Checks if a write operation is allowed for a tenant.
    pub fn check_write(&mut self, tenant_id: u64, bytes: u64) -> QuotaCheckResult {
        self.stats.checks_performed += 1;

        let usage = self.quota_manager.current_usage(tenant_id);
        let policy = self.quota_manager.get_policy(tenant_id);

        let result = if let Some(policy) = policy {
            if policy.hard_limit_bytes == 0 {
                QuotaCheckResult::Rejected {
                    reason: "Hard limit is 0 bytes, all writes are rejected".to_string(),
                    limit_bytes: 0,
                    used_bytes: usage.bytes_used,
                }
            } else if usage.bytes_used + bytes > policy.hard_limit_bytes {
                QuotaCheckResult::Rejected {
                    reason: format!(
                        "Write of {} bytes would exceed hard limit of {} bytes (currently using {})",
                        bytes, policy.hard_limit_bytes, usage.bytes_used
                    ),
                    limit_bytes: policy.hard_limit_bytes,
                    used_bytes: usage.bytes_used,
                }
            } else if policy.soft_limit_bytes > 0 {
                let warn_threshold = (policy.soft_limit_bytes as u128
                    * self.config.warn_threshold_pct as u128
                    / 100) as u64;
                if usage.bytes_used + bytes > warn_threshold {
                    let available = if policy.hard_limit_bytes > usage.bytes_used + bytes {
                        policy.hard_limit_bytes - usage.bytes_used - bytes
                    } else {
                        0
                    };
                    let used_pct = if policy.hard_limit_bytes > 0 {
                        (((usage.bytes_used + bytes) * 100 / policy.hard_limit_bytes) as u8)
                            .min(100)
                    } else {
                        0
                    };
                    QuotaCheckResult::Warning {
                        available_bytes: available,
                        used_pct,
                    }
                } else {
                    QuotaCheckResult::Allowed
                }
            } else {
                QuotaCheckResult::Allowed
            }
        } else {
            QuotaCheckResult::Allowed
        };

        match &result {
            QuotaCheckResult::Allowed => {
                self.stats.allowed_count += 1;
                self.stats.total_bytes_allowed += bytes;
            }
            QuotaCheckResult::Warning { .. } => {
                self.stats.allowed_count += 1;
                self.stats.warning_count += 1;
                self.stats.total_bytes_allowed += bytes;
            }
            QuotaCheckResult::Rejected { .. } => {
                self.stats.rejected_count += 1;
                self.stats.total_bytes_rejected += bytes;
            }
        }

        if self.config.track_accounting {
            self.accounting
                .record_op(TenantId(tenant_id), IoDirection::Write, bytes, 0);
        }

        result
    }

    /// Checks if a read operation is allowed (reads are only tracked, never rejected).
    pub fn check_read(&mut self, tenant_id: u64, bytes: u64) -> QuotaCheckResult {
        self.stats.checks_performed += 1;
        self.stats.allowed_count += 1;
        self.stats.total_bytes_allowed += bytes;

        if self.config.track_accounting {
            self.accounting
                .record_op(TenantId(tenant_id), IoDirection::Read, bytes, 0);
        }

        QuotaCheckResult::Allowed
    }

    /// Records that a write was committed (update quota usage).
    pub fn record_write(&mut self, tenant_id: u64, bytes: u64) {
        let _ = self.quota_manager.record_usage(tenant_id, bytes as i64);
    }

    /// Records that data was freed (update quota usage downward).
    pub fn record_free(&mut self, tenant_id: u64, bytes: u64) {
        let usage = self.quota_manager.current_usage(tenant_id);
        let new_usage = usage.bytes_used.saturating_sub(bytes);
        let delta = (usage.bytes_used as i64) - (new_usage as i64);
        let _ = self.quota_manager.record_usage(tenant_id, -delta);
    }

    /// Gets current usage for a tenant.
    pub fn current_usage(&self, tenant_id: u64) -> u64 {
        self.quota_manager.current_usage(tenant_id).bytes_used
    }

    /// Gets current I/O stats for a tenant.
    pub fn io_stats(&self, tenant_id: u64) -> Option<TenantIoStats> {
        self.accounting.get_stats(TenantId(tenant_id))
    }

    /// Gets enforcement stats.
    pub fn stats(&self) -> &QuotaEnforcementStats {
        &self.stats
    }

    /// Removes all tracking for a tenant.
    pub fn remove_tenant(&mut self, tenant_id: u64) {
        self.quota_manager.remove_tenant(tenant_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_write_when_no_quota_set_returns_allowed() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        let result = proxy.check_write(1, 1000);
        assert!(matches!(result, QuotaCheckResult::Allowed));
    }

    #[test]
    fn check_write_below_soft_limit_returns_allowed() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        let result = proxy.check_write(1, 5000);
        assert!(matches!(result, QuotaCheckResult::Allowed));
    }

    #[test]
    fn check_write_at_warning_threshold_returns_warning() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        proxy.record_write(1, 8000);
        let result = proxy.check_write(1, 2000);
        assert!(matches!(result, QuotaCheckResult::Warning { .. }));
    }

    #[test]
    fn check_write_above_hard_limit_returns_rejected() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        proxy.record_write(1, 15000);
        let result = proxy.check_write(1, 10000);
        assert!(matches!(result, QuotaCheckResult::Rejected { .. }));
    }

    #[test]
    fn rejected_result_has_correct_limit_bytes() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        proxy.record_write(1, 15000);
        let result = proxy.check_write(1, 10000);
        if let QuotaCheckResult::Rejected { limit_bytes, .. } = result {
            assert_eq!(limit_bytes, 20000);
        } else {
            panic!("Expected Rejected");
        }
    }

    #[test]
    fn rejected_result_has_correct_used_bytes() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        proxy.record_write(1, 15000);
        let result = proxy.check_write(1, 10000);
        if let QuotaCheckResult::Rejected { used_bytes, .. } = result {
            assert_eq!(used_bytes, 15000);
        } else {
            panic!("Expected Rejected");
        }
    }

    #[test]
    fn warning_result_has_correct_available_bytes() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 10000, 20000);
        proxy.record_write(1, 8000);
        let result = proxy.check_write(1, 2000);
        if let QuotaCheckResult::Warning {
            available_bytes, ..
        } = result
        {
            assert_eq!(available_bytes, 10000);
        } else {
            panic!("Expected Warning");
        }
    }

    #[test]
    fn is_allowed_true_for_allowed() {
        let result = QuotaCheckResult::Allowed;
        assert!(result.is_allowed());
    }

    #[test]
    fn is_allowed_true_for_warning() {
        let result = QuotaCheckResult::Warning {
            available_bytes: 1000,
            used_pct: 50,
        };
        assert!(result.is_allowed());
    }

    #[test]
    fn is_rejected_true_for_rejected() {
        let result = QuotaCheckResult::Rejected {
            reason: "test".to_string(),
            limit_bytes: 1000,
            used_bytes: 500,
        };
        assert!(result.is_rejected());
    }

    #[test]
    fn stats_allowed_count_increments_on_allowed() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.check_write(1, 100);
        assert_eq!(proxy.stats().allowed_count, 1);
    }

    #[test]
    fn stats_rejected_count_increments_on_rejected() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 100, 100);
        proxy.record_write(1, 50);
        proxy.check_write(1, 100);
        assert_eq!(proxy.stats().rejected_count, 1);
    }

    #[test]
    fn stats_warning_count_increments_on_warning() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 100, 200);
        proxy.record_write(1, 80);
        proxy.check_write(1, 20);
        assert_eq!(proxy.stats().warning_count, 1);
    }

    #[test]
    fn stats_total_bytes_rejected_accumulates_on_rejected() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 100, 100);
        proxy.record_write(1, 50);
        proxy.check_write(1, 100);
        proxy.check_write(1, 200);
        assert_eq!(proxy.stats().total_bytes_rejected, 300);
    }

    #[test]
    fn stats_total_bytes_allowed_accumulates_on_allowed() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.check_write(1, 100);
        proxy.check_write(1, 200);
        assert_eq!(proxy.stats().total_bytes_allowed, 300);
    }

    #[test]
    fn record_write_increases_current_usage() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.record_write(1, 1000);
        assert_eq!(proxy.current_usage(1), 1000);
    }

    #[test]
    fn record_free_decreases_current_usage_floor_at_0() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.record_write(1, 1000);
        proxy.record_free(1, 500);
        assert_eq!(proxy.current_usage(1), 500);

        proxy.record_free(1, 1000);
        assert_eq!(proxy.current_usage(1), 0);
    }

    #[test]
    fn check_read_never_rejects() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 100, 100);
        proxy.record_write(1, 200);
        let result = proxy.check_read(1, 1000);
        assert!(matches!(result, QuotaCheckResult::Allowed));
    }

    #[test]
    fn check_read_tracks_accounting_if_track_accounting_true() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig {
            track_accounting: true,
            ..Default::default()
        });
        proxy.check_read(1, 1000);
        let stats = proxy.io_stats(1);
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().bytes_read, 1000);
    }

    #[test]
    fn multiple_tenants_tracked_independently() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 1000, 2000);
        proxy.set_tenant_quota(2, 500, 1000);

        proxy.record_write(1, 100);
        proxy.record_write(2, 200);

        assert_eq!(proxy.current_usage(1), 100);
        assert_eq!(proxy.current_usage(2), 200);
    }

    #[test]
    fn remove_tenant_clears_usage_tracking() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 1000, 2000);
        proxy.record_write(1, 100);
        proxy.remove_tenant(1);

        let policy = proxy.quota_manager.get_policy(1);
        assert!(policy.is_none());
        assert_eq!(proxy.current_usage(1), 0);
    }

    #[test]
    fn hard_limit_0_rejects_all_writes() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 100, 0);
        let result = proxy.check_write(1, 10);
        assert!(matches!(result, QuotaCheckResult::Rejected { .. }));
    }

    #[test]
    fn set_tenant_quota_updates_existing_tenant() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.set_tenant_quota(1, 1000, 2000);
        proxy.set_tenant_quota(1, 5000, 10000);

        let policy = proxy.quota_manager.get_policy(1).unwrap();
        assert_eq!(policy.soft_limit_bytes, 5000);
        assert_eq!(policy.hard_limit_bytes, 10000);
    }

    #[test]
    fn checks_performed_increments_on_every_check() {
        let mut proxy = QuotaEnforcementProxy::new(QuotaEnforcementConfig::default());
        proxy.check_write(1, 100);
        proxy.check_read(1, 100);
        proxy.check_write(2, 200);
        assert_eq!(proxy.stats().checks_performed, 3);
    }

    #[test]
    fn quota_enforcement_config_default() {
        let config = QuotaEnforcementConfig::default();
        assert_eq!(config.warn_threshold_pct, 90);
        assert!(config.track_accounting);
        assert_eq!(config.max_tenants, 1024);
    }

    #[test]
    fn quota_policy_default() {
        let policy = QuotaPolicy::default();
        assert_eq!(policy.soft_limit_bytes, u64::MAX);
        assert_eq!(policy.hard_limit_bytes, u64::MAX);
        assert_eq!(policy.warn_threshold_pct, 90);
    }
}
