use std::collections::HashMap;

pub type ShardId = u16;

#[derive(Debug, Clone)]
pub struct DedupCoordinatorConfig {
    pub num_shards: u16,
    pub local_node_id: u32,
}

impl Default for DedupCoordinatorConfig {
    fn default() -> Self {
        Self {
            num_shards: 256,
            local_node_id: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DedupCoordinatorStats {
    pub local_lookups: u64,
    pub remote_lookups: u64,
    pub local_hits: u64,
    pub remote_hits: u64,
    pub fingerprints_owned: u64,
    pub cross_node_savings_bytes: u64,
}

impl DedupCoordinatorStats {
    pub fn total_lookups(&self) -> u64 {
        self.local_lookups + self.remote_lookups
    }
    pub fn total_hits(&self) -> u64 {
        self.local_hits + self.remote_hits
    }
    pub fn hit_rate(&self) -> f64 {
        let t = self.total_lookups();
        if t == 0 {
            0.0
        } else {
            self.total_hits() as f64 / t as f64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupLookupResult {
    FoundLocal { hash: [u8; 32] },
    FoundRemote { hash: [u8; 32], node_id: u32 },
    NotFound,
}

pub struct NodeFingerprintStore {
    entries: HashMap<[u8; 32], u32>,
}

impl NodeFingerprintStore {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
    pub fn register(&mut self, hash: [u8; 32], node_id: u32) {
        self.entries.insert(hash, node_id);
    }
    pub fn lookup(&self, hash: &[u8; 32]) -> Option<u32> {
        self.entries.get(hash).copied()
    }
    pub fn remove(&mut self, hash: &[u8; 32]) -> bool {
        self.entries.remove(hash).is_some()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct DedupCoordinator {
    config: DedupCoordinatorConfig,
    store: NodeFingerprintStore,
    stats: DedupCoordinatorStats,
}

impl DedupCoordinator {
    pub fn new(config: DedupCoordinatorConfig) -> Self {
        Self {
            config,
            store: NodeFingerprintStore::new(),
            stats: DedupCoordinatorStats::default(),
        }
    }

    pub fn shard_for_hash(&self, hash: &[u8; 32]) -> ShardId {
        let mut v = 0u64;
        for i in 0..8 {
            v = v.wrapping_shl(8) | (hash[i] as u64);
        }
        (v % self.config.num_shards as u64) as ShardId
    }

    pub fn register(&mut self, hash: [u8; 32], node_id: u32) {
        self.store.register(hash, node_id);
        self.stats.fingerprints_owned += 1;
    }

    pub fn lookup(&mut self, hash: &[u8; 32]) -> DedupLookupResult {
        match self.store.lookup(hash) {
            Some(node_id) if node_id == self.config.local_node_id => {
                self.stats.local_lookups += 1;
                self.stats.local_hits += 1;
                DedupLookupResult::FoundLocal { hash: *hash }
            }
            Some(node_id) => {
                self.stats.remote_lookups += 1;
                self.stats.remote_hits += 1;
                DedupLookupResult::FoundRemote {
                    hash: *hash,
                    node_id,
                }
            }
            None => {
                self.stats.local_lookups += 1;
                DedupLookupResult::NotFound
            }
        }
    }

    pub fn record_savings(&mut self, bytes_saved: u64) {
        self.stats.cross_node_savings_bytes += bytes_saved;
    }

    pub fn stats(&self) -> &DedupCoordinatorStats {
        &self.stats
    }
    pub fn fingerprint_count(&self) -> usize {
        self.store.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_coordinator_config_default() {
        let config = DedupCoordinatorConfig::default();
        assert_eq!(config.num_shards, 256);
        assert_eq!(config.local_node_id, 0);
    }

    #[test]
    fn shard_for_hash_deterministic() {
        let config = DedupCoordinatorConfig::default();
        let coordinator = DedupCoordinator::new(config);
        let hash: [u8; 32] = [0x12; 32];
        let shard1 = coordinator.shard_for_hash(&hash);
        let shard2 = coordinator.shard_for_hash(&hash);
        assert_eq!(shard1, shard2);
    }

    #[test]
    fn shard_for_hash_within_range() {
        let config = DedupCoordinatorConfig {
            num_shards: 256,
            local_node_id: 0,
        };
        let coordinator = DedupCoordinator::new(config);
        for i in 0..100 {
            let hash: [u8; 32] = [i; 32];
            let shard = coordinator.shard_for_hash(&hash);
            assert!(shard < 256);
        }
    }

    #[test]
    fn shard_for_hash_different_hashes() {
        let config = DedupCoordinatorConfig::default();
        let coordinator = DedupCoordinator::new(config);
        let hash1: [u8; 32] = [0x01; 32];
        let hash2: [u8; 32] = [0xFF; 32];
        let shard1 = coordinator.shard_for_hash(&hash1);
        let shard2 = coordinator.shard_for_hash(&hash2);
        assert!(shard1 != shard2);
    }

    #[test]
    fn register_increments_owned() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0xAA; 32];
        coordinator.register(hash, 0);
        assert_eq!(coordinator.stats().fingerprints_owned, 1);
    }

    #[test]
    fn lookup_not_found() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0xBB; 32];
        let result = coordinator.lookup(&hash);
        assert_eq!(result, DedupLookupResult::NotFound);
    }

    #[test]
    fn lookup_found_local() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0xCC; 32];
        coordinator.register(hash, config.local_node_id);
        let result = coordinator.lookup(&hash);
        assert_eq!(result, DedupLookupResult::FoundLocal { hash });
    }

    #[test]
    fn lookup_found_remote() {
        let config = DedupCoordinatorConfig {
            num_shards: 256,
            local_node_id: 0,
        };
        let mut coordinator = DedupCoordinator::new(config);
        let hash: [u8; 32] = [0xDD; 32];
        coordinator.register(hash, 1);
        let result = coordinator.lookup(&hash);
        assert_eq!(result, DedupLookupResult::FoundRemote { hash, node_id: 1 });
    }

    #[test]
    fn lookup_found_local_increments_local_hits() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0xEE; 32];
        coordinator.register(hash, config.local_node_id);
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().local_hits, 1);
    }

