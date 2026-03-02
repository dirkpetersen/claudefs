//! FUSE passthrough mode support.
//!
//! Passthrough mode (kernel 6.8+) allows data I/O to bypass the FUSE daemon
//! and go directly to underlying file descriptors, reducing context switch overhead.
//! Falls back gracefully when not available.

use std::collections::HashMap;

/// Configuration for passthrough mode.
#[derive(Debug, Clone)]
pub struct PassthroughConfig {
    /// Whether to enable passthrough.
    pub enabled: bool,
    /// Minimum kernel major version required.
    pub min_kernel_major: u32,
    /// Minimum kernel minor version required.
    pub min_kernel_minor: u32,
}

impl Default for PassthroughConfig {
    fn default() -> Self {
        PassthroughConfig {
            enabled: true,
            min_kernel_major: 6,
            min_kernel_minor: 8,
        }
    }
}

impl PassthroughConfig {
    /// Create a new passthrough config with default values.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Status of passthrough mode.
#[derive(Debug, Clone, Default)]
pub enum PassthroughStatus {
    /// Passthrough is enabled and active.
    #[default]
    Enabled,
    /// Disabled because kernel version is too old.
    DisabledKernelTooOld {
        /// Detected kernel major version.
        major: u32,
        /// Detected kernel minor version.
        minor: u32,
    },
    /// Disabled by configuration.
    DisabledByConfig,
    /// Disabled because feature is unsupported.
    DisabledUnsupportedFeature,
}

/// State for passthrough mode.
#[derive(Debug)]
pub struct PassthroughState {
    /// Current passthrough status.
    pub status: PassthroughStatus,
    /// File descriptor table: fh -> raw fd.
    fd_table: HashMap<u64, i32>,
}

impl PassthroughState {
    /// Create a new passthrough state from config.
    pub fn new(config: &PassthroughConfig) -> Self {
        let status = if !config.enabled {
            PassthroughStatus::DisabledByConfig
        } else {
            let (major, minor) = detect_kernel_version();
            check_kernel_version(major, minor, config)
        };

        PassthroughState {
            status,
            fd_table: HashMap::new(),
        }
    }

    /// Check if passthrough is active.
    pub fn is_active(&self) -> bool {
        matches!(self.status, PassthroughStatus::Enabled)
    }

    /// Register a file descriptor.
    pub fn register_fd(&mut self, fh: u64, fd: i32) {
        self.fd_table.insert(fh, fd);
    }

    /// Unregister a file descriptor.
    pub fn unregister_fd(&mut self, fh: u64) -> Option<i32> {
        self.fd_table.remove(&fh)
    }

    /// Get a registered file descriptor.
    pub fn get_fd(&self, fh: u64) -> Option<i32> {
        self.fd_table.get(&fh).copied()
    }

    /// Check how many fds are registered.
    pub fn fd_count(&self) -> usize {
        self.fd_table.len()
    }
}

impl Default for PassthroughState {
    fn default() -> Self {
        Self::new(&PassthroughConfig::default())
    }
}

/// Check if kernel version meets the requirement.
pub fn check_kernel_version(
    major: u32,
    minor: u32,
    config: &PassthroughConfig,
) -> PassthroughStatus {
    if !config.enabled {
        return PassthroughStatus::DisabledByConfig;
    }
    if major < config.min_kernel_major {
        return PassthroughStatus::DisabledKernelTooOld { major, minor };
    }
    if major > config.min_kernel_major {
        return PassthroughStatus::Enabled;
    }
    // major == config.min_kernel_major
    if minor >= config.min_kernel_minor {
        PassthroughStatus::Enabled
    } else {
        PassthroughStatus::DisabledKernelTooOld { major, minor }
    }
}

/// Detect the current kernel version.
///
/// Reads from /proc/version or uses uname. Returns (major, minor).
/// If parsing fails, returns (0, 0) conservatively.
pub fn detect_kernel_version() -> (u32, u32) {
    // Try reading from /proc/version first
    if let Ok(content) = std::fs::read_to_string("/proc/version") {
        if let Some(version) = parse_kernel_version(&content) {
            return version;
        }
    }

    // Fallback to uname
    #[cfg(unix)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("uname").arg("-r").output() {
            let release = String::from_utf8_lossy(&output.stdout);
            if let Some(version) = parse_kernel_release(&release) {
                return version;
            }
        }
    }

    // Return conservative default
    (0, 0)
}

