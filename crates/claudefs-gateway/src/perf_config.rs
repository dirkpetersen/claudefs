//! Gateway performance tuning configuration

use thiserror::Error;

/// Gateway protocol type for per-protocol tuning
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayProtocol {
    /// NFSv3 protocol
    Nfs,
    /// pNFS protocol
    Pnfs,
    /// S3 API protocol
    S3,
    /// SMB protocol
    Smb,
}

/// Buffer size configuration (bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferConfig {
    /// Receive buffer size in bytes (default: 256KB)
    pub recv_buf_size: usize,
    /// Send buffer size in bytes (default: 256KB)
    pub send_buf_size: usize,
    /// Maximum request size in bytes (default: 64MB)
    pub max_request_size: usize,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            recv_buf_size: 256 * 1024,
            send_buf_size: 256 * 1024,
            max_request_size: 64 * 1024 * 1024,
        }
    }
}

/// Connection pool limits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// Maximum number of connections in the pool
    pub max_connections: usize,
    /// Maximum connections per client
    pub max_per_client: usize,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Keepalive interval in seconds
    pub keepalive_interval_secs: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_per_client: 64,
            idle_timeout_secs: 300,
            connect_timeout_ms: 5000,
            keepalive_interval_secs: 60,
        }
    }
}

/// Timeout configuration (all in milliseconds)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutConfig {
    /// Read timeout in milliseconds
    pub read_timeout_ms: u64,
    /// Write timeout in milliseconds
    pub write_timeout_ms: u64,
    /// Idle timeout in milliseconds
    pub idle_timeout_ms: u64,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            read_timeout_ms: 30000,
            write_timeout_ms: 30000,
            idle_timeout_ms: 300000,
            request_timeout_ms: 60000,
        }
    }
}

/// Auto-tuning mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoTuneMode {
    /// Auto-tuning disabled
    Disabled,
    /// Conservative auto-tuning (10% steps)
    #[default]
    Conservative,
    /// Aggressive auto-tuning (25% steps)
    Aggressive,
}

/// Auto-tuning configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoTuneConfig {
    /// Auto-tuning mode
    pub mode: AutoTuneMode,
    /// Measurement window in seconds
    pub measurement_window_secs: u64,
    /// Target CPU percentage (0-100)
    pub target_cpu_percent: u8,
    /// Target p99 latency in milliseconds
    pub target_latency_p99_ms: u64,
}

impl Default for AutoTuneConfig {
    fn default() -> Self {
        Self {
            mode: AutoTuneMode::Conservative,
            measurement_window_secs: 60,
            target_cpu_percent: 70,
            target_latency_p99_ms: 100,
        }
    }
}

/// Full performance configuration for a gateway protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfConfig {
    /// Gateway protocol
    pub protocol: GatewayProtocol,
    /// Buffer configuration
    pub buffers: BufferConfig,
    /// Connection pool configuration
    pub connections: ConnectionConfig,
    /// Timeout configuration
    pub timeouts: TimeoutConfig,
    /// Auto-tuning configuration
    pub auto_tune: AutoTuneConfig,
}

