//! Quota Manager for per-tenant storage quota enforcement.
//!
//! Provides soft/hard quota limits, admin overrides, and usage tracking.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::tenant_isolator::TenantId;
use crate::error::ReduceError;

/// Quota configuration with thresholds and grace period settings.
pub struct QuotaConfig {
    /// Soft quota threshold as percentage (default 90%)
    pub soft_quota_percent: f64,
    /// Hard quota limit as percentage (default 100%)
    pub hard_quota_percent: f64,
    /// Grace period in seconds (default 300)
    pub grace_period_secs: u64,
    /// Enable admin override capability (default true)
    pub admin_override_enabled: bool,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            soft_quota_percent: 90.0,
            hard_quota_percent: 100.0,
            grace_period_secs: 300,
            admin_override_enabled: true,
        }
    }
}

/// Per-tenant quota state tracking used bytes, limits, and dedup credits.
pub struct TenantQuota {
    /// The tenant identifier
    pub tenant_id: TenantId,
    /// Maximum storage allowed in bytes
    pub limit_bytes: u64,
    /// Current used storage in bytes
    pub used_bytes: u64,
    /// Whether soft quota has been triggered
    pub soft_quota_triggered: bool,
    /// Timestamp when hard quota was first exceeded (if applicable)
    pub hard_quota_timestamp: Option<Instant>,
    /// Map of block IDs to their dedup credit amounts
    pub dedup_credits: HashMap<u64, u64>,
}

/// Quota decision returned by check_quota indicating allowed/srestricted/rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QuotaDecision {
    /// Write fully allowed, no restrictions
    AllowedFull,
    /// Write allowed with admin override but restricted
    AllowedRestricted,
    /// Soft quota exceeded, warning issued
    SoftQuotaWarning,
    /// Hard quota exceeded, write rejected
    Rejected,
}

/// Usage reason for accounting purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReason {
    /// Kind of operation causing usage
    pub kind: UsageKind,
    /// Additional metadata for the operation
    pub metadata: HashMap<String, String>,
}

/// Types of operations that affect quota usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageKind {
    /// Regular write operation
    Write,
    /// Deduplication savings
    Dedup,
    /// Compression savings
    Compression,
    /// Tiering to cold storage
    Tiering,
    /// EC repair operation
    Repair,
    /// Snapshot creation
    Snapshot,
}

/// Tenant usage summary for monitoring and reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUsage {
    /// Tenant identifier
    pub tenant_id: TenantId,
    /// Current used bytes
    pub used_bytes: u64,
    /// Limit in bytes
    pub limit_bytes: u64,
    /// Percentage of quota used
    pub percent_used: f64,
    /// Whether soft quota warning is active
    pub soft_quota_warning: bool,
    /// Whether hard quota is exceeded
    pub hard_quota_exceeded: bool,
    /// Total dedup savings in bytes
    pub dedup_savings_bytes: u64,
}

/// Quota enforcement metrics for monitoring.
#[derive(Debug, Clone)]
pub struct QuotaMetrics {
    /// Number of soft quota warnings issued
    pub soft_warnings: u64,
    /// Number of hard quota rejections
    pub hard_rejections: u64,
    /// Number of admin overrides applied
    pub admin_overrides: u64,
    /// Total number of quota checks performed
    pub total_checked: u64,
}

impl Default for QuotaMetrics {
    fn default() -> Self {
        Self {
            soft_warnings: 0,
            hard_rejections: 0,
            admin_overrides: 0,
            total_checked: 0,
        }
    }
}

/// Main quota manager for enforcing per-tenant storage limits.
pub struct QuotaManager {
    quotas: Arc<RwLock<HashMap<TenantId, TenantQuota>>>,
    accounting: Arc<RwLock<HashMap<TenantId, u64>>>,
    config: QuotaConfig,
    metrics: Arc<RwLock<QuotaMetrics>>,
}

impl QuotaManager {
    /// Create a new QuotaManager with the given configuration.
    pub fn new(config: QuotaConfig) -> Self {
        Self {
            quotas: Arc::new(RwLock::new(HashMap::new())),
            accounting: Arc::new(RwLock::new(HashMap::new())),
            config,
            metrics: Arc::new(RwLock::new(QuotaMetrics::default())),
        }
    }

