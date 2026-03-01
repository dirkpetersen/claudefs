#![warn(missing_docs)]

//! ClaudeFS FUSE subsystem: FUSE v3 daemon, passthrough mode, client-side caching.
//!
//! This crate implements the FUSE v3 filesystem daemon for ClaudeFS.
//! It handles all VFS operations from the kernel and dispatches them to
//! the metadata service (claudefs-meta) and transport layer (claudefs-transport).

pub mod attr;
pub mod cache;
pub mod capability;
pub mod client_auth;
pub mod datacache;
pub mod deleg;
pub mod dirnotify;
pub mod error;
pub mod fallocate;
pub mod filesystem;
pub mod health;
pub mod inode;
pub mod interrupt;
pub mod io_priority;
pub mod locking;
pub mod migration;
pub mod mmap;
pub mod mount;
pub mod openfile;
pub mod operations;
pub mod passthrough;
pub mod perf;
pub mod posix_acl;
pub mod prefetch;
pub mod quota_enforce;
pub mod ratelimit;
pub mod reconnect;
pub mod server;
pub mod session;
pub mod snapshot;
pub mod symlink;
pub mod tiering_hints;
pub mod tracing_client;
pub mod transport;
pub mod worm;
pub mod writebuf;
pub mod xattr;

pub use error::{FuseError, Result};
