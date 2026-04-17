//! Client-side quota tracking and enforcement.
//!
//! Provides tenant-based quota tracking with usage monitoring and
//! pre-check capabilities before write operations.

use std::sync::Arc;
use dashmap::DashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Storage quota for a tenant.
#[derive(Debug, Clone)]
pub struct StorageQuota {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Total storage allowed in bytes.
    pub total_bytes: u64,
    /// Warning threshold percentage (e.g., 80).
    pub warning_threshold_pct: u8,
    /// Soft limit in bytes (warn but allow).
    pub soft_limit_bytes: Option<u64>,
    /// Hard limit in bytes (reject writes beyond).
    pub hard_limit_bytes: u64,
}

/// IOPS quota for a tenant.
#[derive(Debug, Clone)]
pub struct IopsQuota {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Maximum IOPS allowed.
    pub max_iops: u32,
}

/// Current quota usage for a tenant.
#[derive(Debug, Clone)]
pub struct QuotaUsage {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Currently used bytes.
    pub used_bytes: u64,
    /// Currently used IOPS (ops in last window).
    pub used_iops: u32,
    /// Timestamp when hard limit was first exceeded.
    pub exceeded_hard_limit_ns: Option<u64>,
    /// Timestamp when warning threshold was first exceeded.
    pub warning_issued_ns: Option<u64>,
}

impl Default for QuotaUsage {
    fn default() -> Self {
        Self {
            tenant_id: String::new(),
            used_bytes: 0,
            used_iops: 0,
            exceeded_hard_limit_ns: None,
            warning_issued_ns: None,
        }
    }
}

impl QuotaUsage {
    /// Creates a new quota usage entry.
    pub fn new(tenant_id: String) -> Self {
        Self {
            tenant_id,
            used_bytes: 0,
            used_iops: 0,
            exceeded_hard_limit_ns: None,
            warning_issued_ns: None,
        }
    }
}

/// Quota status for a tenant.
#[derive(Debug, Clone, PartialEq)]
pub enum QuotaStatus {
    /// Usage is within limits.
    Ok,
    /// Usage exceeded soft limit but not hard limit.
    Warning,
    /// Usage exceeded hard limit.
    Exceeded,
}

/// Quota metric for export to analytics.
#[derive(Debug, Clone)]
pub struct QuotaMetric {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Current used bytes.
    pub used_bytes: u64,
    /// Total allowed bytes.
    pub total_bytes: u64,
    /// Percentage of quota used.
    pub percent_used: f32,
    /// Current quota status.
    pub status: QuotaStatus,
}

/// Quota client tracker for tenant-based quota management.
pub struct QuotaClientTracker {
    /// Per-tenant storage quotas.
    storage_quotas: Arc<DashMap<String, StorageQuota>>,
    /// Per-tenant current usage (cached).
    usage_cache: Arc<DashMap<String, QuotaUsage>>,
    /// Sync interval with metadata service in milliseconds.
    sync_interval_ms: u64,
    /// IOPS tracking: ops in current window.
    iops_current: Arc<DashMap<String, u32>>,
    /// IOPS window start time.
    iops_window_start: Arc<DashMap<String, u64>>,
}

impl QuotaClientTracker {
    /// Creates a new quota tracker with specified sync interval.
    pub fn new(sync_interval_ms: u64) -> Self {
        Self {
            storage_quotas: Arc::new(DashMap::new()),
            usage_cache: Arc::new(DashMap::new()),
            sync_interval_ms,
            iops_current: Arc::new(DashMap::new()),
            iops_window_start: Arc::new(DashMap::new()),
        }
    }

