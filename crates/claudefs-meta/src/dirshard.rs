//! Directory sharding module for hot directories.
//!
//! When a directory receives thousands of file creates per second, the directory's
//! metadata is partitioned across multiple nodes, each handling a subset of entries.
//! This module tracks directory operation rates and automatically shards hot directories.

use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_secs()
}

fn hash_entry_name(name: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Configuration for directory sharding behavior.
#[derive(Clone, Debug)]
pub struct DirShardConfig {
    /// Operations per window before triggering auto-sharding.
    pub shard_threshold: u64,
    /// Operations per window to trigger un-sharding.
    pub unshard_threshold: u64,
    /// Number of shards to split a hot directory into.
    pub num_shards: u16,
    /// Time window (in seconds) for counting operations.
    pub window_secs: u64,
}

impl Default for DirShardConfig {
    fn default() -> Self {
        Self {
            shard_threshold: 1000,
            unshard_threshold: 100,
            num_shards: 16,
            window_secs: 60,
        }
    }
}

/// State of a directory's sharding configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DirShardState {
    /// Directory is not sharded, all entries on one node.
    Unsharded,
    /// Directory entries are distributed across nodes.
    Sharded {
        /// Map of shard index to node ID.
        shard_map: Vec<NodeId>,
        /// Number of shards.
        num_shards: u16,
        /// Unix timestamp when the directory was sharded.
        sharded_at: u64,
    },
}

#[derive(Clone, Debug)]
struct OpCounter {
    count: u64,
    window_start: u64,
}

/// Manager for directory sharding operations.
///
/// Tracks operation rates on directories and manages automatic sharding
/// of hot directories across multiple nodes.
pub struct DirShardManager {
    op_counts: RwLock<HashMap<InodeId, OpCounter>>,
    shard_states: RwLock<HashMap<InodeId, DirShardState>>,
    config: DirShardConfig,
}

