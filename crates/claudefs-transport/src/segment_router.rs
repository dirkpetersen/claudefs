//! EC stripe-aware segment routing for distributed filesystem.
//!
//! Computes which cluster nodes hold each EC stripe for a given segment. Implements the
//! data placement strategy from D1 (4+2 Reed-Solomon EC) and D8 (metadata-local primary,
//! distributed EC stripes). Used by A5 (FUSE client) for parallel reads and by A1 (storage)
//! for write distribution.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// EC stripe configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcConfig {
    /// 4 data + 2 parity (requires 6+ nodes). Default for large clusters.
    FourPlusTwo,
    /// 2 data + 1 parity (requires 3+ nodes). For small clusters.
    TwoPlusOne,
}

impl EcConfig {
    /// Number of data shards.
    pub fn data_shards(&self) -> usize {
        match self {
            EcConfig::FourPlusTwo => 4,
            EcConfig::TwoPlusOne => 2,
        }
    }

    /// Number of parity shards.
    pub fn parity_shards(&self) -> usize {
        match self {
            EcConfig::FourPlusTwo => 2,
            EcConfig::TwoPlusOne => 1,
        }
    }

    /// Total shards (data + parity).
    pub fn total_shards(&self) -> usize {
        self.data_shards() + self.parity_shards()
    }

    /// Minimum nodes required.
    pub fn min_nodes(&self) -> usize {
        self.total_shards()
    }
}

impl Default for EcConfig {
    fn default() -> Self {
        EcConfig::FourPlusTwo
    }
}

/// A segment identifier (64-bit, derived from segment's BLAKE3 hash prefix or offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(pub u64);

/// A stripe assignment: which node holds which shard index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripeAssignment {
    /// Node identifier.
    pub node_id: [u8; 16],
    /// Shard index (0..data_shards for data, data_shards..total_shards for parity).
    pub shard_index: usize,
    /// Whether this is a parity shard.
    pub is_parity: bool,
}

/// Result of routing a segment to EC stripe nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentPlacement {
    /// Segment identifier.
    pub segment_id: SegmentId,
    /// EC configuration used.
    pub ec_config: EcConfig,
    /// The stripe assignments, one per shard.
    pub stripes: Vec<StripeAssignment>,
    /// The primary node (metadata-owning node, from consistent hash of segment_id).
    pub primary_node: [u8; 16],
}

impl SegmentPlacement {
    /// Get the node for a specific shard index.
    pub fn node_for_shard(&self, shard_index: usize) -> Option<&StripeAssignment> {
        self.stripes.get(shard_index)
    }

    /// Get all data shard assignments.
    pub fn data_stripes(&self) -> Vec<&StripeAssignment> {
        self.stripes.iter().filter(|s| !s.is_parity).collect()
    }

    /// Get all parity shard assignments.
    pub fn parity_stripes(&self) -> Vec<&StripeAssignment> {
        self.stripes.iter().filter(|s| s.is_parity).collect()
    }

    /// Whether this segment can be reconstructed if `failed_nodes` are unavailable.
    /// For 4+2: can reconstruct with up to 2 failures. For 2+1: up to 1 failure.
    pub fn can_reconstruct(&self, failed_nodes: &[[u8; 16]]) -> bool {
        let max_failures = self.ec_config.parity_shards();
        let unique_failed: std::collections::HashSet<[u8; 16]> =
            failed_nodes.iter().copied().collect();

        let failed_count = self
            .stripes
            .iter()
            .filter(|s| unique_failed.contains(&s.node_id))
            .count();

        failed_count <= max_failures
    }
}

/// Error type for segment routing.
#[derive(Debug, Error)]
pub enum SegmentRouterError {
    /// Not enough nodes available for the EC configuration.
    #[error("not enough nodes: need {needed} for {config:?}, have {available}")]
    InsufficientNodes {
        /// Required number of nodes.
        needed: usize,
        /// Available number of nodes.
        available: usize,
        /// EC configuration.
        config: EcConfig,
    },
    /// Shard index out of range.
    #[error("shard index {0} out of range")]
    ShardIndexOutOfRange(usize),
}

/// Configuration for the segment router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentRouterConfig {
    /// EC configuration to use.
    pub ec_config: EcConfig,
    /// Seed for deterministic placement (set to 0 in production, non-zero for testing).
    pub placement_seed: u64,
}

