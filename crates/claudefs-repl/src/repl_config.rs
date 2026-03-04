//! Runtime configuration management with hot-reload support.
//!
//! Manages the replication configuration at runtime, supporting hot-reload
//! without restart. Config can be updated via admin API (A8) or config file.

use std::sync::RwLock;
use thiserror::Error;

/// Complete replication runtime configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplConfig {
    /// Local site identifier.
    pub local_site_id: u64,
    /// Maximum entries per batch.
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds.
    pub batch_timeout_ms: u64,
    /// Compression algorithm: "none"|"lz4"|"zstd".
    pub compress_algo: String,
    /// Max lag entries before triggering catchup.
    pub max_lag_before_catchup: u64,
    /// Max lag entries before requiring snapshot.
    pub max_lag_before_snapshot: u64,
    /// Heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: u64,
    /// Acknowledgment timeout in milliseconds.
    pub ack_timeout_ms: u64,
    /// Sliding window size.
    pub window_size: usize,
    /// Enable TLS for replication.
    pub enable_tls: bool,
    /// Number of replication threads.
    pub repl_threads: usize,
    /// Metrics prefix.
    pub metrics_prefix: String,
}

impl Default for ReplConfig {
    fn default() -> Self {
        Self {
            local_site_id: 0,
            max_batch_size: 1000,
            batch_timeout_ms: 100,
            compress_algo: "lz4".to_string(),
            max_lag_before_catchup: 10_000,
            max_lag_before_snapshot: 1_000_000,
            heartbeat_interval_ms: 5_000,
            ack_timeout_ms: 10_000,
            window_size: 32,
            enable_tls: true,
            repl_threads: 2,
            metrics_prefix: "claudefs_repl".to_string(),
        }
    }
}

/// A configuration update (partial update — only changed fields).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigDiff {
    /// New max_batch_size.
    pub max_batch_size: Option<usize>,
    /// New batch_timeout_ms.
    pub batch_timeout_ms: Option<u64>,
    /// New compress_algo.
    pub compress_algo: Option<String>,
    /// New max_lag_before_catchup.
    pub max_lag_before_catchup: Option<u64>,
    /// New max_lag_before_snapshot.
    pub max_lag_before_snapshot: Option<u64>,
    /// New heartbeat_interval_ms.
    pub heartbeat_interval_ms: Option<u64>,
    /// New ack_timeout_ms.
    pub ack_timeout_ms: Option<u64>,
    /// New window_size.
    pub window_size: Option<usize>,
    /// New enable_tls.
    pub enable_tls: Option<bool>,
    /// New repl_threads.
    pub repl_threads: Option<usize>,
    /// New metrics_prefix.
    pub metrics_prefix: Option<String>,
}

/// Error for config operations.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ConfigError {
    /// Invalid config value.
    #[error("invalid value for {field}: {reason}")]
    InvalidValue {
        /// Field name.
        field: String,
        /// Reason for invalidity.
        reason: String,
    },
    /// Validation failed.
    #[error("validation failed: {0}")]
    ValidationFailed(String),
}

/// Versioned config snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigVersion {
    /// Version number.
    pub version: u64,
    /// The config snapshot.
    pub config: ReplConfig,
    /// Unix ms when this version was created.
    pub updated_at_ms: u64,
}

/// Runtime config manager supporting hot-reload.
#[derive(Debug)]
pub struct ReplConfigManager {
    current: RwLock<ConfigVersion>,
    version_counter: RwLock<u64>,
}

impl ReplConfigManager {
    /// Create a new config manager with the given initial config.
    pub fn new(initial: ReplConfig) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let version = ConfigVersion {
            version: 1,
            config: initial,
            updated_at_ms: now_ms,
        };

