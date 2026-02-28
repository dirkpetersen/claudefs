#![warn(missing_docs)]

//! ClaudeFS FUSE subsystem: FUSE v3 daemon, passthrough mode, client-side caching

pub mod server;
pub mod filesystem;
pub mod passthrough;
pub mod cache;
pub mod operations;