    #[test]
    fn lookup_found_remote_increments_remote_hits() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0xFF; 32];
        coordinator.register(hash, 1);
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().remote_hits, 1);
    }

    #[test]
    fn lookup_not_found_increments_local_lookups() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0x11; 32];
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().local_lookups, 1);
    }

    #[test]
    fn stats_total_lookups() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash1: [u8; 32] = [0x12; 32];
        let hash2: [u8; 32] = [0x13; 32];
        coordinator.register(hash2, 1);
        coordinator.lookup(&hash1);
        coordinator.lookup(&hash2);
        assert_eq!(coordinator.stats().total_lookups(), 2);
    }

    #[test]
    fn stats_total_hits() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0x14; 32];
        coordinator.register(hash, config.local_node_id);
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().total_hits(), 1);
    }

    #[test]
    fn hit_rate_zero_when_no_lookups() {
        let config = DedupCoordinatorConfig::default();
        let coordinator = DedupCoordinator::new(config);
        assert_eq!(coordinator.stats().hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_after_all_hits() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0x15; 32];
        coordinator.register(hash, config.local_node_id);
        coordinator.lookup(&hash);
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().hit_rate(), 1.0);
    }

    #[test]
    fn hit_rate_after_all_misses() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0x16; 32];
        coordinator.lookup(&hash);
        coordinator.lookup(&hash);
        assert_eq!(coordinator.stats().hit_rate(), 0.0);
    }

    #[test]
    fn record_savings_accumulates() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        coordinator.record_savings(1000);
        coordinator.record_savings(2000);
        assert_eq!(coordinator.stats().cross_node_savings_bytes, 3000);
    }

    #[test]
    fn fingerprint_count_matches_store() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        coordinator.register([0x20; 32], 0);
        coordinator.register([0x21; 32], 0);
        coordinator.register([0x22; 32], 0);
        assert_eq!(coordinator.fingerprint_count(), 3);
    }

    #[test]
    fn node_fingerprint_store_empty() {
        let store = NodeFingerprintStore::new();
        assert!(store.is_empty());
    }

    #[test]
    fn node_fingerprint_store_register() {
        let mut store = NodeFingerprintStore::new();
        store.register([0x30; 32], 1);
        assert_eq!(store.lookup(&[0x30; 32]), Some(1));
    }

    #[test]
    fn node_fingerprint_store_remove() {
        let mut store = NodeFingerprintStore::new();
        store.register([0x31; 32], 1);
        assert!(store.remove(&[0x31; 32]));
        assert!(store.is_empty());
    }

    #[test]
    fn node_fingerprint_store_remove_missing() {
        let mut store = NodeFingerprintStore::new();
        assert!(!store.remove(&[0x32; 32]));
    }

    #[test]
    fn multiple_nodes_same_hash() {
        let mut config = DedupCoordinatorConfig::default();
        let mut coordinator = DedupCoordinator::new(config.clone());
        let hash: [u8; 32] = [0x40; 32];
        coordinator.register(hash, 1);
        coordinator.register(hash, 2);
        let result = coordinator.lookup(&hash);
        if let DedupLookupResult::FoundRemote { node_id, .. } = result {
            assert_eq!(node_id, 2);
        } else {
            panic!("Expected FoundRemote");
        }
    }

    #[test]
    fn different_local_node_ids() {
        let mut config1 = DedupCoordinatorConfig {
            num_shards: 256,
            local_node_id: 0,
        };
        let mut config2 = DedupCoordinatorConfig {
            num_shards: 256,
            local_node_id: 1,
        };
        let mut coord1 = DedupCoordinator::new(config1.clone());
        let mut coord2 = DedupCoordinator::new(config2.clone());
        let hash: [u8; 32] = [0x50; 32];
        coord1.register(hash, config1.local_node_id);
        coord2.register(hash, config2.local_node_id);
        coord1.lookup(&hash);
        coord2.lookup(&hash);
        let result1 = coord1.lookup(&hash);
        let result2 = coord2.lookup(&hash);
        assert!(matches!(result1, DedupLookupResult::FoundLocal { .. }));
        assert!(matches!(result2, DedupLookupResult::FoundLocal { .. }));
        assert_ne!(config1.local_node_id, config2.local_node_id);
    }
}
