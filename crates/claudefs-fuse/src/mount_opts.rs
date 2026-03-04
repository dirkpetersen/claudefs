#![warn(missing_docs)]

//! FUSE mount option configuration.
//!
//! This module provides types for building and serializing FUSE mount options
//! that control how the filesystem is mounted in the kernel.

use std::path::PathBuf;

/// Read/write access mode for the mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadWriteMode {
    /// Read-write access (default).
    #[default]
    ReadWrite,
    /// Read-only access.
    ReadOnly,
}

/// Cache behavior for file data and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// No caching, all reads go to the daemon.
    #[default]
    None,
    /// Relaxed caching, may return stale data.
    Relaxed,
    /// Strict caching, guarantees freshness.
    Strict,
}

/// Configuration for a FUSE mount point.
#[derive(Debug, Clone)]
pub struct MountOptions {
    /// Source path or identifier for the filesystem.
    pub source: PathBuf,
    /// Target mount point in the filesystem.
    pub target: PathBuf,
    /// Read/write or read-only mode.
    pub read_only: ReadWriteMode,
    /// Allow other users to access the mount.
    pub allow_other: bool,
    /// Use kernel's default permission checking.
    pub default_permissions: bool,
    /// Cache mode for file data and attributes.
    pub cache_mode: CacheMode,
    /// Maximum number of pending background requests.
    pub max_background: u32,
    /// Threshold for kernel to start queuing requests.
    pub congestion_threshold: u32,
    /// Bypass page cache for all file I/O.
    pub direct_io: bool,
    /// Use kernel's page cache for reads.
    pub kernel_cache: bool,
    /// Automatically unmount on daemon exit.
    pub auto_unmount: bool,
    /// Pre-opened file descriptor for the mount.
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
    /// Creates a new mount configuration with the given source and target.
    pub fn new(source: PathBuf, target: PathBuf) -> Self {
        Self {
            source,
            target,
            ..Default::default()
        }
    }

    /// Converts mount options to a vector of FUSE command-line arguments.
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

        if self.read_only == ReadWriteMode::ReadOnly {
            args.push("-r".to_string());
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
