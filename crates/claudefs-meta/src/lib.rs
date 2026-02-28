#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

pub mod consensus;
pub mod inode;
pub mod directory;
pub mod kvstore;
pub mod journal;
pub mod replication;