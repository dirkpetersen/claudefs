use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashRingConfig {
    pub virtual_nodes_per_member: usize,
}

impl Default for HashRingConfig {
    fn default() -> Self {
        Self {
            virtual_nodes_per_member: 150,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingMember {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct RingStats {
    pub total_members: usize,
    pub total_virtual_nodes: usize,
}

pub struct HashRing {
    ring: BTreeMap<u64, RingMember>,
    config: HashRingConfig,
    stats: RingStats,
}

impl HashRing {
    pub fn new(config: HashRingConfig) -> Self {
        Self {
            ring: BTreeMap::new(),
            config,
            stats: RingStats::default(),
        }
    }

    pub fn add_member(&mut self, member: RingMember) {
        for v in 0..self.config.virtual_nodes_per_member {
            let mut buf = member.id.to_le_bytes().to_vec();
            buf.extend_from_slice(&(v as u64).to_le_bytes());
            let key = hash_key(&buf);
            self.ring.insert(key, member.clone());
        }
        self.stats.total_members += 1;
        self.stats.total_virtual_nodes += self.config.virtual_nodes_per_member;
    }

    pub fn remove_member(&mut self, id: u32) {
        let keys_to_remove: Vec<u64> = self
            .ring
            .iter()
            .filter(|(_, m)| m.id == id)
            .map(|(&k, _)| k)
            .collect();

        let removed_count = keys_to_remove.len();
        for key in keys_to_remove {
            self.ring.remove(&key);
        }

        if removed_count > 0 {
            self.stats.total_members -= 1;
            self.stats.total_virtual_nodes -= removed_count;
        }
    }

    pub fn get_member(&self, key: &[u8]) -> Option<&RingMember> {
        if self.ring.is_empty() {
            return None;
        }
        let h = hash_key(key);
        if let Some((_, member)) = self.ring.range(h..).next() {
            return Some(member);
        }
        self.ring.first_key_value().map(|(_, v)| v)
    }

    pub fn get_members(&self, key: &[u8], count: usize) -> Vec<&RingMember> {
        if self.ring.is_empty() || count == 0 {
            return Vec::new();
        }
        let h = hash_key(key);
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (_, member) in self.ring.range(h..) {
            if seen.insert(member.id) {
                result.push(member);
                if result.len() >= count {
                    return result;
                }
            }
        }

        for (_, member) in &self.ring {
            if seen.insert(member.id) {
                result.push(member);
                if result.len() >= count {
                    break;
                }
            }
        }

        result
    }

    pub fn member_count(&self) -> usize {
        self.stats.total_members
    }

    pub fn stats(&self) -> &RingStats {
        &self.stats
    }
}

fn hash_key(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ring_config_default() {
        let config = HashRingConfig::default();
        assert_eq!(config.virtual_nodes_per_member, 150);
    }

    #[test]
    fn add_single_member() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        assert_eq!(ring.member_count(), 1);
    }

    #[test]
    fn add_multiple_members() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        ring.add_member(RingMember {
            id: 3,
            label: "node3".to_string(),
        });
        assert_eq!(ring.member_count(), 3);
    }

    #[test]
    fn get_member_empty_ring() {
        let ring = HashRing::new(HashRingConfig::default());
        assert!(ring.get_member(b"key").is_none());
    }

    #[test]
    fn get_member_single_member() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        let member = ring.get_member(b"any_key");
        assert!(member.is_some());
        assert_eq!(member.unwrap().id, 1);
    }

    #[test]
    fn get_member_multiple_members() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let member = ring.get_member(b"test_key");
        assert!(member.is_some());
        assert!(member.unwrap().id == 1 || member.unwrap().id == 2);
    }

    #[test]
    fn remove_member() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        ring.remove_member(1);
        assert_eq!(ring.member_count(), 1);
    }

    #[test]
    fn remove_nonexistent_member() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        let count = ring.member_count();
        ring.remove_member(999);
        assert_eq!(ring.member_count(), count);
    }

    #[test]
    fn get_members_count_limited() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let members = ring.get_members(b"key", 5);
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn get_members_empty_ring() {
        let ring = HashRing::new(HashRingConfig::default());
        let members = ring.get_members(b"key", 3);
        assert!(members.is_empty());
    }

    #[test]
    fn get_members_dedup() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let members = ring.get_members(b"key", 10);
        let ids: Vec<u32> = members.iter().map(|m| m.id).collect();
        let unique: std::collections::HashSet<u32> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn stats_total_members() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let stats = ring.stats();
        assert_eq!(stats.total_members, 2);
    }

    #[test]
    fn stats_total_virtual_nodes() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let stats = ring.stats();
        assert_eq!(stats.total_virtual_nodes, 300);
    }

    #[test]
    fn consistent_hashing_same_key() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        let m1 = ring.get_member(b"consistent_key");
        let m2 = ring.get_member(b"consistent_key");
        assert_eq!(m1, m2);
    }

    #[test]
    fn distribution_reasonable() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        ring.add_member(RingMember {
            id: 3,
            label: "node3".to_string(),
        });

        let mut counts = [0usize; 3];
        for i in 0..1000 {
            let key = format!("key{}", i);
            if let Some(m) = ring.get_member(key.as_bytes()) {
                counts[(m.id - 1) as usize] += 1;
            }
        }

        for count in counts.iter() {
            assert!(*count > 100, "Each member should get >10% of 1000 keys");
        }
    }

    #[test]
    fn add_remove_member() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        ring.remove_member(1);
        assert_eq!(ring.member_count(), 1);
    }

    #[test]
    fn ring_member_equality() {
        let m1 = RingMember {
            id: 1,
            label: "node1".to_string(),
        };
        let m2 = RingMember {
            id: 1,
            label: "node1".to_string(),
        };
        let m3 = RingMember {
            id: 2,
            label: "node2".to_string(),
        };
        assert_eq!(m1, m2);
        assert_ne!(m1, m3);
    }

    #[test]
    fn get_members_returns_ordered() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        ring.add_member(RingMember {
            id: 2,
            label: "node2".to_string(),
        });
        ring.add_member(RingMember {
            id: 3,
            label: "node3".to_string(),
        });
        let members = ring.get_members(b"key", 3);
        let ids: Vec<u32> = members.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn wrap_around() {
        let mut ring = HashRing::new(HashRingConfig::default());
        ring.add_member(RingMember {
            id: 1,
            label: "node1".to_string(),
        });
        let member = ring.get_member(b"\xff\xff\xff\xff\xff\xff\xff\xff");
        assert!(member.is_some());
    }

    #[test]
    fn large_ring() {
        let mut ring = HashRing::new(HashRingConfig::default());
        for i in 1..=10 {
            ring.add_member(RingMember {
                id: i,
                label: format!("node{}", i),
            });
        }

        let mut counts = [0usize; 10];
        for i in 0..1000 {
            let key = format!("key{}", i);
            if let Some(m) = ring.get_member(key.as_bytes()) {
                counts[(m.id - 1) as usize] += 1;
            }
        }

        let nonzero = counts.iter().filter(|&&c| c > 0).count();
        assert_eq!(nonzero, 10);
    }
}