        Self {
            current: RwLock::new(version),
            version_counter: RwLock::new(1),
        }
    }

    /// Get current config snapshot.
    pub fn current(&self) -> ReplConfig {
        self.current.read().unwrap().config.clone()
    }

    /// Get current version number.
    pub fn version(&self) -> u64 {
        self.current.read().unwrap().version
    }

    /// Apply a partial config update, returning the new version.
    pub fn apply_diff(&self, diff: ConfigDiff, now_ms: u64) -> Result<u64, ConfigError> {
        let mut guard = self.current.write().unwrap();

        let mut new_config = guard.config.clone();

        if let Some(v) = diff.max_batch_size {
            new_config.max_batch_size = v;
        }
        if let Some(v) = diff.batch_timeout_ms {
            new_config.batch_timeout_ms = v;
        }
        if let Some(v) = diff.compress_algo {
            new_config.compress_algo = v;
        }
        if let Some(v) = diff.max_lag_before_catchup {
            new_config.max_lag_before_catchup = v;
        }
        if let Some(v) = diff.max_lag_before_snapshot {
            new_config.max_lag_before_snapshot = v;
        }
        if let Some(v) = diff.heartbeat_interval_ms {
            new_config.heartbeat_interval_ms = v;
        }
        if let Some(v) = diff.ack_timeout_ms {
            new_config.ack_timeout_ms = v;
        }
        if let Some(v) = diff.window_size {
            new_config.window_size = v;
        }
        if let Some(v) = diff.enable_tls {
            new_config.enable_tls = v;
        }
        if let Some(v) = diff.repl_threads {
            new_config.repl_threads = v;
        }
        if let Some(v) = diff.metrics_prefix {
            new_config.metrics_prefix = v;
        }

        validate_config(&new_config)?;

        let mut counter = self.version_counter.write().unwrap();
        *counter += 1;
        let new_version = *counter;

        guard.version = new_version;
        guard.config = new_config;
        guard.updated_at_ms = now_ms;

        Ok(new_version)
    }

    /// Validate a full config.
    pub fn validate(config: &ReplConfig) -> Result<(), ConfigError> {
        validate_config(config)
    }

    /// Get history length (for test/debug).
    pub fn history_len(&self) -> usize {
        // We only keep current version, so history is 1
        1
    }

    /// Get the current ConfigVersion.
    pub fn current_version(&self) -> ConfigVersion {
        self.current.read().unwrap().clone()
    }
}

