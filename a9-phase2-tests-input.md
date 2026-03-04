# A9 Phase 2: New Test Modules for claudefs-tests Crate

You are writing Rust test code for the `claudefs-tests` crate in the ClaudeFS distributed filesystem project. These tests are in the `crates/claudefs-tests/src/` directory and import from sibling crates.

## Context

A9 (Test & Validation) owns the `claudefs-tests` crate. The crate currently has 1576 tests across ~50 modules. We are adding 3 new test modules for Phase 2 coverage:

1. `fuse_path_resolver_tests.rs` — tests for new PathResolver in claudefs-fuse
2. `mgmt_phase2_tests.rs` — tests for MetricsCollector and MetadataConsumer in claudefs-mgmt
3. `gateway_cluster_backend_tests.rs` — integration tests for ClusterVfsBackend in claudefs-gateway

## Shared Conventions
- Error handling: `thiserror` for library errors, `anyhow` at binary entry points
- Serialization: `serde` + `bincode`
- Async: Tokio with `#[tokio::test]` for async tests
- Logging: `tracing`
- Testing: `proptest` for property-based tests where appropriate
- NO unsafe code in tests

## Module 1: `fuse_path_resolver_tests.rs`

The `claudefs-fuse` crate exposes `pub mod path_resolver` with these types:

```rust
pub struct ResolvedComponent {
    pub name: String,
    pub ino: InodeId,  // type alias for u64 from claudefs_fuse::inode
    pub parent_ino: InodeId,
    pub generation: u64,
}

pub struct ResolvedPath {
    pub path: String,
    pub components: Vec<ResolvedComponent>,
    pub final_ino: InodeId,
    pub resolved_at: Instant,
}

impl ResolvedPath {
    pub fn is_stale(&self, generations: &GenerationTracker) -> bool;
    pub fn depth(&self) -> usize;
}

pub enum PathResolveError {
    ComponentNotFound { name: String, parent: InodeId },
    TooDeep { depth: usize, limit: usize },
    Stale { name: String },
    InvalidPath { reason: String },
}

pub type PathResolveResult<T> = Result<T, PathResolveError>;

pub struct PathResolverConfig {
    pub max_depth: usize,
    pub cache_capacity: usize,
    pub ttl: Duration,
}

pub struct GenerationTracker {
    // private field
}

impl GenerationTracker {
    pub fn new() -> Self;
    pub fn get(&self, ino: InodeId) -> u64;
    pub fn bump(&mut self, ino: InodeId) -> u64;
    pub fn set(&mut self, ino: InodeId, gen: u64);
    pub fn remove(&mut self, ino: InodeId);
}

pub struct PathResolverStats {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub stale_hits: u64,
    pub toctou_detected: u64,
    pub invalidations: u64,
}

pub struct PathResolver {
    // private fields
}

impl PathResolver {
    pub fn new(config: PathResolverConfig) -> Self;
    pub fn insert(&mut self, path: &str, resolved: ResolvedPath);
    pub fn lookup(&mut self, path: &str) -> Option<ResolvedPath>;
    pub fn record_component(&mut self, component: ResolvedComponent);
    pub fn invalidate_prefix(&mut self, path_prefix: &str);
    pub fn bump_generation(&mut self, ino: InodeId) -> u64;
    pub fn is_generation_current(&self, ino: InodeId, gen: u64) -> bool;
    pub fn validate_path(path: &str) -> PathResolveResult<Vec<&str>>;
    pub fn stats(&self) -> &PathResolverStats;
}
```

Note: `InodeId` is `u64` (type alias from `claudefs_fuse::inode::InodeId`).

Write `fuse_path_resolver_tests.rs` with at least 30 tests covering:

1. **PathResolverConfig** — default values (max_depth=64, cache_capacity=1000, ttl=30s), custom config
2. **GenerationTracker** — new, get returns 0 for unknown ino, bump increments, set/get, remove
3. **ResolvedPath::is_stale** — not stale when generations match, stale when bumped
4. **ResolvedPath::depth** — returns component count
5. **PathResolver::validate_path** — valid paths, empty path, absolute path, dotdot, trailing slash, single component, multiple components, whitespace
6. **PathResolver::insert + lookup** — basic cache insert/lookup, cache miss, cache hit increments stats
7. **PathResolver::lookup stale** — insert path, bump generation, lookup returns None (stale hit)
8. **PathResolver::invalidate_prefix** — exact match removed, sub-paths removed, other paths preserved, stats.invalidations incremented
9. **PathResolver::bump_generation** — bumps generation, is_generation_current before/after bump
10. **PathResolver::stats** — initial zeros, hits/misses tracked
11. **Cache capacity eviction** — inserting more than cache_capacity evicts entries
12. **proptest** — validate_path accepts arbitrary non-empty, non-slash-starting, no-dotdot paths