impl PerfConfig {
    /// Creates default config for the given protocol
    ///
    /// NFS: larger buffers (512KB), more connections (2048), longer timeouts
    /// S3: smaller buffers (128KB), fewer connections (512), shorter timeouts
    /// pNFS: similar to NFS
    /// SMB: similar to S3
    pub fn for_protocol(protocol: GatewayProtocol) -> Self {
        match protocol {
            GatewayProtocol::Nfs | GatewayProtocol::Pnfs => Self {
                protocol,
                buffers: BufferConfig {
                    recv_buf_size: 512 * 1024,
                    send_buf_size: 512 * 1024,
                    max_request_size: 64 * 1024 * 1024,
                },
                connections: ConnectionConfig {
                    max_connections: 2048,
                    max_per_client: 64,
                    idle_timeout_secs: 300,
                    connect_timeout_ms: 5000,
                    keepalive_interval_secs: 60,
                },
                timeouts: TimeoutConfig {
                    read_timeout_ms: 30000,
                    write_timeout_ms: 30000,
                    idle_timeout_ms: 300000,
                    request_timeout_ms: 60000,
                },
                auto_tune: AutoTuneConfig::default(),
            },
            GatewayProtocol::S3 | GatewayProtocol::Smb => Self {
                protocol,
                buffers: BufferConfig {
                    recv_buf_size: 128 * 1024,
                    send_buf_size: 128 * 1024,
                    max_request_size: 64 * 1024 * 1024,
                },
                connections: ConnectionConfig {
                    max_connections: 512,
                    max_per_client: 32,
                    idle_timeout_secs: 180,
                    connect_timeout_ms: 3000,
                    keepalive_interval_secs: 30,
                },
                timeouts: TimeoutConfig {
                    read_timeout_ms: 15000,
                    write_timeout_ms: 15000,
                    idle_timeout_ms: 180000,
                    request_timeout_ms: 30000,
                },
                auto_tune: AutoTuneConfig::default(),
            },
        }
    }

    /// Returns the protocol for this config
    pub fn protocol(&self) -> &GatewayProtocol {
        &self.protocol
    }
}

/// Validates PerfConfig values
pub struct PerfConfigValidator;

impl PerfConfigValidator {
    /// Validates the given PerfConfig
    pub fn validate(config: &PerfConfig) -> Result<(), PerfConfigError> {
        if config.connections.max_connections == 0 {
            return Err(PerfConfigError::ZeroMaxConnections);
        }
        if config.connections.max_per_client == 0 {
            return Err(PerfConfigError::ZeroMaxPerClient);
        }
        if config.connections.max_per_client > config.connections.max_connections {
            return Err(PerfConfigError::PerClientExceedsMax(
                config.connections.max_per_client,
                config.connections.max_connections,
            ));
        }
        if config.buffers.recv_buf_size == 0 {
            return Err(PerfConfigError::ZeroBufferSize);
        }
        if config.buffers.send_buf_size == 0 {
            return Err(PerfConfigError::ZeroBufferSize);
        }
        if config.buffers.max_request_size == 0 {
            return Err(PerfConfigError::ZeroBufferSize);
        }
        if config.timeouts.read_timeout_ms == 0 {
            return Err(PerfConfigError::ZeroTimeout);
        }
        if config.timeouts.write_timeout_ms == 0 {
            return Err(PerfConfigError::ZeroTimeout);
        }
        if config.auto_tune.target_cpu_percent > 100 {
            return Err(PerfConfigError::InvalidCpuTarget(
                config.auto_tune.target_cpu_percent,
            ));
        }
        if config.auto_tune.measurement_window_secs == 0 {
            return Err(PerfConfigError::ZeroMeasurementWindow);
        }
        Ok(())
    }
}

/// Performance configuration errors
#[derive(Debug, Error)]
pub enum PerfConfigError {
    #[error("max_connections must be > 0")]
    ZeroMaxConnections,
    #[error("max_per_client must be > 0")]
    ZeroMaxPerClient,
    #[error("max_per_client ({0}) must be <= max_connections ({1})")]
    PerClientExceedsMax(usize, usize),
    #[error("buffer size must be > 0")]
    ZeroBufferSize,
    #[error("timeout must be > 0")]
    ZeroTimeout,
    #[error("target_cpu_percent must be <= 100, got {0}")]
    InvalidCpuTarget(u8),
    #[error("measurement_window_secs must be > 0")]
    ZeroMeasurementWindow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_config_default_values() {
        let config = BufferConfig::default();
        assert_eq!(config.recv_buf_size, 256 * 1024);
        assert_eq!(config.send_buf_size, 256 * 1024);
        assert_eq!(config.max_request_size, 64 * 1024 * 1024);
    }

