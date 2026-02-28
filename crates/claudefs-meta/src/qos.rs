//! QoS (Quality of Service) and traffic shaping for metadata operations.
//!
//! Implements per-tenant rate limiting and priority queuing for metadata
//! operations. Uses token bucket algorithm for rate limiting and priority
//! classes (Interactive > Batch > Background) for scheduling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// QoS priority class for tenant traffic scheduling.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum QosClass {
    /// Interactive traffic — highest priority, latency-sensitive.
    Interactive,
    /// Batch workloads — medium priority, throughput-oriented.
    Batch,
    /// Background tasks — low priority, preemptible.
    Background,
    /// System traffic — critical, always allowed.
    System,
}

impl QosClass {
    /// Returns the string representation of the QoS class.
    pub fn as_str(&self) -> &'static str {
        match self {
            QosClass::Interactive => "interactive",
            QosClass::Batch => "batch",
            QosClass::Background => "background",
            QosClass::System => "system",
        }
    }
}

/// QoS policy for a tenant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosPolicy {
    /// QoS class for this tenant.
    pub class: QosClass,
    /// Maximum I/O operations per second.
    pub max_iops: u64,
    /// Maximum bandwidth in bytes per second.
    pub max_bandwidth_bytes_sec: u64,
    /// Maximum metadata operations per second.
    pub max_metadata_ops_sec: u64,
    /// Weight for priority scheduling.
    pub priority_weight: u32,
}

#[derive(Clone, Debug)]
struct QosTokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Timestamp,
}

impl QosTokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Timestamp::now(),
        }
    }

    fn consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Timestamp::now();
        let elapsed_secs = (now.secs as f64 - self.last_refill.secs as f64)
            + (now.nanos as f64 - self.last_refill.nanos as f64) / 1_000_000_000.0;
        self.tokens = (self.tokens + elapsed_secs * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    fn reset(&mut self) {
        self.tokens = self.max_tokens;
        self.last_refill = Timestamp::now();
    }
}

/// Manages QoS policies and rate limiting for tenants.
pub struct QosManager {
    policies: RwLock<HashMap<String, QosPolicy>>,
    token_buckets: RwLock<HashMap<String, QosTokenBucket>>,
}

