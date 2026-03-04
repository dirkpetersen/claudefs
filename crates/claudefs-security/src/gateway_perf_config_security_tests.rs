//! Gateway performance configuration security tests.
//!
//! Part of A10 Phase 25: Gateway perf-config security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::perf_config::{
        AutoTuneConfig, AutoTuneMode, BufferConfig, ConnectionConfig, GatewayProtocol, PerfConfig,
        PerfConfigError, PerfConfigValidator, TimeoutConfig,
    };

    fn make_nfs_config() -> PerfConfig {
        PerfConfig::for_protocol(GatewayProtocol::Nfs)
    }

    fn make_s3_config() -> PerfConfig {
        PerfConfig::for_protocol(GatewayProtocol::S3)
    }

    fn make_pnfs_config() -> PerfConfig {
        PerfConfig::for_protocol(GatewayProtocol::Pnfs)
    }

    fn make_smb_config() -> PerfConfig {
        PerfConfig::for_protocol(GatewayProtocol::Smb)
    }

    fn make_config_with_max_connections(max_connections: usize) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_connections,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_max_per_client(max_per_client: usize) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_per_client,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_recv_buf_size(recv_buf_size: usize) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                recv_buf_size,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_send_buf_size(send_buf_size: usize) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                send_buf_size,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_max_request_size(max_request_size: usize) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig {
                max_request_size,
                ..Default::default()
            },
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_read_timeout_ms(read_timeout_ms: u64) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig {
                read_timeout_ms,
                ..Default::default()
            },
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_write_timeout_ms(write_timeout_ms: u64) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig {
                write_timeout_ms,
                ..Default::default()
            },
            auto_tune: AutoTuneConfig::default(),
        }
    }

    fn make_config_with_measurement_window(measurement_window_secs: u64) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig {
                measurement_window_secs,
                ..Default::default()
            },
        }
    }

    fn make_config_with_cpu_target(target_cpu_percent: u8) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig::default(),
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig {
                target_cpu_percent,
                ..Default::default()
            },
        }
    }

    fn make_config_with_per_client_and_max(
        max_per_client: usize,
        max_connections: usize,
    ) -> PerfConfig {
        PerfConfig {
            protocol: GatewayProtocol::Nfs,
            buffers: BufferConfig::default(),
            connections: ConnectionConfig {
                max_per_client,
                max_connections,
                ..Default::default()
            },
            timeouts: TimeoutConfig::default(),
            auto_tune: AutoTuneConfig::default(),
        }
    }

    // ============================================================================
    // Category 1: Default Configurations (5 tests)
    // ============================================================================

    #[test]
    fn test_buffer_config_defaults() {
        let config = BufferConfig::default();
        assert_eq!(config.recv_buf_size, 256 * 1024);
        assert_eq!(config.send_buf_size, 256 * 1024);
        assert_eq!(config.max_request_size, 64 * 1024 * 1024);
    }

    #[test]
    fn test_connection_config_defaults() {
        let config = ConnectionConfig::default();
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.max_per_client, 64);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.connect_timeout_ms, 5000);
    }

    #[test]
    fn test_timeout_config_defaults() {
        let config = TimeoutConfig::default();
        assert_eq!(config.read_timeout_ms, 30000);
        assert_eq!(config.write_timeout_ms, 30000);
        assert_eq!(config.idle_timeout_ms, 300000);
        assert_eq!(config.request_timeout_ms, 60000);
    }

    #[test]
    fn test_auto_tune_defaults() {
        let config = AutoTuneConfig::default();
        assert_eq!(config.mode, AutoTuneMode::Conservative);
        assert_eq!(config.measurement_window_secs, 60);
        assert_eq!(config.target_cpu_percent, 70);
        assert_eq!(config.target_latency_p99_ms, 100);
        // FINDING-GW-PERF-01: conservative auto-tune is default — prevents aggressive resource consumption
    }

    #[test]
    fn test_auto_tune_mode_variants() {
        let _ = AutoTuneMode::Disabled;
        let _ = AutoTuneMode::Conservative;
        let _ = AutoTuneMode::Aggressive;
        assert_eq!(AutoTuneConfig::default().mode, AutoTuneMode::Conservative);
    }

    // ============================================================================
    // Category 2: Per-Protocol Configuration (5 tests)
    // ============================================================================

    #[test]
    fn test_nfs_has_larger_buffers() {
        let config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        assert_eq!(config.buffers.recv_buf_size, 512 * 1024);
        assert_eq!(config.connections.max_connections, 2048);
        assert_eq!(config.connections.idle_timeout_secs, 300);
        // FINDING-GW-PERF-02: NFS gets larger buffers and more connections — matches protocol needs
    }

    #[test]
    fn test_s3_has_smaller_buffers() {
        let config = PerfConfig::for_protocol(GatewayProtocol::S3);
        assert_eq!(config.buffers.recv_buf_size, 128 * 1024);
        assert_eq!(config.connections.max_connections, 512);
        assert_eq!(config.connections.idle_timeout_secs, 180);
    }

    #[test]
    fn test_pnfs_matches_nfs() {
        let nfs = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        let pnfs = PerfConfig::for_protocol(GatewayProtocol::Pnfs);
        assert_eq!(nfs.buffers.recv_buf_size, pnfs.buffers.recv_buf_size);
        assert_eq!(nfs.buffers.send_buf_size, pnfs.buffers.send_buf_size);
        assert_eq!(
            nfs.connections.max_connections,
            pnfs.connections.max_connections
        );
    }

    #[test]
    fn test_smb_matches_s3() {
        let s3 = PerfConfig::for_protocol(GatewayProtocol::S3);
        let smb = PerfConfig::for_protocol(GatewayProtocol::Smb);
        assert_eq!(s3.buffers.recv_buf_size, smb.buffers.recv_buf_size);
        assert_eq!(s3.buffers.send_buf_size, smb.buffers.send_buf_size);
        assert_eq!(
            s3.connections.max_connections,
            smb.connections.max_connections
        );
    }

    #[test]
    fn test_protocol_accessor() {
        let nfs_config = PerfConfig::for_protocol(GatewayProtocol::Nfs);
        assert_eq!(nfs_config.protocol(), &GatewayProtocol::Nfs);
        let s3_config = PerfConfig::for_protocol(GatewayProtocol::S3);
        assert_eq!(s3_config.protocol(), &GatewayProtocol::S3);
    }

    // ============================================================================
    // Category 3: Validation — Zero Values (5 tests)
    // ============================================================================

    #[test]
    fn test_validate_rejects_zero_max_connections() {
        let config = make_config_with_max_connections(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroMaxConnections)));
        // FINDING-GW-PERF-03: zero connections rejected — prevents DoS from misconfiguration
    }

    #[test]
    fn test_validate_rejects_zero_max_per_client() {
        let config = make_config_with_max_per_client(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroMaxPerClient)));
    }

    #[test]
    fn test_validate_rejects_zero_recv_buf() {
        let config = make_config_with_recv_buf_size(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
    }

    #[test]
    fn test_validate_rejects_zero_send_buf() {
        let config = make_config_with_send_buf_size(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
    }

    #[test]
    fn test_validate_rejects_zero_max_request_size() {
        let config = make_config_with_max_request_size(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroBufferSize)));
        // FINDING-GW-PERF-04: zero buffer sizes rejected — prevents I/O stalls
    }

    #[test]
    fn test_validate_rejects_zero_read_timeout() {
        let config = make_config_with_read_timeout_ms(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroTimeout)));
    }

    #[test]
    fn test_validate_rejects_zero_write_timeout() {
        let config = make_config_with_write_timeout_ms(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(result, Err(PerfConfigError::ZeroTimeout)));
    }

    #[test]
    fn test_validate_rejects_zero_measurement_window() {
        let config = make_config_with_measurement_window(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::ZeroMeasurementWindow)
        ));
        // FINDING-GW-PERF-05: zero measurement window rejected — prevents divide-by-zero in auto-tune
    }

    // ============================================================================
    // Category 4: Validation — Boundary Values (5 tests)
    // ============================================================================

    #[test]
    fn test_validate_rejects_per_client_exceeds_max() {
        let config = make_config_with_per_client_and_max(100, 10);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::PerClientExceedsMax(100, 10))
        ));
        // FINDING-GW-PERF-06: per-client limit cannot exceed total — prevents single client monopolizing connections
    }

    #[test]
    fn test_validate_rejects_cpu_target_101() {
        let config = make_config_with_cpu_target(101);
        let result = PerfConfigValidator::validate(&config);
        assert!(matches!(
            result,
            Err(PerfConfigError::InvalidCpuTarget(101))
        ));
        // FINDING-GW-PERF-07: CPU target capped at 100% — prevents invalid auto-tune targets
    }

    #[test]
    fn test_validate_accepts_cpu_target_100() {
        let config = make_config_with_cpu_target(100);
        let result = PerfConfigValidator::validate(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_accepts_cpu_target_0() {
        let config = make_config_with_cpu_target(0);
        let result = PerfConfigValidator::validate(&config);
        assert!(result.is_ok());
        // FINDING-GW-PERF-08: zero CPU target valid — effectively disables CPU-based tuning
    }

    #[test]
    fn test_validate_accepts_default_configs() {
        assert!(PerfConfigValidator::validate(&make_nfs_config()).is_ok());
        assert!(PerfConfigValidator::validate(&make_s3_config()).is_ok());
        assert!(PerfConfigValidator::validate(&make_pnfs_config()).is_ok());
        assert!(PerfConfigValidator::validate(&make_smb_config()).is_ok());
        // FINDING-GW-PERF-09: all default configs pass validation — safe out-of-the-box
    }

    // ============================================================================
    // Category 5: Config Construction & Edge Cases (5 tests)
    // ============================================================================

    #[test]
    fn test_per_client_equals_max_accepted() {
        let config = make_config_with_per_client_and_max(100, 100);
        let result = PerfConfigValidator::validate(&config);
        assert!(result.is_ok());
        // FINDING-GW-PERF-10: equal per-client and total limits valid — single-client mode supported
    }

    #[test]
    fn test_s3_shorter_timeouts_than_nfs() {
        let nfs = make_nfs_config();
        let s3 = make_s3_config();
        assert!(s3.timeouts.read_timeout_ms < nfs.timeouts.read_timeout_ms);
        assert!(s3.timeouts.request_timeout_ms < nfs.timeouts.request_timeout_ms);
        // FINDING-GW-PERF-11: S3 uses shorter timeouts — HTTP protocol expects faster responses
    }

    #[test]
    fn test_nfs_more_connections_than_s3() {
        let nfs = make_nfs_config();
        let s3 = make_s3_config();
        assert!(nfs.connections.max_connections > s3.connections.max_connections);
        assert!(nfs.connections.max_per_client > s3.connections.max_per_client);
    }

    #[test]
    fn test_all_protocol_variants() {
        let _ = GatewayProtocol::Nfs;
        let _ = GatewayProtocol::Pnfs;
        let _ = GatewayProtocol::S3;
        let _ = GatewayProtocol::Smb;
        assert_ne!(GatewayProtocol::Nfs, GatewayProtocol::S3);
        assert_ne!(GatewayProtocol::Pnfs, GatewayProtocol::Smb);
    }

    #[test]
    fn test_config_is_copy() {
        let original = make_nfs_config();
        let copied = original;
        let modified = PerfConfig {
            protocol: GatewayProtocol::S3,
            ..original
        };
        assert_eq!(original.protocol(), &GatewayProtocol::Nfs);
        assert_eq!(copied.protocol(), &GatewayProtocol::Nfs);
        assert_eq!(modified.protocol(), &GatewayProtocol::S3);
    }
}
