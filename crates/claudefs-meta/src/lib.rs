#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

/// Raft consensus implementation
pub mod consensus;
/// Directory operations
pub mod directory;
/// Inode operations
pub mod inode;
/// Metadata journal for replication
pub mod journal;
/// Embedded key-value store
pub mod kvstore;
/// Distributed lock manager
pub mod locking;
/// Multi-Raft group manager
pub mod multiraft;
/// Speculative path resolution with caching
pub mod pathres;
/// Cross-site replication
pub mod replication;
/// High-level metadata service API
pub mod service;
/// Shard routing for distributed metadata
pub mod shard;
/// Core types for the metadata service
pub mod types;
/// Extended attribute operations
pub mod xattr;

pub use locking::{LockManager, LockType};
pub use multiraft::MultiRaftManager;
pub use pathres::{PathCacheEntry, PathResolver};
pub use service::MetadataService;
pub use shard::{ShardAssigner, ShardInfo, ShardRouter};
pub use xattr::XattrStore;

/// Re-export key types for external users
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