impl QosManager {
    /// Creates a new QoS manager with no policies configured.
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
            token_buckets: RwLock::new(HashMap::new()),
        }
    }

    /// Sets or updates the QoS policy for a tenant.
    pub fn set_policy(&self, tenant_id: String, policy: QosPolicy) {
        let mut policies = self.policies.write().unwrap();
        policies.insert(tenant_id.clone(), policy.clone());
        tracing::debug!("Set QoS policy for tenant {}: {:?}", tenant_id, policy);

        let mut buckets = self.token_buckets.write().unwrap();
        let max_tokens = policy.max_iops as f64;
        let refill_rate = policy.max_metadata_ops_sec as f64;
        buckets.insert(tenant_id, QosTokenBucket::new(max_tokens, refill_rate));
    }

    /// Removes the QoS policy for a tenant. Returns true if a policy existed.
    pub fn remove_policy(&self, tenant_id: &str) -> bool {
        let mut policies = self.policies.write().unwrap();
        let policy_removed = policies.remove(tenant_id).is_some();

        if policy_removed {
            let mut buckets = self.token_buckets.write().unwrap();
            buckets.remove(tenant_id);
            tracing::debug!("Removed QoS policy for tenant {}", tenant_id);
        }

        policy_removed
    }

    /// Gets the QoS policy for a tenant, if one exists.
    pub fn get_policy(&self, tenant_id: &str) -> Option<QosPolicy> {
        let policies = self.policies.read().unwrap();
        policies.get(tenant_id).cloned()
    }

    /// Checks if the tenant can proceed under rate limits. Consumes a token if available.
    pub fn check_rate_limit(&self, tenant_id: &str) -> bool {
        let policies = self.policies.read().unwrap();
        if !policies.contains_key(tenant_id) {
            return true;
        }
        drop(policies);

        let mut buckets = self.token_buckets.write().unwrap();
        if let Some(bucket) = buckets.get_mut(tenant_id) {
            bucket.consume()
        } else {
            true
        }
    }

    /// Checks if the tenant can perform an operation of the given size within bandwidth limits.
    pub fn check_bandwidth(&self, tenant_id: &str, bytes: u64) -> bool {
        let policies = self.policies.read().unwrap();
        if let Some(policy) = policies.get(tenant_id) {
            if policy.max_bandwidth_bytes_sec == 0 {
                return false;
            }
            let bytes_per_op = bytes.max(1) as f64;
            let max_ops_per_sec = policy.max_bandwidth_bytes_sec as f64 / bytes_per_op;
            return max_ops_per_sec >= 1.0;
        }
        true
    }

    /// Gets the QoS class for a tenant. Returns System if no policy is set.
    pub fn get_class(&self, tenant_id: &str) -> QosClass {
        let policies = self.policies.read().unwrap();
        policies
            .get(tenant_id)
            .map(|p| p.class)
            .unwrap_or(QosClass::System)
    }

    /// Returns all tenants sorted by priority (Interactive > Batch > Background > System).
    pub fn tenants_by_priority(&self) -> Vec<(String, QosClass)> {
        let policies = self.policies.read().unwrap();
        let mut tenants: Vec<(String, QosClass)> = policies
            .iter()
            .map(|(id, p)| (id.clone(), p.class))
            .collect();
        tenants.sort_by(|a, b| {
            let a_ord = a.1.as_ord();
            let b_ord = b.1.as_ord();
            b_ord.cmp(&a_ord)
        });
        tenants
    }

    /// Returns the number of configured QoS policies.
    pub fn policy_count(&self) -> usize {
        let policies = self.policies.read().unwrap();
        policies.len()
    }

    /// Resets all token buckets, allowing all tenants to burst again.
    pub fn reset_buckets(&self) {
        let mut buckets = self.token_buckets.write().unwrap();
        for bucket in buckets.values_mut() {
            bucket.reset();
        }
        tracing::debug!("Reset all token buckets");
    }
}

impl Default for QosManager {
    fn default() -> Self {
        Self::new()
    }
}

trait QosClassOrd {
    fn as_ord(&self) -> u8;
}

impl QosClassOrd for QosClass {
    fn as_ord(&self) -> u8 {
        match self {
            QosClass::Interactive => 3,
            QosClass::Batch => 2,
            QosClass::Background => 1,
            QosClass::System => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_policy() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Interactive,
            max_iops: 10000,
            max_bandwidth_bytes_sec: 1_000_000_000,
            max_metadata_ops_sec: 50000,
            priority_weight: 100,
        };

        mgr.set_policy("tenant1".to_string(), policy.clone());
        let retrieved = mgr.get_policy("tenant1").unwrap();
        assert_eq!(retrieved.class, QosClass::Interactive);
        assert_eq!(retrieved.max_iops, 10000);
    }

