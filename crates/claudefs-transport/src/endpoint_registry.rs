//! Endpoint registry: maps NodeId to transport addresses for RDMA/TCP routing.

use crate::routing::NodeId;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

/// A network transport address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TransportAddr {
    /// TCP socket address.
    Tcp(std::net::SocketAddr),
    /// RDMA fabric address (host + port).
    Rdma {
        /// RDMA host address.
        host: String,
        /// RDMA port number.
        port: u16,
    },
}

/// Protocol preference for address selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolPreference {
    /// Prefer RDMA, fall back to TCP.
    RdmaFirst,
    /// TCP only (default).
    #[default]
    TcpOnly,
}

/// A list of addresses for one node.
#[derive(Debug, Clone)]
pub struct EndpointList {
    /// The node identifier.
    pub node_id: NodeId,
    /// Available transport addresses.
    pub addrs: Vec<TransportAddr>,
    /// Address selection preference.
    pub preference: ProtocolPreference,
    /// Expiry time; None means static/pinned (never expires).
    pub expires_at: Option<Instant>,
}

/// Configuration for the endpoint registry.
#[derive(Debug, Clone)]
pub struct EndpointRegistryConfig {
    /// TTL in seconds for gossip-learned entries. Default: 60.
    pub ttl_secs: u64,
    /// Maximum number of registry entries. Default: 4096.
    pub max_entries: usize,
}

impl Default for EndpointRegistryConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 60,
            max_entries: 4096,
        }
    }
}

/// Stats snapshot returned by [`EndpointRegistry::stats`].
#[derive(Debug, Clone, Default)]
pub struct EndpointRegistryStats {
    /// Current number of entries.
    pub entries: usize,
    /// Total resolve() calls.
    pub lookups: u64,
    /// Lookups that found a valid (non-expired) entry.
    pub hits: u64,
    /// Lookups that found no entry.
    pub misses: u64,
    /// Number of stale entries evicted.
    pub stale_evictions: u64,
    /// Number of static (non-expiring) entries.
    pub static_entries: usize,
}

struct Inner {
    entries: HashMap<NodeId, EndpointList>,
    stats: EndpointRegistryStats,
    config: EndpointRegistryConfig,
}

/// Thread-safe cluster endpoint registry.
pub struct EndpointRegistry {
    inner: RwLock<Inner>,
}

impl EndpointRegistry {
    /// Creates a new endpoint registry with the given configuration.
    pub fn new(config: EndpointRegistryConfig) -> Self {
        Self {
            inner: RwLock::new(Inner {
                entries: HashMap::new(),
                stats: EndpointRegistryStats::default(),
                config,
            }),
        }
    }

    /// Registers a static endpoint with no expiry (never expires).
    pub fn register_static(
        &self,
        node_id: NodeId,
        addrs: Vec<TransportAddr>,
        pref: ProtocolPreference,
    ) {
        let mut inner = self.inner.write().unwrap();
        inner.entries.insert(
            node_id,
            EndpointList {
                node_id,
                addrs,
                preference: pref,
                expires_at: None,
            },
        );
        inner.stats.entries = inner.entries.len();
        inner.stats.static_entries = inner
            .entries
            .values()
            .filter(|e| e.expires_at.is_none())
            .count();
    }

    /// Registers a gossip-learned endpoint with TTL-based expiry.
    pub fn register_gossip(&self, node_id: NodeId, addrs: Vec<TransportAddr>) {
        let mut inner = self.inner.write().unwrap();
        let ttl = inner.config.ttl_secs;
        let expires_at = Instant::now() + std::time::Duration::from_secs(ttl);

        // Check if we need to evict to make room
        if inner.entries.len() >= inner.config.max_entries && !inner.entries.contains_key(&node_id)
        {
            // Find entry to evict: prefer expired, then static, then earliest expiry
            let mut evict_target: Option<NodeId> = None;

            // First: try to evict an expired entry
            for (id, entry) in inner.entries.iter() {
                if let Some(exp) = entry.expires_at {
                    if exp <= Instant::now() {
                        evict_target = Some(*id);
                        break;
                    }
                }
            }

            // Second: if no expired, try to evict a static entry (static entries are less valuable)
            if evict_target.is_none() {
                for (id, entry) in inner.entries.iter() {
                    if entry.expires_at.is_none() {
                        evict_target = Some(*id);
                        break;
                    }
                }
            }

            // Third: if no static, evict the gossip entry with earliest expiry
            if evict_target.is_none() {
                let mut earliest: Option<(Instant, NodeId)> = None;
                for (id, entry) in inner.entries.iter() {
                    if let Some(exp) = entry.expires_at {
                        if let Some((earliest_exp, _)) = earliest {
                            if exp < earliest_exp {
                                earliest = Some((exp, *id));
                            }
                        } else {
                            earliest = Some((exp, *id));
                        }
                    }
                }
                if let Some((_, id)) = earliest {
                    evict_target = Some(id);
                }
            }

            if let Some(target) = evict_target {
                inner.entries.remove(&target);
                if inner.entries.get(&target).is_none() {
                    inner.stats.stale_evictions += 1;
                }
            }
        }

        inner.entries.insert(
            node_id,
            EndpointList {
                node_id,
                addrs,
                preference: ProtocolPreference::TcpOnly,
                expires_at: Some(expires_at),
            },
        );
        inner.stats.entries = inner.entries.len();
        inner.stats.static_entries = inner
            .entries
            .values()
            .filter(|e| e.expires_at.is_none())
            .count();
    }

