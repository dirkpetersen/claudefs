//! Read-only follower query routing for relaxed POSIX mode.
//!
//! In strict mode, all reads go to the Raft leader (linearizable).
//! In relaxed mode, reads can be served by followers with bounded staleness.
//! This reduces leader load for read-heavy workloads (docs/metadata.md).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::types::*;

/// Read consistency level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadConsistency {
    /// Linearizable reads via ReadIndex protocol (leader only).
    Linearizable,
    /// Bounded staleness: follower reads within max_staleness.
    BoundedStaleness,
    /// Read from any replica (fastest, possibly stale).
    ReadAny,
}

/// Configuration for follower read routing.
#[derive(Clone, Debug)]
pub struct FollowerReadConfig {
    /// Default consistency level for reads.
    pub default_consistency: ReadConsistency,
    /// Maximum acceptable staleness for bounded reads (in log entries).
    pub max_stale_entries: u64,
    /// Maximum acceptable staleness in time.
    pub max_stale_duration: Duration,
    /// How often to refresh follower status (heartbeat interval).
    pub status_refresh_interval: Duration,
}

impl Default for FollowerReadConfig {
    fn default() -> Self {
        Self {
            default_consistency: ReadConsistency::Linearizable,
            max_stale_entries: 100,
            max_stale_duration: Duration::from_secs(5),
            status_refresh_interval: Duration::from_millis(500),
        }
    }
}

/// Tracks a follower's replication status.
#[derive(Clone, Debug)]
pub struct FollowerStatus {
    /// Node ID of the follower.
    pub node_id: NodeId,
    /// Last known applied index on this follower.
    pub last_applied: LogIndex,
    /// When the status was last updated.
    pub last_updated: Instant,
    /// Whether the follower is considered healthy.
    pub healthy: bool,
    /// Round-trip latency estimate.
    pub latency_us: u64,
}

/// Result of a read routing decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadTarget {
    /// Route to the leader.
    Leader(NodeId),
    /// Route to a specific follower.
    Follower(NodeId),
    /// No suitable target found.
    Unavailable,
}

/// Routes read requests to appropriate replicas based on consistency requirements.
pub struct FollowerReadRouter {
    config: FollowerReadConfig,
    leader_id: Option<NodeId>,
    leader_commit_index: LogIndex,
    followers: HashMap<NodeId, FollowerStatus>,
}

impl FollowerReadRouter {
    /// Creates a new router with the given configuration.
    pub fn new(config: FollowerReadConfig) -> Self {
        Self {
            config,
            leader_id: None,
            leader_commit_index: LogIndex::new(0),
            followers: HashMap::new(),
        }
    }

    /// Sets the current leader.
    pub fn set_leader(&mut self, leader_id: NodeId, commit_index: LogIndex) {
        self.leader_id = Some(leader_id);
        self.leader_commit_index = commit_index;
    }

    /// Updates a follower's replication status.
    pub fn update_follower(&mut self, node_id: NodeId, last_applied: LogIndex, latency_us: u64) {
        let status = self
            .followers
            .entry(node_id)
            .or_insert_with(|| FollowerStatus {
                node_id,
                last_applied: LogIndex::new(0),
                last_updated: Instant::now(),
                healthy: true,
                latency_us: 0,
            });
        status.last_applied = last_applied;
        status.last_updated = Instant::now();
        status.healthy = true;
        status.latency_us = latency_us;
    }

    /// Marks a follower as unhealthy (e.g., missed heartbeats).
    pub fn mark_unhealthy(&mut self, node_id: &NodeId) {
        if let Some(status) = self.followers.get_mut(node_id) {
            status.healthy = false;
        }
    }

    /// Removes a follower from tracking.
    pub fn remove_follower(&mut self, node_id: &NodeId) {
        self.followers.remove(node_id);
    }

    /// Routes a read request based on the requested consistency level.
    pub fn route_read(&self, consistency: ReadConsistency) -> ReadTarget {
        match consistency {
            ReadConsistency::Linearizable => match self.leader_id {
                Some(id) => ReadTarget::Leader(id),
                None => ReadTarget::Unavailable,
            },
            ReadConsistency::BoundedStaleness => self.find_best_follower_bounded(),
            ReadConsistency::ReadAny => self.find_best_follower_any(),
        }
    }

    /// Routes using the default consistency level.
    pub fn route_default(&self) -> ReadTarget {
        self.route_read(self.config.default_consistency)
    }

    /// Returns the number of healthy followers.
    pub fn healthy_follower_count(&self) -> usize {
        self.followers.values().filter(|f| f.healthy).count()
    }

    /// Returns all tracked followers.
    pub fn followers(&self) -> Vec<&FollowerStatus> {
        self.followers.values().collect()
    }

    /// Checks if a specific follower is within staleness bounds.
    pub fn is_within_bounds(&self, node_id: &NodeId) -> bool {
        let Some(status) = self.followers.get(node_id) else {
            return false;
        };
        if !status.healthy {
            return false;
        }
        let entry_lag = self
            .leader_commit_index
            .as_u64()
            .saturating_sub(status.last_applied.as_u64());
        if entry_lag > self.config.max_stale_entries {
            return false;
        }
        status.last_updated.elapsed() <= self.config.max_stale_duration
    }

