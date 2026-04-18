#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

/// POSIX permission checking
pub mod access;
/// Access integration bridging DAC and POSIX ACL permission systems
pub mod access_integration;
/// POSIX Access Control Lists (ACLs) for fine-grained permission control
pub mod acl;
/// Persistent file-backed KV store with WAL and checkpoint
pub mod btree_store;
/// Change Data Capture (CDC) event streaming
pub mod cdc;
/// Metadata checkpoint manager for fast restart
pub mod checkpoint;
/// Concurrent inode operations verification
pub mod concurrent_inode_ops;
/// Conflict detection and resolution for cross-site replication
pub mod conflict;
/// Raft consensus implementation
pub mod consensus;
/// Cross-shard operation coordinator using two-phase commit
pub mod cross_shard;
/// Recursive directory tree walker
pub mod dir_walk;
/// Directory operations
pub mod directory;
/// Directory sharding for hot directories
pub mod dirshard;
/// Open file handle management
pub mod filehandle;
/// CAS fingerprint index for deduplication
pub mod fingerprint;
/// Fingerprint index integration for distributed deduplication
pub mod fingerprint_index_integration;
/// Read-only follower query routing for relaxed POSIX mode
pub mod follower_read;
/// Metadata integrity checker (fsck) for distributed filesystem
pub mod fsck;
/// Metadata garbage collector for orphaned inodes, expired tombstones, stale locks
pub mod gc;
/// Hard link management for efficient file linking
pub mod hardlink;
/// Metadata node health diagnostics and readiness probes
pub mod health;
/// Inode operations
pub mod inode;
/// Inode generation numbers for NFS export consistency
pub mod inode_gen;
/// Metadata journal for replication
pub mod journal;
/// Journal compaction for storage optimization
pub mod journal_compactor;
/// Journal tailing API for cross-site replication
pub mod journal_tailer;
/// Embedded key-value store
pub mod kvstore;
/// Lazy deletion tracking for POSIX unlink-while-open semantics
pub mod lazy_delete;
/// Lease-based metadata caching
pub mod lease;
/// Automatic lease renewal with configurable TTL threshold
pub mod lease_renew;
/// Distributed lock manager
pub mod locking;
/// SWIM-based cluster membership tracking
pub mod membership;
/// SWIM-based cross-site failure detector
pub mod membership_failure_detector;
/// Metadata service metrics collector
pub mod metrics;
/// Modification time tracking for inode updates
pub mod mtime_tracker;
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
/// Quota integration service for write enforcement
pub mod quota_integration;
/// Per-tenant quota tracking with soft/hard limits
pub mod quota_tracker;
/// Cross-site quota configuration replication with conflict resolution
pub mod quota_replication;
/// Multi-tenant namespace isolation for metadata operations
pub mod tenant_isolator;
/// QoS coordination between metadata and transport services
pub mod qos_coordinator;
/// Persistent Raft log store for crash-safe consensus state
pub mod raft_log;
/// Raft-integrated metadata service (Phase 2)
pub mod raftservice;
/// Byte-range lock manager for POSIX file locking
pub mod range_lock;
/// Per-client metadata operation rate limiting
pub mod rate_limit;
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
/// Per-shard statistics for monitoring and rebalancing
pub mod shard_stats;
/// Raft log snapshot and compaction
pub mod snapshot;
/// Space accounting for directories
pub mod space_accounting;
/// Symlink storage and resolution with loop detection
pub mod symlink;
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
/// POSIX umask and mode calculation
pub mod umask;
/// Watch/notify for directory change events
pub mod watch;
/// WORM (Write Once Read Many) compliance module
pub mod worm;
/// Extended attribute operations
pub mod xattr;
/// Distributed transaction coordinator for cross-shard atomic operations
pub mod distributed_transaction;
/// Per-client session state and lease tracking
pub mod client_session;
/// Cross-site snapshot transfer for disaster recovery
pub mod snapshot_transfer;

#[cfg(test)]
mod proptests;

