//! Gateway wire validation, audit trail, and access log security tests.
//!
//! Part of A10 Phase 19: Gateway wire/audit/access-log security audit

use claudefs_gateway::access_log::{AccessLog, AccessLogEntry, AccessLogStats, GatewayProtocol};
use claudefs_gateway::gateway_audit::{
    AuditConfig, AuditEventType, AuditRecord, AuditSeverity, AuditTrail,
};
use claudefs_gateway::wire::{
    compute_etag, format_mode, generate_request_id, is_valid_iso8601, parse_mode,
    validate_nfs_count, validate_nfs_fh, validate_nfs_filename, validate_nfs_path,
    validate_part_number, validate_s3_key, validate_s3_size, validate_upload_id,
};

#[cfg(test)]
mod tests {
    use super::*;

    // Category 1: Wire Protocol Validation — NFS (5 tests)

    #[test]
    fn test_wire_nfs_fh_valid() {
        assert!(validate_nfs_fh(&[1, 2, 3]).is_ok());
        assert!(validate_nfs_fh(&[0u8; 64]).is_ok());
        assert!(validate_nfs_fh(&[]).is_err());
        assert!(validate_nfs_fh(&[0u8; 65]).is_err());
    }

    #[test]
    fn test_wire_nfs_filename_validation() {
        assert!(validate_nfs_filename("file.txt").is_ok());
        assert!(validate_nfs_filename("").is_err());
        assert!(validate_nfs_filename(&"a".repeat(256)).is_err());
        assert!(validate_nfs_filename("a/b").is_err());
        assert!(validate_nfs_filename("a\0b").is_err());
    }

    #[test]
    fn test_wire_nfs_path_validation() {
        assert!(validate_nfs_path("/export/data").is_ok());
        assert!(validate_nfs_path("/").is_ok());
        assert!(validate_nfs_path("relative").is_err());
        assert!(validate_nfs_path(&"/".repeat(1025)).is_err());
        assert!(validate_nfs_path("/a\0b").is_err());
    }

    #[test]
    fn test_wire_nfs_count_validation() {
        assert!(validate_nfs_count(1).is_ok());
        assert!(validate_nfs_count(1_048_576).is_ok());
        assert!(validate_nfs_count(0).is_err());
        assert!(validate_nfs_count(1_048_577).is_err());
    }

    #[test]
    fn test_wire_mode_format_parse_roundtrip() {
        assert_eq!(format_mode(0o755), "rwxr-xr-x");
        assert_eq!(format_mode(0o644), "rw-r--r--");
        assert_eq!(format_mode(0o000), "---------");
        assert_eq!(parse_mode("755").unwrap(), 0o755);
        assert_eq!(parse_mode("rwxr-xr-x").unwrap(), 0o755);
        assert!(parse_mode("xyz").is_err());
    }

    // Category 2: Wire Protocol Validation — S3 & Utility (5 tests)

    #[test]
    fn test_wire_s3_key_validation() {
        assert!(validate_s3_key("file.txt").is_ok());
        assert!(validate_s3_key("folder/file").is_ok());
        assert!(validate_s3_key("").is_err());
        assert!(validate_s3_key(&"a".repeat(1025)).is_err());
        assert!(validate_s3_key("/leading").is_err());
    }

    #[test]
    fn test_wire_s3_size_validation() {
        assert!(validate_s3_size(0).is_ok());
        assert!(validate_s3_size(1000).is_ok());
        assert!(validate_s3_size(5_497_558_138_880).is_ok());
        assert!(validate_s3_size(5_497_558_138_881).is_err());
    }

    #[test]
    fn test_wire_part_number_upload_id() {
        assert!(validate_part_number(1).is_ok());
        assert!(validate_part_number(10000).is_ok());
        assert!(validate_part_number(0).is_err());
        assert!(validate_part_number(10001).is_err());
        assert!(validate_upload_id("abc123").is_ok());
        assert!(validate_upload_id("").is_err());
    }

    #[test]
    fn test_wire_etag_computation() {
        assert_eq!(compute_etag(b"hello").len(), 32);
        assert_eq!(compute_etag(b"").len(), 32);
        assert_eq!(compute_etag(b"hello"), compute_etag(b"hello"));
        assert_ne!(compute_etag(b"hello"), compute_etag(b"world"));
    }

    #[test]
    fn test_wire_iso8601_and_request_id() {
        assert!(is_valid_iso8601("2024-01-15T10:30:00Z"));
        assert!(!is_valid_iso8601("not-a-date"));
        assert!(!is_valid_iso8601("2024/01/15"));
        assert_eq!(generate_request_id(1).len(), 32);
        assert_ne!(generate_request_id(1), generate_request_id(2));
    }

