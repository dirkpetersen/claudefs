#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

/// POSIX permission checking
pub mod access;
/// Change Data Capture (CDC) event streaming
pub mod cdc;
/// Conflict detection and resolution for cross-site replication
pub mod conflict;
/// Raft consensus implementation
pub mod consensus;
/// Directory operations
pub mod directory;
/// Open file handle management
pub mod filehandle;
/// CAS fingerprint index for deduplication
pub mod fingerprint;
/// Inode operations
pub mod inode;
/// Metadata journal for replication
pub mod journal;
/// Embedded key-value store
pub mod kvstore;
/// Lease-based metadata caching
pub mod lease;
/// Distributed lock manager
pub mod locking;
/// SWIM-based cluster membership tracking
pub mod membership;
/// Metadata service metrics collector
pub mod metrics;
/// Multi-Raft group manager
pub mod multiraft;
/// Speculative path resolution with caching
pub mod pathres;
/// Per-user/group quota management
pub mod quota;
/// Raft-integrated metadata service (Phase 2)
pub mod raftservice;
/// Linearizable reads via ReadIndex protocol
pub mod readindex;
/// Cross-site replication
pub mod replication;
/// Async metadata RPC protocol types for transport integration
pub mod rpc;
/// High-level metadata service API
pub mod service;
/// Shard routing for distributed metadata
pub mod shard;
/// Raft log snapshot and compaction
pub mod snapshot;
/// Distributed transaction coordinator (two-phase commit)
pub mod transaction;
/// Core types for the metadata service
pub mod types;
/// UID/GID mapping for cross-site replication
pub mod uidmap;
/// Watch/notify for directory change events
pub mod watch;
/// WORM (Write Once Read Many) compliance module
pub mod worm;
/// Extended attribute operations
pub mod xattr;

pub use access::{AccessMode, UserContext};
pub use cdc::{CdcCursor, CdcEvent, CdcStream};
pub use conflict::{ConflictDetector, ConflictEvent, ConflictWinner};
pub use filehandle::{FileHandle, FileHandleManager, OpenFlags};
pub use lease::{LeaseManager, LeaseType};
pub use locking::{LockManager, LockType};
pub use metrics::{MetadataMetrics, MetricOp, MetricsCollector, OpMetrics};
pub use multiraft::MultiRaftManager;
pub use pathres::{NegativeCacheEntry, PathCacheEntry, PathResolver};
pub use quota::{QuotaEntry, QuotaLimit, QuotaManager, QuotaTarget, QuotaUsage};
pub use raftservice::{RaftMetadataService, RaftServiceConfig};
pub use readindex::{PendingRead, ReadIndexManager, ReadStatus};
pub use rpc::{MetadataRequest, MetadataResponse, RpcDispatcher};
pub use service::MetadataService;
pub use shard::{ShardAssigner, ShardInfo, ShardRouter};
pub use snapshot::{RaftSnapshot, SnapshotManager};
pub use transaction::{Transaction, TransactionId, TransactionManager, TransactionState};
pub use watch::{Watch, WatchEvent, WatchManager};
pub use worm::{RetentionPolicy, WormAuditEvent, WormEntry, WormManager, WormState};
pub use xattr::XattrStore;

/// Re-export key types for external users
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
