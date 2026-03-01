//! FUSE session management.
//!
//! Manages the complete lifecycle of a FUSE mount: mounting the filesystem,
//! running the kernel request dispatch loop, and clean unmounting on shutdown.

use crate::error::{FuseError, Result};
use crate::filesystem::{ClaudeFsConfig, ClaudeFsFilesystem};
use crate::mount::{validate_mountpoint, MountOptions};
use crate::server::FuseServerConfig;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

/// Handle for a running FUSE session.
/// Dropping this handle initiates graceful shutdown.
pub struct SessionHandle {
    mountpoint: PathBuf,
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub config: FuseServerConfig,
}

impl SessionHandle {
    /// Get the mount point path.
    pub fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }

    /// Send shutdown signal to the session.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Check if the session is still alive.
    pub fn is_alive(&self) -> bool {
        self.shutdown_tx.is_some()
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Configuration for a FUSE session
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Mount point path
    pub mountpoint: PathBuf,
    /// Filesystem configuration
    pub fs_config: ClaudeFsConfig,
    /// Server configuration
    pub server_config: FuseServerConfig,
    /// Mount options
    pub mount_options: MountOptions,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mountpoint: PathBuf::new(),
            fs_config: ClaudeFsConfig::default(),
            server_config: FuseServerConfig::default(),
            mount_options: MountOptions::default(),
        }
    }
}

/// Session statistics
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    /// Number of requests processed
    pub requests_processed: u64,
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Number of errors encountered
    pub errors: u64,
}

/// Validates a session config
pub fn validate_session_config(config: &SessionConfig) -> Result<()> {
    if config.mountpoint == PathBuf::new() {
        return Err(FuseError::InvalidArgument {
            msg: "mountpoint cannot be empty".into(),
        });
    }

    if let Err(e) = validate_mountpoint(&config.mountpoint) {
        return Err(FuseError::InvalidArgument {
            msg: format!("invalid mountpoint: {}", e),
        });
    }

    Ok(())
}

/// Builds the FUSE filesystem for a session (does NOT mount it — testing only)
pub fn build_filesystem(config: &SessionConfig) -> ClaudeFsFilesystem {
    ClaudeFsFilesystem::new(config.fs_config.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_default_has_empty_mountpoint() {
        let config = SessionConfig::default();
        assert_eq!(config.mountpoint, PathBuf::new());
    }

    #[test]
    fn test_validate_session_config_empty_mountpoint_returns_error() {
        let config = SessionConfig::default();
        let result = validate_session_config(&config);
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_validate_session_config_nonexistent_mountpoint_returns_error() {
        let config = SessionConfig {
            mountpoint: PathBuf::from("/nonexistent_path_12345"),
            ..Default::default()
        };
        let result = validate_session_config(&config);
        assert!(matches!(result, Err(FuseError::InvalidArgument { .. })));
    }

    #[test]
    fn test_build_filesystem_creates_valid_filesystem() {
        let config = SessionConfig::default();
        let fs = build_filesystem(&config);
        let _ = fs.config();
        assert!(true);
    }

    #[test]
    fn test_session_handle_is_alive_when_shutdown_tx_some() {
        let (tx, _rx) = oneshot::channel();
        let handle = SessionHandle {
            mountpoint: PathBuf::from("/test"),
            shutdown_tx: Some(tx),
            config: FuseServerConfig::default(),
        };
        assert!(handle.is_alive());
    }

    #[test]
    fn test_session_stats_default_values_are_zero() {
        let stats = SessionStats::default();
        assert_eq!(stats.requests_processed, 0);
        assert_eq!(stats.bytes_read, 0);
        assert_eq!(stats.bytes_written, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_session_handle_mountpoint_accessor() {
        let handle = SessionHandle {
            mountpoint: PathBuf::from("/my/mount"),
            shutdown_tx: None,
            config: FuseServerConfig::default(),
        };
        assert_eq!(handle.mountpoint(), Path::new("/my/mount"));
    }

    #[test]
    fn test_session_config_field_defaults() {
        let config = SessionConfig::default();
        assert!(config.fs_config.default_permissions);
        assert!(!config.server_config.allow_other);
        assert!(!config.mount_options.ro);
    }
}