    /// Set the storage quota limit for a tenant.
    pub async fn set_quota(&self, tenant_id: TenantId, limit_bytes: u64) -> Result<(), ReduceError> {
        let mut quotas = self.quotas.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        quotas.insert(tenant_id, TenantQuota {
            tenant_id,
            limit_bytes,
            used_bytes: 0,
            soft_quota_triggered: false,
            hard_quota_timestamp: None,
            dedup_credits: HashMap::new(),
        });
        info!("Set quota for tenant {:?}: {} bytes", tenant_id, limit_bytes);
        Ok(())
    }

    /// Check if a write operation is allowed under quota limits.
    pub async fn check_quota(
        &self,
        tenant_id: TenantId,
        write_bytes: u64,
        is_admin: bool,
    ) -> Result<QuotaDecision, ReduceError> {
        let mut metrics = self.metrics.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        metrics.total_checked += 1;

        let quotas = self.quotas.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let tenant_quota = match quotas.get(&tenant_id) {
            Some(q) => q,
            None => return Ok(QuotaDecision::AllowedFull),
        };

        let soft_threshold = (tenant_quota.limit_bytes as f64 * self.config.soft_quota_percent / 100.0) as u64;
        let hard_limit = tenant_quota.limit_bytes;

        let new_used = tenant_quota.used_bytes.saturating_add(write_bytes);

        if is_admin && self.config.admin_override_enabled {
            metrics.admin_overrides += 1;
            debug!("Admin override for tenant {:?}: {} bytes", tenant_id, write_bytes);
            return Ok(QuotaDecision::AllowedRestricted);
        }

        if new_used > hard_limit {
            metrics.hard_rejections += 1;
            warn!("Hard quota exceeded for tenant {:?}: {} > {}", tenant_id, new_used, hard_limit);
            return Ok(QuotaDecision::Rejected);
        }

        if new_used > soft_threshold {
            metrics.soft_warnings += 1;
            debug!("Soft quota warning for tenant {:?}: {} > {}", tenant_id, new_used, soft_threshold);
            return Ok(QuotaDecision::SoftQuotaWarning);
        }

        Ok(QuotaDecision::AllowedFull)
    }

    /// Update usage for a tenant with a delta (positive = charge, negative = credit).
    pub async fn update_usage(
        &self,
        tenant_id: TenantId,
        delta_bytes: i64,
        reason: UsageReason,
    ) -> Result<(), ReduceError> {
        let mut quotas = self.quotas.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let tenant_quota = quotas.get_mut(&tenant_id).ok_or_else(|| {
            ReduceError::InvalidInput(format!("Tenant {:?} not found", tenant_id))
        })?;

if delta_bytes < 0 {
            // Negative delta = savings (dedup/compression)
            let savings = (-delta_bytes) as u64;
            tenant_quota.used_bytes = tenant_quota.used_bytes.saturating_sub(savings);
            let mut accounting = self.accounting.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
            *accounting.entry(tenant_id).or_insert(0) += savings;
        } else {
            tenant_quota.used_bytes = tenant_quota.used_bytes.saturating_add(delta_bytes as u64);
        }

        let soft_threshold = (tenant_quota.limit_bytes as f64 * self.config.soft_quota_percent / 100.0) as u64;
        tenant_quota.soft_quota_triggered = tenant_quota.used_bytes > soft_threshold;

        debug!("Updated usage for tenant {:?}: delta={}, reason={:?}", tenant_id, delta_bytes, reason.kind);
        Ok(())
    }

    /// Apply a deduplication credit to a tenant.
    pub async fn apply_dedup_credit(
        &self,
        tenant_id: TenantId,
        block_id: u64,
        credit_bytes: u64,
    ) -> Result<(), ReduceError> {
        let mut quotas = self.quotas.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let tenant_quota = quotas.get_mut(&tenant_id).ok_or_else(|| {
            ReduceError::InvalidInput(format!("Tenant {:?} not found", tenant_id))
        })?;

        let credit = credit_bytes.min(tenant_quota.used_bytes);
        tenant_quota.used_bytes -= credit;
        tenant_quota.dedup_credits.insert(block_id, credit_bytes);

        let mut accounting = self.accounting.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        *accounting.entry(tenant_id).or_insert(0) += credit_bytes;

        debug!("Applied dedup credit for tenant {:?}: block={}, credit={}", tenant_id, block_id, credit_bytes);
        Ok(())
    }

