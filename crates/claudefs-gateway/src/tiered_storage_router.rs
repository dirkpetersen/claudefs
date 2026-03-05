use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use thiserror::Error;
use crate::cross_protocol_consistency::Protocol;

/// Storage tier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    Hot,
    Warm,
    Cold,
}

/// Access pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    Sequential,
    Random,
    Streaming,
    Unknown,
}

/// Tier hint
#[derive(Debug, Clone)]
pub struct TierHint {
    pub tier: StorageTier,
    pub reason: String,
    pub confidence: f64,
}

/// Object tier metadata
#[derive(Debug, Clone)]
pub struct ObjectTierMetadata {
    pub inode_id: u64,
    pub object_key: String,
    pub current_tier: StorageTier,
    pub access_pattern: AccessPattern,
    pub last_access_ns: u64,
    pub access_count: u64,
    pub size_bytes: u64,
    pub promoted_at: Option<u64>,
    pub demoted_at: Option<u64>,
}

/// Tiering policy
#[derive(Debug, Clone)]
pub struct TieringPolicy {
    pub promotion_threshold: u64,
    pub demotion_threshold_ms: u64,
    pub prefetch_distance_kb: u64,
    pub cold_tier_cost_us: u64,
}

impl Default for TieringPolicy {
    fn default() -> Self {
        Self {
            promotion_threshold: 10,
            demotion_threshold_ms: 3600000,
            prefetch_distance_kb: 256,
            cold_tier_cost_us: 50000,
        }
    }
}

/// Access record for pattern detection
#[derive(Debug, Clone)]
pub struct AccessRecord {
    pub inode_id: u64,
    pub offset: u64,
    pub size: u64,
    pub timestamp_ns: u64,
    pub source: Protocol,
}

/// Tiering metrics
#[derive(Debug, Clone, Default)]
pub struct TieringMetrics {
    pub hot_tier_reads: u64,
    pub cold_tier_reads: u64,
    pub prefetch_hits: u64,
    pub prefetch_misses: u64,
    pub promotions: u64,
    pub demotions: u64,
    pub tier_change_latency_us: Vec<u64>,
}

/// Tiering errors
#[derive(Error, Debug)]
pub enum TieringError {
    #[error("invalid tier")]
    InvalidTier,
    #[error("promotion failed")]
    PromotionFailed,
    #[error("demotion failed")]
    DemotionFailed,
    #[error("object not found")]
    ObjectNotFound,
}

/// Tiering router
pub struct TieringRouter {
    object_metadata: Arc<DashMap<u64, ObjectTierMetadata>>,
    policy: Arc<TieringPolicy>,
    access_trace: Arc<std::sync::Mutex<VecDeque<AccessRecord>>>,
    metrics: Arc<std::sync::Mutex<TieringMetrics>>,
    max_trace_size: usize,
}

impl TieringRouter {
    pub fn new(policy: TieringPolicy) -> Self {
        Self {
            object_metadata: Arc::new(DashMap::new()),
            policy: Arc::new(policy),
            access_trace: Arc::new(tokio::sync::RwLock::new(VecDeque::new())),
            metrics: Arc::new(tokio::sync::RwLock::new(TieringMetrics::default())),
            max_trace_size: 10000,
        }
    }

    pub async fn record_access(
        &self,
        inode_id: u64,
        offset: u64,
        size: u64,
        protocol: Protocol,
    ) -> Result<AccessRecord, TieringError> {
        let now_ns = current_time_ns();

        let record = AccessRecord {
            inode_id,
            offset,
            size,
            timestamp_ns: now_ns,
            source: protocol,
        };

        let mut trace = self.access_trace.lock();
        trace.push_back(record.clone());
        if trace.len() > self.max_trace_size {
            trace.pop_front();
        }

        // Update object metadata access count
        self.object_metadata
            .entry(inode_id)
            .or_insert_with(|| ObjectTierMetadata {
                inode_id,
                object_key: format!("obj_{}", inode_id),
                current_tier: StorageTier::Hot,
                access_pattern: AccessPattern::Unknown,
                last_access_ns: now_ns,
                access_count: 1,
                size_bytes: size,
                promoted_at: None,
                demoted_at: None,
            })
            .last_access_ns = now_ns;

        if let Some(mut obj) = self.object_metadata.get_mut(&inode_id) {
            obj.access_count += 1;
        }

        Ok(record)
    }

    pub fn detect_access_pattern(&self, inode_id: u64) -> AccessPattern {
        let trace = self.access_trace.lock();
        let accesses: Vec<_> = trace
            .iter()
            .filter(|a| a.inode_id == inode_id)
            .collect();

        if accesses.len() < 2 {
            return AccessPattern::Unknown;
        }

        let mut sequential_count = 0;
        for window in accesses.windows(2) {
            if let [a, b] = window {
                if b.offset > a.offset && (b.offset - a.offset) <= 1_000_000 {
                    sequential_count += 1;
                }
            }
        }

        let seq_ratio = sequential_count as f64 / (accesses.len() - 1) as f64;
        if seq_ratio > 0.7 {
            AccessPattern::Sequential
        } else if seq_ratio < 0.3 {
            AccessPattern::Random
        } else {
            AccessPattern::Streaming
        }
    }