    /// Resolves the best address for a node based on its preference.
    /// Returns `None` if not found or expired.
    pub fn resolve(&self, node_id: &NodeId) -> Option<TransportAddr> {
        let inner = self.inner.write().unwrap();
        let mut stats = inner.stats.clone();
        stats.lookups += 1;

        let entry = match inner.entries.get(node_id) {
            Some(e) => e.clone(),
            None => {
                drop(inner);
                let mut inner = self.inner.write().unwrap();
                inner.stats = stats;
                inner.stats.misses += 1;
                return None;
            }
        };

        // Check expiry
        if let Some(exp) = entry.expires_at {
            if exp <= Instant::now() {
                drop(inner);
                let mut inner = self.inner.write().unwrap();
                inner.stats = stats;
                inner.stats.misses += 1;
                inner.entries.remove(node_id);
                inner.stats.entries = inner.entries.len();
                inner.stats.static_entries = inner
                    .entries
                    .values()
                    .filter(|e| e.expires_at.is_none())
                    .count();
                inner.stats.stale_evictions += 1;
                return None;
            }
        }

        // Filter by preference
        let filtered = match entry.preference {
            ProtocolPreference::TcpOnly => entry
                .addrs
                .iter()
                .filter(|a| matches!(a, TransportAddr::Tcp(_)))
                .cloned()
                .collect::<Vec<_>>(),
            ProtocolPreference::RdmaFirst => {
                let rdma: Vec<_> = entry
                    .addrs
                    .iter()
                    .filter(|a| matches!(a, TransportAddr::Rdma { .. }))
                    .cloned()
                    .collect();
                if rdma.is_empty() {
                    entry
                        .addrs
                        .iter()
                        .filter(|a| matches!(a, TransportAddr::Tcp(_)))
                        .cloned()
                        .collect()
                } else {
                    rdma
                }
            }
        };

        let result = filtered.into_iter().next();

        drop(inner);
        let mut inner = self.inner.write().unwrap();
        inner.stats = stats;
        inner.stats.hits += 1;

        result
    }

    /// Returns all addresses for a node, filtered by preference.
    /// Returns empty vector if not found or expired.
    pub fn resolve_all(&self, node_id: &NodeId) -> Vec<TransportAddr> {
        let inner = self.inner.write().unwrap();

        let entry = match inner.entries.get(node_id) {
            Some(e) => e.clone(),
            None => return Vec::new(),
        };

        // Check expiry
        if let Some(exp) = entry.expires_at {
            if exp <= Instant::now() {
                drop(inner);
                let mut inner = self.inner.write().unwrap();
                inner.entries.remove(node_id);
                inner.stats.entries = inner.entries.len();
                inner.stats.static_entries = inner
                    .entries
                    .values()
                    .filter(|e| e.expires_at.is_none())
                    .count();
                inner.stats.stale_evictions += 1;
                return Vec::new();
            }
        }

        // Filter by preference
        match entry.preference {
            ProtocolPreference::TcpOnly => entry
                .addrs
                .iter()
                .filter(|a| matches!(a, TransportAddr::Tcp(_)))
                .cloned()
                .collect(),
            ProtocolPreference::RdmaFirst => {
                let mut rdma: Vec<_> = entry
                    .addrs
                    .iter()
                    .filter(|a| matches!(a, TransportAddr::Rdma { .. }))
                    .cloned()
                    .collect();
                if rdma.is_empty() {
                    entry
                        .addrs
                        .iter()
                        .filter(|a| matches!(a, TransportAddr::Tcp(_)))
                        .cloned()
                        .collect()
                } else {
                    let mut tcp: Vec<_> = entry
                        .addrs
                        .iter()
                        .filter(|a| matches!(a, TransportAddr::Tcp(_)))
                        .cloned()
                        .collect();
                    rdma.append(&mut tcp);
                    rdma
                }
            }
        }
    }