    /// Get current usage information for a tenant.
    pub async fn get_tenant_usage(&self, tenant_id: TenantId) -> Result<TenantUsage, ReduceError> {
        let quotas = self.quotas.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        let accounting = self.accounting.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;

        let tenant_quota = quotas.get(&tenant_id).ok_or_else(|| {
            ReduceError::InvalidInput(format!("Tenant {:?} not found", tenant_id))
        })?;

        let dedup_savings = accounting.get(&tenant_id).copied().unwrap_or(0);
        let percent_used = if tenant_quota.limit_bytes > 0 {
            (tenant_quota.used_bytes as f64 / tenant_quota.limit_bytes as f64) * 100.0
        } else {
            0.0
        };

        let soft_threshold = (tenant_quota.limit_bytes as f64 * self.config.soft_quota_percent / 100.0) as u64;
        let hard_limit = tenant_quota.limit_bytes;

        Ok(TenantUsage {
            tenant_id,
            used_bytes: tenant_quota.used_bytes,
            limit_bytes: tenant_quota.limit_bytes,
            percent_used,
            soft_quota_warning: tenant_quota.used_bytes > soft_threshold,
            hard_quota_exceeded: tenant_quota.used_bytes > hard_limit,
            dedup_savings_bytes: dedup_savings,
        })
    }

    /// Get current quota enforcement metrics.
    pub fn get_metrics(&self) -> QuotaMetrics {
        self.metrics.read().map(|m| m.clone()).unwrap_or_default()
    }

    /// Remove a tenant and all associated quota state.
    pub async fn remove_tenant(&self, tenant_id: TenantId) -> Result<(), ReduceError> {
        let mut quotas = self.quotas.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        quotas.remove(&tenant_id);
        
        let mut accounting = self.accounting.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        accounting.remove(&tenant_id);
        
        info!("Removed tenant {:?}", tenant_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_and_check_quota() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        
        let decision = manager.check_quota(TenantId(1), 500, false).await.unwrap();
        assert_eq!(decision, QuotaDecision::AllowedFull);
    }

    #[tokio::test]
    async fn test_soft_quota_warning() {
        let mut config = QuotaConfig::default();
        config.soft_quota_percent = 50.0;
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        
        let decision = manager.check_quota(TenantId(1), 600, false).await.unwrap();
        assert_eq!(decision, QuotaDecision::SoftQuotaWarning);
    }

    #[tokio::test]
    async fn test_hard_quota_rejection() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        
        let decision = manager.check_quota(TenantId(1), 1100, false).await.unwrap();
        assert_eq!(decision, QuotaDecision::Rejected);
    }

    #[tokio::test]
    async fn test_admin_override() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        
        let decision = manager.check_quota(TenantId(1), 1500, true).await.unwrap();
        assert_eq!(decision, QuotaDecision::AllowedRestricted);
    }

    #[tokio::test]
    async fn test_update_usage() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        
        manager.update_usage(TenantId(1), 100, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        let usage = manager.get_tenant_usage(TenantId(1)).await.unwrap();
        assert_eq!(usage.used_bytes, 100);
    }

    #[tokio::test]
    async fn test_dedup_credit() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        manager.update_usage(TenantId(1), 1000, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        manager.apply_dedup_credit(TenantId(1), 42, 500).await.unwrap();
        
        let usage = manager.get_tenant_usage(TenantId(1)).await.unwrap();
        assert_eq!(usage.used_bytes, 500);
        assert_eq!(usage.dedup_savings_bytes, 500);
    }

    #[tokio::test]
    async fn test_remove_tenant() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        manager.set_quota(TenantId(1), 1000).await.unwrap();
        manager.remove_tenant(TenantId(1)).await.unwrap();
        
        let decision = manager.check_quota(TenantId(1), 500, false).await.unwrap();
        assert_eq!(decision, QuotaDecision::AllowedFull);
    }

    #[tokio::test]
    async fn test_tenant_not_found_allowed() {
        let config = QuotaConfig::default();
        let manager = QuotaManager::new(config);
        
        let decision = manager.check_quota(TenantId(999), 500, false).await.unwrap();
        assert_eq!(decision, QuotaDecision::AllowedFull);
    }
}