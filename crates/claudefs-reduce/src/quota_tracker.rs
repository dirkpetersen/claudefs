//! Space quota tracking for namespaces (directories/tenants).
//!
//! Tracks both logical bytes (before reduction) and physical bytes
//! (after dedup+compression+encryption), enabling administrators
//! to understand actual flash consumption vs apparent file sizes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A namespace identifier (e.g., directory path hash, tenant ID).
pub type NamespaceId = u64;

/// Quota configuration for one namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// Maximum logical bytes (apparent file sizes). 0 = unlimited.
    pub max_logical_bytes: u64,
    /// Maximum physical bytes (actual flash usage after reduction). 0 = unlimited.
    pub max_physical_bytes: u64,
}

/// Current usage counters for a namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Logical bytes (sum of uncompressed file data written).
    pub logical_bytes: u64,
    /// Physical bytes (actual flash consumption after dedup+compress).
    pub physical_bytes: u64,
    /// Number of chunks written (unique, post-dedup).
    pub chunk_count: u64,
    /// Number of dedup hits (chunks skipped because already stored).
    pub dedup_hits: u64,
}

impl QuotaUsage {
    /// Compute the reduction ratio: logical / physical.
    /// Returns 1.0 if physical == 0.
    pub fn reduction_ratio(&self) -> f64 {
        if self.physical_bytes == 0 {
            return 1.0;
        }
        self.logical_bytes as f64 / self.physical_bytes as f64
    }
}

/// Violation kinds returned by quota checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaViolation {
    /// Logical quota exceeded.
    LogicalQuotaExceeded {
        /// Namespace that exceeded.
        namespace: NamespaceId,
        /// Current usage + proposed write.
        current: u64,
        /// Configured limit.
        limit: u64,
    },
    /// Physical quota exceeded.
    PhysicalQuotaExceeded {
        /// Namespace that exceeded.
        namespace: NamespaceId,
        /// Current usage + proposed write.
        current: u64,
        /// Configured limit.
        limit: u64,
    },
}

/// Central quota tracker for all namespaces.
pub struct QuotaTracker {
    configs: HashMap<NamespaceId, QuotaConfig>,
    usages: HashMap<NamespaceId, QuotaUsage>,
}