    // Category 3: Audit Trail — Event Recording (5 tests)

    #[test]
    fn test_audit_severity_ordering() {
        assert!(AuditSeverity::Info < AuditSeverity::Warning);
        assert!(AuditSeverity::Critical > AuditSeverity::Warning);
        assert!(AuditSeverity::Info < AuditSeverity::Critical);
    }

    #[test]
    fn test_audit_event_type_severity_mapping() {
        assert_eq!(AuditEventType::AuthSuccess.severity(), AuditSeverity::Info);
        assert_eq!(
            AuditEventType::AuthFailure.severity(),
            AuditSeverity::Warning
        );
        assert_eq!(
            AuditEventType::ExportViolation.severity(),
            AuditSeverity::Warning
        );
        assert_eq!(
            AuditEventType::RateLimitTriggered.severity(),
            AuditSeverity::Info
        );
        assert_eq!(AuditEventType::AclDenied.severity(), AuditSeverity::Warning);
        assert_eq!(
            AuditEventType::TokenRevoked.severity(),
            AuditSeverity::Warning
        );
        assert_eq!(
            AuditEventType::TlsHandshakeFailed.severity(),
            AuditSeverity::Critical
        );
        assert_eq!(
            AuditEventType::UnauthorizedOperation.severity(),
            AuditSeverity::Critical
        );
    }

    #[test]
    fn test_audit_trail_record_and_query() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(
            AuditEventType::AuthSuccess,
            "10.0.0.1",
            "alice",
            "/data",
            "login ok",
        );
        trail.record(
            AuditEventType::AuthFailure,
            "10.0.0.2",
            "bob",
            "/data",
            "bad password",
        );
        trail.record(
            AuditEventType::TlsHandshakeFailed,
            "10.0.0.3",
            "charlie",
            "/",
            "tls error",
        );

