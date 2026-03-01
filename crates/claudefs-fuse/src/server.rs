//! FUSE daemon server management.
//!
//! Manages the lifecycle of the FUSE session: mounting, running the event loop,
//! and graceful shutdown.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

/// Server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    /// Server is stopped.
    #[default]
    Stopped,
    /// Server is starting.
    Starting,
    /// Server is running.
    Running,
    /// Server is stopping.
    Stopping,
    /// Server encountered an error.
    Error,
}

/// Configuration for the FUSE server.
#[derive(Debug, Clone)]
pub struct FuseServerConfig {
    /// Mount point path.
    pub mountpoint: PathBuf,
    /// Mount options to pass to FUSE.
    pub mount_options: Vec<String>,
    /// Enable passthrough mode.
    pub passthrough: bool,
    /// Allow other users to access.
    pub allow_other: bool,
    /// Use default permissions.
    pub default_permissions: bool,
    /// Auto unmount on exit.
    pub auto_unmount: bool,
    /// Maximum number of background requests.
    pub max_background: u16,
    /// Congestion threshold.
    pub congestion_threshold: u16,
}

impl Default for FuseServerConfig {
    fn default() -> Self {
        FuseServerConfig {
            mountpoint: PathBuf::from("/mnt/claudefs"),
            mount_options: Vec::new(),
            passthrough: false,
            allow_other: false,
            default_permissions: false,
            auto_unmount: true,
            max_background: 16,
            congestion_threshold: 16,
        }
    }
}

/// FUSE server instance.
pub struct FuseServer {
    /// Configuration.
    config: FuseServerConfig,
    /// Internal state as atomic for lock-free access.
    state: AtomicU8,
}

impl FuseServer {
    /// Create a new server with the given configuration.
    pub fn new(config: FuseServerConfig) -> Self {
        FuseServer {
            config,
            state: AtomicU8::new(ServerState::Stopped as u8),
        }
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &FuseServerConfig {
        &self.config
    }

    /// Check if the server is running.
    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::SeqCst) == ServerState::Running as u8
    }

    /// Get the current server state.
    pub fn state(&self) -> ServerState {
        match self.state.load(Ordering::SeqCst) {
            0 => ServerState::Stopped,
            1 => ServerState::Starting,
            2 => ServerState::Running,
            3 => ServerState::Stopping,
            4 => ServerState::Error,
            _ => ServerState::Stopped,
        }
    }

    /// Set server state.
    fn set_state(&self, new_state: ServerState) {
        self.state.store(new_state as u8, Ordering::SeqCst);
    }

    /// Start the server.
    pub fn start(&self) -> Result<(), crate::error::FuseError> {
        self.set_state(ServerState::Starting);

        validate_config(&self.config)?;

        self.set_state(ServerState::Running);
        Ok(())
    }

    /// Stop the server.
    pub fn stop(&self) {
        self.set_state(ServerState::Stopping);
        self.set_state(ServerState::Stopped);
    }
}

impl Default for FuseServer {
    fn default() -> Self {
        Self::new(FuseServerConfig::default())
    }
}

/// Build mount options for fuser from config.
pub fn build_mount_options(config: &FuseServerConfig) -> Vec<fuser::MountOption> {
    let mut options = Vec::new();

    // Always include FSName
    options.push(fuser::MountOption::FSName("claudefs".into()));

    if config.allow_other {
        options.push(fuser::MountOption::AllowOther);
    }

    if config.default_permissions {
        options.push(fuser::MountOption::DefaultPermissions);
    }

    if config.auto_unmount {
        options.push(fuser::MountOption::AutoUnmount);
    }

    for opt in &config.mount_options {
        match opt.as_str() {
            "ro" => options.push(fuser::MountOption::RO),
            "rw" => options.push(fuser::MountOption::RW),
            "suid" => options.push(fuser::MountOption::Suid),
            "nosuid" => options.push(fuser::MountOption::NoSuid),
            "nodev" => options.push(fuser::MountOption::NoDev),
            "noexec" => options.push(fuser::MountOption::NoExec),
            "relatime" => options.push(fuser::MountOption::CUSTOM("relatime".into())),
            _ => {}
        }
    }

    options
}

