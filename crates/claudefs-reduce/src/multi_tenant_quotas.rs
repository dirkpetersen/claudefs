use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use crate::error::ReduceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId(pub u64);

#[derive(Debug, Clone)]
pub struct QuotaLimit {
    pub soft_limit_bytes: u64,
    pub hard_limit_bytes: u64,
    pub enforce_on_write: bool,
}

impl QuotaLimit {
    pub fn new(soft_limit_bytes: u64, hard_limit_bytes: u64, enforce_on_write: bool) -> Self {
        Self {
            soft_limit_bytes,
            hard_limit_bytes,
            enforce_on_write,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuotaUsage {
    pub tenant_id: TenantId,
    pub used_bytes: Arc<AtomicU64>,
    pub compressed_bytes: Arc<AtomicU64>,
    pub dedup_saved_bytes: Arc<AtomicU64>,
    pub last_update_ms: u64,
}

impl QuotaUsage {
    pub fn new(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            used_bytes: Arc::new(AtomicU64::new(0)),
            compressed_bytes: Arc::new(AtomicU64::new(0)),
            dedup_saved_bytes: Arc::new(AtomicU64::new(0)),
            last_update_ms: 0,
        }
    }

    pub fn get_used_bytes(&self) -> u64 {
        self.used_bytes.load(Ordering::Relaxed)
    }

    pub fn get_compressed_bytes(&self) -> u64 {
        self.compressed_bytes.load(Ordering::Relaxed)
    }

    pub fn get_dedup_saved_bytes(&self) -> u64 {
        self.dedup_saved_bytes.load(Ordering::Relaxed)
    }

    pub fn update_last_access(&mut self, timestamp_ms: u64) {
        self.last_update_ms = timestamp_ms;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaAction {
    Allowed,
    SoftLimitWarn,
    HardLimitReject,
}

pub struct MultiTenantQuotas {
    quotas: Arc<RwLock<HashMap<TenantId, QuotaLimit>>>,
    usage: Arc<RwLock<HashMap<TenantId, QuotaUsage>>>,
}

impl Default for MultiTenantQuotas {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiTenantQuotas {
    pub fn new() -> Self {
        Self {
            quotas: Arc::new(RwLock::new(HashMap::new())),
            usage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set_quota(&self, tenant_id: TenantId, limit: QuotaLimit) -> Result<(), ReduceError> {
        if limit.soft_limit_bytes > limit.hard_limit_bytes && limit.hard_limit_bytes != 0 {
            return Err(ReduceError::InvalidInput(
                "soft_limit cannot exceed hard_limit".to_string(),
            ));
        }

        let mut quotas = self.quotas.write().map_err(|e| {
            ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
        })?;
        quotas.insert(tenant_id, limit.clone());

        let mut usage = self.usage.write().map_err(|e| {
            ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
        })?;
        usage
            .entry(tenant_id)
            .or_insert_with(|| QuotaUsage::new(tenant_id));

        info!("Set quota for tenant {:?}: {:?}", tenant_id, limit);
        Ok(())
    }

    pub fn check_quota(
        &self,
        tenant_id: TenantId,
        num_bytes: u64,
    ) -> Result<QuotaAction, ReduceError> {
        let quotas = self.quotas.read().map_err(|e| {
            ReduceError::InvalidInput(format!("Failed to acquire read lock: {}", e))
        })?;

        let usage = self.usage.read().map_err(|e| {
            ReduceError::InvalidInput(format!("Failed to acquire read lock: {}", e))
        })?;

        let limit = match quotas.get(&tenant_id) {
            Some(l) => l,
            None => return Ok(QuotaAction::Allowed),
        };

        if limit.hard_limit_bytes == 0 {
            return Ok(QuotaAction::Allowed);
        }

        let current_used = usage
            .get(&tenant_id)
            .map(|u| u.get_used_bytes())
            .unwrap_or(0);

        let new_used = current_used.saturating_add(num_bytes);

        if limit.hard_limit_bytes > 0 && new_used > limit.hard_limit_bytes {
            warn!(
                "Tenant {:?} hard limit exceeded: {} > {}",
                tenant_id, new_used, limit.hard_limit_bytes
            );
            return Ok(QuotaAction::HardLimitReject);
        }

        if limit.soft_limit_bytes > 0 && new_used > limit.soft_limit_bytes {
            debug!(
                "Tenant {:?} soft limit warning: {} > {}",
                tenant_id, new_used, limit.soft_limit_bytes
            );
            return Ok(QuotaAction::SoftLimitWarn);
        }

        Ok(QuotaAction::Allowed)
    }

    pub fn record_write(
        &self,
        tenant_id: TenantId,
        raw_bytes: u64,
        compressed_bytes: u64,
        dedup_saved: u64,
    ) -> Result<(), ReduceError> {
        let mut usage = self.usage.write().map_err(|e| {
            ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
        })?;

        let tenant_usage = usage
            .entry(tenant_id)
            .or_insert_with(|| QuotaUsage::new(tenant_id));
        tenant_usage
            .used_bytes
            .fetch_add(raw_bytes, Ordering::Relaxed);
        tenant_usage
            .compressed_bytes
            .fetch_add(compressed_bytes, Ordering::Relaxed);
        tenant_usage
            .dedup_saved_bytes
            .fetch_add(dedup_saved, Ordering::Relaxed);
        tenant_usage.last_update_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        debug!(
            "Recorded write for tenant {:?}: raw={}, compressed={}, dedup_saved={}",
            tenant_id, raw_bytes, compressed_bytes, dedup_saved
        );
        Ok(())
    }

    pub fn get_usage(&self, tenant_id: TenantId) -> Option<QuotaUsage> {
        let usage = self.usage.read().ok()?;
        let tenant_usage = usage.get(&tenant_id)?;

        Some(QuotaUsage {
            tenant_id,
            used_bytes: Arc::new(AtomicU64::new(tenant_usage.get_used_bytes())),
            compressed_bytes: Arc::new(AtomicU64::new(tenant_usage.get_compressed_bytes())),
            dedup_saved_bytes: Arc::new(AtomicU64::new(tenant_usage.get_dedup_saved_bytes())),
            last_update_ms: tenant_usage.last_update_ms,
        })
    }

    pub fn get_utilization_percent(&self, tenant_id: TenantId) -> f64 {
        let quotas = match self.quotas.read() {
            Ok(q) => q,
            Err(_) => return 0.0,
        };

        let usage = match self.usage.read() {
            Ok(u) => u,
            Err(_) => return 0.0,
        };

        let limit = match quotas.get(&tenant_id) {
            Some(l) if l.hard_limit_bytes > 0 => l.hard_limit_bytes,
            _ => return 0.0,
        };

        let current_used = usage
            .get(&tenant_id)
            .map(|u| u.get_used_bytes())
            .unwrap_or(0);

        (current_used as f64 / limit as f64) * 100.0
    }

    pub fn prune_inactive_tenants(&self, cutoff_ms: u64) -> Vec<TenantId> {
        let mut usage = match self.usage.write() {
            Ok(u) => u,
            Err(_) => return vec![],
        };

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut pruned = Vec::new();
        usage.retain(|tenant_id, tenant_usage| {
            let is_inactive = current_time.saturating_sub(tenant_usage.last_update_ms) > cutoff_ms;
            if is_inactive {
                pruned.push(*tenant_id);
            }
            !is_inactive
        });

        if !pruned.is_empty() {
            info!("Pruned {} inactive tenants", pruned.len());
        }

        pruned
    }

    pub fn get_dedup_ratio(&self, tenant_id: TenantId) -> f64 {
        let usage = match self.usage.read() {
            Ok(u) => u,
            Err(_) => return 1.0,
        };

        let tenant_usage = match usage.get(&tenant_id) {
            Some(u) => u,
            None => return 1.0,
        };

        let used = tenant_usage.get_used_bytes();
        let compressed = tenant_usage.get_compressed_bytes();

        if used == 0 || compressed == 0 {
            return 1.0;
        }

        used as f64 / compressed as f64
    }
}

#[derive(Debug, Clone)]
pub struct QuotaMetrics {
    pub tenant_id: u64,
    pub used_bytes: u64,
    pub compressed_bytes: u64,
    pub dedup_saved_bytes: u64,
    pub soft_limit_bytes: u64,
    pub hard_limit_bytes: u64,
    pub utilization_percent: f64,
    pub dedup_ratio: f64,
}

impl MultiTenantQuotas {
    pub fn export_metrics(&self) -> Vec<QuotaMetrics> {
        let quotas = match self.quotas.read() {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let usage = match self.usage.read() {
            Ok(u) => u,
            Err(_) => return vec![],
        };

        let mut metrics = Vec::new();

        for (tenant_id, tenant_usage) in usage.iter() {
            let limit = quotas.get(tenant_id);
            let used = tenant_usage.get_used_bytes();
            let hard_limit = limit.map(|l| l.hard_limit_bytes).unwrap_or(0);
            let soft_limit = limit.map(|l| l.soft_limit_bytes).unwrap_or(0);

            let utilization = if hard_limit > 0 {
                (used as f64 / hard_limit as f64) * 100.0
            } else {
                0.0
            };

            metrics.push(QuotaMetrics {
                tenant_id: tenant_id.0,
                used_bytes: used,
                compressed_bytes: tenant_usage.get_compressed_bytes(),
                dedup_saved_bytes: tenant_usage.get_dedup_saved_bytes(),
                soft_limit_bytes: soft_limit,
                hard_limit_bytes: hard_limit,
                utilization_percent: utilization,
                dedup_ratio: self.get_dedup_ratio(*tenant_id),
            });
        }

        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tenant_quota() {
        let quotas = MultiTenantQuotas::new();
        let limit = QuotaLimit::new(1000, 2000, true);
        quotas.set_quota(TenantId(1), limit).unwrap();
    }

    #[test]
    fn test_update_quota_limits() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(2000, 4000, false))
            .unwrap();

        let usage = quotas.get_usage(TenantId(1));
        assert!(usage.is_some());
    }

    #[test]
    fn test_record_writes_track_usage() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();

        quotas.record_write(TenantId(1), 100, 80, 10).unwrap();
        quotas.record_write(TenantId(1), 200, 160, 20).unwrap();

        let usage = quotas.get_usage(TenantId(1)).unwrap();
        assert_eq!(usage.get_used_bytes(), 300);
        assert_eq!(usage.get_compressed_bytes(), 240);
        assert_eq!(usage.get_dedup_saved_bytes(), 30);
    }

    #[test]
    fn test_check_quota_allowed() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();

        let result = quotas.check_quota(TenantId(1), 500).unwrap();
        assert_eq!(result, QuotaAction::Allowed);
    }

    #[test]
    fn test_check_quota_soft_limit_warn() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, false))
            .unwrap();
        quotas.record_write(TenantId(1), 900, 900, 0).unwrap();

        let result = quotas.check_quota(TenantId(1), 200).unwrap();
        assert_eq!(result, QuotaAction::SoftLimitWarn);
    }

    #[test]
    fn test_check_quota_hard_limit_reject() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1900, 1900, 0).unwrap();

        let result = quotas.check_quota(TenantId(1), 200).unwrap();
        assert_eq!(result, QuotaAction::HardLimitReject);
    }

