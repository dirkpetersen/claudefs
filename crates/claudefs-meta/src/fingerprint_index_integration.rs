//! Fingerprint index integration module bridging A2's fingerprint_index and A3's dedup_coordinator.
//!
//! This module provides distributed deduplication across nodes by:
//! - Using consistent hashing to route fingerprint lookups to the appropriate node
//! - Tracking statistics for local vs remote lookups and deduplication savings
//! - Providing a clean interface for A3's DedupCoordinator

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::fingerprint::FingerprintIndex;
use crate::types::{InodeId, MetaError, Timestamp};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FingerprintRouterConfig {
    pub local_node_id: u32,
    pub num_shards: u16,
    pub remote_coordinators: HashMap<u32, RemoteCoordinatorInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteCoordinatorInfo {
    pub node_id: u32,
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FingerprintLookupRequest {
    pub hash: [u8; 32],
    pub size: u64,
    pub source_inode: InodeId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FingerprintLookupResult {
    Local {
        location: u64,
        ref_count: u64,
        size: u64,
    },
    Remote {
        node_id: u32,
        ref_count: u64,
        size: u64,
    },
    NotFound,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FingerprintRouterStats {
    pub local_lookups: u64,
    pub remote_lookups: u64,
    pub local_hits: u64,
    pub remote_hits: u64,
    pub cross_node_savings_bytes: u64,
    pub last_lookup_time: Option<Timestamp>,
}

pub struct FingerprintRouter {
    config: FingerprintRouterConfig,
    local_index: Arc<FingerprintIndex>,
    stats: Arc<RwLock<FingerprintRouterStats>>,
}

impl FingerprintRouter {
    pub fn new(config: FingerprintRouterConfig, local_index: Arc<FingerprintIndex>) -> Self {
        Self {
            config,
            local_index,
            stats: Arc::new(RwLock::new(FingerprintRouterStats::default())),
        }
    }

    pub fn lookup(
        &mut self,
        req: &FingerprintLookupRequest,
    ) -> Result<FingerprintLookupResult, MetaError> {
        let shard = self.get_shard_for_hash(&req.hash);
        let owner_node = self.route_to_node(shard);

        if owner_node == self.config.local_node_id {
            let mut stats = self.stats.write().unwrap();
            stats.local_lookups += 1;
            stats.last_lookup_time = Some(Timestamp::now());

            if let Some(entry) = self.local_index.lookup(&req.hash) {
                stats.local_hits += 1;
                return Ok(FingerprintLookupResult::Local {
                    location: entry.block_location,
                    ref_count: entry.ref_count,
                    size: entry.size,
                });
            }
            return Ok(FingerprintLookupResult::NotFound);
        } else {
            let mut stats = self.stats.write().unwrap();
            stats.remote_lookups += 1;
            stats.last_lookup_time = Some(Timestamp::now());

            if self.config.remote_coordinators.contains_key(&owner_node) {
                return Ok(FingerprintLookupResult::Remote {
                    node_id: owner_node,
                    ref_count: 0,
                    size: req.size,
                });
            } else {
                return Ok(FingerprintLookupResult::NotFound);
            }
        }
    }

    pub fn register_new_fingerprint(
        &mut self,
        hash: [u8; 32],
        location: u64,
        size: u64,
    ) -> Result<(), MetaError> {
        let shard = self.get_shard_for_hash(&hash);
        let owner_node = self.route_to_node(shard);

        if owner_node == self.config.local_node_id {
            self.local_index.insert(hash, location, size)?;
        }

        Ok(())
    }

    pub fn get_shard_for_hash(&self, hash: &[u8; 32]) -> u16 {
        (fnv_hash(hash) % self.config.num_shards as u64) as u16
    }

    pub fn route_to_node(&self, shard: u16) -> u32 {
        if self.config.remote_coordinators.is_empty() {
            return self.config.local_node_id;
        }

        let num_nodes = self.config.remote_coordinators.len() + 1;
        let shard_idx = shard as usize;
        let node_idx = shard_idx % num_nodes;

        if node_idx == 0 {
            return self.config.local_node_id;
        }

        let mut node_ids: Vec<u32> = self.config.remote_coordinators.keys().copied().collect();
        node_ids.sort();

        if node_idx - 1 < node_ids.len() {
            node_ids[node_idx - 1]
        } else {
            self.config.local_node_id
        }
    }

    pub fn record_lookup(&mut self, local: bool, hit: bool, bytes_saved: u64) {
        let mut stats = self.stats.write().unwrap();
        if local {
            stats.local_lookups += 1;
            if hit {
                stats.local_hits += 1;
            }
        } else {
            stats.remote_lookups += 1;
            if hit {
                stats.remote_hits += 1;
            }
        }
        stats.cross_node_savings_bytes += bytes_saved;
        stats.last_lookup_time = Some(Timestamp::now());
    }

    pub fn stats(&self) -> FingerprintRouterStats {
        self.stats.read().unwrap().clone()
    }

    pub fn dedup_potential(&self, new_hash: [u8; 32], size: u64) -> Option<u64> {
        let shard = self.get_shard_for_hash(&new_hash);
        let owner_node = self.route_to_node(shard);

        if owner_node == self.config.local_node_id {
            if let Some(entry) = self.local_index.lookup(&new_hash) {
                return Some(entry.size);
            }
        }

        None
    }

    pub fn local_hit_rate(&self) -> f64 {
        let stats = self.stats.read().unwrap();
        if stats.local_lookups == 0 {
            return 0.0;
        }
        (stats.local_hits as f64) / (stats.local_lookups as f64) * 100.0
    }

    pub fn remote_hit_rate(&self) -> f64 {
        let stats = self.stats.read().unwrap();
        if stats.remote_lookups == 0 {
            return 0.0;
        }
        (stats.remote_hits as f64) / (stats.remote_lookups as f64) * 100.0
    }

    pub fn total_lookups(&self) -> u64 {
        let stats = self.stats.read().unwrap();
        stats.local_lookups + stats.remote_lookups
    }
}

fn fnv_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash = hash.wrapping_mul(0x100000001b3);
        hash ^= byte as u64;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(i: u8) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash[0] = i;
        hash[31] = i.wrapping_add(1);
        hash
    }

    fn make_config(num_shards: u16) -> FingerprintRouterConfig {
        FingerprintRouterConfig {
            local_node_id: 1,
            num_shards,
            remote_coordinators: HashMap::new(),
        }
    }

    fn make_config_with_remotes(
        local_node: u32,
        num_shards: u16,
        remotes: &[(u32, &str)],
    ) -> FingerprintRouterConfig {
        let mut remote_map = HashMap::new();
        for (node_id, addr) in remotes {
            remote_map.insert(
                *node_id,
                RemoteCoordinatorInfo {
                    node_id: *node_id,
                    address: addr.to_string(),
                },
            );
        }
        FingerprintRouterConfig {
            local_node_id: local_node,
            num_shards,
            remote_coordinators: remote_map,
        }
    }

    #[test]
    fn test_local_fingerprint_lookup_hit() {
        let local_index = Arc::new(FingerprintIndex::new());
        let hash = make_hash(1);
        local_index.insert(hash, 1000, 4096).unwrap();

        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        let req = FingerprintLookupRequest {
            hash,
            size: 4096,
            source_inode: InodeId::new(100),
        };

        let result = router.lookup(&req).unwrap();
        match result {
            FingerprintLookupResult::Local {
                location,
                ref_count,
                size,
            } => {
                assert_eq!(location, 1000);
                assert_eq!(ref_count, 1);
                assert_eq!(size, 4096);
            }
            _ => panic!("expected local hit"),
        }

        let stats = router.stats();
        assert_eq!(stats.local_lookups, 1);
        assert_eq!(stats.local_hits, 1);
    }

    #[test]
    fn test_local_fingerprint_lookup_miss() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        let req = FingerprintLookupRequest {
            hash: make_hash(99),
            size: 4096,
            source_inode: InodeId::new(100),
        };

        let result = router.lookup(&req).unwrap();
        match result {
            FingerprintLookupResult::NotFound => {}
            _ => panic!("expected not found"),
        }

        let stats = router.stats();
        assert_eq!(stats.local_lookups, 1);
        assert_eq!(stats.local_hits, 0);
    }

    #[test]
    fn test_remote_fingerprint_lookup_routing() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config_with_remotes(1, 256, &[(2, "192.168.1.2"), (3, "192.168.1.3")]);
        let mut router = FingerprintRouter::new(config, local_index);

        let hash = make_hash(10);
        let shard = router.get_shard_for_hash(&hash);
        let target_node = router.route_to_node(shard);

        assert!(target_node != 1);
    }

    #[test]
    fn test_consistent_hash_sharding() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let router = FingerprintRouter::new(config, local_index);

        let hash = make_hash(42);

        for _ in 0..10 {
            let shard = router.get_shard_for_hash(&hash);
            assert_eq!(shard, router.get_shard_for_hash(&hash));
        }
    }

    #[test]
    fn test_statistics_tracking() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        router.record_lookup(true, true, 4096);
        router.record_lookup(true, false, 0);
        router.record_lookup(false, true, 8192);
        router.record_lookup(false, false, 0);

        let stats = router.stats();
        assert_eq!(stats.local_lookups, 2);
        assert_eq!(stats.remote_lookups, 2);
        assert_eq!(stats.local_hits, 1);
        assert_eq!(stats.remote_hits, 1);
        assert_eq!(stats.cross_node_savings_bytes, 12288);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        router.record_lookup(true, true, 4096);
        router.record_lookup(true, true, 4096);
        router.record_lookup(true, false, 0);
        router.record_lookup(true, false, 0);

        assert_eq!(router.local_hit_rate(), 50.0);
    }

    #[test]
    fn test_remote_hit_rate_calculation() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        router.record_lookup(false, true, 4096);
        router.record_lookup(false, false, 0);

        assert_eq!(router.remote_hit_rate(), 50.0);
    }

    #[test]
    fn test_cross_node_savings() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        router.record_lookup(false, true, 4096);
        router.record_lookup(false, true, 8192);
        router.record_lookup(false, true, 1024);

        let stats = router.stats();
        assert_eq!(stats.cross_node_savings_bytes, 13312);
    }

    #[test]
    fn test_fingerprint_registration_local_shard() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        let hash = make_hash(1);
        router.register_new_fingerprint(hash, 5000, 4096).unwrap();

        let entry = local_index.lookup(&hash).expect("should be registered");
        assert_eq!(entry.block_location, 5000);
        assert_eq!(entry.size, 4096);
    }

    #[test]
    fn test_dedup_potential_returns_bytes_saved() {
        let local_index = Arc::new(FingerprintIndex::new());
        let hash = make_hash(1);
        local_index.insert(hash, 1000, 8192).unwrap();

        let config = make_config(256);
        let router = FingerprintRouter::new(config, local_index);

        let savings = router.dedup_potential(hash, 8192);
        assert_eq!(savings, Some(8192));
    }

    #[test]
    fn test_dedup_potential_not_found() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let router = FingerprintRouter::new(config, local_index);

        let savings = router.dedup_potential(make_hash(99), 4096);
        assert_eq!(savings, None);
    }

    #[test]
    fn test_missing_remote_coordinators() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config_with_remotes(1, 256, &[(2, "192.168.1.2")]);
        let mut router = FingerprintRouter::new(config, local_index);

        let hash = make_hash(10);
        let shard = router.get_shard_for_hash(&hash);
        let target_node = router.route_to_node(shard);

        if target_node == 2 {
            let req = FingerprintLookupRequest {
                hash,
                size: 4096,
                source_inode: InodeId::new(100),
            };
            let result = router.lookup(&req).unwrap();
            match result {
                FingerprintLookupResult::Remote { node_id, .. } => {
                    assert_eq!(node_id, 2);
                }
                _ => panic!("expected remote result"),
            }
        }
    }

    #[test]
    fn test_multiple_registrations_increment_ref_count() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        let hash = make_hash(1);
        router.register_new_fingerprint(hash, 1000, 4096).unwrap();
        router.register_new_fingerprint(hash, 2000, 4096).unwrap();

        let entry = local_index.lookup(&hash).expect("should exist");
        assert_eq!(entry.ref_count, 2);
    }

    #[test]
    fn test_shard_mapping_consistency() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let router = FingerprintRouter::new(config, local_index);

        for i in 0..100u8 {
            let hash = make_hash(i);
            let shard1 = router.get_shard_for_hash(&hash);
            let shard2 = router.get_shard_for_hash(&hash);
            assert_eq!(shard1, shard2, "hash {} should map to same shard", i);
        }
    }

    #[test]
    fn test_node_mapping_consistency() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config_with_remotes(1, 256, &[(2, "192.168.1.2")]);
        let router = FingerprintRouter::new(config, local_index);

        for shard in 0..256u16 {
            let node1 = router.route_to_node(shard);
            let node2 = router.route_to_node(shard);
            assert_eq!(node1, node2, "shard {} should map to same node", shard);
        }
    }

    #[test]
    fn test_stats_persistence_across_lookups() {
        let local_index = Arc::new(FingerprintIndex::new());
        let hash1 = make_hash(1);
        local_index.insert(hash1, 1000, 4096).unwrap();

        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        for i in 0..5 {
            let req = FingerprintLookupRequest {
                hash: if i < 3 { hash1 } else { make_hash(99) },
                size: 4096,
                source_inode: InodeId::new(100 + i),
            };
            router.lookup(&req).unwrap();
        }

        let total = router.total_lookups();
        assert_eq!(total, 5);
    }

    #[test]
    fn test_empty_router() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let router = FingerprintRouter::new(config, local_index);

        assert_eq!(router.local_hit_rate(), 0.0);
        assert_eq!(router.remote_hit_rate(), 0.0);
        assert_eq!(router.total_lookups(), 0);

        let stats = router.stats();
        assert_eq!(stats.local_lookups, 0);
        assert_eq!(stats.remote_lookups, 0);
        assert_eq!(stats.local_hits, 0);
        assert_eq!(stats.remote_hits, 0);
    }

    #[test]
    fn test_large_fingerprint_set() {
        let local_index = Arc::new(FingerprintIndex::new());
        let config = make_config(256);
        let mut router = FingerprintRouter::new(config, local_index);

        for i in 0..1000u16 {
            let mut hash = [0u8; 32];
            hash[0] = (i & 0xff) as u8;
            hash[1] = ((i >> 8) & 0xff) as u8;
            router
                .register_new_fingerprint(hash, i as u64 * 1000, 4096)
                .unwrap();
        }

        assert_eq!(local_index.entry_count(), 1000);

        for i in 0..1000u16 {
            let mut hash = [0u8; 32];
            hash[0] = (i & 0xff) as u8;
            hash[1] = ((i >> 8) & 0xff) as u8;
            let savings = router.dedup_potential(hash, 4096);
            assert_eq!(savings, Some(4096));
        }

        let savings = router.dedup_potential(make_hash(255), 4096);
        assert_eq!(savings, None);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_for_hash() {
        let config = FingerprintRouterConfig {
            local_node_id: 1,
            num_shards: 16,
            remote_coordinators: HashMap::new(),
        };
        let index = Arc::new(FingerprintIndex::new());
        let router = FingerprintRouter::new(config, index);
        
        let hash1 = [0u8; 32];
        let shard1 = router.get_shard_for_hash(&hash1);
        assert!(shard1 < 16);
    }

    #[test]
    fn test_router_config() {
        let config = FingerprintRouterConfig {
            local_node_id: 1,
            num_shards: 16,
            remote_coordinators: HashMap::new(),
        };
        assert_eq!(config.local_node_id, 1);
        assert_eq!(config.num_shards, 16);
    }

    #[test]
    fn test_stats_initial() {
        let stats = FingerprintRouterStats {
            local_lookups: 0,
            remote_lookups: 0,
            local_hits: 0,
            remote_hits: 0,
            cross_node_savings_bytes: 0,
        };
        assert_eq!(stats.local_lookups, 0);
        assert_eq!(stats.local_hits, 0);
    }

    #[test]
    fn test_lookup_result_variants() {
        let _local = FingerprintLookupResult::Local {
            location: 100,
            ref_count: 2,
            size: 4096,
        };
        let _remote = FingerprintLookupResult::Remote {
            node_id: 2,
            ref_count: 1,
            size: 4096,
        };
        let _not_found = FingerprintLookupResult::NotFound;
    }
}