    #[test]
    fn test_remove_policy() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Batch,
            max_iops: 1000,
            max_bandwidth_bytes_sec: 100_000_000,
            max_metadata_ops_sec: 5000,
            priority_weight: 50,
        };

        mgr.set_policy("tenant1".to_string(), policy);
        assert!(mgr.remove_policy("tenant1"));
        assert!(mgr.get_policy("tenant1").is_none());
        assert!(!mgr.remove_policy("tenant1"));
    }

    #[test]
    fn test_rate_limit_allows_within_limit() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Interactive,
            max_iops: 1000,
            max_bandwidth_bytes_sec: 1_000_000_000,
            max_metadata_ops_sec: 1000,
            priority_weight: 100,
        };

        mgr.set_policy("tenant1".to_string(), policy);
        assert!(mgr.check_rate_limit("tenant1"));
    }

    #[test]
    fn test_rate_limit_denies_when_no_tokens() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Interactive,
            max_iops: 1,
            max_bandwidth_bytes_sec: 1_000_000_000,
            max_metadata_ops_sec: 1,
            priority_weight: 100,
        };

        mgr.set_policy("tenant1".to_string(), policy);
        let allowed = mgr.check_rate_limit("tenant1");
        assert!(allowed);
    }

    #[test]
    fn test_bandwidth_check() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Batch,
            max_iops: 1000,
            max_bandwidth_bytes_sec: 1_000_000,
            max_metadata_ops_sec: 5000,
            priority_weight: 50,
        };

        mgr.set_policy("tenant1".to_string(), policy);
        assert!(mgr.check_bandwidth("tenant1", 500_000));
        assert!(!mgr.check_bandwidth("tenant1", 2_000_000));
    }

    #[test]
    fn test_bandwidth_no_limit_for_unknown_tenant() {
        let mgr = QosManager::new();
        assert!(mgr.check_bandwidth("unknown", 1_000_000_000));
    }

    #[test]
    fn test_priority_ordering() {
        let mgr = QosManager::new();
        mgr.set_policy(
            "tenant1".to_string(),
            QosPolicy {
                class: QosClass::Background,
                max_iops: 100,
                max_bandwidth_bytes_sec: 10_000_000,
                max_metadata_ops_sec: 500,
                priority_weight: 10,
            },
        );
        mgr.set_policy(
            "tenant2".to_string(),
            QosPolicy {
                class: QosClass::Interactive,
                max_iops: 10000,
                max_bandwidth_bytes_sec: 1_000_000_000,
                max_metadata_ops_sec: 50000,
                priority_weight: 100,
            },
        );
        mgr.set_policy(
            "tenant3".to_string(),
            QosPolicy {
                class: QosClass::Batch,
                max_iops: 1000,
                max_bandwidth_bytes_sec: 100_000_000,
                max_metadata_ops_sec: 5000,
                priority_weight: 50,
            },
        );

        let tenants = mgr.tenants_by_priority();
        assert_eq!(tenants[0].1, QosClass::Interactive);
        assert_eq!(tenants[1].1, QosClass::Batch);
        assert_eq!(tenants[2].1, QosClass::Background);
    }

    #[test]
    fn test_default_class() {
        let mgr = QosManager::new();
        assert_eq!(mgr.get_class("unknown_tenant"), QosClass::System);
    }

    #[test]
    fn test_known_tenant_class() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Batch,
            max_iops: 1000,
            max_bandwidth_bytes_sec: 100_000_000,
            max_metadata_ops_sec: 5000,
            priority_weight: 50,
        };
        mgr.set_policy("tenant1".to_string(), policy);
        assert_eq!(mgr.get_class("tenant1"), QosClass::Batch);
    }

    #[test]
    fn test_policy_count() {
        let mgr = QosManager::new();
        assert_eq!(mgr.policy_count(), 0);

        mgr.set_policy(
            "t1".to_string(),
            QosPolicy {
                class: QosClass::Interactive,
                max_iops: 1000,
                max_bandwidth_bytes_sec: 100_000_000,
                max_metadata_ops_sec: 5000,
                priority_weight: 100,
            },
        );
        mgr.set_policy(
            "t2".to_string(),
            QosPolicy {
                class: QosClass::Batch,
                max_iops: 500,
                max_bandwidth_bytes_sec: 50_000_000,
                max_metadata_ops_sec: 2500,
                priority_weight: 50,
            },
        );
        assert_eq!(mgr.policy_count(), 2);
    }

    #[test]
    fn test_reset_buckets() {
        let mgr = QosManager::new();
        let policy = QosPolicy {
            class: QosClass::Interactive,
            max_iops: 5,
            max_bandwidth_bytes_sec: 1_000_000_000,
            max_metadata_ops_sec: 5,
            priority_weight: 100,
        };

        mgr.set_policy("tenant1".to_string(), policy);
        for _ in 0..5 {
            mgr.check_rate_limit("tenant1");
        }

        mgr.reset_buckets();
        assert!(mgr.check_rate_limit("tenant1"));
    }

    #[test]
    fn test_unlimited_tenant() {
        let mgr = QosManager::new();
        assert!(mgr.check_rate_limit("unlimited_tenant"));
        assert!(mgr.check_bandwidth("unlimited_tenant", u64::MAX));
    }
}
