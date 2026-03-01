#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

/// POSIX permission checking
pub mod access;
/// Persistent file-backed KV store with WAL and checkpoint
pub mod btree_store;
/// Change Data Capture (CDC) event streaming
pub mod cdc;
/// Conflict detection and resolution for cross-site replication
pub mod conflict;
/// Raft consensus implementation
pub mod consensus;
/// Cross-shard operation coordinator using two-phase commit
pub mod cross_shard;
/// Directory operations
pub mod directory;
/// Directory sharding for hot directories
pub mod dirshard;
/// Open file handle management
pub mod filehandle;
/// CAS fingerprint index for deduplication
pub mod fingerprint;
/// Read-only follower query routing for relaxed POSIX mode
pub mod follower_read;
/// Inode operations
pub mod inode;
/// Inode generation numbers for NFS export consistency
pub mod inode_gen;
/// Metadata journal for replication
pub mod journal;
/// Journal tailing API for cross-site replication
pub mod journal_tailer;
/// Embedded key-value store
pub mod kvstore;
/// Lease-based metadata caching
pub mod lease;
/// Automatic lease renewal with configurable TTL threshold
pub mod lease_renew;
/// Distributed lock manager
pub mod locking;
/// SWIM-based cluster membership tracking
pub mod membership;
/// Metadata service metrics collector
pub mod metrics;
/// Multi-Raft group manager
pub mod multiraft;
/// Negative lookup cache
pub mod neg_cache;
/// Unified metadata server node
pub mod node;
/// MetadataNode snapshot and restore for disaster recovery
pub mod node_snapshot;
/// Speculative path resolution with caching
pub mod pathres;
/// Prefetch engine for metadata
pub mod prefetch;
/// QoS (Quality of Service) and traffic shaping for metadata operations
pub mod qos;
/// Per-user/group quota management
pub mod quota;
/// Persistent Raft log store for crash-safe consensus state
pub mod raft_log;
/// Raft-integrated metadata service (Phase 2)
pub mod raftservice;
/// Linearizable reads via ReadIndex protocol
pub mod readindex;
/// Cross-site replication
pub mod replication;
/// Async metadata RPC protocol types for transport integration
pub mod rpc;
/// Online node scaling and shard rebalancing
pub mod scaling;
/// High-level metadata service API
pub mod service;
/// Shard routing for distributed metadata
pub mod shard;
/// Raft log snapshot and compaction
pub mod snapshot;
/// Multi-tenant namespace isolation
pub mod tenant;
/// Distributed tracing context for metadata operations
pub mod tracecontext;
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
pub use btree_store::PersistentKvStore;
pub use cdc::{CdcCursor, CdcEvent, CdcStream};
pub use conflict::{ConflictDetector, ConflictEvent, ConflictWinner};
pub use cross_shard::{CrossShardCoordinator, CrossShardResult};
pub use dirshard::{DirShardConfig, DirShardManager, DirShardState};
pub use filehandle::{FileHandle, FileHandleManager, OpenFlags};
pub use fingerprint::FingerprintIndex;
pub use follower_read::{FollowerReadConfig, FollowerReadRouter, ReadConsistency, ReadTarget};
pub use inode_gen::{Generation, InodeGenManager, NfsFileHandle};
pub use journal_tailer::{JournalTailer, ReplicationBatch, TailerConfig, TailerCursor};
pub use kvstore::{KvStore, MemoryKvStore};
pub use lease::{LeaseManager, LeaseType};
pub use lease_renew::{LeaseRenewConfig, LeaseRenewManager, RenewalAction};
pub use locking::{LockManager, LockType};
pub use membership::{MemberInfo, MembershipEvent, MembershipManager, NodeState};
pub use metrics::{MetadataMetrics, MetricOp, MetricsCollector, OpMetrics};
pub use multiraft::MultiRaftManager;
pub use neg_cache::{NegCacheConfig, NegativeCache};
pub use node::{ClusterStatus, DirEntryPlus, MetadataNode, MetadataNodeConfig, StatFs};
pub use node_snapshot::NodeSnapshot;
pub use pathres::{NegativeCacheEntry, PathCacheEntry, PathResolver};
pub use qos::{QosClass, QosManager, QosPolicy};
pub use quota::{QuotaEntry, QuotaLimit, QuotaManager, QuotaTarget, QuotaUsage};
pub use raft_log::RaftLogStore;
pub use raftservice::{RaftMetadataService, RaftServiceConfig};
pub use readindex::{PendingRead, ReadIndexManager, ReadStatus};
pub use rpc::{MetadataRequest, MetadataResponse, RpcDispatcher};
pub use scaling::{MigrationStatus, MigrationTask, ScalingManager, ShardPlacement};
pub use service::MetadataService;
pub use service::MetadataServiceConfig;
pub use shard::{ShardAssigner, ShardInfo, ShardRouter};
pub use snapshot::{RaftSnapshot, SnapshotManager};
pub use tenant::{TenantConfig, TenantId, TenantManager, TenantUsage};
pub use tracecontext::{SpanId, SpanRecord, SpanStatus, TraceCollector, TraceContext, TraceId};
pub use transaction::{Transaction, TransactionId, TransactionManager, TransactionState};
pub use watch::{Watch, WatchEvent, WatchManager};
pub use worm::{RetentionPolicy, WormAuditEvent, WormEntry, WormManager, WormState};
pub use xattr::XattrStore;

/// Re-export key types for external users
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
