# Task: Write endpoint_registry.rs for claudefs-transport crate

Write the complete file to `crates/claudefs-transport/src/endpoint_registry.rs`.

## Context

This crate is part of ClaudeFS, a distributed POSIX file system in Rust. The crate has:
- `#![warn(missing_docs)]` in lib.rs - add doc comments to all pub items
- Dependencies: serde, thiserror available (but user requests NOT using thiserror for this file)
- NodeId type is defined in `crate::routing::NodeId` as:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct NodeId(u64);
// with new(id: u64), as_u64() -> u64, From<u64>, From<NodeId> for u64
```

## Types to implement

```rust
//! Endpoint registry: maps NodeId to transport addresses for RDMA/TCP routing.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;
use crate::routing::NodeId;

/// A network transport address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TransportAddr {
    /// TCP socket address.
    Tcp(std::net::SocketAddr),
    /// RDMA fabric address (host + port).
    Rdma { host: String, port: u16 },
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
        Self { ttl_secs: 60, max_entries: 4096 }
    }
}

/// Stats snapshot returned by EndpointRegistry::stats().
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
```

## EndpointRegistry methods to implement

1. `new(config: EndpointRegistryConfig) -> Self` - Create new registry with config

2. `register_static(&self, node_id: NodeId, addrs: Vec<TransportAddr>, pref: ProtocolPreference)` - Register with no expiry (expires_at = None)

3. `register_gossip(&self, node_id: NodeId, addrs: Vec<TransportAddr>)` - Register with expiry at now + ttl_secs. Preference defaults to TcpOnly. When adding would exceed max_entries, evict the entry with the soonest expires_at (prefer evicting expired entries over static ones first, then non-expired gossip entries by earliest expiry).

4. `resolve(&self, node_id: &NodeId) -> Option<TransportAddr>` - Returns best addr per preference. Returns None if not found or expired. Increments stats.lookups and either hits or misses. Evicts the entry if expired (increments stale_evictions). For TcpOnly: only return Tcp variants. For RdmaFirst: return RDMA addrs first, then TCP addrs if no RDMA available.

5. `resolve_all(&self, node_id: &NodeId) -> Vec<TransportAddr>` - All addrs for node, filtered by preference. Empty if not found/expired. Same expiry behavior as resolve.

6. `remove(&self, node_id: &NodeId) -> bool` - Remove entry, returns true if was present

7. `evict_expired(&self) -> usize` - Remove all expired entries, return count removed. Update stale_evictions and entries stats.

8. `known_nodes(&self) -> Vec<NodeId>` - All non-expired node IDs

9. `stats(&self) -> EndpointRegistryStats` - Return a clone of current stats

## Address selection logic

For `TcpOnly` preference: filter to only `TransportAddr::Tcp` variants.
For `RdmaFirst` preference: return RDMA addresses first (all Rdma variants), then TCP addresses.

When resolving, if the filtered list is empty, return None (for resolve()) or empty Vec (for resolve_all()).

## Tests (30+ required)

Add a `#[cfg(test)] mod tests { ... }` with at least 30 tests covering:
- Static registration and resolve
- TcpOnly filters out RDMA addresses
- RdmaFirst returns RDMA before TCP
- Gossip entry with future expiry resolves OK
- Gossip entry with past expiry returns None on resolve (and is evicted)
- evict_expired() returns count and clears expired entries
- remove() returns true for known, false for unknown
- Overwriting static with second register_static
- resolve_all ordering with RdmaFirst
- resolve_unknown returns None
- stats (hits, misses, stale_evictions, entries, static_entries)
- max_entries limit enforcement
- Static entry never expires
- Thread safety (spawn threads, concurrent register+resolve)
- Multiple addrs per node
- RdmaFirst with no RDMA falls back to TCP
- Default config values (ttl_secs: 60, max_entries: 4096)
- Empty registry behavior
- Overwriting gossip entry with new addrs

## Important notes

- DO NOT use thiserror - use plain error types or avoid Result entirely
- Use `serde::{Serialize, Deserialize}` derive on TransportAddr (already shown in type definition)
- No async, no tokio - this is synchronous code using RwLock
- `#![warn(missing_docs)]` is on the crate - add doc comments to ALL pub items
- No `println!` in production code
- Use `std::time::Instant::now()` for current time
- The struct Inner is private (no pub), EndpointRegistry is public

## File header

Start with the module doc comment shown above:
```rust
//! Endpoint registry: maps NodeId to transport addresses for RDMA/TCP routing.
```

Write the COMPLETE file to `crates/claudefs-transport/src/endpoint_registry.rs`.