    #[test]
    fn test_multiple_tenant_isolation() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas
            .set_quota(TenantId(2), QuotaLimit::new(500, 1000, true))
            .unwrap();

        quotas.record_write(TenantId(1), 500, 500, 0).unwrap();
        quotas.record_write(TenantId(2), 500, 500, 0).unwrap();

        let usage1 = quotas.get_usage(TenantId(1)).unwrap();
        let usage2 = quotas.get_usage(TenantId(2)).unwrap();

        assert_eq!(usage1.get_used_bytes(), 500);
        assert_eq!(usage2.get_used_bytes(), 500);

        let result1 = quotas.check_quota(TenantId(1), 1600).unwrap();
        let result2 = quotas.check_quota(TenantId(2), 600).unwrap();
        assert_eq!(result1, QuotaAction::HardLimitReject);
        assert_eq!(result2, QuotaAction::HardLimitReject);
    }

    #[test]
    fn test_quota_enforcement_returns_error_at_hard_limit() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(100, 100, true))
            .unwrap();
        quotas.record_write(TenantId(1), 100, 100, 0).unwrap();

        let result = quotas.check_quota(TenantId(1), 1).unwrap();
        assert_eq!(result, QuotaAction::HardLimitReject);
    }

    #[test]
    fn test_dedup_ratio_calculation_basic() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(10000, 20000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1000, 800, 200).unwrap();

        let ratio = quotas.get_dedup_ratio(TenantId(1));
        assert!((ratio - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_dedup_ratio_no_dedup() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(10000, 20000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1000, 1000, 0).unwrap();

        let ratio = quotas.get_dedup_ratio(TenantId(1));
        assert!((ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_dedup_ratio_high_dedup() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(100000, 200000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1000, 100, 900).unwrap();

        let ratio = quotas.get_dedup_ratio(TenantId(1));
        assert!((ratio - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_inactive_tenant_pruning() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas
            .set_quota(TenantId(2), QuotaLimit::new(1000, 2000, true))
            .unwrap();

        quotas.record_write(TenantId(1), 100, 100, 0).unwrap();
        quotas.record_write(TenantId(2), 100, 100, 0).unwrap();

        if let Some(mut usage) = quotas.get_usage(TenantId(1)) {
            usage.last_update_ms = 0;
            if let Ok(mut u) = quotas.usage.write() {
                u.insert(TenantId(1), usage);
            }
        }

        let pruned = quotas.prune_inactive_tenants(1);
        assert!(pruned.contains(&TenantId(1)));
    }

    #[test]
    fn test_concurrent_write_recording() {
        use std::sync::Arc;
        use std::thread;

        let quotas = Arc::new(MultiTenantQuotas::new());
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(100000, 200000, true))
            .unwrap();

        let quotas_clone = Arc::clone(&quotas);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                quotas_clone.record_write(TenantId(1), 1, 1, 0).unwrap();
            }
        });

        for _ in 0..1000 {
            quotas.record_write(TenantId(1), 1, 1, 0).unwrap();
        }

        handle.join().unwrap();

        let usage = quotas.get_usage(TenantId(1)).unwrap();
        assert_eq!(usage.get_used_bytes(), 2000);
    }

    #[test]
    fn test_quota_metrics_export() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1000, 800, 100).unwrap();

        let metrics = quotas.export_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].tenant_id, 1);
        assert_eq!(metrics[0].used_bytes, 1000);
    }

    #[test]
    fn test_get_utilization_percent() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 1000, 1000, 0).unwrap();

        let util = quotas.get_utilization_percent(TenantId(1));
        assert!((util - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_soft_limit_greater_than_hard_limit_fails() {
        let quotas = MultiTenantQuotas::new();
        let result = quotas.set_quota(TenantId(1), QuotaLimit::new(2000, 1000, true));
        assert!(result.is_err());
    }

    #[test]
    fn test_tenant_not_found_returns_allowed() {
        let quotas = MultiTenantQuotas::new();
        let result = quotas.check_quota(TenantId(999), 100).unwrap();
        assert_eq!(result, QuotaAction::Allowed);
    }

    #[test]
    fn test_tenant_not_found_returns_none() {
        let quotas = MultiTenantQuotas::new();
        let usage = quotas.get_usage(TenantId(999));
        assert!(usage.is_none());
    }

    #[test]
    fn test_get_usage_clones_data() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();
        quotas.record_write(TenantId(1), 100, 80, 10).unwrap();

        let usage1 = quotas.get_usage(TenantId(1)).unwrap();
        quotas.record_write(TenantId(1), 50, 40, 5).unwrap();
        let usage2 = quotas.get_usage(TenantId(1)).unwrap();

        assert_eq!(usage1.get_used_bytes(), 100);
        assert_eq!(usage2.get_used_bytes(), 150);
    }

    #[test]
    fn test_quota_limit_zero_hard_limit_allows_writes() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 0, true))
            .unwrap();
        quotas.record_write(TenantId(1), 10000, 10000, 0).unwrap();

        let result = quotas.check_quota(TenantId(1), 10000).unwrap();
        assert_eq!(result, QuotaAction::Allowed);
    }

    #[test]
    fn test_prune_empty_tenant_map() {
        let quotas = MultiTenantQuotas::new();
        let pruned = quotas.prune_inactive_tenants(1000);
        assert!(pruned.is_empty());
    }

    #[test]
    fn test_dedup_ratio_zero_used() {
        let quotas = MultiTenantQuotas::new();
        quotas
            .set_quota(TenantId(1), QuotaLimit::new(1000, 2000, true))
            .unwrap();

        let ratio = quotas.get_dedup_ratio(TenantId(1));
        assert!((ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_utilization_no_quota_set() {
        let quotas = MultiTenantQuotas::new();
        quotas.record_write(TenantId(1), 1000, 1000, 0).unwrap();

        let util = quotas.get_utilization_percent(TenantId(1));
        assert!((util - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_quota_action_display() {
        let action = QuotaAction::Allowed;
        assert_eq!(format!("{:?}", action), "Allowed");

        let action = QuotaAction::SoftLimitWarn;
        assert_eq!(format!("{:?}", action), "SoftLimitWarn");

        let action = QuotaAction::HardLimitReject;
        assert_eq!(format!("{:?}", action), "HardLimitReject");
    }
}
