//! Tiered storage routing (hot/warm/cold) with access pattern detection.
//!
//! This module tracks access patterns and routes reads based on storage tier,
//! managing promotion/demotion between tiers.

use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;

use crate::protocol::Protocol;

/// Storage tier classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StorageTier {
    /// NVMe: fast direct access
    Hot,
    /// Memory cache: medium speed
    Warm,
    /// S3: cold storage, fetch on demand
    Cold,
}

/// Detected access pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccessPattern {
    /// Monotonically increasing offsets
    Sequential,
    /// Random offset distribution
    Random,
    /// Large sequential reads
    Streaming,
    /// Insufficient data to classify
    Unknown,
}

/// Tier recommendation with confidence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TierHint {
    /// Recommended tier
    pub tier: StorageTier,
    /// Reason for recommendation
    pub reason: String,
    /// Confidence 0.0-1.0
    pub confidence: f64,
}

/// Object metadata for tiering decisions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ObjectTierMetadata {
    /// Inode ID
    pub inode_id: u64,
    /// S3 object key
    pub object_key: String,
    /// Current tier
    pub current_tier: StorageTier,
    /// Detected access pattern
    pub access_pattern: AccessPattern,
    /// Last access time (ms since epoch)
    pub last_access_ms: u64,
    /// Total number of accesses
    pub access_count: u64,
    /// Size in bytes
    pub size_bytes: u64,
}

/// Tiering policy configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TieringPolicy {
    /// Access count threshold to promote from cold to warm
    pub promotion_threshold: u64,
    /// Time in milliseconds to demote from warm to cold
    pub demotion_threshold_ms: u64,
    /// Prefetch distance in KB for sequential reads
    pub prefetch_distance_kb: u64,
    /// Estimated latency to cold tier in microseconds
    pub cold_tier_cost_us: u64,
}

/// Individual access record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccessRecord {
    /// Inode being accessed
    pub inode_id: u64,
    /// Byte offset in object
    pub offset: u64,
    /// Number of bytes accessed
    pub size: u64,
    /// Access time (ms since epoch)
    pub timestamp_ms: u64,
    /// Which protocol accessed it
    pub source: Protocol,
}

/// Tiering metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TieringMetrics {
    /// Reads from hot tier
    pub hot_tier_reads: u64,
    /// Reads from cold tier
    pub cold_tier_reads: u64,
    /// Prefetch list hits
    pub prefetch_hits: u64,
    /// Prefetch list misses
    pub prefetch_misses: u64,
    /// Objects promoted to hot
    pub promotions: u64,
    /// Objects demoted to cold
    pub demotions: u64,
}

/// Errors for tiering operations.
#[derive(Debug, Error)]
pub enum TieringError {
    /// Invalid storage tier
    #[error("Invalid storage tier")]
    InvalidTier,
    /// Promotion failed
    #[error("Tier promotion failed")]
    PromotionFailed,
    /// Demotion failed
    #[error("Tier demotion failed")]
    DemotionFailed,
    /// Object not found
    #[error("Object not found")]
    ObjectNotFound,
}

/// Tiered storage router.
pub struct TieringRouter {
    object_metadata: Arc<DashMap<u64, ObjectTierMetadata>>,
    policy: Arc<TieringPolicy>,
    access_trace: Arc<parking_lot::Mutex<VecDeque<AccessRecord>>>,
    metrics: Arc<parking_lot::Mutex<TieringMetrics>>,
}

impl TieringRouter {
    /// Create a new tiering router with policy.
    pub fn new(policy: TieringPolicy) -> Self {
        Self {
            object_metadata: Arc::new(DashMap::new()),
            policy: Arc::new(policy),
            access_trace: Arc::new(parking_lot::Mutex::new(VecDeque::with_capacity(10000))),
            metrics: Arc::new(parking_lot::Mutex::new(TieringMetrics {
                hot_tier_reads: 0,
                cold_tier_reads: 0,
                prefetch_hits: 0,
                prefetch_misses: 0,
                promotions: 0,
                demotions: 0,
            })),
        }
    }