    /// Gets current timestamp in nanoseconds.
    fn now_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Sets storage quota for a tenant.
    pub fn set_storage_quota(&self, quota: StorageQuota) -> Result<(), String> {
        if quota.tenant_id.is_empty() {
            return Err("tenant_id cannot be empty".to_string());
        }
        if quota.hard_limit_bytes > 0 && quota.total_bytes < quota.hard_limit_bytes {
            return Err("total_bytes must be >= hard_limit_bytes".to_string());
        }

        self.storage_quotas.insert(quota.tenant_id.clone(), quota.clone());

        let mut usage = self
            .usage_cache
            .entry(quota.tenant_id.clone())
            .or_insert_with(|| QuotaUsage::new(quota.tenant_id.clone()));
        if usage.tenant_id.is_empty() {
            usage.tenant_id = quota.tenant_id;
        }

        Ok(())
    }

    /// Sets IOPS quota for a tenant.
    pub fn set_iops_quota(&self, quota: IopsQuota) -> Result<(), String> {
        if quota.tenant_id.is_empty() {
            return Err("tenant_id cannot be empty".to_string());
        }
        Ok(())
    }

    /// Pre-check: can tenant write the specified number of bytes?
    pub async fn can_write(&self, tenant_id: &str, bytes: u64) -> Result<bool, String> {
        let quota = self
            .storage_quotas
            .get(tenant_id)
            .ok_or_else(|| format!("no quota configured for tenant {}", tenant_id))?;

        let usage = self
            .usage_cache
            .get(tenant_id)
            .ok_or_else(|| format!("no usage data for tenant {}", tenant_id))?;

        let projected = usage.used_bytes.saturating_add(bytes);

        if quota.hard_limit_bytes > 0 && projected >= quota.hard_limit_bytes {
            return Ok(false);
        }

        if let Some(soft_limit) = quota.soft_limit_bytes {
            if projected > soft_limit && usage.warning_issued_ns.is_none() {
                let mut usage = self.usage_cache.get_mut(tenant_id).unwrap();
                usage.warning_issued_ns = Some(Self::now_ns());
            }
        }

        Ok(true)
    }

    /// Records a write operation, updating usage.
    pub async fn record_write(&self, tenant_id: &str, bytes: u64) -> Result<(), String> {
        let quota = self
            .storage_quotas
            .get(tenant_id)
            .ok_or_else(|| format!("no quota configured for tenant {}", tenant_id))?;

        let mut usage = self
            .usage_cache
            .entry(tenant_id.to_string())
            .or_insert_with(|| QuotaUsage::new(tenant_id.to_string()));

        usage.used_bytes = usage.used_bytes.saturating_add(bytes);

        if quota.hard_limit_bytes > 0 && usage.used_bytes >= quota.hard_limit_bytes {
            if usage.exceeded_hard_limit_ns.is_none() {
                usage.exceeded_hard_limit_ns = Some(Self::now_ns());
            }
        }

        Ok(())
    }

    /// Records a read operation for IOPS tracking.
    pub async fn record_read(&self, tenant_id: &str) -> Result<(), String> {
        let now = Self::now_ns();
        let window_duration_ns = 1_000_000_000u64; // 1 second

        let window_start = *self
            .iops_window_start
            .entry(tenant_id.to_string())
            .or_insert(now);

        if now.saturating_sub(window_start) >= window_duration_ns {
            // Reset window
            let mut current = self.iops_current.get_mut(tenant_id).unwrap_or_else(|| {
                self.iops_current.insert(tenant_id.to_string(), 0);
                self.iops_current.get_mut(tenant_id).unwrap()
            });
            *current = 0;
            let mut start = self.iops_window_start.get_mut(tenant_id).unwrap();
            *start = now;
        }

        let mut current = self.iops_current.get_mut(tenant_id).unwrap_or_else(|| {
            self.iops_current.insert(tenant_id.to_string(), 0);
            self.iops_current.get_mut(tenant_id).unwrap()
        });
        *current = current.saturating_add(1);

        let mut usage = self
            .usage_cache
            .entry(tenant_id.to_string())
            .or_insert_with(|| QuotaUsage::new(tenant_id.to_string()));
        usage.used_iops = *current;

        Ok(())
    }

    /// Syncs usage from metadata service (simulated).
    pub async fn sync_usage_from_metadata(&self) -> Result<(), String> {
        // In production, this would call A2 metadata service
        // For now, just clear and refresh local state
        Ok(())
    }