    /// Removes an entry from the registry.
    /// Returns `true` if the entry was present, `false` otherwise.
    pub fn remove(&self, node_id: &NodeId) -> bool {
        let mut inner = self.inner.write().unwrap();
        let removed = inner.entries.remove(node_id).is_some();
        if removed {
            inner.stats.entries = inner.entries.len();
            inner.stats.static_entries = inner
                .entries
                .values()
                .filter(|e| e.expires_at.is_none())
                .count();
        }
        removed
    }

    /// Evicts all expired entries from the registry.
    /// Returns the count of removed entries.
    pub fn evict_expired(&self) -> usize {
        let mut inner = self.inner.write().unwrap();
        let now = Instant::now();

        let expired_ids: Vec<NodeId> = inner
            .entries
            .iter()
            .filter(|(_, e)| e.expires_at.map_or(false, |exp| exp <= now))
            .map(|(id, _)| *id)
            .collect();

        for id in &expired_ids {
            inner.entries.remove(id);
        }

        inner.stats.stale_evictions += expired_ids.len() as u64;
        inner.stats.entries = inner.entries.len();
        inner.stats.static_entries = inner
            .entries
            .values()
            .filter(|e| e.expires_at.is_none())
            .count();

        expired_ids.len()
    }

    /// Returns all known (non-expired) node IDs.
    pub fn known_nodes(&self) -> Vec<NodeId> {
        let inner = self.inner.read().unwrap();
        let now = Instant::now();

        inner
            .entries
            .iter()
            .filter(|(_, e)| e.expires_at.map_or(true, |exp| exp > now))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns a snapshot of the current registry statistics.
    pub fn stats(&self) -> EndpointRegistryStats {
        let inner = self.inner.read().unwrap();
        inner.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::thread;
    use std::time::Duration;

    fn tcp_addr(port: u16) -> TransportAddr {
        TransportAddr::Tcp(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port,
        ))
    }

    fn rdma_addr(host: &str, port: u16) -> TransportAddr {
        TransportAddr::Rdma {
            host: host.to_string(),
            port,
        }
    }

    fn node_id(v: u64) -> NodeId {
        NodeId::new(v)
    }

    #[test]
    fn test_static_registration_and_resolve() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);
        let addr = tcp_addr(8080);