    /// Record an access to an object.
    pub fn record_access(
        &self,
        inode_id: u64,
        offset: u64,
        size: u64,
        protocol: Protocol,
    ) -> Result<AccessRecord, TieringError> {
        let record = AccessRecord {
            inode_id,
            offset,
            size,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            source: protocol,
        };

        // Update access count
        self.object_metadata
            .entry(inode_id)
            .and_modify(|m| {
                m.access_count += 1;
                m.last_access_ms = record.timestamp_ms;
            })
            .or_insert_with(|| ObjectTierMetadata {
                inode_id,
                object_key: format!("obj-{}", inode_id),
                current_tier: StorageTier::Cold,
                access_pattern: AccessPattern::Unknown,
                last_access_ms: record.timestamp_ms,
                access_count: 1,
                size_bytes: size,
            });

        // Record in trace
        {
            let mut trace = self.access_trace.lock();
            trace.push_back(record.clone());
            if trace.len() > 10000 {
                trace.pop_front();
            }
        }

        Ok(record)
    }

    /// Detect access pattern for an object.
    pub fn detect_access_pattern(&self, inode_id: u64) -> AccessPattern {
        let trace = self.access_trace.lock();
        let mut last_offset: Option<u64> = None;
        let mut is_sequential = true;
        let mut count = 0;

        for record in trace.iter().rev().take(20) {
            if record.inode_id != inode_id {
                continue;
            }

            count += 1;
            if let Some(last) = last_offset {
                let diff = if record.offset > last {
                    record.offset - last
                } else if last > record.offset {
                    last - record.offset
                } else {
                    0
                };

                // Large jumps (>1MB) suggest random or streaming
                if diff > 1024 * 1024 {
                    is_sequential = false;
                }
            }
            last_offset = Some(record.offset);
        }

        match count {
            0..=2 => AccessPattern::Unknown,
            _ if is_sequential => AccessPattern::Sequential,
            _ => AccessPattern::Random,
        }
    }

    /// Get tier recommendation for an object.
    pub fn get_tier_hint(&self, inode_id: u64) -> TierHint {
        if let Some(metadata) = self.object_metadata.get(&inode_id) {
            let tier = match metadata.access_count {
                0..=10 => StorageTier::Cold,
                11..=100 => StorageTier::Warm,
                _ => StorageTier::Hot,
            };

            let reason = match tier {
                StorageTier::Hot => "frequent_access".to_string(),
                StorageTier::Warm => "moderate_access".to_string(),
                StorageTier::Cold => "infrequent_access".to_string(),
            };

            TierHint {
                tier,
                reason,
                confidence: 0.8,
            }
        } else {
            TierHint {
                tier: StorageTier::Cold,
                reason: "new_object".to_string(),
                confidence: 0.5,
            }
        }
    }

    /// Promote object to hot tier.
    pub fn promote_to_hot(&self, inode_id: u64) -> Result<(), TieringError> {
        self.object_metadata.alter(&inode_id, |_k, mut v| {
            v.current_tier = StorageTier::Hot;
            v
        });

        let mut metrics = self.metrics.lock();
        metrics.promotions += 1;

        Ok(())
    }

    /// Demote object to cold tier.
    pub fn demote_to_cold(&self, inode_id: u64) -> Result<(), TieringError> {
        self.object_metadata.alter(&inode_id, |_k, mut v| {
            v.current_tier = StorageTier::Cold;
            v
        });

        let mut metrics = self.metrics.lock();
        metrics.demotions += 1;

        Ok(())
    }

    /// Compute prefetch list for sequential reads.
    pub fn compute_prefetch_list(&self, inode_id: u64, current_offset: u64) -> Vec<(u64, u64)> {
        let mut prefetch_list = Vec::new();
        let prefetch_distance = self.policy.prefetch_distance_kb * 1024;

        // Simple linear prefetch for sequential patterns
        let offset_base = (current_offset / prefetch_distance) * prefetch_distance;
        for i in 0..4 {
            let offset = offset_base + (i * prefetch_distance);
            prefetch_list.push((offset, 64 * 1024)); // 64KB chunks
        }

        // Truncate if exceeds object size
        if let Some(metadata) = self.object_metadata.get(&inode_id) {
            if metadata.size_bytes > 0 {
                prefetch_list.retain(|(offset, _)| *offset < metadata.size_bytes);
            }
        }

        prefetch_list
    }

    /// Get current tier for an object.
    pub fn current_tier(&self, inode_id: u64) -> Option<StorageTier> {
        self.object_metadata.get(&inode_id).map(|m| m.current_tier)
    }

