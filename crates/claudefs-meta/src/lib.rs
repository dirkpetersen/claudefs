#![warn(missing_docs)]

//! ClaudeFS metadata subsystem: Distributed metadata, Raft consensus, inode/directory operations

/// Raft consensus implementation for distributed metadata.
pub mod consensus;
/// Directory operations and hierarchical namespace.
pub mod directory;
/// Inode operations and attributes.
pub mod inode;
/// Replication journal and WAL (write-ahead log).
pub mod journal;
/// Distributed key-value store for metadata.
pub mod kvstore;
/// Cross-site replication and conflict resolution.
pub mod replication;
/// Core types for metadata operations.
pub mod types;