impl Default for SegmentRouterConfig {
    fn default() -> Self {
        SegmentRouterConfig {
            ec_config: EcConfig::FourPlusTwo,
            placement_seed: 0,
        }
    }
}

/// Statistics for segment routing.
pub struct SegmentRouterStats {
    pub placements_computed: AtomicU64,
    pub placement_errors: AtomicU64,
    pub shard_lookups: AtomicU64,
}

impl SegmentRouterStats {
    /// Create new segment router statistics.
    pub fn new() -> Self {
        SegmentRouterStats {
            placements_computed: AtomicU64::new(0),
            placement_errors: AtomicU64::new(0),
            shard_lookups: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self) -> SegmentRouterStatsSnapshot {
        SegmentRouterStatsSnapshot {
            placements_computed: self.placements_computed.load(Ordering::Relaxed),
            placement_errors: self.placement_errors.load(Ordering::Relaxed),
            shard_lookups: self.shard_lookups.load(Ordering::Relaxed),
        }
    }
}

impl Default for SegmentRouterStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of segment router statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct SegmentRouterStatsSnapshot {
    pub placements_computed: u64,
    pub placement_errors: u64,
    pub shard_lookups: u64,
}

/// Routes segments to EC stripe nodes using consistent hashing.
pub struct SegmentRouter {
    config: SegmentRouterConfig,
    stats: Arc<SegmentRouterStats>,
}

impl SegmentRouter {
    /// Create a new segment router.
    pub fn new(config: SegmentRouterConfig) -> Self {
        SegmentRouter {
            config,
            stats: Arc::new(SegmentRouterStats::new()),
        }
    }

    /// Compute the EC stripe placement for a segment across the given nodes.
    ///
    /// Algorithm: use a deterministic hash of (segment_id XOR shard_index) to pick
    /// from available nodes, without replacement (each shard on a different node).
    /// If nodes.len() < ec_config.total_shards(), returns InsufficientNodes.
    pub fn place_segment(
        &self,
        segment_id: SegmentId,
        nodes: &[[u8; 16]],
    ) -> Result<SegmentPlacement, SegmentRouterError> {
        let total_shards = self.config.ec_config.total_shards();

        if nodes.len() < total_shards {
            self.stats.placement_errors.fetch_add(1, Ordering::Relaxed);
            return Err(SegmentRouterError::InsufficientNodes {
                needed: total_shards,
                available: nodes.len(),
                config: self.config.ec_config,
            });
        }

        let mut selected_nodes = Vec::with_capacity(total_shards);
        let mut used_indices = std::collections::HashSet::new();

        for shard_index in 0..total_shards {
            let hash = self.compute_hash(segment_id.0, shard_index);

            let mut best_idx = 0;
            let mut best_score = u64::MAX;

            for (idx, _node) in nodes.iter().enumerate() {
                if used_indices.contains(&idx) {
                    continue;
                }

                let node_hash = self.node_hash(nodes[idx]);
                let score = hash.wrapping_add(node_hash);

                if score < best_score {
                    best_score = score;
                    best_idx = idx;
                }
            }

            used_indices.insert(best_idx);
            selected_nodes.push((shard_index, nodes[best_idx]));
        }

        let data_shards = self.config.ec_config.data_shards();

        let stripes: Vec<StripeAssignment> = selected_nodes
            .into_iter()
            .map(|(shard_index, node_id)| StripeAssignment {
                node_id,
                shard_index,
                is_parity: shard_index >= data_shards,
            })
            .collect();

        let primary_node = stripes.first().map(|s| s.node_id).unwrap_or([0u8; 16]);

        self.stats
            .placements_computed
            .fetch_add(1, Ordering::Relaxed);

        Ok(SegmentPlacement {
            segment_id,
            ec_config: self.config.ec_config,
            stripes,
            primary_node,
        })
    }

    /// Determine which shard a specific node holds for a given segment.
    /// Returns None if the node is not involved in this segment's placement.
    pub fn shard_for_node(
        &self,
        segment_id: SegmentId,
        node_id: &[u8; 16],
        nodes: &[[u8; 16]],
    ) -> Option<usize> {
        self.stats.shard_lookups.fetch_add(1, Ordering::Relaxed);

        let placement = self.place_segment(segment_id, nodes).ok()?;

        placement
            .stripes
            .iter()
            .find(|s| &s.node_id == node_id)
            .map(|s| s.shard_index)
    }