pub use access::{AccessMode, UserContext};
pub use access_integration::{AccessCheckContext, AccessCheckResult, AccessOperation, SetAttrOp};
pub use acl::{Acl, AclEntry, AclStore, AclTag};
pub use btree_store::PersistentKvStore;
pub use cdc::{CdcCursor, CdcEvent, CdcStream};
pub use checkpoint::{Checkpoint, CheckpointManager, CheckpointMeta};
pub use concurrent_inode_ops::{
    AttrChanges, ClientId as ConcurrentClientId, ConcurrentOpContext, InodeOp, LinearizabilityResult, Violation,
};
pub use conflict::{ConflictDetector, ConflictEvent, ConflictWinner};
pub use cross_shard::{CrossShardCoordinator, CrossShardResult};
pub use dir_walk::{DirWalker, WalkConfig, WalkControl, WalkEntry, WalkStats};
pub use dirshard::{DirShardConfig, DirShardManager, DirShardState};
pub use filehandle::{FileHandle, FileHandleManager, OpenFlags};
pub use fingerprint::FingerprintIndex;
pub use fingerprint_index_integration::{
    FingerprintLookupRequest, FingerprintLookupResult, FingerprintRouter, FingerprintRouterConfig,
    FingerprintRouterStats, RemoteCoordinatorInfo,
};
pub use follower_read::{FollowerReadConfig, FollowerReadRouter, ReadConsistency, ReadTarget};
pub use fsck::{FsckConfig, FsckFinding, FsckIssue, FsckRepairAction, FsckReport, FsckSeverity};
pub use hardlink::HardLinkStore;
pub use health::{ComponentHealth, HealthChecker, HealthReport, HealthStatus, HealthThresholds};
pub use inode_gen::{Generation, InodeGenManager, NfsFileHandle};
pub use journal_compactor::{CompactEntry, CompactOpType, CompactionStats, JournalCompactor};
pub use journal_tailer::{JournalTailer, ReplicationBatch, TailerConfig, TailerCursor};
pub use kvstore::{KvStore, MemoryKvStore};
pub use lazy_delete::{LazyDeleteEntry, LazyDeleteStore};
pub use lease::{LeaseManager, LeaseType};
pub use lease_renew::{LeaseRenewConfig, LeaseRenewManager, RenewalAction};
pub use locking::{LockManager, LockType};
pub use membership::{MemberInfo, MembershipEvent, MembershipManager, NodeState};
pub use membership_failure_detector::{
    MemberInfo as FdMemberInfo, MemberState, MembershipFailureDetector,
};
pub use metrics::{MetadataMetrics, MetricOp, MetricsCollector, OpMetrics};
pub use mtime_tracker::{MtimeBatch, MtimeReason, MtimeStore, MtimeUpdate};
pub use multiraft::MultiRaftManager;
pub use neg_cache::{NegCacheConfig, NegativeCache};
pub use node::{ClusterStatus, DirEntryPlus, MetadataNode, MetadataNodeConfig, StatFs};
pub use node_snapshot::NodeSnapshot;
pub use pathres::{NegativeCacheEntry, PathCacheEntry, PathResolver};
pub use prefetch::{PrefetchConfig, PrefetchEngine, PrefetchRequest, PrefetchResult};
pub use qos::{QosClass, QosManager, QosPolicy};
pub use qos_coordinator::{
    Priority, OpType, RequestId, QosRequest, QosContext, QosMetrics, QosViolation, QosHint,
    QosMetricsSummary, QosCoordinatorConfig, QosCoordinator,
};
pub use quota::{QuotaEntry, QuotaLimit, QuotaManager, QuotaTarget, QuotaUsage};
pub use quota_tracker::{
    QuotaType, TenantQuota, QuotaUsage as TenantQuotaUsage, ViolationType, Severity, QuotaViolation,
    QuotaTrackerConfig, QuotaTracker,
};
pub use tenant_isolator::{
    TenantNamespace, TenantCapabilities, TenantContext, IsolationViolationType, IsolationViolation,
    TenantIsolatorConfig, TenantIsolator,
};
pub use raft_log::RaftLogStore;
pub use raftservice::{RaftMetadataService, RaftServiceConfig};
pub use range_lock::{RangeLock, RangeLockManager};
pub use rate_limit::{ClientId, RateLimitConfig, RateLimitDecision, RateLimitStats, RateLimiter};
pub use readindex::{PendingRead, ReadIndexManager, ReadStatus};
pub use rpc::{MetadataRequest, MetadataResponse, RpcDispatcher};
pub use scaling::{MigrationStatus, MigrationTask, ScalingManager, ShardPlacement};
pub use service::MetadataService;
pub use service::MetadataServiceConfig;
pub use shard::{ShardAssigner, ShardInfo, ShardRouter};
pub use shard_stats::{ClusterShardStats, ShardStats};
pub use snapshot::{RaftSnapshot, SnapshotManager};
pub use snapshot_transfer::{
    CompressionType, SnapshotId, MetadataSnapshot, SnapshotTransferConfig,
    TransferState, TransferProgress, SnapshotTransferEngine, RemoteRestorationResult,
};
pub use space_accounting::{DirUsage, SpaceAccountingStore};
pub use symlink::SymlinkStore;
pub use tenant::{TenantConfig, TenantId, TenantManager, TenantUsage};
pub use tracecontext::{SpanId, SpanRecord, SpanStatus, TraceCollector, TraceContext, TraceId};
pub use transaction::{Transaction, TransactionId, TransactionManager, TransactionState};
pub use watch::{Watch, WatchEvent, WatchManager};
pub use worm::{RetentionPolicy, WormAuditEvent, WormEntry, WormManager, WormState};
pub use xattr::XattrStore;
pub use distributed_transaction::{
    DistributedTransactionEngine, TransactionId as DtxnId, TransactionOp, TransactionState as DtxnState,
    TransactionVote, CommitResult, LockToken, DeadlockDetectionGraph,
};
pub use client_session::{
    SessionManager, SessionId, ClientId as SessionClientId, OperationId, SessionState, PendingOperation,
    OpResult, SessionLeaseRenewal, SessionManagerConfig, SessionMetrics, ClientSession,
};

/// Re-export key types for external users
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