        assert_eq!(trail.len(), 3);
        let warning_records = trail.records_by_severity(AuditSeverity::Warning);
        assert_eq!(warning_records.len(), 2);
        let auth_failure_records = trail.records_by_type(&AuditEventType::AuthFailure);
        assert_eq!(auth_failure_records.len(), 1);
        assert_eq!(trail.critical_count(), 1);
        assert_eq!(trail.warning_count(), 1);
    }

    #[test]
    fn test_audit_trail_disabled() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Info,
            max_records: 1000,
            enabled: false,
        };
        let mut trail = AuditTrail::new(config);
        let result = trail.record(AuditEventType::AuthSuccess, "10.0.0.1", "user", "/", "msg");
        assert!(result.is_none());
        assert!(trail.is_empty());
        assert_eq!(trail.len(), 0);
    }

    #[test]
    fn test_audit_trail_min_severity_filter() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Warning,
            max_records: 1000,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        let r1 = trail.record(AuditEventType::AuthSuccess, "10.0.0.1", "user", "/", "msg");
        assert!(r1.is_none());
        let r2 = trail.record(AuditEventType::AuthFailure, "10.0.0.1", "user", "/", "msg");
        assert!(r2.is_some());
        let r3 = trail.record(
            AuditEventType::TlsHandshakeFailed,
            "10.0.0.1",
            "user",
            "/",
            "msg",
        );
        assert!(r3.is_some());
        assert_eq!(trail.len(), 2);
    }

    // Category 4: Audit Trail — Ring Buffer & Edge Cases (5 tests)

    #[test]
    fn test_audit_ring_buffer_eviction() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Info,
            max_records: 3,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthSuccess, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(AuditEventType::AuthSuccess, "1.0.0.3", "u3", "/p3", "m3");
        trail.record(AuditEventType::AuthSuccess, "1.0.0.4", "u4", "/p4", "m4");

        assert_eq!(trail.len(), 3);
        let records = trail.records_by_severity(AuditSeverity::Info);
        assert!(!records.iter().any(|r| r.client_addr == "1.0.0.1"));
    }

    #[test]
    fn test_audit_record_fields() {
        let record = AuditRecord::new(
            42,
            AuditEventType::AuthFailure,
            "192.168.1.1",
            "alice",
            "/data",
            "bad password",
        );
        assert_eq!(record.id, 42);
        assert_eq!(record.severity, AuditSeverity::Warning);
        assert_eq!(record.client_addr, "192.168.1.1");
        assert_eq!(record.principal, "alice");
        assert_eq!(record.resource, "/data");
        assert_eq!(record.message, "bad password");
        assert_eq!(record.timestamp_ms, 0);
    }

    #[test]
    fn test_audit_config_defaults() {
        let config = AuditConfig::default();
        assert_eq!(config.min_severity, AuditSeverity::Info);
        assert_eq!(config.max_records, 10000);
        assert!(config.enabled);
    }

    #[test]
    fn test_audit_trail_clear() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "10.0.0.1", "user", "/", "msg1");
        trail.record(AuditEventType::AuthFailure, "10.0.0.2", "user", "/", "msg2");
        trail.clear();
        assert!(trail.is_empty());
        assert_eq!(trail.len(), 0);
    }

    #[test]
    fn test_audit_trail_id_monotonic() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "10.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "10.0.0.2", "u2", "/p2", "m2");
        trail.record(AuditEventType::AuthSuccess, "10.0.0.3", "u3", "/p3", "m3");
        trail.record(AuditEventType::AuthSuccess, "10.0.0.4", "u4", "/p4", "m4");
        trail.record(AuditEventType::AuthSuccess, "10.0.0.5", "u5", "/p5", "m5");

        let records: Vec<_> = trail.records_by_severity(AuditSeverity::Info);
        let ids: Vec<u64> = records.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
    }

    // Category 5: Access Log — Ring Buffer & Stats (5 tests)

    #[test]
    fn test_access_log_entry_builder() {
        let entry = AccessLogEntry::new("10.0.0.1", GatewayProtocol::Nfs3, "READ", "/file")
            .with_status(0)
            .with_bytes(4096)
            .with_duration_us(500)
            .with_uid(1000);
        assert_eq!(entry.client_ip, "10.0.0.1");
        assert_eq!(entry.protocol, GatewayProtocol::Nfs3);
        assert_eq!(entry.operation, "READ");
        assert_eq!(entry.resource, "/file");
        assert_eq!(entry.status, 0);
        assert_eq!(entry.bytes, 4096);
        assert_eq!(entry.duration_us, 500);
        assert_eq!(entry.uid, 1000);
        assert!(!entry.is_error());

        let error_entry =
            AccessLogEntry::new("10.0.0.1", GatewayProtocol::Nfs3, "READ", "/file").with_status(13);
        assert!(error_entry.is_error());
    }

    #[test]
    fn test_access_log_ring_buffer() {
        let log = AccessLog::new(3);
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "A", "/1"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "B", "/2"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "C", "/3"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "D", "/4"));

        assert_eq!(log.len(), 3);
        let recent = log.recent(3);
        assert!(!recent.iter().any(|e| e.operation == "A"));
        assert_eq!(recent[0].operation, "D");
    }

    #[test]
    fn test_access_log_stats_tracking() {
        let log = AccessLog::new(10);
        log.append(AccessLogEntry::new("ip1", GatewayProtocol::Nfs3, "READ", "/").with_bytes(1000));
        log.append(
            AccessLogEntry::new("ip2", GatewayProtocol::Nfs3, "WRITE", "/").with_bytes(2000),
        );
        log.append(
            AccessLogEntry::new("ip3", GatewayProtocol::Nfs3, "READ", "/")
                .with_status(5)
                .with_bytes(500),
        );

        let stats = log.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.error_count, 1);
        assert_eq!(stats.total_bytes, 3500);
        let error_rate = stats.error_rate();
        assert!((error_rate - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_access_log_filter_protocol_and_client() {
        let log = AccessLog::new(10);
        log.append(AccessLogEntry::new(
            "10.0.0.1",
            GatewayProtocol::Nfs3,
            "GETATTR",
            "/",
        ));
        log.append(AccessLogEntry::new(
            "10.0.0.2",
            GatewayProtocol::S3,
            "GET",
            "bucket/key",
        ));
        log.append(AccessLogEntry::new(
            "10.0.0.1",
            GatewayProtocol::Nfs3,
            "READ",
            "/file",
        ));

        let nfs_entries = log.filter_protocol(GatewayProtocol::Nfs3);
        assert_eq!(nfs_entries.len(), 2);
        let s3_entries = log.filter_protocol(GatewayProtocol::S3);
        assert_eq!(s3_entries.len(), 1);
        let client_entries = log.filter_client("10.0.0.1");
        assert_eq!(client_entries.len(), 2);
    }

    #[test]
    fn test_access_log_stats_avg_and_rate() {
        let stats = AccessLogStats::default();
        assert_eq!(stats.avg_duration_us(), 0);
        assert_eq!(stats.error_rate(), 0.0);
        assert_eq!(stats.requests_per_sec(0), 0.0);

        let mut stats = AccessLogStats::default();
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(100),
        );
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(200),
        );
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(300),
        );

        assert_eq!(stats.avg_duration_us(), 200);
        assert!((stats.requests_per_sec(10) - 0.3).abs() < 0.001);
    }
}