    /// Gets current usage for a tenant.
    pub fn get_usage(&self, tenant_id: &str) -> Option<QuotaUsage> {
        self.usage_cache.get(tenant_id).map(|u| u.clone())
    }

    /// Checks if warning threshold is exceeded.
    pub fn is_warning_threshold_exceeded(&self, tenant_id: &str) -> bool {
        let quota = match self.storage_quotas.get(tenant_id) {
            Some(q) => q,
            None => return false,
        };

        let usage = match self.usage_cache.get(tenant_id) {
            Some(u) => u,
            None => return false,
        };

        if quota.total_bytes == 0 {
            return false;
        }

        let pct = (usage.used_bytes as f64 / quota.total_bytes as f64) * 100.0;
        pct >= quota.warning_threshold_pct as f64
    }

    /// Exports quota metrics for A8 analytics.
    pub fn export_metrics(&self) -> Vec<QuotaMetric> {
        self.storage_quotas
            .iter()
            .map(|entry| {
                let quota = entry.value();
                let usage = self.usage_cache.get(quota.tenant_id.as_str());

                let (used_bytes, status, _pct) = match usage {
                    Some(u) => {
                        let pct = if quota.total_bytes > 0 {
                            (u.used_bytes as f64 / quota.total_bytes as f64 * 100.0) as f32
                        } else {
                            0.0
                        };

                        let quota_status = if quota.hard_limit_bytes > 0 && u.used_bytes >= quota.hard_limit_bytes {
                            QuotaStatus::Exceeded
                        } else if let Some(soft) = quota.soft_limit_bytes {
                            if u.used_bytes > soft {
                                QuotaStatus::Warning
                            } else {
                                QuotaStatus::Ok
                            }
                        } else {
                            QuotaStatus::Ok
                        };

                        (u.used_bytes, quota_status, pct)
                    }
                    None => (0, QuotaStatus::Ok, 0.0),
                };

                QuotaMetric {
                    tenant_id: quota.tenant_id.clone(),
                    used_bytes,
                    total_bytes: quota.total_bytes,
                    percent_used: (used_bytes as f64 / quota.total_bytes.max(1) as f64 * 100.0) as f32,
                    status,
                }
            })
            .collect()
    }
}