/// Validate a configuration.
fn validate_config(config: &ReplConfig) -> Result<(), ConfigError> {
    if config.max_batch_size == 0 || config.max_batch_size > 100_000 {
        return Err(ConfigError::InvalidValue {
            field: "max_batch_size".to_string(),
            reason: "must be in 1..=100000".to_string(),
        });
    }

    if config.batch_timeout_ms == 0 || config.batch_timeout_ms > 60_000 {
        return Err(ConfigError::InvalidValue {
            field: "batch_timeout_ms".to_string(),
            reason: "must be in 1..=60000".to_string(),
        });
    }

    if !matches!(config.compress_algo.as_str(), "none" | "lz4" | "zstd") {
        return Err(ConfigError::InvalidValue {
            field: "compress_algo".to_string(),
            reason: "must be one of: none, lz4, zstd".to_string(),
        });
    }

    if config.window_size == 0 || config.window_size > 1024 {
        return Err(ConfigError::InvalidValue {
            field: "window_size".to_string(),
            reason: "must be in 1..=1024".to_string(),
        });
    }

    if config.repl_threads == 0 || config.repl_threads > 32 {
        return Err(ConfigError::InvalidValue {
            field: "repl_threads".to_string(),
            reason: "must be in 1..=32".to_string(),
        });
    }

    if config.heartbeat_interval_ms < 100 || config.heartbeat_interval_ms > 300_000 {
        return Err(ConfigError::InvalidValue {
            field: "heartbeat_interval_ms".to_string(),
            reason: "must be in 100..=300000".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_default_config_passes_validation() {
        let config = ReplConfig::default();
        assert!(ReplConfigManager::validate(&config).is_ok());
    }

    #[test]
    fn test_invalid_max_batch_size_zero() {
        let mut config = ReplConfig::default();
        config.max_batch_size = 0;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_invalid_max_batch_size_too_large() {
        let mut config = ReplConfig::default();
        config.max_batch_size = 100_001;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_invalid_batch_timeout() {
        let mut config = ReplConfig::default();
        config.batch_timeout_ms = 0;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_invalid_compress_algo() {
        let mut config = ReplConfig::default();
        config.compress_algo = "invalid".to_string();
        let result = ReplConfigManager::validate(&config);
        assert!(
            matches!(result, Err(ConfigError::InvalidValue { field, .. }) if field == "compress_algo")
        );
    }

    #[test]
    fn test_valid_compress_algo_values() {
        for algo in ["none", "lz4", "zstd"] {
            let mut config = ReplConfig::default();
            config.compress_algo = algo.to_string();
            assert!(ReplConfigManager::validate(&config).is_ok());
        }
    }

    #[test]
    fn test_invalid_window_size() {
        let mut config = ReplConfig::default();
        config.window_size = 0;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_invalid_repl_threads() {
        let mut config = ReplConfig::default();
        config.repl_threads = 33;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_invalid_heartbeat_interval() {
        let mut config = ReplConfig::default();
        config.heartbeat_interval_ms = 50;
        let result = ReplConfigManager::validate(&config);
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));
    }

    #[test]
    fn test_apply_diff_increments_version() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let diff = ConfigDiff {
            max_batch_size: Some(2000),
            ..Default::default()
        };

        let new_version = manager.apply_diff(diff, now_ms()).unwrap();
        assert_eq!(new_version, 2);
        assert_eq!(manager.version(), 2);
    }

    #[test]
    fn test_apply_diff_updates_config() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let diff = ConfigDiff {
            max_batch_size: Some(500),
            compress_algo: Some("zstd".to_string()),
            ..Default::default()
        };

        manager.apply_diff(diff, now_ms()).unwrap();

        let current = manager.current();
        assert_eq!(current.max_batch_size, 500);
        assert_eq!(current.compress_algo, "zstd");
    }

    #[test]
    fn test_apply_diff_validates_before_commit() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let diff = ConfigDiff {
            max_batch_size: Some(0), // invalid
            ..Default::default()
        };

        let result = manager.apply_diff(diff, now_ms());
        assert!(matches!(result, Err(ConfigError::InvalidValue { .. })));

        // Config should not have changed
        assert_eq!(manager.current().max_batch_size, 1000);
    }

    #[test]
    fn test_sequential_diffs_stack() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let diff1 = ConfigDiff {
            max_batch_size: Some(500),
            ..Default::default()
        };
        manager.apply_diff(diff1, now_ms()).unwrap();

        let diff2 = ConfigDiff {
            window_size: Some(64),
            ..Default::default()
        };
        manager.apply_diff(diff2, now_ms()).unwrap();

        let diff3 = ConfigDiff {
            enable_tls: Some(false),
            ..Default::default()
        };
        manager.apply_diff(diff3, now_ms()).unwrap();

        let current = manager.current();
        assert_eq!(current.max_batch_size, 500);
        assert_eq!(current.window_size, 64);
        assert!(!current.enable_tls);
        assert_eq!(manager.version(), 4); // 1 initial + 3 diffs
    }

    #[test]
    fn test_concurrent_reads_while_applying_diff() {
        use std::sync::Arc;

        let manager = Arc::new(ReplConfigManager::new(ReplConfig::default()));
        let manager_clone = Arc::clone(&manager);

        let reader = thread::spawn(move || {
            for _ in 0..100 {
                let _ = manager_clone.current();
                let _ = manager_clone.version();
            }
        });

        let writer = thread::spawn(move || {
            for i in 0..10 {
                let diff = ConfigDiff {
                    max_batch_size: Some(1000 + i),
                    ..Default::default()
                };
                let _ = manager_clone.apply_diff(diff, now_ms());
            }
        });

        reader.join().unwrap();
        writer.join().unwrap();

        // Should not panic and version should have been updated
        assert!(manager.version() > 1);
    }

    #[test]
    fn test_current_version_returns_full_version() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let version = manager.current_version();
        assert_eq!(version.version, 1);
        assert_eq!(version.config, ReplConfig::default());
    }

    #[test]
    fn test_history_len() {
        let manager = ReplConfigManager::new(ReplConfig::default());
        assert_eq!(manager.history_len(), 1);
    }

    #[test]
    fn test_repl_config_default_values() {
        let config = ReplConfig::default();
        assert_eq!(config.local_site_id, 0);
        assert_eq!(config.max_batch_size, 1000);
        assert_eq!(config.batch_timeout_ms, 100);
        assert_eq!(config.compress_algo, "lz4");
        assert_eq!(config.max_lag_before_catchup, 10_000);
        assert_eq!(config.max_lag_before_snapshot, 1_000_000);
        assert_eq!(config.heartbeat_interval_ms, 5_000);
        assert_eq!(config.ack_timeout_ms, 10_000);
        assert_eq!(config.window_size, 32);
        assert_eq!(config.enable_tls, true);
        assert_eq!(config.repl_threads, 2);
        assert_eq!(config.metrics_prefix, "claudefs_repl");
    }

    #[test]
    fn test_config_diff_default() {
        let diff = ConfigDiff::default();
        assert_eq!(diff.max_batch_size, None);
        assert_eq!(diff.compress_algo, None);
    }

    #[test]
    fn test_all_fields_in_config_diff() {
        let diff = ConfigDiff {
            max_batch_size: Some(500),
            batch_timeout_ms: Some(200),
            compress_algo: Some("none".to_string()),
            max_lag_before_catchup: Some(5000),
            max_lag_before_snapshot: Some(500000),
            heartbeat_interval_ms: Some(3000),
            ack_timeout_ms: Some(15000),
            window_size: Some(64),
            enable_tls: Some(false),
            repl_threads: Some(4),
            metrics_prefix: Some("test_prefix".to_string()),
        };

        assert!(diff.max_batch_size.is_some());
        assert!(diff.compress_algo.is_some());
        assert!(diff.enable_tls.is_some());
    }

    #[test]
    fn test_apply_all_fields_of_diff() {
        let manager = ReplConfigManager::new(ReplConfig::default());

        let diff = ConfigDiff {
            max_batch_size: Some(500),
            batch_timeout_ms: Some(200),
            compress_algo: Some("none".to_string()),
            max_lag_before_catchup: Some(5000),
            max_lag_before_snapshot: Some(500000),
            heartbeat_interval_ms: Some(3000),
            ack_timeout_ms: Some(15000),
            window_size: Some(64),
            enable_tls: Some(false),
            repl_threads: Some(4),
            metrics_prefix: Some("test_prefix".to_string()),
        };

        manager.apply_diff(diff, now_ms()).unwrap();

        let config = manager.current();
        assert_eq!(config.max_batch_size, 500);
        assert_eq!(config.batch_timeout_ms, 200);
        assert_eq!(config.compress_algo, "none");
        assert_eq!(config.max_lag_before_catchup, 5000);
        assert_eq!(config.max_lag_before_snapshot, 500000);
        assert_eq!(config.heartbeat_interval_ms, 3000);
        assert_eq!(config.ack_timeout_ms, 15000);
        assert_eq!(config.window_size, 64);
        assert!(!config.enable_tls);
        assert_eq!(config.repl_threads, 4);
        assert_eq!(config.metrics_prefix, "test_prefix");
    }

    #[test]
    fn test_config_version_fields() {
        let now = now_ms();
        let version = ConfigVersion {
            version: 42,
            config: ReplConfig::default(),
            updated_at_ms: now,
        };

        assert_eq!(version.version, 42);
        assert_eq!(version.updated_at_ms, now);
    }
}