    #[test]
    fn connection_config_default_values() {
        let config = ConnectionConfig::default();
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.max_per_client, 64);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.connect_timeout_ms, 5000);
        assert_eq!(config.keepalive_interval_secs, 60);
    }

    #[test]
    fn timeout_config_default_values() {
        let config = TimeoutConfig::default();
        assert_eq!(config.read_timeout_ms, 30000);
        assert_eq!(config.write_timeout_ms, 30000);
        assert_eq!(config.idle_timeout_ms, 300000);
        assert_eq!(config.request_timeout_ms, 60000);
    }

    #[test]
    fn auto_tune_config_default_is_conservative() {
        let config = AutoTuneConfig::default();
        assert_eq!(config.mode, AutoTuneMode::Conservative);
        assert_eq!(config.measurement_window_secs, 60);
        assert_eq!(config.target_cpu_percent, 70);
        assert_eq!(config.target_latency_p99_ms, 100);
    }

    #[test]
    fn perf_config_for_protocol_nfs_has_larger_buffers() {
        let config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        assert_eq!(config.buffers.recv_buf_size, 512 * 1024);
        assert_eq!(config.connections.max_connections, 2048);
    }

    #[test]
    fn perf_config_for_protocol_s3_has_smaller_buffers() {
        let config = PerfConfig::for_protocol(GatewayProtocol::S3);
        assert_eq!(config.buffers.recv_buf_size, 128 * 1024);
        assert_eq!(config.connections.max_connections, 512);
    }

    #[test]
    fn perf_config_for_protocol_pnfs_matches_nfs_buffer_size() {
        let nfs_config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        let pnfs_config = PerfConfig::for_protocol(GatewayProtocol::Pnfs);
        assert_eq!(
            nfs_config.buffers.recv_buf_size,
            pnfs_config.buffers.recv_buf_size
        );
        assert_eq!(
            nfs_config.buffers.send_buf_size,
            pnfs_config.buffers.send_buf_size
        );
    }

    #[test]
    fn perf_config_validator_accepts_valid_default_nfs() {
        let config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        assert!(PerfConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn perf_config_validator_accepts_valid_default_s3() {
        let config = PerfConfig::for_protocol(GatewayProtocol::S3);
        assert!(PerfConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn validator_rejects_max_connections_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_connections: 0,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroMaxConnections)));
    }

    #[test]
    fn validator_rejects_max_per_client_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_per_client: 0,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroMaxPerClient)));
    }

    #[test]
    fn validator_rejects_max_per_client_exceeds_max() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_connections: 10,
                max_per_client: 100,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::PerClientExceedsMax(100, 10))
        ));
    }

    #[test]
    fn validator_rejects_recv_buf_size_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                recv_buf_size: 0,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
    }

    #[test]
    fn validator_rejects_send_buf_size_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                send_buf_size: 0,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
    }

    #[test]
    fn validator_rejects_max_request_size_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                max_request_size: 0,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
    }

    #[test]
    fn validator_rejects_read_timeout_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig {
                read_timeout_ms: 0,
                ..Default::default()
            },
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroTimeout)));
    }

    #[test]
    fn validator_rejects_write_timeout_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig {
                write_timeout_ms: 0,
                ..Default::default()
            },
            auto_tune: AutoTuneConfig::default(),
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroTimeout)));
    }

    #[test]
    fn validator_rejects_target_cpu_percent_101() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig {
                target_cpu_percent: 101,
                ..Default::default()
            },
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::InvalidCpuTarget(101))
        ));
    }

    #[test]
    fn validator_accepts_target_cpu_percent_100() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig {
                target_cpu_percent: 100,
                ..Default::default()
            },
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validator_rejects_measurement_window_zero() {
        let config = PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig {
                measurement_window_secs: 0,
                ..Default::default()
            },
        };
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::ZeroMeasurementWindow)
        ));
    }

    #[test]
    fn auto_tune_mode_variants_exist() {
        let _ = AutoTuneMode::Disabled;
        let _ = AutoTuneMode::Conservative;
        let _ = AutoTuneMode::Aggressive;
    }

    #[test]
    fn perf_config_protocol_returns_the_protocol() {
        let config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        assert_eq!(config.protocol(), &GatewayProtocol::Nfs);

        let config = PerfConfig::for_protocol(GatewayProtocol::S3);
        assert_eq!(config.protocol(), &GatewayProtocol::S3);
    }
}
