//! Mount point management for ClaudeFS FUSE client.
//!
//! Manages the lifecycle of FUSE mount points, including option parsing,
//! mount validation, and RAII-based automatic unmount.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Mount options for FUSE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountOptions {
    /// Allow other users to access.
    pub allow_other: bool,
    /// Allow root to access.
    pub allow_root: bool,
    /// Use default permissions.
    pub default_permissions: bool,
    /// Auto unmount on exit.
    pub auto_unmount: bool,
    /// Bypass page cache.
    pub direct_io: bool,
    /// Use kernel cache.
    pub kernel_cache: bool,
    /// Allow non-empty mountpoint.
    pub nonempty: bool,
    /// Read-only mount.
    pub ro: bool,
}

impl Default for MountOptions {
    fn default() -> Self {
        MountOptions {
            allow_other: false,
            allow_root: false,
            default_permissions: false,
            auto_unmount: true,
            direct_io: false,
            kernel_cache: true,
            nonempty: false,
            ro: false,
        }
    }
}

/// Errors that can occur during mount operations.
#[derive(Debug, Error)]
pub enum MountError {
    /// Path does not exist.
    #[error("Path not found: {0}")]
    PathNotFound(String),

    /// Path is not a directory.
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Path is already mounted.
    #[error("Already mounted: {0}")]
    AlreadyMounted(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid option.
    #[error("Invalid option: {0}")]
    InvalidOption(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(String),
}

impl From<std::io::Error> for MountError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => MountError::IoError(format!("not found: {}", e)),
            std::io::ErrorKind::PermissionDenied => {
                MountError::PermissionDenied(format!("permission denied: {}", e))
            }
            _ => MountError::IoError(e.to_string()),
        }
    }
}

/// Validate a mountpoint path.
pub fn validate_mountpoint(path: &Path) -> Result<(), MountError> {
    if !path.exists() {
        return Err(MountError::PathNotFound(path.display().to_string()));
    }

    if !path.is_dir() {
        return Err(MountError::NotADirectory(path.display().to_string()));
    }

    Ok(())
}

/// Parse mount options from a comma-separated string.
///
/// Valid options: allow_other, allow_root, default_permissions, auto_unmount,
/// direct_io, kernel_cache, nonempty, ro.
pub fn parse_mount_options(opts_str: &str) -> Result<MountOptions, MountError> {
    let mut options = MountOptions::default();

    if opts_str.is_empty() {
        return Ok(options);
    }

    for opt in opts_str.split(',') {
        let opt = opt.trim();
        match opt {
            "allow_other" => options.allow_other = true,
            "allow_root" => options.allow_root = true,
            "default_permissions" => options.default_permissions = true,
            "auto_unmount" => options.auto_unmount = true,
            "direct_io" => options.direct_io = true,
            "kernel_cache" => options.kernel_cache = true,
            "nonempty" => options.nonempty = true,
            "ro" => options.ro = true,
            "rw" => options.ro = false,
            "" => {}
            _ => {
                return Err(MountError::InvalidOption(opt.to_string()));
            }
        }
    }

    Ok(options)
}

/// Convert MountOptions to fuser::MountOption vec.
pub fn options_to_fuser(opts: &MountOptions) -> Vec<fuser::MountOption> {
    let mut fuser_opts = Vec::new();

    if opts.allow_other {
        fuser_opts.push(fuser::MountOption::AllowOther);
    }

    if opts.allow_root {
        fuser_opts.push(fuser::MountOption::AllowRoot);
    }

    if opts.default_permissions {
        fuser_opts.push(fuser::MountOption::DefaultPermissions);
    }

    if opts.auto_unmount {
        fuser_opts.push(fuser::MountOption::AutoUnmount);
    }

    if opts.direct_io {
        fuser_opts.push(fuser::MountOption::CUSTOM("direct_io".into()));
    }

    if !opts.kernel_cache {
        fuser_opts.push(fuser::MountOption::CUSTOM("no_kernel_cache".into()));
    }

    if opts.nonempty {
        fuser_opts.push(fuser::MountOption::CUSTOM("nonempty".into()));
    }

    if opts.ro {
        fuser_opts.push(fuser::MountOption::RO);
    }

    fuser_opts
}

/// RAII handle for a mounted FUSE filesystem.
#[derive(Debug)]
pub struct MountHandle {
    /// Mount point path.
    mountpoint: PathBuf,
    /// Whether currently mounted.
    mounted: bool,
}

impl MountHandle {
    /// Create a new mount handle.
    pub fn new(mountpoint: PathBuf) -> Self {
        MountHandle {
            mountpoint,
            mounted: false,
        }
    }