impl DirShardManager {
    /// Creates a new manager with the given configuration.
    pub fn new(config: DirShardConfig) -> Self {
        Self {
            op_counts: RwLock::new(HashMap::new()),
            shard_states: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Creates a new manager with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(DirShardConfig::default())
    }

    /// Records an operation on a directory.
    ///
    /// Returns `Some(shard_count)` if the directory was just sharded (crossed the threshold).
    pub fn record_op(&self, dir: InodeId) -> Option<u16> {
        let now = unix_timestamp_secs();
        let mut counts = self.op_counts.write().unwrap();
        let counter = counts.entry(dir).or_insert(OpCounter {
            count: 0,
            window_start: now,
        });

        if now - counter.window_start >= self.config.window_secs {
            counter.count = 0;
            counter.window_start = now;
        }

        counter.count += 1;

        if counter.count >= self.config.shard_threshold {
            let states = self.shard_states.read().unwrap();
            if !matches!(states.get(&dir), Some(DirShardState::Sharded { .. })) {
                drop(states);
                return Some(self.config.num_shards);
            }
        }
        None
    }

    /// Routes an entry name to a shard index using consistent hashing.
    ///
    /// Returns `(shard_index, node_id)` if the directory is sharded,
    /// or `None` if unsharded.
    pub fn route_entry(&self, dir: InodeId, name: &str) -> Option<(u16, NodeId)> {
        let states = self.shard_states.read().unwrap();
        match states.get(&dir) {
            Some(DirShardState::Sharded {
                shard_map,
                num_shards,
                ..
            }) => {
                let hash = hash_entry_name(name);
                let shard_index = (hash % *num_shards as u64) as u16;
                let node_id = shard_map.get(shard_index as usize).copied()?;
                Some((shard_index, node_id))
            }
            _ => None,
        }
    }

    /// Manually shards a directory across the given nodes.
    pub fn shard_directory(&self, dir: InodeId, nodes: &[NodeId]) -> Result<(), MetaError> {
        if nodes.is_empty() {
            return Err(MetaError::KvError(
                "cannot shard with empty node list".to_string(),
            ));
        }
        let num_shards = nodes.len() as u16;
        let shard_map: Vec<NodeId> = nodes.to_vec();
        let mut states = self.shard_states.write().unwrap();
        states.insert(
            dir,
            DirShardState::Sharded {
                shard_map,
                num_shards,
                sharded_at: unix_timestamp_secs(),
            },
        );
        Ok(())
    }

    /// Un-shards a directory (merges back to single node).
    pub fn unshard_directory(&self, dir: InodeId) {
        let mut states = self.shard_states.write().unwrap();
        states.insert(dir, DirShardState::Unsharded);
    }

    /// Checks if a directory is currently sharded.
    pub fn is_sharded(&self, dir: InodeId) -> bool {
        let states = self.shard_states.read().unwrap();
        matches!(states.get(&dir), Some(DirShardState::Sharded { .. }))
    }

    /// Returns the shard state for a directory.
    pub fn get_shard_state(&self, dir: InodeId) -> DirShardState {
        let states = self.shard_states.read().unwrap();
        states
            .get(&dir)
            .cloned()
            .unwrap_or(DirShardState::Unsharded)
    }

    /// Returns the current operation count for a directory.
    pub fn get_op_count(&self, dir: InodeId) -> u64 {
        let counts = self.op_counts.read().unwrap();
        counts.get(&dir).map(|c| c.count).unwrap_or(0)
    }

    /// Resets the operation counter for a directory (called at window boundary).
    pub fn reset_op_count(&self, dir: InodeId) {
        let mut counts = self.op_counts.write().unwrap();
        if let Some(counter) = counts.get_mut(&dir) {
            counter.count = 0;
            counter.window_start = unix_timestamp_secs();
        }
    }

    /// Returns all currently sharded directories.
    pub fn sharded_directories(&self) -> Vec<InodeId> {
        let states = self.shard_states.read().unwrap();
        states
            .iter()
            .filter(|(_, state)| matches!(state, DirShardState::Sharded { .. }))
            .map(|(inode, _)| *inode)
            .collect()
    }

    /// Returns directories that are candidates for un-sharding (below threshold).
    pub fn unshard_candidates(&self) -> Vec<InodeId> {
        let counts = self.op_counts.read().unwrap();
        let states = self.shard_states.read().unwrap();
        let now = unix_timestamp_secs();
        let mut candidates = Vec::new();
        for (inode, state) in states.iter() {
            if let DirShardState::Sharded { .. } = state {
                if let Some(counter) = counts.get(inode) {
                    if now - counter.window_start >= self.config.window_secs
                        && counter.count <= self.config.unshard_threshold
                    {
                        candidates.push(*inode);
                    }
                }
            }
        }
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DirShardConfig::default();
        assert_eq!(config.shard_threshold, 1000);
        assert_eq!(config.unshard_threshold, 100);
        assert_eq!(config.num_shards, 16);
        assert_eq!(config.window_secs, 60);
    }

    #[test]
    fn test_record_op_counting() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(100);

        for _ in 0..500 {
            manager.record_op(dir);
        }
        assert_eq!(manager.get_op_count(dir), 500);
    }

    #[test]
    fn test_threshold_detection() {
        let config = DirShardConfig {
            shard_threshold: 100,
            ..Default::default()
        };
        let manager = DirShardManager::new(config);
        let dir = InodeId::new(200);

        let result = manager.record_op(dir);
        assert!(result.is_none());

        for _ in 0..100 {
            manager.record_op(dir);
        }
        let result = manager.record_op(dir);
        assert_eq!(result, Some(16));
    }

