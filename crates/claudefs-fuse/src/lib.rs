#![warn(missing_docs)]

//! ClaudeFS FUSE subsystem.

pub mod attr;
pub mod buffer_pool;
pub mod cache;
pub mod cache_coherence;
pub mod capability;
pub mod client_auth;
pub mod crash_recovery;
pub mod datacache;
pub mod deleg;
pub mod dir_cache;
pub mod dirnotify;
pub mod error;
pub mod fadvise;
pub mod fallocate;
pub mod filesystem;
pub mod flock;
pub mod health;
pub mod idmap;
pub mod inode;
pub mod interrupt;
pub mod io_priority;
pub mod locking;
pub mod migration;
pub mod mmap;
pub mod mount;
pub mod mount_opts;
pub mod multipath;
pub mod notify_filter;
pub mod openfile;
pub mod operations;
pub mod otel_trace;
pub mod passthrough;
pub mod path_resolver;
pub mod perf;
pub mod posix_acl;
pub mod prefetch;
pub mod quota_enforce;
pub mod ratelimit;
pub mod reconnect;
pub mod sec_policy;
pub mod server;
pub mod session;
pub mod snapshot;
pub mod symlink;
pub mod tiering_hints;
pub mod tracing_client;
pub mod transport;
pub mod workload_class;
pub mod worm;
pub mod writebuf;
pub mod xattr;

pub use error::{FuseError, Result};