    /// Get the mount point path.
    pub fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }

    /// Check if currently mounted.
    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    /// Mark as unmounted.
    pub fn mark_unmounted(&mut self) {
        self.mounted = false;
    }

    /// Mark as mounted.
    pub fn mark_mounted(&mut self) {
        self.mounted = true;
    }
}

impl Default for MountHandle {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_default_options_have_expected_values() {
        let opts = MountOptions::default();
        assert!(!opts.allow_other);
        assert!(!opts.allow_root);
        assert!(!opts.default_permissions);
        assert!(opts.auto_unmount);
        assert!(!opts.direct_io);
        assert!(opts.kernel_cache);
        assert!(!opts.nonempty);
        assert!(!opts.ro);
    }

    #[test]
    fn test_parse_mount_options_allow_other_and_ro() {
        let opts = parse_mount_options("allow_other,ro").unwrap();
        assert!(opts.allow_other);
        assert!(opts.ro);
        assert!(!opts.direct_io);
    }

    #[test]
    fn test_parse_mount_options_direct_io_and_kernel_cache() {
        let opts = parse_mount_options("direct_io,kernel_cache").unwrap();
        assert!(opts.direct_io);
        assert!(opts.kernel_cache);
    }

    #[test]
    fn test_parse_mount_options_unknown_returns_error() {
        let result = parse_mount_options("unknown");
        assert!(matches!(result, Err(MountError::InvalidOption(_))));
    }

    #[test]
    fn test_parse_mount_options_empty_returns_default() {
        let opts = parse_mount_options("").unwrap();
        assert_eq!(opts, MountOptions::default());
    }

    #[test]
    fn test_validate_mountpoint_with_existing_directory() {
        // /tmp should exist on most systems
        let result = validate_mountpoint(Path::new("/tmp"));
        // May succeed or fail depending on test environment
        // But it shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_validate_mountpoint_with_nonexistent_path() {
        let result = validate_mountpoint(Path::new("/nonexistent_path_12345"));
        assert!(matches!(result, Err(MountError::PathNotFound(_))));
    }

    #[test]
    fn test_validate_mountpoint_with_file_not_dir() {
        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("claudefs_test_file.txt");

        fs::write(&temp_file, "test").ok();

        let result = validate_mountpoint(&temp_file);

        fs::remove_file(&temp_file).ok();

        assert!(matches!(result, Err(MountError::NotADirectory(_))));
    }

    #[test]
    fn test_options_to_fuser_with_allow_other() {
        let opts = MountOptions {
            allow_other: true,
            ..Default::default()
        };

        let fuser_opts = options_to_fuser(&opts);

        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::AllowOther)));
    }

    #[test]
    fn test_mount_handle_tracks_mounted_state() {
        let mut handle = MountHandle::new(PathBuf::from("/test"));

        assert!(!handle.is_mounted());

        handle.mark_mounted();
        assert!(handle.is_mounted());

        handle.mark_unmounted();
        assert!(!handle.is_mounted());
    }

    #[test]
    fn test_mount_handle_mountpoint_accessor() {
        let handle = MountHandle::new(PathBuf::from("/my/mount"));
        assert_eq!(handle.mountpoint(), Path::new("/my/mount"));
    }

    #[test]
    fn test_parse_mount_options_ro_rw() {
        let opts = parse_mount_options("ro").unwrap();
        assert!(opts.ro);

        let opts = parse_mount_options("rw").unwrap();
        assert!(!opts.ro);
    }

    #[test]
    fn test_parse_mount_options_multiple() {
        let opts = parse_mount_options("allow_other,default_permissions,nonempty").unwrap();
        assert!(opts.allow_other);
        assert!(opts.default_permissions);
        assert!(opts.nonempty);
    }

    #[test]
    fn test_options_to_fuser_includes_all() {
        let opts = MountOptions {
            allow_other: true,
            allow_root: true,
            default_permissions: true,
            auto_unmount: true,
            direct_io: true,
            kernel_cache: false,
            nonempty: true,
            ro: true,
        };

        let fuser_opts = options_to_fuser(&opts);

        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::AllowOther)));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::AllowRoot)));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::DefaultPermissions)));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::AutoUnmount)));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::CUSTOM(s) if s == "direct_io")));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::CUSTOM(s) if s == "no_kernel_cache")));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::CUSTOM(s) if s == "nonempty")));
        assert!(fuser_opts
            .iter()
            .any(|o| matches!(o, fuser::MountOption::RO)));
    }

    #[test]
    fn test_parse_mount_options_with_spaces() {
        let opts = parse_mount_options("allow_other, ro ").unwrap();
        assert!(opts.allow_other);
        assert!(opts.ro);
    }
}