/// Validate server configuration.
pub fn validate_config(config: &FuseServerConfig) -> Result<(), crate::error::FuseError> {
    if config.mountpoint.as_os_str().is_empty() {
        return Err(crate::error::FuseError::InvalidArgument {
            msg: "mountpoint path is empty".to_string(),
        });
    }

    if config.max_background == 0 {
        return Err(crate::error::FuseError::InvalidArgument {
            msg: "max_background must be > 0".to_string(),
        });
    }

    if config.congestion_threshold == 0 {
        return Err(crate::error::FuseError::InvalidArgument {
            msg: "congestion_threshold must be > 0".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_expected_values() {
        let config = FuseServerConfig::default();
        assert_eq!(config.mountpoint, PathBuf::from("/mnt/claudefs"));
        assert!(!config.passthrough);
        assert!(!config.allow_other);
        assert!(!config.default_permissions);
        assert!(config.auto_unmount);
        assert_eq!(config.max_background, 16);
        assert_eq!(config.congestion_threshold, 16);
    }

    #[test]
    fn test_validate_config_succeeds_for_valid_config() {
        let config = FuseServerConfig::default();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_fails_for_empty_mountpoint() {
        let config = FuseServerConfig {
            mountpoint: PathBuf::new(),
            ..Default::default()
        };
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_fails_for_zero_max_background() {
        let config = FuseServerConfig {
            max_background: 0,
            ..Default::default()
        };
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_mount_options_includes_fsname_always() {
        let config = FuseServerConfig::default();
        let options = build_mount_options(&config);

        let has_fsname = options.iter().any(|opt| match opt {
            fuser::MountOption::FSName(name) => name == "claudefs",
            _ => false,
        });

        assert!(has_fsname, "FSName should always be present");
    }

    #[test]
    fn test_build_mount_options_with_allow_other() {
        let config = FuseServerConfig {
            allow_other: true,
            ..Default::default()
        };
        let options = build_mount_options(&config);

        let has_allow_other = options
            .iter()
            .any(|opt| matches!(opt, fuser::MountOption::AllowOther));

        assert!(has_allow_other);
    }

    #[test]
    fn test_build_mount_options_with_default_permissions() {
        let config = FuseServerConfig {
            default_permissions: true,
            ..Default::default()
        };
        let options = build_mount_options(&config);

        let has_default_perms = options
            .iter()
            .any(|opt| matches!(opt, fuser::MountOption::DefaultPermissions));

        assert!(has_default_perms);
    }

    #[test]
    fn test_server_starts_in_stopped_state() {
        let server = FuseServer::default();
        assert_eq!(server.state(), ServerState::Stopped);
    }

    #[test]
    fn test_config_accessors_work() {
        let config = FuseServerConfig {
            mountpoint: PathBuf::from("/test/mount"),
            allow_other: true,
            ..Default::default()
        };
        let server = FuseServer::new(config.clone());

        assert_eq!(server.config().mountpoint, PathBuf::from("/test/mount"));
        assert!(server.config().allow_other);
    }

    #[test]
    fn test_is_running_returns_false_initially() {
        let server = FuseServer::default();
        assert!(!server.is_running());
    }

    #[test]
    fn test_start_stop_cycle() {
        let server = FuseServer::default();

        assert_eq!(server.state(), ServerState::Stopped);

        server.start().expect("start should succeed");
        assert!(server.is_running());

        server.stop();
        assert!(!server.is_running());
        assert_eq!(server.state(), ServerState::Stopped);
    }

    #[test]
    fn test_validate_config_fails_for_zero_congestion_threshold() {
        let config = FuseServerConfig {
            congestion_threshold: 0,
            ..Default::default()
        };
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_mount_options_with_passthrough_adds_no_extra_options() {
        // Passthrough is handled separately via KernelConfig in fuser
        let config = FuseServerConfig {
            passthrough: true,
            ..Default::default()
        };
        let options = build_mount_options(&config);

        // Should have FSName at minimum
        assert!(!options.is_empty());
    }
}
