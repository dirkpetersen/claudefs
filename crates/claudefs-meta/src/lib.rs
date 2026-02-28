#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

pub mod consensus;
pub mod directory;
pub mod inode;
pub mod journal;
pub mod kvstore;
pub mod replication;
pub mod types;

// Re-export commonly used types for convenience
pub use types::{
    DirEntry, FileType, InodeAttr, InodeId, LogEntry, LogIndex, MetaError, MetaOp, NodeId,
    RaftMessage, RaftState, ReplicationState, ShardId, Term, Timestamp, VectorClock,
};
