//! Multi-tenant data isolation for quota enforcement and data separation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

/// Unique identifier for a tenant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub u64);

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Priority level for a tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum TenantPriority {
    /// Lowest priority.
    Low,
    /// Normal priority.
    Normal,
    /// High priority.
    High,
    /// Critical priority — reserved for system operations.
    Critical,
}

/// Policy defining limits for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantPolicy {
    /// The tenant this policy applies to.
    pub tenant_id: TenantId,
    /// Maximum bytes allowed.
    pub quota_bytes: u64,
    /// Maximum IOPS allowed.
    pub max_iops: u32,
    /// Priority level.
    pub priority: TenantPriority,
}

/// Usage tracking for a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantUsage {
    /// The tenant this usage belongs to.
    pub tenant_id: TenantId,
    /// Bytes used.
    pub bytes_used: u64,
    /// IOPS consumed (sliding window).
    pub iops_used: u32,
    /// Number of chunks stored.
    pub chunks_stored: u64,
}

impl TenantUsage {
    /// Returns quota utilization as a ratio (0.0 to 1.0+, may exceed 1.0 if over quota).
    pub fn quota_utilization(&self, policy: &TenantPolicy) -> f64 {
        if policy.quota_bytes == 0 {
            return 0.0;
        }
        self.bytes_used as f64 / policy.quota_bytes as f64
    }

    /// Returns true if the tenant has exceeded their byte quota.
    pub fn is_quota_exceeded(&self, policy: &TenantPolicy) -> bool {
        self.bytes_used > policy.quota_bytes
    }
}

/// Multi-tenant data isolator for quota enforcement.
#[derive(Debug, Default)]
pub struct TenantIsolator {
    policies: HashMap<TenantId, TenantPolicy>,
    usage: HashMap<TenantId, TenantUsage>,
}

impl TenantIsolator {
    /// Create a new empty tenant isolator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tenant with the given policy.
    pub fn register_tenant(&mut self, policy: TenantPolicy) {
        let tenant_id = policy.tenant_id;
        self.policies.insert(tenant_id, policy);
        self.usage.entry(tenant_id).or_insert_with(|| TenantUsage {
            tenant_id,
            ..Default::default()
        });
    }

    /// Get the policy for a tenant.
    pub fn get_policy(&self, tenant_id: TenantId) -> Option<&TenantPolicy> {
        self.policies.get(&tenant_id)
    }

    /// Get the usage for a tenant.
    pub fn get_usage(&self, tenant_id: TenantId) -> Option<&TenantUsage> {
        self.usage.get(&tenant_id)
    }

    /// Record a write for a tenant.
    /// Returns an error if the tenant would exceed their quota.
    pub fn record_write(&mut self, tenant_id: TenantId, bytes: u64) -> Result<(), TenantError> {
        let policy = self
            .policies
            .get(&tenant_id)
            .ok_or(TenantError::UnknownTenant { tenant_id })?;

        let usage = self.usage.get_mut(&tenant_id).unwrap();
        let new_bytes = usage.bytes_used.saturating_add(bytes);

        if new_bytes > policy.quota_bytes {
            return Err(TenantError::QuotaExceeded {
                tenant_id,
                used: new_bytes,
                limit: policy.quota_bytes,
            });
        }

        usage.bytes_used = new_bytes;
        Ok(())
    }

    /// Record a new chunk stored for a tenant.
    pub fn record_chunk(
        &mut self,
        tenant_id: TenantId,
        chunk_size: u64,
    ) -> Result<(), TenantError> {
        if !self.policies.contains_key(&tenant_id) {
            return Err(TenantError::UnknownTenant { tenant_id });
        }

        let usage = self.usage.get_mut(&tenant_id).unwrap();
        usage.chunks_stored = usage.chunks_stored.saturating_add(1);
        usage.bytes_used = usage.bytes_used.saturating_add(chunk_size);
        Ok(())
    }

    /// Reset usage counters for a tenant.
    pub fn reset_usage(&mut self, tenant_id: TenantId) {
        if let Some(usage) = self.usage.get_mut(&tenant_id) {
            usage.bytes_used = 0;
            usage.iops_used = 0;
            usage.chunks_stored = 0;
        }
    }