    /// Get current metrics.
    pub fn metrics(&self) -> TieringMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for TieringRouter {
    fn default() -> Self {
        Self::new(TieringPolicy {
            promotion_threshold: 50,
            demotion_threshold_ms: 86400000, // 1 day
            prefetch_distance_kb: 256,
            cold_tier_cost_us: 5000,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_sequential_accesses() {
        let router = TieringRouter::default();

        router
            .record_access(100, 0, 4096, Protocol::Nfs3)
            .expect("record 1");
        router
            .record_access(100, 4096, 4096, Protocol::Nfs3)
            .expect("record 2");
        router
            .record_access(100, 8192, 4096, Protocol::Nfs3)
            .expect("record 3");

        assert_eq!(router.metrics().hot_tier_reads, 0); // Metrics not auto-updated
    }

    #[test]
    fn test_detect_sequential_pattern() {
        let router = TieringRouter::default();

        for i in 0..5 {
            let offset = (i * 4096) as u64;
            let _ = router.record_access(100, offset, 4096, Protocol::Nfs3);
        }

        let pattern = router.detect_access_pattern(100);
        assert_eq!(pattern, AccessPattern::Sequential);
    }

    #[test]
    fn test_detect_random_pattern() {
        let router = TieringRouter::default();

        let offsets = vec![0, 100_000, 50_000, 200_000, 10_000];
        for offset in offsets {
            let _ = router.record_access(100, offset, 4096, Protocol::Nfs3);
        }

        let pattern = router.detect_access_pattern(100);
        assert_eq!(pattern, AccessPattern::Random);
    }

    #[test]
    fn test_get_tier_hint_new_object() {
        let router = TieringRouter::default();
        let hint = router.get_tier_hint(999);

        assert_eq!(hint.tier, StorageTier::Cold);
        assert_eq!(hint.reason, "new_object");
    }

    #[test]
    fn test_tier_hint_based_on_access_count() {
        let router = TieringRouter::default();

        // Record many accesses
        for i in 0..150 {
            let _ = router.record_access(100, (i * 4096) as u64, 4096, Protocol::Nfs3);
        }

        let hint = router.get_tier_hint(100);
        assert_eq!(hint.tier, StorageTier::Hot);
        assert_eq!(hint.reason, "frequent_access");
    }

    #[test]
    fn test_promote_to_hot() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 4096, Protocol::Nfs3);

        router.promote_to_hot(100).expect("promote succeeds");

        let tier = router.current_tier(100).expect("tier exists");
        assert_eq!(tier, StorageTier::Hot);

        let metrics = router.metrics();
        assert_eq!(metrics.promotions, 1);
    }

    #[test]
    fn test_demote_to_cold() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 4096, Protocol::Nfs3);

        router.promote_to_hot(100).expect("promote");
        router.demote_to_cold(100).expect("demote succeeds");

        let tier = router.current_tier(100).expect("tier exists");
        assert_eq!(tier, StorageTier::Cold);

        let metrics = router.metrics();
        assert_eq!(metrics.demotions, 1);
    }

    #[test]
    fn test_compute_prefetch_list() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 1024 * 1024, Protocol::Nfs3);

        let prefetch = router.compute_prefetch_list(100, 0);
        assert!(!prefetch.is_empty());
        assert!(prefetch[0].1 > 0); // Has size
    }

    #[test]
    fn test_unknown_pattern_with_few_accesses() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 4096, Protocol::Nfs3);
        let _ = router.record_access(100, 4096, 4096, Protocol::Nfs3);

        let pattern = router.detect_access_pattern(100);
        assert_eq!(pattern, AccessPattern::Unknown);
    }

    #[test]
    fn test_multiple_objects() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 4096, Protocol::Nfs3);
        let _ = router.record_access(200, 0, 4096, Protocol::Nfs3);

        assert!(router.current_tier(100).is_some());
        assert!(router.current_tier(200).is_some());
    }

    #[test]
    fn test_nonexistent_object_tier() {
        let router = TieringRouter::default();
        let tier = router.current_tier(999);

        assert!(tier.is_none());
    }

    #[test]
    fn test_metrics_tracking() {
        let router = TieringRouter::default();

        let _ = router.record_access(100, 0, 4096, Protocol::Nfs3);
        let _ = router.record_access(100, 4096, 4096, Protocol::Nfs3);

        let metrics = router.metrics();
        assert_eq!(metrics.hot_tier_reads, 0); // Not tracked in record_access
    }
}
