//! Per-tenant storage and IOPS quota tracking with soft/hard limits.
//!
//! This module provides per-tenant quota enforcement for multi-tenant file systems,
//! supporting storage limits and IOPS caps with configurable soft/hard limits.

use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

pub use crate::tenant::TenantId;
use crate::types::{MetaError, Timestamp};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaType {
    Storage(u64),
    Iops(u64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TenantQuota {
    pub tenant_id: TenantId,
    pub storage_limit_bytes: u64,
    pub iops_limit: u64,
    pub soft_limit_warning_pct: f64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaUsage {
    pub tenant_id: TenantId,
    pub used_storage_bytes: u64,
    pub used_iops_this_second: u64,
    pub storage_pct: f64,
    pub iops_pct: f64,
    pub last_updated: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationType {
    StorageExceeded,
    IopsExceeded,
    BothExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaViolation {
    pub tenant_id: TenantId,
    pub violation_type: ViolationType,
    pub current_usage: u64,
    pub quota_limit: u64,
    pub exceeded_by_pct: f64,
    pub timestamp: Timestamp,
    pub severity: Severity,
}

#[derive(Clone, Debug)]
pub struct QuotaTrackerConfig {
    pub default_soft_limit_pct: f64,
    pub violation_history_size: usize,
    pub iops_window_secs: u64,
}

impl Default for QuotaTrackerConfig {
    fn default() -> Self {
        Self {
            default_soft_limit_pct: 80.0,
            violation_history_size: 1000,
            iops_window_secs: 1,
        }
    }
}

pub struct QuotaTracker {
    quotas: DashMap<TenantId, TenantQuota>,
    usage: DashMap<TenantId, Arc<Mutex<QuotaUsage>>>,
    violations: std::sync::RwLock<Vec<QuotaViolation>>,
    config: QuotaTrackerConfig,
}

impl QuotaTracker {
    pub fn new(config: QuotaTrackerConfig) -> Self {
        Self {
            quotas: DashMap::new(),
            usage: DashMap::new(),
            violations: std::sync::RwLock::new(Vec::with_capacity(config.violation_history_size)),
            config,
        }
    }

    pub fn add_quota(&self, tenant_id: TenantId, storage_bytes: u64, iops: u64) -> Result<(), MetaError> {
        if storage_bytes == 0 && iops == 0 {
            return Err(MetaError::InvalidArgument("quota limits cannot be zero".to_string()));
        }

        let now = Timestamp::now();
        let quota = TenantQuota {
            tenant_id: tenant_id.clone(),
            storage_limit_bytes: storage_bytes,
            iops_limit: iops,
            soft_limit_warning_pct: self.config.default_soft_limit_pct,
            created_at: now,
            updated_at: now,
        };

        self.quotas.insert(tenant_id.clone(), quota);

        let usage = QuotaUsage {
            tenant_id: tenant_id.clone(),
            used_storage_bytes: 0,
            used_iops_this_second: 0,
            storage_pct: 0.0,
            iops_pct: 0.0,
            last_updated: now,
        };
        self.usage.insert(tenant_id, Arc::new(Mutex::new(usage)));

        Ok(())
    }

    pub fn update_quota(&self, tenant_id: TenantId, new_storage: u64, new_iops: u64) -> Result<(), MetaError> {
        let mut quota = self.quotas.get_mut(&tenant_id)
            .ok_or_else(|| MetaError::NotFound(format!("tenant {} not found", tenant_id)))?;

        quota.storage_limit_bytes = new_storage;
        quota.iops_limit = new_iops;
        quota.updated_at = Timestamp::now();

        if new_storage > 0 {
            let usage_lock = self.usage.get(&tenant_id)
                .ok_or_else(|| MetaError::NotFound(format!("tenant {} usage not found", tenant_id)))?;
            let mut usage = usage_lock.lock().unwrap();
            usage.storage_pct = (usage.used_storage_bytes as f64 / new_storage as f64) * 100.0;
        }

        if new_iops > 0 {
            let usage_lock = self.usage.get(&tenant_id)
                .ok_or_else(|| MetaError::NotFound(format!("tenant {} usage not found", tenant_id)))?;
            let mut usage = usage_lock.lock().unwrap();
            usage.iops_pct = (usage.used_iops_this_second as f64 / new_iops as f64) * 100.0;
        }

        Ok(())
    }

    pub fn get_quota(&self, tenant_id: &TenantId) -> Option<TenantQuota> {
        self.quotas.get(tenant_id).map(|q| q.clone())
    }

    pub fn iter_quotas(&self) -> impl Iterator<Item = (TenantId, TenantQuota)> + '_ {
        self.quotas.iter().map(|entry| (entry.key().clone(), entry.value().clone()))
    }

    pub fn get_usage(&self, tenant_id: &TenantId) -> Option<QuotaUsage> {
        self.usage.get(tenant_id).map(|u| {
            let usage = u.lock().unwrap();
            usage.clone()
        })
    }

    pub fn check_storage_available(&self, tenant_id: &TenantId, bytes_needed: u64) -> Result<bool, QuotaViolation> {
        let quota = self.quotas.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: 0,
                quota_limit: 0,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        let usage_lock = self.usage.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: 0,
                quota_limit: quota.storage_limit_bytes,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        let mut usage = usage_lock.lock().unwrap();
        let new_total = usage.used_storage_bytes.saturating_add(bytes_needed);
        let new_pct = (new_total as f64 / quota.storage_limit_bytes as f64) * 100.0;

        if new_pct >= 100.0 {
            let violation = QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: new_total,
                quota_limit: quota.storage_limit_bytes,
                exceeded_by_pct: new_pct - 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        if new_pct >= quota.soft_limit_warning_pct {
            tracing::warn!(
                "Tenant {} storage usage {}% exceeds soft limit {}%",
                tenant_id.as_str(),
                new_pct,
                quota.soft_limit_warning_pct
            );
            let violation = QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: new_total,
                quota_limit: quota.storage_limit_bytes,
                exceeded_by_pct: new_pct - quota.soft_limit_warning_pct,
                timestamp: Timestamp::now(),
                severity: Severity::Warning,
            };
            self.record_violation(violation);
        }

        usage.used_storage_bytes = new_total;
        usage.storage_pct = new_pct;
        usage.last_updated = Timestamp::now();

        Ok(true)
    }

    pub fn check_iops_available(&self, tenant_id: &TenantId) -> Result<bool, QuotaViolation> {
        let quota = self.quotas.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::IopsExceeded,
                current_usage: 0,
                quota_limit: 0,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        if quota.iops_limit == 0 {
            return Ok(true);
        }

        let usage_lock = self.usage.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::IopsExceeded,
                current_usage: 0,
                quota_limit: quota.iops_limit,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        let mut usage = usage_lock.lock().unwrap();
        let current_iops = usage.used_iops_this_second + 1;
        let iops_pct = (current_iops as f64 / quota.iops_limit as f64) * 100.0;

        if iops_pct >= 100.0 {
            let violation = QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::IopsExceeded,
                current_usage: current_iops,
                quota_limit: quota.iops_limit,
                exceeded_by_pct: iops_pct - 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        if iops_pct >= quota.soft_limit_warning_pct {
            tracing::warn!(
                "Tenant {} IOPS usage {}% exceeds soft limit {}%",
                tenant_id.as_str(),
                iops_pct,
                quota.soft_limit_warning_pct
            );
            let violation = QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::IopsExceeded,
                current_usage: current_iops,
                quota_limit: quota.iops_limit,
                exceeded_by_pct: iops_pct - quota.soft_limit_warning_pct,
                timestamp: Timestamp::now(),
                severity: Severity::Warning,
            };
            self.record_violation(violation);
        }

        usage.used_iops_this_second = current_iops;
        usage.iops_pct = iops_pct;
        usage.last_updated = Timestamp::now();

        Ok(true)
    }

    pub fn record_storage_write(&self, tenant_id: &TenantId, bytes_written: u64) -> Result<(), QuotaViolation> {
        let quota = self.quotas.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: 0,
                quota_limit: 0,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        let usage_lock = self.usage.get(tenant_id)
            .ok_or_else(|| QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: 0,
                quota_limit: quota.storage_limit_bytes,
                exceeded_by_pct: 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            })?;

        let mut usage = usage_lock.lock().unwrap();
        let new_used = usage.used_storage_bytes.saturating_add(bytes_written);
        let new_pct = if quota.storage_limit_bytes > 0 {
            (new_used as f64 / quota.storage_limit_bytes as f64) * 100.0
        } else {
            0.0
        };

        if new_pct >= 100.0 {
            return Err(QuotaViolation {
                tenant_id: tenant_id.clone(),
                violation_type: ViolationType::StorageExceeded,
                current_usage: new_used,
                quota_limit: quota.storage_limit_bytes,
                exceeded_by_pct: new_pct - 100.0,
                timestamp: Timestamp::now(),
                severity: Severity::Critical,
            });
        }

        usage.used_storage_bytes = new_used;
        usage.storage_pct = new_pct;
        usage.last_updated = Timestamp::now();

        Ok(())
    }

    pub fn record_storage_delete(&self, tenant_id: &TenantId, bytes_freed: u64) {
        if let Some(usage_lock) = self.usage.get(tenant_id) {
            let mut usage = usage_lock.lock().unwrap();
            usage.used_storage_bytes = usage.used_storage_bytes.saturating_sub(bytes_freed);
            if let Some(quota) = self.quotas.get(tenant_id) {
                if quota.storage_limit_bytes > 0 {
                    usage.storage_pct = (usage.used_storage_bytes as f64 / quota.storage_limit_bytes as f64) * 100.0;
                }
            }
            usage.last_updated = Timestamp::now();
        }
    }

    pub fn get_violations(&self, tenant_id: &TenantId) -> Vec<QuotaViolation> {
        let violations = self.violations.read().unwrap();
        violations.iter()
            .filter(|v| v.tenant_id == *tenant_id)
            .cloned()
            .collect()
    }

    pub fn reset_iops_window(&self) {
        for entry in self.usage.iter() {
            let mut usage = entry.lock().unwrap();
            usage.used_iops_this_second = 0;
            usage.iops_pct = 0.0;
        }
    }

    fn record_violation(&self, violation: QuotaViolation) {
        let mut violations = self.violations.write().unwrap();
        if violations.len() >= self.config.violation_history_size {
            violations.remove(0);
        }
        violations.push(violation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tracker() -> QuotaTracker {
        QuotaTracker::new(QuotaTrackerConfig::default())
    }

    #[test]
    fn test_create_and_get_quota() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let quota = tracker.get_quota(&tenant).unwrap();
        assert_eq!(quota.storage_limit_bytes, 1_000_000_000);
        assert_eq!(quota.iops_limit, 10000);
    }

    #[test]
    fn test_update_quota() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        tracker.update_quota(tenant.clone(), 2_000_000_000, 20000).unwrap();
        
        let quota = tracker.get_quota(&tenant).unwrap();
        assert_eq!(quota.storage_limit_bytes, 2_000_000_000);
        assert_eq!(quota.iops_limit, 20000);
    }

    #[test]
    fn test_check_storage_available_below_limit() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 500_000_000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_storage_available_at_limit() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 1_000_000_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_storage_available_above_limit_rejected() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 1_000_000_001);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().violation_type, ViolationType::StorageExceeded);
    }

    #[test]
    fn test_check_iops_available_below_limit() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let result = tracker.check_iops_available(&tenant);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_iops_available_above_limit_rejected() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 2).unwrap();
        
        tracker.check_iops_available(&tenant).unwrap();
        let result = tracker.check_iops_available(&tenant);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().violation_type, ViolationType::IopsExceeded);
    }

    #[test]
    fn test_record_storage_write() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        tracker.record_storage_write(&tenant, 100_000_000).unwrap();
        
        let usage = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage.used_storage_bytes, 100_000_000);
    }

    #[test]
    fn test_record_storage_delete() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        tracker.record_storage_write(&tenant, 100_000_000).unwrap();
        tracker.record_storage_delete(&tenant, 50_000_000);
        
        let usage = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage.used_storage_bytes, 50_000_000);
    }

    #[test]
    fn test_soft_limit_warning_at_80_percent() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 800_000_001);
        assert!(result.is_ok());
    }

    #[test]
    fn test_hard_limit_rejection_at_100_percent() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 1001);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_tenants_concurrent_quota_checks() {
        let tracker = make_tracker();
        
        tracker.add_quota(TenantId::new("tenant1"), 1_000_000_000, 10000).unwrap();
        tracker.add_quota(TenantId::new("tenant2"), 2_000_000_000, 20000).unwrap();
        
        assert!(tracker.check_storage_available(&TenantId::new("tenant1"), 500_000_000).is_ok());
        assert!(tracker.check_storage_available(&TenantId::new("tenant2"), 1_500_000_000).is_ok());
    }

    #[tokio::test]
    async fn test_iops_window_reset_behavior() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000, 100).unwrap();
        tracker.check_iops_available(&tenant).unwrap();
        
        let usage1 = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage1.used_iops_this_second, 1);
        
        tracker.reset_iops_window();
        
        let usage2 = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage2.used_iops_this_second, 0);
    }

    #[tokio::test]
    async fn test_violation_history_tracking() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 100, 10000).unwrap();
        
        let _ = tracker.check_storage_available(&tenant, 101);
        
        let violations = tracker.get_violations(&tenant);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_recover_from_over_quota_state_delete_to_free() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1000, 10000).unwrap();
        tracker.record_storage_write(&tenant, 999).unwrap();

        let usage_before = tracker.get_usage(&tenant).unwrap();
        assert!((usage_before.storage_pct - 99.9).abs() < 0.1);

        tracker.record_storage_delete(&tenant, 500);

        let usage_after = tracker.get_usage(&tenant).unwrap();
        assert!((usage_after.storage_pct - 49.9).abs() < 0.1);
    }

    #[test]
    fn test_tenant_id_equality_and_display() {
        let t1 = TenantId::new("tenant1");
        let t2 = TenantId::new("tenant1");
        let t3 = TenantId::new("tenant2");
        
        assert_eq!(t1, t2);
        assert_ne!(t1, t3);
        assert_eq!(format!("{}", t1), "tenant1");
    }

    #[test]
    fn test_quota_violation_severity_calculation() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1000, 10000).unwrap();
        
        let result = tracker.check_storage_available(&tenant, 1100);
        let violation = result.unwrap_err();
        
        assert_eq!(violation.severity, Severity::Critical);
    }

    #[test]
    fn test_usage_percentage_calculations() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1000, 1000).unwrap();
        tracker.record_storage_write(&tenant, 500).unwrap();
        
        let usage = tracker.get_usage(&tenant).unwrap();
        assert!((usage.storage_pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_storage_and_iops_limits_edge_cases() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1_000_000_000_000, 1_000_000_000).unwrap();

        assert!(tracker.check_storage_available(&tenant, 500_000_000_000).is_ok());
        assert!(tracker.check_iops_available(&tenant).is_ok());
    }

    #[test]
    fn test_unknown_tenant_returns_not_found() {
        let tracker = make_tracker();
        let tenant = TenantId::new("unknown");
        
        assert!(tracker.get_quota(&tenant).is_none());
        assert!(tracker.get_usage(&tenant).is_none());
    }

    #[test]
    fn test_add_quota_validates_parameters() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        let result = tracker.add_quota(tenant.clone(), 0, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_quota_for_nonexistent_tenant() {
        let tracker = make_tracker();
        let tenant = TenantId::new("nonexistent");
        
        let result = tracker.update_quota(tenant, 1000, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_record_storage_delete_doesnt_go_negative() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 1000, 10000).unwrap();
        tracker.record_storage_delete(&tenant, 1000);
        
        let usage = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage.used_storage_bytes, 0);
    }

    #[test]
    fn test_concurrent_writes_to_same_tenant() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 10000, 10000).unwrap();
        
        tracker.record_storage_write(&tenant, 100).unwrap();
        tracker.record_storage_write(&tenant, 200).unwrap();
        tracker.record_storage_write(&tenant, 300).unwrap();
        
        let usage = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage.used_storage_bytes, 600);
    }

    #[test]
    fn test_get_violations_returns_copy_not_reference() {
        let tracker = make_tracker();
        let tenant = TenantId::new("tenant1");
        
        tracker.add_quota(tenant.clone(), 100, 10000).unwrap();
        let _ = tracker.check_storage_available(&tenant, 101);
        
        let violations = tracker.get_violations(&tenant);
        assert_eq!(violations.len(), 1);
    }
}