Helper to create a `ResolvedPath` for testing:
```rust
fn make_resolved(path: &str, ino: u64, gen: u64) -> ResolvedPath {
    ResolvedPath {
        path: path.to_string(),
        components: vec![ResolvedComponent {
            name: path.to_string(),
            ino,
            parent_ino: 1,
            generation: gen,
        }],
        final_ino: ino,
        resolved_at: std::time::Instant::now(),
    }
}
```

## Module 2: `mgmt_phase2_tests.rs`

The `claudefs-mgmt` crate exposes:

```rust
// From metrics.rs
pub struct ClusterMetrics {
    // prometheus counters/gauges/histograms
}
impl ClusterMetrics {
    pub fn new() -> Self;
    pub fn render_prometheus(&self) -> String;
    // counters: iops_read, iops_write, bytes_read, bytes_write, replication_conflicts_total
    // gauges: nodes_total, nodes_healthy, nodes_degraded, nodes_offline,
    //         capacity_total_bytes, capacity_used_bytes, capacity_available_bytes,
    //         replication_lag_secs, dedupe_hit_rate, compression_ratio,
    //         s3_queue_depth
    // histograms: latency_read_us, latency_write_us, s3_flush_latency_ms
}

// From metrics_collector.rs
pub struct MetricsCollector {
    // private fields
}
impl MetricsCollector {
    pub fn new(metrics: Arc<ClusterMetrics>, interval_secs: u64) -> Self;
    pub fn start(&self) -> tokio::task::JoinHandle<()>;
    pub fn stop(&self);
}
```

Write `mgmt_phase2_tests.rs` with at least 25 tests covering:

1. **ClusterMetrics::new** — creates without panic
2. **ClusterMetrics::render_prometheus** — initial output contains metric names
3. **Counter increments** — add to iops_read, iops_write, bytes_read, bytes_write; verify in prometheus output
4. **Gauge sets** — set nodes_total, nodes_healthy, capacity values; verify in render
5. **Histogram observe** — observe latency values; verify histogram buckets in output
6. **MetricsCollector::new** — creates with correct interval, not running initially
7. **MetricsCollector start/stop lifecycle** — start sets running flag, stop clears it
8. **MetricsCollector::start returns handle** — handle can be awaited on abort
9. **Prometheus output format** — metric names contain "claudefs_" prefix
10. **Multiple observes** — histogram accumulates count
11. **S3 queue depth** — set s3_queue_depth, verify in output
12. **Dedupe ratio** — set dedupe_hit_rate, compression_ratio, verify in output
13. **Replication lag** — set replication_lag_secs, verify in output
14. **Concurrent access** — MetricsCollector with Arc<ClusterMetrics>, two threads can read/write
15. **proptest** — any positive f64 can be observed in histogram without panic

Note: For async tests use `#[tokio::test]`.

## Module 3: `gateway_cluster_backend_tests.rs`

The `claudefs-gateway` crate exposes:

```rust
// From cluster_backend.rs
pub type NodeInfo = crate::gateway_conn_pool::BackendNode;

#[derive(Debug, Clone, Default)]
pub struct ClusterStats {
    pub total_rpc_calls: u64,
    pub successful_rpcs: u64,
    pub failed_rpcs: u64,
    pub backend_bytes_read: u64,
    pub backend_bytes_written: u64,
    pub last_success: Option<std::time::Instant>,
}

pub struct ClusterVfsBackend {
    // private
}

impl ClusterVfsBackend {
    pub fn new(nodes: Vec<NodeInfo>, config: ConnPoolConfig) -> Self;
    pub fn with_cluster_name(self, name: &str) -> Self;
    pub fn stats(&self) -> ClusterStats;
    // private: fn record_rpc(&self, success: bool)
    // private: fn not_implemented(&self, op: &str) -> GatewayError
}

// VfsBackend trait methods (all return Err(NotImplemented)):
impl VfsBackend for ClusterVfsBackend {
    fn getattr(&self, fh: &FileHandle3) -> Result<Fattr3>;
    fn lookup(&self, dir_fh: &FileHandle3, name: &str) -> Result<LookupResult>;
    fn read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Result<(Vec<u8>, bool)>;
    fn write(&self, fh: &FileHandle3, offset: u64, data: &[u8]) -> Result<u32>;
    fn readdir(&self, dir_fh: &FileHandle3, cookie: u64, count: u32) -> Result<ReadDirResult>;
    fn mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
    fn create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
    fn remove(&self, dir_fh: &FileHandle3, name: &str) -> Result<()>;
    fn rename(&self, from_dir: &FileHandle3, from_name: &str, to_dir: &FileHandle3, to_name: &str) -> Result<()>;
    fn readlink(&self, fh: &FileHandle3) -> Result<String>;
    fn symlink(&self, dir_fh: &FileHandle3, name: &str, target: &str) -> Result<(FileHandle3, Fattr3)>;
    fn fsstat(&self, fh: &FileHandle3) -> Result<FsStatResult>;
    fn fsinfo(&self, fh: &FileHandle3) -> Result<FsInfoResult>;
    fn pathconf(&self, fh: &FileHandle3) -> Result<PathConfResult>;
    fn access(&self, fh: &FileHandle3, uid: u32, gid: u32, access_bits: u32) -> Result<u32>;
}
```

Also from gateway_conn_pool.rs:
```rust
pub struct ConnPoolConfig {
    pub max_connections_per_node: usize,
    pub health_check_interval_ms: u64,
    pub connect_timeout_ms: u64,
}

impl Default for ConnPoolConfig {
    fn default() -> Self { max=10, health=30000, timeout=5000 }
}

pub struct BackendNode {
    pub id: String,
    pub addr: String,
    pub port: u16,
}
```

Write `gateway_cluster_backend_tests.rs` with at least 30 tests covering:

1. **Backend construction** — new with empty nodes, custom cluster name, with_cluster_name builder
2. **Initial stats** — all zero, last_success is None
3. **Error type validation** — each VFS operation returns Err(GatewayError::NotImplemented) containing the op name
4. **Stats accounting per op** — each VFS op call increments total_rpc_calls and failed_rpcs
5. **All 15 VFS ops return errors** — getattr, lookup, read, write, readdir, mkdir, create, remove, rename, readlink, symlink, fsstat, fsinfo, pathconf, access
6. **Stats after multiple calls** — N calls = N total_rpc_calls, N failed_rpcs
7. **FileHandle3 edge cases** — empty fh (1 byte per NFS spec), max size fh (64 bytes), with fh data containing all zeros
8. **Cluster name in error messages** — error feature field mentions the op name
9. **ConnPoolConfig defaults** — max=10, health=30000, timeout=5000
10. **BackendNode construction** — id, addr, port fields set correctly
11. **Multiple backends** — create multiple backends, each has independent stats
12. **Thread safety** — Arc<ClusterVfsBackend> can be used from multiple threads (use std::sync::Arc)
13. **proptest** — any 1-64 byte file handle triggers NotImplemented from getattr

Use these imports:
```rust
use claudefs_gateway::cluster_backend::{ClusterStats, ClusterVfsBackend};
use claudefs_gateway::gateway_conn_pool::{BackendNode, ConnPoolConfig};
use claudefs_gateway::nfs::VfsBackend;
use claudefs_gateway::protocol::FileHandle3;
use claudefs_gateway::error::GatewayError;
```

## Output Format

For each module, output the complete Rust source file, starting with `//!` module doc comment, then all the `use` statements, then all the tests. Use `#[test]` for synchronous tests, `#[tokio::test]` for async.

For proptest tests, use:
```rust
use proptest::prelude::*;
proptest! {
    #[test]
    fn prop_xxx(x in strategy) {
        // ...
    }
}
```

Output all 3 files clearly labeled:
```
=== FILE: fuse_path_resolver_tests.rs ===
<content>
=== FILE: mgmt_phase2_tests.rs ===
<content>
=== FILE: gateway_cluster_backend_tests.rs ===
<content>
```
