#![warn(missing_docs)]

//! ClaudeFS FUSE subsystem: FUSE v3 daemon, passthrough mode, client-side caching.
//!
//! This crate implements the FUSE v3 filesystem daemon for ClaudeFS.
//! It handles all VFS operations from the kernel and dispatches them to
//! the metadata service (claudefs-meta) and transport layer (claudefs-transport).

pub mod attr;
pub mod cache;
pub mod error;
pub mod filesystem;
pub mod inode;
pub mod mount;
pub mod operations;
pub mod passthrough;
pub mod server;

pub use error::{FuseError, Result};