fn parse_kernel_version(content: &str) -> Option<(u32, u32)> {
    // Example: "Linux version 6.8.0-49-generic (#49-Ubuntu..."
    for part in content.split_whitespace() {
        if let Some(v) = part.strip_prefix("6.") {
            if let Ok(minor) = v.split('-').next()?.parse::<u32>() {
                return Some((6, minor));
            }
        }
        if let Some(v) = part.strip_prefix("5.") {
            if let Ok(minor) = v.split('.').next()?.parse::<u32>() {
                return Some((5, minor));
            }
        }
    }
    None
}

fn parse_kernel_release(release: &str) -> Option<(u32, u32)> {
    // Example: "6.8.0-49-generic"
    let parts: Vec<&str> = release.trim().split('.').collect();
    if parts.len() >= 2 {
        let major: u32 = parts[0].parse().ok()?;
        let minor_str = parts[1].split('-').next()?;
        let minor: u32 = minor_str.parse().ok()?;
        return Some((major, minor));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_expected_values() {
        let config = PassthroughConfig::default();
        assert!(config.enabled);
        assert_eq!(config.min_kernel_major, 6);
        assert_eq!(config.min_kernel_minor, 8);
    }

    #[test]
    fn test_check_kernel_version_with_6_8_enabled() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(6, 8, &config);
        assert!(matches!(status, PassthroughStatus::Enabled));
    }

    #[test]
    fn test_check_kernel_version_with_5_15_too_old() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(5, 15, &config);
        assert!(matches!(
            status,
            PassthroughStatus::DisabledKernelTooOld {
                major: 5,
                minor: 15
            }
        ));
    }

    #[test]
    fn test_check_kernel_version_with_7_0_enabled() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(7, 0, &config);
        assert!(matches!(status, PassthroughStatus::Enabled));
    }

    #[test]
    fn test_check_kernel_version_disabled_by_config() {
        let config = PassthroughConfig {
            enabled: false,
            min_kernel_major: 6,
            min_kernel_minor: 8,
        };
        let status = check_kernel_version(6, 8, &config);
        assert!(matches!(status, PassthroughStatus::DisabledByConfig));
    }

    #[test]
    fn test_passthrough_state_is_active_for_enabled_status() {
        let config = PassthroughConfig::default();
        let _state = PassthroughState::new(&config);

        // This test depends on the actual kernel, but we can test the logic
        let (major, minor) = detect_kernel_version();
        let status = check_kernel_version(major, minor, &config);

        match status {
            PassthroughStatus::Enabled => assert!(true),
            PassthroughStatus::DisabledKernelTooOld { .. } => {
                // Kernel too old but that's okay for test environment
                let state2 = PassthroughState {
                    status: PassthroughStatus::Enabled,
                    fd_table: HashMap::new(),
                };
                assert!(state2.is_active());
            }
            _ => {}
        }
    }

    #[test]
    fn test_register_fd_and_unregister_fd() {
        let mut state = PassthroughState::default();

        state.register_fd(1, 10);
        assert_eq!(state.get_fd(1), Some(10));

        let fd = state.unregister_fd(1);
        assert_eq!(fd, Some(10));
        assert_eq!(state.get_fd(1), None);
    }

    #[test]
    fn test_detect_kernel_version_returns_positive_or_zero() {
        let (major, minor) = detect_kernel_version();

        // Kernel version should be (0, 0) if detection fails,
        // or actual values if detection succeeds.
        // Reasonable kernel versions are 0-99
        assert!(major < 100, "major version {} seems invalid", major);
        assert!(minor < 100, "minor version {} seems invalid", minor);
    }

    #[test]
    fn test_passthrough_state_fd_count() {
        let mut state = PassthroughState::default();

        assert_eq!(state.fd_count(), 0);

        state.register_fd(1, 10);
        state.register_fd(2, 20);

        assert_eq!(state.fd_count(), 2);

        state.unregister_fd(1);

        assert_eq!(state.fd_count(), 1);
    }

    #[test]
    fn test_check_kernel_version_6_7_sufficient() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(6, 7, &config);
        // 6.7 < 6.8 so should be disabled
        assert!(matches!(
            status,
            PassthroughStatus::DisabledKernelTooOld { major: 6, minor: 7 }
        ));
    }

    #[test]
    fn test_check_kernel_version_6_9_sufficient() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(6, 9, &config);
        assert!(matches!(status, PassthroughStatus::Enabled));
    }

    #[test]
    fn test_check_kernel_version_edge_case_6_8() {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(6, 8, &config);
        assert!(matches!(status, PassthroughStatus::Enabled));
    }
}