impl Default for QuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            usages: HashMap::new(),
        }
    }

    /// Set (or replace) the quota config for a namespace.
    pub fn set_quota(&mut self, namespace: NamespaceId, config: QuotaConfig) {
        self.configs.insert(namespace, config);
    }

    /// Get the quota config for a namespace, if any.
    pub fn get_quota(&self, namespace: NamespaceId) -> Option<&QuotaConfig> {
        self.configs.get(&namespace)
    }

    /// Remove a namespace and its usage counters.
    pub fn remove_namespace(&mut self, namespace: NamespaceId) {
        self.configs.remove(&namespace);
        self.usages.remove(&namespace);
    }

    /// Record a new (unique) chunk written to a namespace.
    ///
    /// `logical_bytes`: raw bytes contributed by this write.
    /// `physical_bytes`: actual flash bytes used after reduction.
    pub fn record_write(
        &mut self,
        namespace: NamespaceId,
        logical_bytes: u64,
        physical_bytes: u64,
    ) {
        let usage = self.usages.entry(namespace).or_default();
        usage.logical_bytes += logical_bytes;
        usage.physical_bytes += physical_bytes;
        usage.chunk_count += 1;
    }

    /// Record a dedup hit — chunk was already stored, zero new physical bytes.
    ///
    /// `logical_bytes` is the apparent size still counted toward logical quota.
    pub fn record_dedup_hit(&mut self, namespace: NamespaceId, logical_bytes: u64) {
        let usage = self.usages.entry(namespace).or_default();
        usage.logical_bytes += logical_bytes;
        usage.dedup_hits += 1;
    }

    /// Record a delete — decrements logical and physical byte counts.
    /// Will not go below zero.
    pub fn record_delete(
        &mut self,
        namespace: NamespaceId,
        logical_bytes: u64,
        physical_bytes: u64,
    ) {
        let usage = self.usages.entry(namespace).or_default();
        usage.logical_bytes = usage.logical_bytes.saturating_sub(logical_bytes);
        usage.physical_bytes = usage.physical_bytes.saturating_sub(physical_bytes);
    }

    /// Check if the given proposed write would violate quota.
    ///
    /// Returns `Ok(())` if within limits, `Err(QuotaViolation)` if not.
    /// Does NOT mutate state — call `record_write` separately.
    pub fn check_write(
        &self,
        namespace: NamespaceId,
        logical_bytes: u64,
        physical_bytes: u64,
    ) -> Result<(), QuotaViolation> {
        let current = self.usage(namespace);

        let config = self.configs.get(&namespace);

        if let Some(cfg) = config {
            if cfg.max_logical_bytes > 0 {
                let new_logical = current.logical_bytes + logical_bytes;
                if new_logical > cfg.max_logical_bytes {
                    return Err(QuotaViolation::LogicalQuotaExceeded {
                        namespace,
                        current: new_logical,
                        limit: cfg.max_logical_bytes,
                    });
                }
            }

            if cfg.max_physical_bytes > 0 {
                let new_physical = current.physical_bytes + physical_bytes;
                if new_physical > cfg.max_physical_bytes {
                    return Err(QuotaViolation::PhysicalQuotaExceeded {
                        namespace,
                        current: new_physical,
                        limit: cfg.max_physical_bytes,
                    });
                }
            }
        }

        Ok(())
    }

    /// Get current usage for a namespace (returns default zeroes if not tracked).
    pub fn usage(&self, namespace: NamespaceId) -> QuotaUsage {
        self.usages.get(&namespace).cloned().unwrap_or_default()
    }

    /// List all namespaces with any recorded usage.
    pub fn namespaces(&self) -> Vec<NamespaceId> {
        let mut ns: Vec<NamespaceId> = self.usages.keys().copied().collect();
        ns.sort();
        ns
    }

    /// Compute total usage across all namespaces.
    pub fn total_usage(&self) -> QuotaUsage {
        let mut total = QuotaUsage::default();
        for usage in self.usages.values() {
            total.logical_bytes += usage.logical_bytes;
            total.physical_bytes += usage.physical_bytes;
            total.chunk_count += usage.chunk_count;
            total.dedup_hits += usage.dedup_hits;
        }
        total
    }

    /// Reset usage counters for a namespace (keep quota config).
    pub fn reset_usage(&mut self, namespace: NamespaceId) {
        if let Some(usage) = self.usages.get_mut(&namespace) {
            *usage = QuotaUsage::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_empty() {
        let tracker = QuotaTracker::new();
        assert!(tracker.namespaces().is_empty());
    }

    #[test]
    fn test_set_get_quota() {
        let mut tracker = QuotaTracker::new();
        let config = QuotaConfig {
            max_logical_bytes: 1000,
            max_physical_bytes: 500,
        };
        tracker.set_quota(42, config.clone());

        let retrieved = tracker.get_quota(42).expect("should have quota");
        assert_eq!(retrieved.max_logical_bytes, 1000);
        assert_eq!(retrieved.max_physical_bytes, 500);
    }

    #[test]
    fn test_default_quota_unlimited() {
        let config = QuotaConfig::default();
        assert_eq!(config.max_logical_bytes, 0);
        assert_eq!(config.max_physical_bytes, 0);
    }

    #[test]
    fn test_record_write_increments_logical() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 100, 50);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 100);
    }

    #[test]
    fn test_record_write_increments_physical() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 100, 50);

        let usage = tracker.usage(1);
        assert_eq!(usage.physical_bytes, 50);
    }

    #[test]
    fn test_record_write_increments_chunk_count() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 100, 50);
        tracker.record_write(1, 100, 50);

        let usage = tracker.usage(1);
        assert_eq!(usage.chunk_count, 2);
    }

    #[test]
    fn test_record_dedup_hit() {
        let mut tracker = QuotaTracker::new();
        tracker.record_dedup_hit(1, 100);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 100);
        assert_eq!(usage.physical_bytes, 0);
        assert_eq!(usage.dedup_hits, 1);
    }

    #[test]
    fn test_record_delete_decrements() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 200, 100);
        tracker.record_delete(1, 50, 25);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 150);
        assert_eq!(usage.physical_bytes, 75);
    }

    #[test]
    fn test_record_delete_no_underflow() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 50, 25);
        tracker.record_delete(1, 100, 50);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 0);
        assert_eq!(usage.physical_bytes, 0);
    }

    #[test]
    fn test_check_write_within_limits() {
        let tracker = QuotaTracker::new();

        let result = tracker.check_write(1, 100, 50);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_write_logical_exceeded() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 100,
                max_physical_bytes: 0,
            },
        );
        tracker.record_write(1, 80, 40);

        let result = tracker.check_write(1, 50, 25);
        assert!(result.is_err());
        match result {
            Err(QuotaViolation::LogicalQuotaExceeded {
                namespace,
                current,
                limit,
            }) => {
                assert_eq!(namespace, 1);
                assert_eq!(current, 130);
                assert_eq!(limit, 100);
            }
            _ => panic!("expected LogicalQuotaExceeded"),
        }
    }

    #[test]
    fn test_check_write_physical_exceeded() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 0,
                max_physical_bytes: 100,
            },
        );
        tracker.record_write(1, 80, 80);

        let result = tracker.check_write(1, 50, 50);
        assert!(result.is_err());
        match result {
            Err(QuotaViolation::PhysicalQuotaExceeded {
                namespace,
                current,
                limit,
            }) => {
                assert_eq!(namespace, 1);
                assert_eq!(current, 130);
                assert_eq!(limit, 100);
            }
            _ => panic!("expected PhysicalQuotaExceeded"),
        }
    }

    #[test]
    fn test_check_write_unlimited_quota() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(1, QuotaConfig::default());
        tracker.record_write(1, 1_000_000, 500_000);

        let result = tracker.check_write(1, 1_000_000, 500_000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_namespace() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(1, QuotaConfig::default());
        tracker.record_write(1, 100, 50);

        tracker.remove_namespace(1);

        assert!(tracker.get_quota(1).is_none());
        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 0);
    }

    #[test]
    fn test_total_usage() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(1, 100, 50);
        tracker.record_write(2, 200, 100);
        tracker.record_dedup_hit(1, 50);

        let total = tracker.total_usage();
        assert_eq!(total.logical_bytes, 350);
        assert_eq!(total.physical_bytes, 150);
        assert_eq!(total.chunk_count, 2);
        assert_eq!(total.dedup_hits, 1);
    }

    #[test]
    fn test_reset_usage() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 1000,
                ..Default::default()
            },
        );
        tracker.record_write(1, 100, 50);

        tracker.reset_usage(1);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 0);
        assert_eq!(usage.physical_bytes, 0);

        let quota = tracker.get_quota(1);
        assert!(quota.is_some());
        assert_eq!(quota.unwrap().max_logical_bytes, 1000);
    }

    #[test]
    fn test_reduction_ratio_normal() {
        let usage = QuotaUsage {
            logical_bytes: 100,
            physical_bytes: 25,
            chunk_count: 1,
            dedup_hits: 0,
        };

        let ratio = usage.reduction_ratio();
        assert!((ratio - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_reduction_ratio_zero_physical() {
        let usage = QuotaUsage {
            logical_bytes: 100,
            physical_bytes: 0,
            chunk_count: 0,
            dedup_hits: 0,
        };

        let ratio = usage.reduction_ratio();
        assert!((ratio - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_namespaces_list() {
        let mut tracker = QuotaTracker::new();
        tracker.record_write(3, 100, 50);
        tracker.record_write(1, 100, 50);
        tracker.record_write(2, 100, 50);

        let ns = tracker.namespaces();
        assert_eq!(ns, vec![1, 2, 3]);
    }

    #[test]
    fn test_quota_tracker_multiple_namespaces_isolated() {
        let mut tracker = QuotaTracker::new();

        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 100,
                max_physical_bytes: 50,
            },
        );
        tracker.set_quota(
            2,
            QuotaConfig {
                max_logical_bytes: 200,
                max_physical_bytes: 100,
            },
        );

        tracker.record_write(1, 50, 25);
        tracker.record_write(2, 100, 50);

        let usage1 = tracker.usage(1);
        let usage2 = tracker.usage(2);

        assert_eq!(usage1.logical_bytes, 50);
        assert_eq!(usage2.logical_bytes, 100);

        // Namespace 1 should exceed with another 60 bytes, namespace 2 should not
        assert!(tracker.check_write(1, 60, 30).is_err());
        assert!(tracker.check_write(2, 60, 30).is_ok());
    }

    #[test]
    fn test_quota_tracker_near_limit() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 100,
                max_physical_bytes: 0,
            },
        );

        tracker.record_write(1, 90, 45);

        // 90 + 9 = 99 should be OK
        assert!(tracker.check_write(1, 9, 5).is_ok());

        // 90 + 11 = 101 should fail
        assert!(tracker.check_write(1, 11, 5).is_err());
    }

    #[test]
    fn test_quota_usage_percentage() {
        let usage = QuotaUsage {
            logical_bytes: 500,
            physical_bytes: 125,
            chunk_count: 10,
            dedup_hits: 2,
        };

        // Reduction ratio = 500 / 125 = 4.0
        let ratio = usage.reduction_ratio();
        assert!((ratio - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_quota_violation_details() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            42,
            QuotaConfig {
                max_logical_bytes: 100,
                max_physical_bytes: 50,
            },
        );
        tracker.record_write(42, 80, 40);

        let result = tracker.check_write(42, 50, 25);
        match result {
            Err(QuotaViolation::LogicalQuotaExceeded {
                namespace,
                current,
                limit,
            }) => {
                assert_eq!(namespace, 42);
                assert_eq!(current, 130);
                assert_eq!(limit, 100);
            }
            _ => panic!("expected LogicalQuotaExceeded"),
        }
    }

    #[test]
    fn test_namespace_id_equality() {
        let ns1: NamespaceId = 1;
        let ns2: NamespaceId = 1;
        let ns3: NamespaceId = 2;

        assert_eq!(ns1, ns2);
        assert_ne!(ns1, ns3);
    }

    #[test]
    fn test_quota_config_default_values() {
        let config = QuotaConfig::default();
        assert_eq!(config.max_logical_bytes, 0);
        assert_eq!(config.max_physical_bytes, 0);
    }

    #[test]
    fn test_quota_tracker_reset_usage() {
        let mut tracker = QuotaTracker::new();
        tracker.set_quota(
            1,
            QuotaConfig {
                max_logical_bytes: 1000,
                max_physical_bytes: 500,
            },
        );
        tracker.record_write(1, 200, 100);
        tracker.record_dedup_hit(1, 50);

        assert_eq!(tracker.usage(1).logical_bytes, 250);
        assert_eq!(tracker.usage(1).physical_bytes, 100);

        tracker.reset_usage(1);

        let usage = tracker.usage(1);
        assert_eq!(usage.logical_bytes, 0);
        assert_eq!(usage.physical_bytes, 0);
        assert_eq!(usage.chunk_count, 0);
        assert_eq!(usage.dedup_hits, 0);

        // Quota config should still exist
        assert!(tracker.get_quota(1).is_some());
    }

    #[test]
    fn test_quota_usage_clone() {
        let usage = QuotaUsage {
            logical_bytes: 100,
            physical_bytes: 50,
            chunk_count: 5,
            dedup_hits: 2,
        };
        let cloned = usage.clone();
        assert_eq!(cloned.logical_bytes, 100);
        assert_eq!(cloned.physical_bytes, 50);
    }

    #[test]
    fn test_quota_config_clone() {
        let config = QuotaConfig {
            max_logical_bytes: 1000,
            max_physical_bytes: 500,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_logical_bytes, 1000);
        assert_eq!(cloned.max_physical_bytes, 500);
    }

    #[test]
    fn test_quota_tracker_default() {
        let tracker = QuotaTracker::default();
        assert!(tracker.namespaces().is_empty());
    }
}