        registry.register_static(id, vec![addr.clone()], ProtocolPreference::TcpOnly);

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, Some(addr));
    }

    #[test]
    fn test_tcp_only_filters_rdma() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![rdma_addr("host1", 5000), tcp_addr(8080)],
            ProtocolPreference::TcpOnly,
        );

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Tcp(_))));
    }

    #[test]
    fn test_rdma_first_returns_rdma_before_tcp() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![tcp_addr(8080), rdma_addr("host1", 5000)],
            ProtocolPreference::RdmaFirst,
        );

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Rdma { .. })));
    }

    #[test]
    fn test_rdma_first_falls_back_to_tcp() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::RdmaFirst);

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Tcp(_))));
    }

    #[test]
    fn test_rdma_first_with_no_rdma_returns_tcp() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::RdmaFirst);

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Tcp(_))));
    }

    #[test]
    fn test_gossip_entry_with_future_expiry_resolves() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);
        let addr = tcp_addr(8080);

        registry.register_gossip(id, vec![addr.clone()]);

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, Some(addr));
    }

    #[test]
    fn test_gossip_entry_with_past_expiry_returns_none() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });
        let id = node_id(1);
        let addr = tcp_addr(8080);

        registry.register_gossip(id, vec![addr]);

        // Wait a tiny bit to ensure expiry
        thread::sleep(Duration::from_millis(10));

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_expired_gossip_entry_is_evicted() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });
        let id = node_id(1);

        registry.register_gossip(id, vec![tcp_addr(8080)]);

        thread::sleep(Duration::from_millis(10));

        registry.resolve(&id);

        let nodes = registry.known_nodes();
        assert!(!nodes.contains(&id));
    }

    #[test]
    fn test_evict_expired_returns_count() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });

        registry.register_gossip(node_id(1), vec![tcp_addr(8080)]);
        registry.register_gossip(node_id(2), vec![tcp_addr(8081)]);

        thread::sleep(Duration::from_millis(10));

        let count = registry.evict_expired();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_evict_expired_clears_entries() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });

        registry.register_gossip(node_id(1), vec![tcp_addr(8080)]);

        thread::sleep(Duration::from_millis(10));

        registry.evict_expired();

        let stats = registry.stats();
        assert_eq!(stats.entries, 0);
    }

    #[test]
    fn test_remove_returns_true_for_known() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);

        let removed = registry.remove(&id);
        assert!(removed);
    }

    #[test]
    fn test_remove_returns_false_for_unknown() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        let removed = registry.remove(&id);
        assert!(!removed);
    }

    #[test]
    fn test_remove_clears_entry() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);
        registry.remove(&id);

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_overwrite_static_with_register_static() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);
        registry.register_static(id, vec![tcp_addr(9090)], ProtocolPreference::RdmaFirst);

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Tcp(_))));
        assert_eq!(resolved.unwrap(), tcp_addr(9090));
    }

    #[test]
    fn test_resolve_all_ordering_with_rdma_first() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![tcp_addr(8080), rdma_addr("host1", 5000), tcp_addr(8081)],
            ProtocolPreference::RdmaFirst,
        );

        let all = registry.resolve_all(&id);
        assert_eq!(all.len(), 3);
        assert!(matches!(all[0], TransportAddr::Rdma { .. }));
    }

    #[test]
    fn test_resolve_unknown_returns_none() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(999);

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_stats_hits() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);
        registry.resolve(&id);
        registry.resolve(&id);

        let stats = registry.stats();
        assert_eq!(stats.lookups, 2);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_stats_misses() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());

        registry.resolve(&node_id(1));
        registry.resolve(&node_id(2));

        let stats = registry.stats();
        assert_eq!(stats.lookups, 2);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
    }

    #[test]
    fn test_stats_stale_evictions() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });
        let id = node_id(1);

        registry.register_gossip(id, vec![tcp_addr(8080)]);

        thread::sleep(Duration::from_millis(10));

        registry.resolve(&id);

        let stats = registry.stats();
        assert_eq!(stats.stale_evictions, 1);
    }

    #[test]
    fn test_stats_entries() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());

        registry.register_static(
            node_id(1),
            vec![tcp_addr(8080)],
            ProtocolPreference::TcpOnly,
        );
        registry.register_static(
            node_id(2),
            vec![tcp_addr(8081)],
            ProtocolPreference::TcpOnly,
        );

        let stats = registry.stats();
        assert_eq!(stats.entries, 2);
    }

    #[test]
    fn test_stats_static_entries() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());

        registry.register_static(
            node_id(1),
            vec![tcp_addr(8080)],
            ProtocolPreference::TcpOnly,
        );
        registry.register_gossip(node_id(2), vec![tcp_addr(8081)]);

        let stats = registry.stats();
        assert_eq!(stats.static_entries, 1);
    }

    #[test]
    fn test_static_entry_never_expires() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);

        thread::sleep(Duration::from_millis(50));

        let resolved = registry.resolve(&id);
        assert!(resolved.is_some());
    }

    #[test]
    fn test_max_entries_limit() {
        let config = EndpointRegistryConfig {
            ttl_secs: 60,
            max_entries: 3,
        };
        let registry = EndpointRegistry::new(config);

        // Register 3 entries
        for i in 1..=3 {
            registry.register_gossip(node_id(i), vec![tcp_addr((8000 + i) as u16)]);
        }

        // Try to add a 4th - should trigger eviction
        registry.register_gossip(node_id(4), vec![tcp_addr(8084)]);

        let stats = registry.stats();
        assert!(stats.entries <= 3);
    }

    #[test]
    fn test_concurrent_register_and_resolve() {
        use std::sync::Arc;
        let registry = Arc::new(EndpointRegistry::new(EndpointRegistryConfig::default()));
        let id = node_id(1);

        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);

        let reg_clone = Arc::clone(&registry);
        let id_clone = id;

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                reg_clone.resolve(&id_clone);
            }
        });

        for _ in 0..100 {
            registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);
        }

        handle.join().unwrap();

        assert!(registry.resolve(&id).is_some());
    }

    #[test]
    fn test_concurrent_multiple_nodes() {
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let reg = EndpointRegistry::new(EndpointRegistryConfig::default());
                let id = node_id(i);
                reg.register_static(
                    id,
                    vec![tcp_addr((8000 + i) as u16)],
                    ProtocolPreference::TcpOnly,
                );
                (reg, id)
            })
            .map(|(reg, id)| {
                thread::spawn(move || {
                    for _ in 0..50 {
                        reg.resolve(&id);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_multiple_addrs_per_node() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![tcp_addr(8080), tcp_addr(8081), rdma_addr("host1", 5000)],
            ProtocolPreference::TcpOnly,
        );

        let all = registry.resolve_all(&id);
        assert_eq!(all.len(), 2); // Only TCP due to TcpOnly preference
    }

    #[test]
    fn test_multiple_addrs_rdma_first() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![
                tcp_addr(8080),
                rdma_addr("host1", 5000),
                rdma_addr("host2", 5001),
                tcp_addr(8081),
            ],
            ProtocolPreference::RdmaFirst,
        );

        let all = registry.resolve_all(&id);
        assert_eq!(all.len(), 4);
        assert!(matches!(all[0], TransportAddr::Rdma { .. }));
        assert!(matches!(all[1], TransportAddr::Rdma { .. }));
    }

    #[test]
    fn test_default_config_values() {
        let config = EndpointRegistryConfig::default();
        assert_eq!(config.ttl_secs, 60);
        assert_eq!(config.max_entries, 4096);
    }

    #[test]
    fn test_empty_registry_known_nodes() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let nodes = registry.known_nodes();
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_empty_registry_stats() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let stats = registry.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.static_entries, 0);
    }

    #[test]
    fn test_gossip_overwrites_previous() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_gossip(id, vec![tcp_addr(8080)]);
        registry.register_gossip(id, vec![tcp_addr(9090)]);

        let resolved = registry.resolve(&id);
        assert_eq!(resolved, Some(tcp_addr(9090)));
    }

    #[test]
    fn test_known_nodes_excludes_expired() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });

        registry.register_static(
            node_id(1),
            vec![tcp_addr(8080)],
            ProtocolPreference::TcpOnly,
        );
        registry.register_gossip(node_id(2), vec![tcp_addr(8081)]);

        thread::sleep(Duration::from_millis(10));

        let nodes = registry.known_nodes();
        assert!(nodes.contains(&node_id(1)));
        assert!(!nodes.contains(&node_id(2)));
    }

    #[test]
    fn test_resolve_all_empty_for_unknown() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let all = registry.resolve_all(&node_id(999));
        assert!(all.is_empty());
    }

    #[test]
    fn test_remove_updates_static_entries_count() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());

        registry.register_static(
            node_id(1),
            vec![tcp_addr(8080)],
            ProtocolPreference::TcpOnly,
        );
        registry.register_gossip(node_id(2), vec![tcp_addr(8081)]);

        registry.remove(&node_id(1));

        let stats = registry.stats();
        assert_eq!(stats.static_entries, 0);
    }

    #[test]
    fn test_tcp_only_filters_all_rdma() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_static(
            id,
            vec![rdma_addr("host1", 5000), rdma_addr("host2", 5001)],
            ProtocolPreference::TcpOnly,
        );

        let resolved = registry.resolve(&id);
        assert!(resolved.is_none());

        let all = registry.resolve_all(&id);
        assert!(all.is_empty());
    }

    #[test]
    fn test_resolve_all_returns_none_when_expired() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig {
            ttl_secs: 0,
            ..Default::default()
        });
        let id = node_id(1);

        registry.register_gossip(id, vec![tcp_addr(8080)]);

        thread::sleep(Duration::from_millis(10));

        let all = registry.resolve_all(&id);
        assert!(all.is_empty());
    }

    #[test]
    fn test_gossip_preference_defaults_to_tcp_only() {
        let registry = EndpointRegistry::new(EndpointRegistryConfig::default());
        let id = node_id(1);

        registry.register_gossip(id, vec![tcp_addr(8080), rdma_addr("host1", 5000)]);

        let resolved = registry.resolve(&id);
        assert!(matches!(resolved, Some(TransportAddr::Tcp(_))));
    }
}
