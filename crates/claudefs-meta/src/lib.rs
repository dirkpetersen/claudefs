#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

pub mod consensus;
pub mod directory;
pub mod inode;
pub mod journal;
pub mod kvstore;
pub mod replication;