    fn find_best_follower_bounded(&self) -> ReadTarget {
        let mut best: Option<&FollowerStatus> = None;

        for status in self.followers.values() {
            if !status.healthy {
                continue;
            }
            if status.last_updated.elapsed() > self.config.max_stale_duration {
                continue;
            }
            let entry_lag = self
                .leader_commit_index
                .as_u64()
                .saturating_sub(status.last_applied.as_u64());
            if entry_lag > self.config.max_stale_entries {
                continue;
            }
            match best {
                None => best = Some(status),
                Some(current_best) => {
                    // Prefer lower latency
                    if status.latency_us < current_best.latency_us {
                        best = Some(status);
                    }
                }
            }
        }

        match best {
            Some(status) => ReadTarget::Follower(status.node_id),
            None => {
                // Fall back to leader if no follower meets bounds
                match self.leader_id {
                    Some(id) => ReadTarget::Leader(id),
                    None => ReadTarget::Unavailable,
                }
            }
        }
    }

    fn find_best_follower_any(&self) -> ReadTarget {
        let mut best: Option<&FollowerStatus> = None;

        for status in self.followers.values() {
            if !status.healthy {
                continue;
            }
            match best {
                None => best = Some(status),
                Some(current_best) => {
                    if status.latency_us < current_best.latency_us {
                        best = Some(status);
                    }
                }
            }
        }

        match best {
            Some(status) => ReadTarget::Follower(status.node_id),
            None => match self.leader_id {
                Some(id) => ReadTarget::Leader(id),
                None => ReadTarget::Unavailable,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> FollowerReadRouter {
        FollowerReadRouter::new(FollowerReadConfig::default())
    }

    #[test]
    fn test_linearizable_routes_to_leader() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));

        let target = router.route_read(ReadConsistency::Linearizable);
        assert_eq!(target, ReadTarget::Leader(NodeId::new(1)));
    }

    #[test]
    fn test_linearizable_unavailable_without_leader() {
        let router = make_router();
        let target = router.route_read(ReadConsistency::Linearizable);
        assert_eq!(target, ReadTarget::Unavailable);
    }

    #[test]
    fn test_bounded_staleness_routes_to_follower() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(95), 500);

        let target = router.route_read(ReadConsistency::BoundedStaleness);
        assert_eq!(target, ReadTarget::Follower(NodeId::new(2)));
    }

    #[test]
    fn test_bounded_staleness_rejects_lagging_follower() {
        let mut router = FollowerReadRouter::new(FollowerReadConfig {
            max_stale_entries: 10,
            ..Default::default()
        });
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(50), 500);

        let target = router.route_read(ReadConsistency::BoundedStaleness);
        // Falls back to leader because follower is too far behind
        assert_eq!(target, ReadTarget::Leader(NodeId::new(1)));
    }

    #[test]
    fn test_bounded_staleness_prefers_lower_latency() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(98), 1000);
        router.update_follower(NodeId::new(3), LogIndex::new(97), 200);

        let target = router.route_read(ReadConsistency::BoundedStaleness);
        assert_eq!(target, ReadTarget::Follower(NodeId::new(3)));
    }

    #[test]
    fn test_read_any_picks_lowest_latency() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(50), 300);
        router.update_follower(NodeId::new(3), LogIndex::new(10), 100);

        let target = router.route_read(ReadConsistency::ReadAny);
        assert_eq!(target, ReadTarget::Follower(NodeId::new(3)));
    }

    #[test]
    fn test_unhealthy_follower_excluded() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(98), 500);
        router.mark_unhealthy(&NodeId::new(2));

        let target = router.route_read(ReadConsistency::BoundedStaleness);
        assert_eq!(target, ReadTarget::Leader(NodeId::new(1)));
    }

    #[test]
    fn test_remove_follower() {
        let mut router = make_router();
        router.update_follower(NodeId::new(2), LogIndex::new(98), 500);
        assert_eq!(router.healthy_follower_count(), 1);

        router.remove_follower(&NodeId::new(2));
        assert_eq!(router.healthy_follower_count(), 0);
    }

    #[test]
    fn test_is_within_bounds() {
        let mut router = make_router();
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(95), 500);

        assert!(router.is_within_bounds(&NodeId::new(2)));
    }

    #[test]
    fn test_is_not_within_bounds_lagging() {
        let mut router = FollowerReadRouter::new(FollowerReadConfig {
            max_stale_entries: 5,
            ..Default::default()
        });
        router.set_leader(NodeId::new(1), LogIndex::new(100));
        router.update_follower(NodeId::new(2), LogIndex::new(50), 500);

        assert!(!router.is_within_bounds(&NodeId::new(2)));
    }

    #[test]
    fn test_default_route_uses_config() {
        let mut router = FollowerReadRouter::new(FollowerReadConfig {
            default_consistency: ReadConsistency::ReadAny,
            ..Default::default()
        });
        router.update_follower(NodeId::new(2), LogIndex::new(50), 100);

        let target = router.route_default();
        assert_eq!(target, ReadTarget::Follower(NodeId::new(2)));
    }

    #[test]
    fn test_healthy_follower_count() {
        let mut router = make_router();
        router.update_follower(NodeId::new(2), LogIndex::new(50), 100);
        router.update_follower(NodeId::new(3), LogIndex::new(60), 200);
        router.update_follower(NodeId::new(4), LogIndex::new(70), 300);
        router.mark_unhealthy(&NodeId::new(3));

        assert_eq!(router.healthy_follower_count(), 2);
    }
}
