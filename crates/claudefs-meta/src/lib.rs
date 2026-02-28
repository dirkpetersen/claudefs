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
/// Cross-site replication
pub mod replication;
/// Core types for the metadata service
pub mod types;

/// Re-export key types for external users
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