    pub fn get_tier_hint(&self, inode_id: u64) -> TierHint {
        let pattern = self.detect_access_pattern(inode_id);
        let metadata = self.object_metadata.get(&inode_id);

        if let Some(obj) = metadata {
            let confidence = (obj.access_count as f64 / 100.0).min(1.0);
            let reason = match pattern {
                AccessPattern::Sequential => "sequential_pattern".to_string(),
                AccessPattern::Random => "random_pattern".to_string(),
                AccessPattern::Streaming => "streaming_pattern".to_string(),
                AccessPattern::Unknown => "insufficient_data".to_string(),
            };

            let tier = if obj.access_count > self.policy.promotion_threshold {
                StorageTier::Hot
            } else {
                StorageTier::Warm
            };

            TierHint { tier, reason, confidence }
        } else {
            TierHint {
                tier: StorageTier::Warm,
                reason: "new_object".to_string(),
                confidence: 0.5,
            }
        }
    }

    pub async fn promote_to_hot(&self, inode_id: u64) -> Result<(), TieringError> {
        if let Some(mut obj) = self.object_metadata.get_mut(&inode_id) {
            obj.current_tier = StorageTier::Hot;
            obj.promoted_at = Some(current_time_ns());

            let mut metrics = self.metrics.lock();
            metrics.promotions += 1;
            Ok(())
        } else {
            Err(TieringError::ObjectNotFound)
        }
    }

    pub async fn demote_to_cold(&self, inode_id: u64) -> Result<(), TieringError> {
        if let Some(mut obj) = self.object_metadata.get_mut(&inode_id) {
            obj.current_tier = StorageTier::Cold;
            obj.demoted_at = Some(current_time_ns());

            let mut metrics = self.metrics.lock();
            metrics.demotions += 1;
            Ok(())
        } else {
            Err(TieringError::ObjectNotFound)
        }
    }

    pub fn compute_prefetch_list(&self, inode_id: u64, current_offset: u64) -> Vec<(u64, u64)> {
        let pattern = self.detect_access_pattern(inode_id);

        match pattern {
            AccessPattern::Sequential => {
                let prefetch_bytes = self.policy.prefetch_distance_kb * 1024;
                vec![
                    (current_offset + prefetch_bytes, prefetch_bytes),
                    (current_offset + 2 * prefetch_bytes, prefetch_bytes),
                ]
            }
            _ => vec![],
        }
    }

    pub fn current_tier(&self, inode_id: u64) -> Option<StorageTier> {
        self.object_metadata.get(&inode_id).map(|obj| obj.current_tier)
    }

    pub fn metrics(&self) -> TieringMetrics {
        self.metrics.lock().clone()
    }
}

impl Default for TieringRouter {
    fn default() -> Self {
        Self::new(TieringPolicy::default())
    }
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_access() {
        let router = TieringRouter::default();
        let record = router
            .record_access(100, 0, 4096, Protocol::NFS)
            .await
            .unwrap();

        assert_eq!(record.inode_id, 100);
        assert_eq!(record.offset, 0);
    }

    #[tokio::test]
    async fn test_get_tier_hint() {
        let router = TieringRouter::default();
        let _ = router
            .record_access(100, 0, 4096, Protocol::NFS)
            .await
            .unwrap();

        let hint = router.get_tier_hint(100);
        assert!(hint.confidence >= 0.0 && hint.confidence <= 1.0);
    }

    #[tokio::test]
    async fn test_promote_to_hot() {
        let router = TieringRouter::default();
        let _ = router
            .record_access(100, 0, 4096, Protocol::NFS)
            .await
            .unwrap();

        router.promote_to_hot(100).await.unwrap();
        assert_eq!(router.current_tier(100), Some(StorageTier::Hot));
    }

    #[tokio::test]
    async fn test_demote_to_cold() {
        let router = TieringRouter::default();
        let _ = router
            .record_access(100, 0, 4096, Protocol::NFS)
            .await
            .unwrap();

        router.demote_to_cold(100).await.unwrap();
        assert_eq!(router.current_tier(100), Some(StorageTier::Cold));
    }

    #[tokio::test]
    async fn test_sequential_prefetch() {
        let router = TieringRouter::default();
        for i in 0..10 {
            let _ = router
                .record_access(100, i * 4096, 4096, Protocol::NFS)
                .await
                .unwrap();
        }

        let prefetch = router.compute_prefetch_list(100, 0);
        assert!(!prefetch.is_empty());
    }

    #[tokio::test]
    async fn test_metrics() {
        let router = TieringRouter::default();
        let _ = router
            .record_access(100, 0, 4096, Protocol::NFS)
            .await
            .unwrap();
        let _ = router.promote_to_hot(100).await;

        let metrics = router.metrics();
        assert_eq!(metrics.promotions, 1);
    }
}
