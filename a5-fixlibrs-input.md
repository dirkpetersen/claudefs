Write the following EXACT content to the file /home/cfs/claudefs/crates/claudefs-fuse/src/lib.rs.
Do not change anything - write it exactly as shown:

```
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
pub mod fallocate;
pub mod fadvise;
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
```

Also write the following EXACT content to /home/cfs/claudefs/crates/claudefs-fuse/src/mount_opts.rs:

```
#![warn(missing_docs)]

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadWriteMode {
    #[default]
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    #[default]
    None,
    Relaxed,
    Strict,
}

#[derive(Debug, Clone)]
pub struct MountOptions {
    pub source: PathBuf,
    pub target: PathBuf,
    pub read_only: ReadWriteMode,
    pub allow_other: bool,
    pub default_permissions: bool,
    pub cache_mode: CacheMode,
    pub max_background: u32,
    pub congestion_threshold: u32,
    pub direct_io: bool,
    pub kernel_cache: bool,
    pub auto_unmount: bool,
    pub fd: Option<i32>,
}

impl Default for MountOptions {
    fn default() -> Self {
        Self {
            source: PathBuf::from("."),
            target: PathBuf::from("/mnt/fuse"),
            read_only: ReadWriteMode::ReadWrite,
            allow_other: false,
            default_permissions: false,
            cache_mode: CacheMode::None,
            max_background: 16,
            congestion_threshold: 12,
            direct_io: false,
            kernel_cache: true,
            auto_unmount: true,
            fd: None,
        }
    }
}

impl MountOptions {
    pub fn new(source: PathBuf, target: PathBuf) -> Self {
        Self {
            source,
            target,
            ..Default::default()
        }
    }

    pub fn to_fuse_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        args.push(self.source.to_string_lossy().to_string());
        args.push(self.target.to_string_lossy().to_string());

        if self.allow_other {
            args.push("-o".to_string());
            args.push("allow_other".to_string());
        }

        if self.default_permissions {
            args.push("-o".to_string());
            args.push("default_permissions".to_string());
        }

        if self.direct_io {
            args.push("-o".to_string());
            args.push("direct_io".to_string());
        }

        match self.read_only {
            ReadWriteMode::ReadOnly => {
                args.push("-r".to_string());
            }
            _ => {}
        }

        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_options_default() {
        let opts = MountOptions::default();
        assert_eq!(opts.read_only, ReadWriteMode::ReadWrite);
        assert_eq!(opts.cache_mode, CacheMode::None);
        assert_eq!(opts.max_background, 16);
    }

    #[test]
    fn test_mount_options_new() {
        let source = PathBuf::from("/data");
        let target = PathBuf::from("/mnt/test");
        let opts = MountOptions::new(source.clone(), target.clone());
        assert_eq!(opts.source, source);
        assert_eq!(opts.target, target);
    }

    #[test]
    fn test_to_fuse_args_basic() {
        let opts = MountOptions::new(PathBuf::from("src"), PathBuf::from("tgt"));
        let args = opts.to_fuse_args();
        assert!(args.contains(&"src".to_string()));
        assert!(args.contains(&"tgt".to_string()));
    }

    #[test]
    fn test_to_fuse_args_read_only() {
        let mut opts = MountOptions::default();
        opts.read_only = ReadWriteMode::ReadOnly;
        let args = opts.to_fuse_args();
        assert!(args.contains(&"-r".to_string()));
    }

    #[test]
    fn test_to_fuse_args_allow_other() {
        let mut opts = MountOptions::default();
        opts.allow_other = true;
        let args = opts.to_fuse_args();
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"allow_other".to_string()));
    }

    #[test]
    fn test_read_write_mode_default() {
        assert_eq!(ReadWriteMode::default(), ReadWriteMode::ReadWrite);
    }

    #[test]
    fn test_cache_mode_default() {
        assert_eq!(CacheMode::default(), CacheMode::None);
    }
}
```