    /// List all segments that involve a given node, filtering from a segment list.
    pub fn segments_on_node(
        &self,
        node_id: &[u8; 16],
        segments: &[SegmentId],
        nodes: &[[u8; 16]],
    ) -> Vec<SegmentId> {
        segments
            .iter()
            .filter(|seg| self.shard_for_node(**seg, node_id, nodes).is_some())
            .copied()
            .collect()
    }

    /// Stats reference.
    pub fn stats(&self) -> Arc<SegmentRouterStats> {
        self.stats.clone()
    }

    fn compute_hash(&self, segment_id: u64, shard_index: usize) -> u64 {
        let combined = segment_id ^ (shard_index as u64) ^ self.config.placement_seed;

        fnv1a_hash(&combined.to_le_bytes())
    }

    fn node_hash(&self, node: [u8; 16]) -> u64 {
        fnv1a_hash(&node)
    }
}

fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

impl Default for SegmentRouter {
    fn default() -> Self {
        Self::new(SegmentRouterConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn make_nodes(count: usize) -> Vec<[u8; 16]> {
        (0..count).map(|i| make_node_id(i as u8)).collect()
    }

    #[test]
    fn test_ec_config_four_plus_two() {
        let config = EcConfig::FourPlusTwo;
        assert_eq!(config.data_shards(), 4);
        assert_eq!(config.parity_shards(), 2);
        assert_eq!(config.total_shards(), 6);
        assert_eq!(config.min_nodes(), 6);
    }

    #[test]
    fn test_ec_config_two_plus_one() {
        let config = EcConfig::TwoPlusOne;
        assert_eq!(config.data_shards(), 2);
        assert_eq!(config.parity_shards(), 1);
        assert_eq!(config.total_shards(), 3);
        assert_eq!(config.min_nodes(), 3);
    }

    #[test]
    fn test_place_segment_success() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        assert_eq!(placement.stripes.len(), 6);
        assert_eq!(placement.ec_config, EcConfig::FourPlusTwo);
    }

    #[test]
    fn test_place_segment_all_different_nodes() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let unique_nodes: std::collections::HashSet<[u8; 16]> =
            placement.stripes.iter().map(|s| s.node_id).collect();

        assert_eq!(unique_nodes.len(), 6);
    }

    #[test]
    fn test_place_segment_insufficient_nodes() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(5);

        let result = router.place_segment(SegmentId(123), &nodes);

        assert!(matches!(
            result,
            Err(SegmentRouterError::InsufficientNodes {
                needed: 6,
                available: 5,
                ..
            })
        ));
    }

    #[test]
    fn test_place_segment_two_plus_one() {
        let config = SegmentRouterConfig {
            ec_config: EcConfig::TwoPlusOne,
            placement_seed: 0,
        };
        let router = SegmentRouter::new(config);
        let nodes = make_nodes(3);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        assert_eq!(placement.stripes.len(), 3);
    }

    #[test]
    fn test_place_segment_deterministic() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let p1 = router.place_segment(SegmentId(123), &nodes).unwrap();
        let p2 = router.place_segment(SegmentId(123), &nodes).unwrap();

