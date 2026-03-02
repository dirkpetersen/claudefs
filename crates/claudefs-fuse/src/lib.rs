#![warn(missing_docs)]

//! ClaudeFS FUSE subsystem.

/// File attributes (mode, uid, gid, size, times), attribute synchronization.
pub mod attr;
/// Memory buffer pool, efficient buffer management and reuse.
pub mod buffer_pool;
/// Distributed cache coherence, lease invalidation, TTL-based expiry.
pub mod cache;
/// Cache coherence protocols (close-to-open, session-based, strict).
pub mod cache_coherence;
/// POSIX capability enforcement, privilege handling.
pub mod capability;
/// mTLS client authentication, certificate validation, session tokens.
pub mod client_auth;
/// Crash recovery, state restoration, in-flight operation recovery.
pub mod crash_recovery;
/// Data caching for hot files, read-ahead cache.
pub mod datacache;
/// File delegation management, delegation grants/revokes.
pub mod deleg;
/// Directory entry caching, listing cache, TTL management.
pub mod dir_cache;
/// Directory change notifications, watch management.
pub mod dirnotify;
/// Error types and result handling.
pub mod error;
/// File access advise (fadvise/madvise), hint processing.
pub mod fadvise;
/// Space preallocation (fallocate), extent management.
pub mod fallocate;
/// Core FUSE operation handlers, async I/O dispatch.
pub mod filesystem;
/// POSIX file locking (fcntl locks), lock table management.
pub mod flock;
/// Server health monitoring, failure detection, recovery triggering.
pub mod health;
/// UID/GID mapping across security domains.
pub mod idmap;
/// Inode table, LRU eviction, in-memory inode cache.
pub mod inode;
/// Signal handling (SIGINT, SIGTERM), graceful shutdown.
pub mod interrupt;
/// Per-process I/O priority classes, priority inheritance.
pub mod io_priority;
/// Low-level locking primitives, synchronization utilities.
pub mod locking;
/// Data migration between tiers, migration tracking.
pub mod migration;
/// Memory-mapped file support, mmap coherence.
pub mod mmap;
/// Mount point initialization, FUSE daemon startup, lifecycle management.
pub mod mount;
/// Mount option parsing, validation, configuration defaults.
pub mod mount_opts;
/// Multipath failover with multiple server paths, path selection.
pub mod multipath;
/// Directory notification filtering, exclusion patterns.
pub mod notify_filter;
/// Open file handle tracking, descriptor reuse, handle-to-inode mapping.
pub mod openfile;
/// Individual operation implementations (read, write, stat, etc.).
pub mod operations;
/// OpenTelemetry integration, trace export.
pub mod otel_trace;
/// Kernel FUSE passthrough mode (6.8+), direct NVMe I/O.
pub mod passthrough;
/// Path name resolution, metadata lookups.
pub mod path_resolver;
/// Performance metric collection, latency profiling, per-operation stats.
pub mod perf;
/// POSIX ACL enforcement, permission checking.
pub mod posix_acl;
/// Read-ahead prefetching, adaptive prefetch tuning, sequential pattern detection.
pub mod prefetch;
/// Quota enforcement per user/group, soft/hard limits.
pub mod quota_enforce;
/// Rate limiting and backpressure, per-class rate control.
pub mod ratelimit;
/// Automatic reconnection with exponential backoff, connection state.
pub mod reconnect;
/// Security policy enforcement, namespace isolation, syscall filtering.
pub mod sec_policy;
/// FUSE daemon lifecycle and I/O loop management.
pub mod server;
/// Session state tracking, per-client context management.
pub mod session;
/// Snapshot metadata tracking, snapshot creation and restoration.
pub mod snapshot;
/// Symbolic link operations, link resolution.
pub mod symlink;
/// S3 tiering hints for intelligent storage placement, heat tracking.
pub mod tiering_hints;
/// Distributed request tracing, span management.
pub mod tracing_client;
/// Transport abstraction layer, pluggable RDMA/TCP.
pub mod transport;
/// Workload classification (database, streaming, AI, web), adaptive tuning.
pub mod workload_class;
/// WORM (Write-Once-Read-Many) enforcement, legal holds, retention.
pub mod worm;
/// Write buffer, threshold-based flushing, range coalescing.
pub mod writebuf;
/// Extended attributes (xattrs), set/get/list/remove operations.
pub mod xattr;

pub use error::{FuseError, Result};