    #[test]
    fn test_entry_routing() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(300);
        let nodes = vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(3),
            NodeId::new(4),
        ];

        manager.shard_directory(dir, &nodes).unwrap();

        let result = manager.route_entry(dir, "file001.txt");
        assert!(result.is_some());
        let (shard_idx, node_id) = result.unwrap();
        assert!(shard_idx < 4);
        assert!(nodes.contains(&node_id));
    }

    #[test]
    fn test_manual_sharding() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(400);
        let nodes = vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)];

        assert!(!manager.is_sharded(dir));

        manager.shard_directory(dir, &nodes).unwrap();

        assert!(manager.is_sharded(dir));
        let state = manager.get_shard_state(dir);
        assert!(matches!(
            state,
            DirShardState::Sharded {
                shard_map,
                num_shards,
                sharded_at: _
            } if shard_map == vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)] && num_shards == 3
        ));
    }

    #[test]
    fn test_unsharding() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(500);
        let nodes = vec![NodeId::new(1), NodeId::new(2)];

        manager.shard_directory(dir, &nodes).unwrap();
        assert!(manager.is_sharded(dir));

        manager.unshard_directory(dir);
        assert!(!manager.is_sharded(dir));
    }

    #[test]
    fn test_shard_state_queries() {
        let manager = DirShardManager::with_defaults();

        let dir1 = InodeId::new(600);
        let dir2 = InodeId::new(700);

        assert_eq!(manager.get_shard_state(dir1), DirShardState::Unsharded);
        assert_eq!(manager.get_shard_state(dir2), DirShardState::Unsharded);

        manager.shard_directory(dir1, &[NodeId::new(1)]).unwrap();

        assert!(matches!(
            manager.get_shard_state(dir1),
            DirShardState::Sharded { .. }
        ));
        assert_eq!(manager.get_shard_state(dir2), DirShardState::Unsharded);
    }

    #[test]
    fn test_op_count_reset() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(800);

        manager.record_op(dir);
        manager.record_op(dir);
        assert_eq!(manager.get_op_count(dir), 2);

        manager.reset_op_count(dir);
        assert_eq!(manager.get_op_count(dir), 0);
    }

    #[test]
    fn test_window_expiry() {
        let config = DirShardConfig {
            window_secs: 1,
            ..Default::default()
        };
        let manager = DirShardManager::new(config);
        let dir = InodeId::new(900);

        manager.record_op(dir);
        manager.record_op(dir);
        assert_eq!(manager.get_op_count(dir), 2);

        std::thread::sleep(std::time::Duration::from_secs(2));

        manager.record_op(dir);
        assert_eq!(manager.get_op_count(dir), 1);
    }

    #[test]
    fn test_unshard_candidates() {
        let config = DirShardConfig {
            shard_threshold: 100,
            unshard_threshold: 50,
            window_secs: 1,
            num_shards: 4,
        };
        let manager = DirShardManager::new(config);
        let dir1 = InodeId::new(1000);
        let dir2 = InodeId::new(1100);

        manager.shard_directory(dir1, &[NodeId::new(1)]).unwrap();
        manager.shard_directory(dir2, &[NodeId::new(1)]).unwrap();

        for _ in 0..30 {
            manager.record_op(dir1);
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        let candidates = manager.unshard_candidates();
        assert!(candidates.contains(&dir1));
    }

    #[test]
    fn test_hash_distribution() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(1200);
        let nodes = vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(3),
            NodeId::new(4),
            NodeId::new(5),
            NodeId::new(6),
            NodeId::new(7),
            NodeId::new(8),
        ];
        manager.shard_directory(dir, &nodes).unwrap();

        let mut counts = [0u16; 8];
        for i in 0..1000 {
            let name = format!("file{:04}.txt", i);
            if let Some((idx, _)) = manager.route_entry(dir, &name) {
                counts[idx as usize] += 1;
            }
        }

        for c in &counts {
            assert!(*c > 0, "each shard should have entries");
        }
    }

    #[test]
    fn test_route_none_for_unsharded() {
        let manager = DirShardManager::with_defaults();
        let dir = InodeId::new(1300);

        let result = manager.route_entry(dir, "test.txt");
        assert!(result.is_none());
    }

    #[test]
    fn test_sharded_directories() {
        let manager = DirShardManager::with_defaults();
        let dir1 = InodeId::new(1400);
        let dir2 = InodeId::new(1500);

        manager.shard_directory(dir1, &[NodeId::new(1)]).unwrap();

        let sharded = manager.sharded_directories();
        assert!(sharded.contains(&dir1));
        assert!(!sharded.contains(&dir2));
    }
}