        for (s1, s2) in p1.stripes.iter().zip(p2.stripes.iter()) {
            assert_eq!(s1.node_id, s2.node_id);
            assert_eq!(s1.shard_index, s2.shard_index);
        }
    }

    #[test]
    fn test_place_segment_different_segments() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let p1 = router.place_segment(SegmentId(100), &nodes).unwrap();
        let p2 = router.place_segment(SegmentId(200), &nodes).unwrap();

        let nodes1: Vec<[u8; 16]> = p1.stripes.iter().map(|s| s.node_id).collect();
        let nodes2: Vec<[u8; 16]> = p2.stripes.iter().map(|s| s.node_id).collect();

        assert_ne!(nodes1, nodes2);
    }

    #[test]
    fn test_data_vs_parity_stripes() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let data = placement.data_stripes();
        let parity = placement.parity_stripes();

        assert_eq!(data.len(), 4);
        assert_eq!(parity.len(), 2);

        for s in &data {
            assert!(!s.is_parity);
            assert!(s.shard_index < 4);
        }

        for s in &parity {
            assert!(s.is_parity);
            assert!(s.shard_index >= 4);
        }
    }

    #[test]
    fn test_node_for_shard() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let shard0 = placement.node_for_shard(0).unwrap();
        assert_eq!(shard0.shard_index, 0);
        assert!(!shard0.is_parity);

        let shard5 = placement.node_for_shard(5).unwrap();
        assert_eq!(shard5.shard_index, 5);
        assert!(shard5.is_parity);

        assert!(placement.node_for_shard(6).is_none());
    }

    #[test]
    fn test_can_reconstruct_zero_failures() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        assert!(placement.can_reconstruct(&[]));
    }

    #[test]
    fn test_can_reconstruct_two_failures() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let failed = vec![nodes[0], nodes[1]];
        assert!(placement.can_reconstruct(&failed));
    }

    #[test]
    fn test_can_reconstruct_three_failures() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let failed = vec![nodes[0], nodes[1], nodes[2]];
        assert!(!placement.can_reconstruct(&failed));
    }

    #[test]
    fn test_shard_for_node() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        for stripe in &placement.stripes {
            let shard = router.shard_for_node(SegmentId(123), &stripe.node_id, &nodes);
            assert_eq!(shard, Some(stripe.shard_index));
        }

        let foreign_node = make_node_id(99);
        let shard = router.shard_for_node(SegmentId(123), &foreign_node, &nodes);
        assert!(shard.is_none());
    }

    #[test]
    fn test_segments_on_node() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let segments: Vec<SegmentId> = (0..10).map(SegmentId).collect();

        let involved = router.segments_on_node(&nodes[0], &segments, &nodes);

        assert!(!involved.is_empty());
        assert!(involved.len() <= segments.len());

        for seg in &involved {
            assert!(segments.contains(seg));
        }
    }

    #[test]
    fn test_stats_counts() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let _ = router.place_segment(SegmentId(1), &nodes);
        let _ = router.place_segment(SegmentId(2), &nodes);
        let _ = router.place_segment(SegmentId(3), &nodes);

        let stats = router.stats().snapshot();
        assert_eq!(stats.placements_computed, 3);
        assert_eq!(stats.placement_errors, 0);
    }

    #[test]
    fn test_stats_errors() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(3);

        let _ = router.place_segment(SegmentId(1), &nodes);

        let stats = router.stats().snapshot();
        assert_eq!(stats.placements_computed, 0);
        assert_eq!(stats.placement_errors, 1);
    }

    #[test]
    fn test_placement_seed_affects_selection() {
        let config1 = SegmentRouterConfig {
            ec_config: EcConfig::FourPlusTwo,
            placement_seed: 0,
        };
        let config2 = SegmentRouterConfig {
            ec_config: EcConfig::FourPlusTwo,
            placement_seed: 12345,
        };

        let router1 = SegmentRouter::new(config1);
        let router2 = SegmentRouter::new(config2);
        let nodes = make_nodes(6);

        let p1 = router1.place_segment(SegmentId(100), &nodes).unwrap();
        let p2 = router2.place_segment(SegmentId(100), &nodes).unwrap();

        let nodes1: Vec<[u8; 16]> = p1.stripes.iter().map(|s| s.node_id).collect();
        let nodes2: Vec<[u8; 16]> = p2.stripes.iter().map(|s| s.node_id).collect();

        assert_ne!(nodes1, nodes2);
    }

    #[test]
    fn test_primary_node() {
        let router = SegmentRouter::default();
        let nodes = make_nodes(6);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        assert_eq!(placement.primary_node, placement.stripes[0].node_id);
    }

    #[test]
    fn test_two_plus_one_can_reconstruct_one_failure() {
        let config = SegmentRouterConfig {
            ec_config: EcConfig::TwoPlusOne,
            placement_seed: 0,
        };
        let router = SegmentRouter::new(config);
        let nodes = make_nodes(3);

        let placement = router.place_segment(SegmentId(123), &nodes).unwrap();

        let failed = vec![nodes[0]];
        assert!(placement.can_reconstruct(&failed));

        let failed2 = vec![nodes[0], nodes[1]];
        assert!(!placement.can_reconstruct(&failed2));
    }

    #[test]
    fn test_segment_router_config_default() {
        let config = SegmentRouterConfig::default();
        assert_eq!(config.ec_config, EcConfig::FourPlusTwo);
        assert_eq!(config.placement_seed, 0);
    }
}