    /// List all registered tenant IDs, sorted.
    pub fn list_tenants(&self) -> Vec<TenantId> {
        let mut ids: Vec<TenantId> = self.policies.keys().copied().collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// Returns tenant IDs that have exceeded their quota.
    pub fn tenants_over_quota(&self) -> Vec<TenantId> {
        self.policies
            .iter()
            .filter_map(|(id, policy)| {
                self.usage.get(id).and_then(|usage| {
                    if usage.is_quota_exceeded(policy) {
                        Some(*id)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

/// Errors that can occur during tenant operations.
#[derive(Debug, Error)]
pub enum TenantError {
    /// Tenant has exceeded their quota.
    #[error("tenant {tenant_id} quota exceeded: used {used} bytes, limit is {limit}")]
    QuotaExceeded {
        /// The tenant that exceeded the quota.
        tenant_id: TenantId,
        /// Bytes used.
        used: u64,
        /// Quota limit.
        limit: u64,
    },
    /// Unknown tenant.
    #[error("unknown tenant {tenant_id}")]
    UnknownTenant {
        /// The unknown tenant ID.
        tenant_id: TenantId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_id_equality() {
        let id1 = TenantId(1);
        let id2 = TenantId(1);
        let id3 = TenantId(2);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn tenant_priority_ordering() {
        assert!(TenantPriority::Critical > TenantPriority::High);
        assert!(TenantPriority::High > TenantPriority::Normal);
        assert!(TenantPriority::Normal > TenantPriority::Low);
    }

    #[test]
    fn register_and_get_policy() {
        let mut isolator = TenantIsolator::new();
        let policy = TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1024 * 1024,
            max_iops: 1000,
            priority: TenantPriority::Normal,
        };
        isolator.register_tenant(policy);

        let retrieved = isolator.get_policy(TenantId(1));
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().quota_bytes, 1024 * 1024);
    }

    #[test]
    fn get_policy_unknown_returns_none() {
        let isolator = TenantIsolator::new();
        assert!(isolator.get_policy(TenantId(999)).is_none());
    }

    #[test]
    fn record_write_increments_usage() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1024,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_write(TenantId(1), 100).unwrap();
        isolator.record_write(TenantId(1), 200).unwrap();

        let usage = isolator.get_usage(TenantId(1)).unwrap();
        assert_eq!(usage.bytes_used, 300);
    }

    #[test]
    fn record_write_quota_exceeded_returns_error() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 100,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_write(TenantId(1), 50).unwrap();
        let result = isolator.record_write(TenantId(1), 100);

        assert!(matches!(result, Err(TenantError::QuotaExceeded { .. })));
    }

    #[test]
    fn record_chunk_increments_chunks_stored() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1024,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_chunk(TenantId(1), 64).unwrap();
        isolator.record_chunk(TenantId(1), 64).unwrap();

        let usage = isolator.get_usage(TenantId(1)).unwrap();
        assert_eq!(usage.chunks_stored, 2);
        assert_eq!(usage.bytes_used, 128);
    }

    #[test]
    fn record_write_unknown_tenant_returns_error() {
        let mut isolator = TenantIsolator::new();
        let result = isolator.record_write(TenantId(999), 100);
        assert!(matches!(result, Err(TenantError::UnknownTenant { .. })));
    }

    #[test]
    fn reset_usage_clears_counters() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1024,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_write(TenantId(1), 100).unwrap();
        isolator.reset_usage(TenantId(1));

        let usage = isolator.get_usage(TenantId(1)).unwrap();
        assert_eq!(usage.bytes_used, 0);
        assert_eq!(usage.iops_used, 0);
        assert_eq!(usage.chunks_stored, 0);
    }

    #[test]
    fn list_tenants_sorted() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(3),
            quota_bytes: 100,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 100,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(2),
            quota_bytes: 100,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        let list = isolator.list_tenants();
        assert_eq!(list, vec![TenantId(1), TenantId(2), TenantId(3)]);
    }

    #[test]
    fn list_tenants_empty() {
        let isolator = TenantIsolator::new();
        assert!(isolator.list_tenants().is_empty());
    }

    #[test]
    fn tenants_over_quota_none() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });
        isolator.record_write(TenantId(1), 100).unwrap();

        assert!(isolator.tenants_over_quota().is_empty());
    }

    #[test]
    fn tenants_over_quota_some() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 100,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(2),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_chunk(TenantId(1), 200).unwrap();
        isolator.record_write(TenantId(2), 100).unwrap();

        let over = isolator.tenants_over_quota();
        assert_eq!(over.len(), 1);
        assert_eq!(over[0], TenantId(1));
    }

    #[test]
    fn quota_utilization_zero() {
        let usage = TenantUsage {
            tenant_id: TenantId(1),
            bytes_used: 0,
            iops_used: 0,
            chunks_stored: 0,
        };
        let policy = TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        };
        assert_eq!(usage.quota_utilization(&policy), 0.0);
    }

    #[test]
    fn quota_utilization_full() {
        let usage = TenantUsage {
            tenant_id: TenantId(1),
            bytes_used: 1000,
            iops_used: 0,
            chunks_stored: 0,
        };
        let policy = TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        };
        assert_eq!(usage.quota_utilization(&policy), 1.0);
    }

    #[test]
    fn is_quota_exceeded_false() {
        let usage = TenantUsage {
            tenant_id: TenantId(1),
            bytes_used: 100,
            iops_used: 0,
            chunks_stored: 0,
        };
        let policy = TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        };
        assert!(!usage.is_quota_exceeded(&policy));
    }

    #[test]
    fn is_quota_exceeded_true() {
        let usage = TenantUsage {
            tenant_id: TenantId(1),
            bytes_used: 1500,
            iops_used: 0,
            chunks_stored: 0,
        };
        let policy = TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        };
        assert!(usage.is_quota_exceeded(&policy));
    }

    #[test]
    fn multiple_tenants_isolated() {
        let mut isolator = TenantIsolator::new();
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(1),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });
        isolator.register_tenant(TenantPolicy {
            tenant_id: TenantId(2),
            quota_bytes: 1000,
            max_iops: 100,
            priority: TenantPriority::Normal,
        });

        isolator.record_write(TenantId(1), 500).unwrap();
        isolator.record_write(TenantId(2), 200).unwrap();

        let usage1 = isolator.get_usage(TenantId(1)).unwrap();
        let usage2 = isolator.get_usage(TenantId(2)).unwrap();

        assert_eq!(usage1.bytes_used, 500);
        assert_eq!(usage2.bytes_used, 200);
    }
}