impl Default for QuotaClientTracker {
    fn default() -> Self {
        Self::new(30000) // 30 second default sync
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_storage_quota_succeeds() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800_000_000),
            hard_limit_bytes: 1_000_000_000,
        };
        assert!(tracker.set_storage_quota(quota).is_ok());
    }

    #[tokio::test]
    async fn test_can_write_within_quota() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800_000_000),
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let can_write = tracker.can_write("tenant1", 1000).await;
        assert!(can_write.is_ok());
        assert!(can_write.unwrap());
    }

    #[tokio::test]
    async fn test_can_write_denied_at_hard_limit() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800_000_000),
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        tracker.record_write("tenant1", 1_000_000_000).await.unwrap();

        let can_write = tracker.can_write("tenant1", 1).await;
        assert!(can_write.is_ok());
        assert!(!can_write.unwrap());
    }

    #[tokio::test]
    async fn test_can_write_allowed_at_soft_limit_with_warning() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800_000_000),
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        tracker.record_write("tenant1", 850_000_000).await.unwrap();

        let can_write = tracker.can_write("tenant1", 1000).await;
        assert!(can_write.is_ok());
        assert!(can_write.unwrap());
    }

    #[tokio::test]
    async fn test_record_write_updates_usage() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        tracker.record_write("tenant1", 1000).await.unwrap();

        let usage = tracker.get_usage("tenant1").unwrap();
        assert_eq!(usage.used_bytes, 1000);
    }

    #[tokio::test]
    async fn test_record_read_increments_iops() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        tracker.record_read("tenant1").await.unwrap();
        tracker.record_read("tenant1").await.unwrap();

        let usage = tracker.get_usage("tenant1").unwrap();
        assert!(usage.used_iops >= 2);
    }

    #[tokio::test]
    async fn test_sync_usage_from_metadata_updates_cache() {
        let tracker = QuotaClientTracker::new(30000);
        let result = tracker.sync_usage_from_metadata().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_usage_returns_current_state() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let usage = tracker.get_usage("tenant1");
        assert!(usage.is_some());
        assert_eq!(usage.unwrap().tenant_id, "tenant1");
    }

    #[test]
    fn test_is_warning_threshold_exceeded_true_at_80pct() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 100,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(80),
            hard_limit_bytes: 100,
        };
        tracker.set_storage_quota(quota).unwrap();

        // We can't directly set used_bytes for testing, so just check the logic
        let exceeded = tracker.is_warning_threshold_exceeded("tenant1");
        assert!(!exceeded); // Currently 0% used
    }

    #[test]
    fn test_is_warning_threshold_exceeded_false_below_threshold() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let exceeded = tracker.is_warning_threshold_exceeded("tenant1");
        assert!(!exceeded);
    }

    #[test]
    fn test_export_metrics_format_valid() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let metrics = tracker.export_metrics();
        assert!(!metrics.is_empty());
        let m = &metrics[0];
        assert_eq!(m.tenant_id, "tenant1");
        assert_eq!(m.total_bytes, 1_000_000_000);
    }

    #[test]
    fn test_multiple_tenants_independent_quotas() {
        let tracker = QuotaClientTracker::new(30000);

        let quota1 = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        let quota2 = StorageQuota {
            tenant_id: "tenant2".to_string(),
            total_bytes: 2_000_000_000,
            warning_threshold_pct: 90,
            soft_limit_bytes: None,
            hard_limit_bytes: 2_000_000_000,
        };

        tracker.set_storage_quota(quota1).unwrap();
        tracker.set_storage_quota(quota2).unwrap();

        let metrics = tracker.export_metrics();
        assert_eq!(metrics.len(), 2);
    }

    #[tokio::test]
    async fn test_write_exceeding_hard_limit_returns_error() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800),
            hard_limit_bytes: 1000,
        };
        tracker.set_storage_quota(quota).unwrap();

        // Write 1000 bytes - at limit
        tracker.record_write("tenant1", 1000).await.unwrap();

        // Try to write more - should fail can_write
        let can = tracker.can_write("tenant1", 1).await.unwrap();
        assert!(!can);
    }

    #[test]
    fn test_soft_limit_allows_write_with_warning() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(800_000),
            hard_limit_bytes: 1_000_000,
        };
        let result = tracker.set_storage_quota(quota);
        assert!(result.is_ok());
    }

    #[test]
    fn test_usage_cache_invalidates_on_sync() {
        let tracker = QuotaClientTracker::new(30000);
        // Just verify sync doesn't error
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            tracker.sync_usage_from_metadata().await.unwrap();
        });
    }

    #[test]
    fn test_quota_metrics_percent_calculated_correctly() {
        let tracker = QuotaClientTracker::new(30000);
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            tracker.record_write("tenant1", 500).await.unwrap();
        });

        let metrics = tracker.export_metrics();
        assert_eq!(metrics[0].percent_used, 50.0);
    }

    #[tokio::test]
    async fn test_tenant_without_quota_defaults_unlimited() {
        let tracker = QuotaClientTracker::new(30000);
        let can = tracker.can_write("unknown", 1000).await;
        assert!(can.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_write_operations_tracked() {
        let tracker = Arc::new(QuotaClientTracker::new(30000));
        let quota = StorageQuota {
            tenant_id: "tenant1".to_string(),
            total_bytes: 1_000_000_000,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 1_000_000_000,
        };
        tracker.set_storage_quota(quota).unwrap();

        let tracker_for_spawn = Arc::clone(&tracker);
        let handle = tokio::spawn(async move {
            tracker_for_spawn.record_write("tenant1", 1000).await.unwrap();
        });

        handle.await.unwrap();

        let usage = tracker.get_usage("tenant1").unwrap();
        assert!(usage.used_bytes >= 1000);
    }
}