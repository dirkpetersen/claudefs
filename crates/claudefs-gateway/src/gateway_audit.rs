//! Security audit trail for ClaudeFS gateway events

use std::collections::VecDeque;

/// Audit event severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditSeverity {
    /// Informational event
    Info,
    /// Warning event
    Warning,
    /// Critical event
    Critical,
}

/// Gateway audit event type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditEventType {
    /// Successful authentication
    AuthSuccess,
    /// Failed authentication attempt
    AuthFailure,
    /// Export permission violation
    ExportViolation,
    /// Rate limit threshold exceeded
    RateLimitTriggered,
    /// ACL permission denied
    AclDenied,
    /// Token has been revoked
    TokenRevoked,
    /// TLS handshake failure
    TlsHandshakeFailed,
    /// Unauthorized operation attempted
    UnauthorizedOperation,
}

impl AuditEventType {
    /// Returns the severity level for this event type
    pub fn severity(&self) -> AuditSeverity {
        match self {
            AuditEventType::AuthSuccess => AuditSeverity::Info,
            AuditEventType::AuthFailure => AuditSeverity::Warning,
            AuditEventType::ExportViolation => AuditSeverity::Warning,
            AuditEventType::RateLimitTriggered => AuditSeverity::Info,
            AuditEventType::AclDenied => AuditSeverity::Warning,
            AuditEventType::TokenRevoked => AuditSeverity::Warning,
            AuditEventType::TlsHandshakeFailed => AuditSeverity::Critical,
            AuditEventType::UnauthorizedOperation => AuditSeverity::Critical,
        }
    }
}

/// A single audit event record
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// Monotonically incrementing ID
    pub id: u64,
    /// Type of audit event
    pub event_type: AuditEventType,
    /// Severity level
    pub severity: AuditSeverity,
    /// Client IP address or "unknown"
    pub client_addr: String,
    /// Username, token ID, or "anonymous"
    pub principal: String,
    /// Path, bucket/key, or ""
    pub resource: String,
    /// Human-readable message
    pub message: String,
    /// Timestamp in milliseconds (0 in tests)
    pub timestamp_ms: u64,
}

impl AuditRecord {
    /// Create a new audit record
    pub fn new(
        id: u64,
        event_type: AuditEventType,
        client_addr: &str,
        principal: &str,
        resource: &str,
        message: &str,
    ) -> Self {
        let severity = event_type.severity();
        Self {
            id,
            event_type,
            severity,
            client_addr: client_addr.to_string(),
            principal: principal.to_string(),
            resource: resource.to_string(),
            message: message.to_string(),
            timestamp_ms: 0,
        }
    }
}

/// Audit trail configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditConfig {
    /// Minimum severity to record
    pub min_severity: AuditSeverity,
    /// Maximum records to store (ring buffer)
    pub max_records: usize,
    /// Whether auditing is enabled
    pub enabled: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            min_severity: AuditSeverity::Info,
            max_records: 10000,
            enabled: true,
        }
    }
}

/// In-memory audit trail with ring buffer semantics
pub struct AuditTrail {
    config: AuditConfig,
    records: VecDeque<AuditRecord>,
    next_id: u64,
}

impl AuditTrail {
    /// Create a new audit trail with the given configuration
    pub fn new(config: AuditConfig) -> Self {
        Self {
            config,
            records: VecDeque::new(),
            next_id: 0,
        }
    }

    /// Record an audit event. Returns the assigned ID, or None if disabled/filtered.
    pub fn record(
        &mut self,
        event_type: AuditEventType,
        client_addr: &str,
        principal: &str,
        resource: &str,
        message: &str,
    ) -> Option<u64> {
        if !self.config.enabled {
            return None;
        }

        let severity = event_type.severity();
        if severity < self.config.min_severity {
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;

        let record = AuditRecord::new(id, event_type, client_addr, principal, resource, message);
        self.records.push_back(record);

        while self.records.len() > self.config.max_records {
            self.records.pop_front();
        }

        Some(id)
    }

    /// Get all records with severity >= min_severity
    pub fn records_by_severity(&self, min: AuditSeverity) -> Vec<&AuditRecord> {
        self.records.iter().filter(|r| r.severity >= min).collect()
    }

    /// Get all records for a specific event type
    pub fn records_by_type(&self, event_type: &AuditEventType) -> Vec<&AuditRecord> {
        self.records
            .iter()
            .filter(|r| &r.event_type == event_type)
            .collect()
    }

    /// Total records stored
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True if no records stored
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Critical events count
    pub fn critical_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.severity == AuditSeverity::Critical)
            .count()
    }

    /// Warning events count
    pub fn warning_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.severity == AuditSeverity::Warning)
            .count()
    }

    /// Clear all records
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_type_severity_auth_success() {
        assert_eq!(AuditEventType::AuthSuccess.severity(), AuditSeverity::Info);
    }

    #[test]
    fn test_audit_event_type_severity_auth_failure() {
        assert_eq!(
            AuditEventType::AuthFailure.severity(),
            AuditSeverity::Warning
        );
    }

    #[test]
    fn test_audit_event_type_severity_export_violation() {
        assert_eq!(
            AuditEventType::ExportViolation.severity(),
            AuditSeverity::Warning
        );
    }

    #[test]
    fn test_audit_event_type_severity_rate_limit() {
        assert_eq!(
            AuditEventType::RateLimitTriggered.severity(),
            AuditSeverity::Info
        );
    }

    #[test]
    fn test_audit_event_type_severity_acl_denied() {
        assert_eq!(AuditEventType::AclDenied.severity(), AuditSeverity::Warning);
    }

    #[test]
    fn test_audit_event_type_severity_token_revoked() {
        assert_eq!(
            AuditEventType::TokenRevoked.severity(),
            AuditSeverity::Warning
        );
    }

    #[test]
    fn test_audit_event_type_severity_tls_handshake_failed() {
        assert_eq!(
            AuditEventType::TlsHandshakeFailed.severity(),
            AuditSeverity::Critical
        );
    }

    #[test]
    fn test_audit_event_type_severity_unauthorized_operation() {
        assert_eq!(
            AuditEventType::UnauthorizedOperation.severity(),
            AuditSeverity::Critical
        );
    }

    #[test]
    fn test_audit_severity_ordering() {
        assert!(AuditSeverity::Info < AuditSeverity::Warning);
        assert!(AuditSeverity::Warning < AuditSeverity::Critical);
        assert!(AuditSeverity::Info < AuditSeverity::Critical);
    }

    #[test]
    fn test_audit_severity_critical_greater_than_warning() {
        assert!(AuditSeverity::Critical > AuditSeverity::Warning);
    }

    #[test]
    fn test_audit_record_new() {
        let record = AuditRecord::new(
            42,
            AuditEventType::AuthFailure,
            "192.168.1.100",
            "alice",
            "/data/files",
            "Invalid password",
        );
        assert_eq!(record.id, 42);
        assert_eq!(record.event_type, AuditEventType::AuthFailure);
        assert_eq!(record.severity, AuditSeverity::Warning);
        assert_eq!(record.client_addr, "192.168.1.100");
        assert_eq!(record.principal, "alice");
        assert_eq!(record.resource, "/data/files");
        assert_eq!(record.message, "Invalid password");
        assert_eq!(record.timestamp_ms, 0);
    }

    #[test]
    fn test_audit_config_default() {
        let config = AuditConfig::default();
        assert_eq!(config.min_severity, AuditSeverity::Info);
        assert_eq!(config.max_records, 10000);
        assert!(config.enabled);
    }

    #[test]
    fn test_audit_trail_new_is_empty() {
        let trail = AuditTrail::new(AuditConfig::default());
        assert!(trail.is_empty());
        assert_eq!(trail.len(), 0);
    }

    #[test]
    fn test_audit_trail_record_returns_id() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        let id = trail.record(
            AuditEventType::AuthSuccess,
            "192.168.1.1",
            "bob",
            "/data",
            "Login successful",
        );
        assert!(id.is_some());
        assert!(!trail.is_empty());
    }

    #[test]
    fn test_audit_trail_record_increments_id() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        let id1 = trail.record(
            AuditEventType::AuthSuccess,
            "192.168.1.1",
            "user1",
            "/path1",
            "msg1",
        );
        let id2 = trail.record(
            AuditEventType::AuthFailure,
            "192.168.1.2",
            "user2",
            "/path2",
            "msg2",
        );
        assert_eq!(id1, Some(0));
        assert_eq!(id2, Some(1));
    }

    #[test]
    fn test_audit_trail_record_returns_none_when_disabled() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Info,
            max_records: 1000,
            enabled: false,
        };
        let mut trail = AuditTrail::new(config);
        let id = trail.record(
            AuditEventType::AuthSuccess,
            "192.168.1.1",
            "user",
            "/path",
            "message",
        );
        assert!(id.is_none());
        assert!(trail.is_empty());
    }

    #[test]
    fn test_audit_trail_record_returns_none_below_min_severity() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Warning,
            max_records: 1000,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        let id = trail.record(
            AuditEventType::AuthSuccess,
            "192.168.1.1",
            "user",
            "/path",
            "message",
        );
        assert!(id.is_none());
        assert!(trail.is_empty());
    }

    #[test]
    fn test_audit_trail_record_stores_critical_when_min_info() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Info,
            max_records: 1000,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        let id = trail.record(
            AuditEventType::TlsHandshakeFailed,
            "192.168.1.1",
            "user",
            "/path",
            "TLS error",
        );
        assert!(id.is_some());
        assert_eq!(trail.len(), 1);
        assert_eq!(trail.critical_count(), 1);
    }

    #[test]
    fn test_audit_trail_ring_buffer_eviction() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Info,
            max_records: 3,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthSuccess, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(AuditEventType::AuthSuccess, "1.0.0.3", "u3", "/p3", "m3");
        assert_eq!(trail.len(), 3);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.4", "u4", "/p4", "m4");
        assert_eq!(trail.len(), 3);
        let records: Vec<_> = trail.records_by_severity(AuditSeverity::Info);
        assert!(records.iter().all(|r| r.client_addr != "1.0.0.1"));
    }

    #[test]
    fn test_audit_trail_records_by_severity() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(
            AuditEventType::TlsHandshakeFailed,
            "1.0.0.3",
            "u3",
            "/p3",
            "m3",
        );

        let info = trail.records_by_severity(AuditSeverity::Info);
        assert_eq!(info.len(), 3);

        let warning = trail.records_by_severity(AuditSeverity::Warning);
        assert_eq!(warning.len(), 2);

        let critical = trail.records_by_severity(AuditSeverity::Critical);
        assert_eq!(critical.len(), 1);
    }

    #[test]
    fn test_audit_trail_records_by_type() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(AuditEventType::AuthFailure, "1.0.0.3", "u3", "/p3", "m3");

        let auth_success = trail.records_by_type(&AuditEventType::AuthSuccess);
        assert_eq!(auth_success.len(), 1);

        let auth_failure = trail.records_by_type(&AuditEventType::AuthFailure);
        assert_eq!(auth_failure.len(), 2);

        let other = trail.records_by_type(&AuditEventType::TlsHandshakeFailed);
        assert!(other.is_empty());
    }

    #[test]
    fn test_audit_trail_len() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        assert_eq!(trail.len(), 0);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        assert_eq!(trail.len(), 1);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.2", "u2", "/p2", "m2");
        assert_eq!(trail.len(), 2);
    }

    #[test]
    fn test_audit_trail_is_empty() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        assert!(trail.is_empty());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        assert!(!trail.is_empty());
    }

    #[test]
    fn test_audit_trail_critical_count() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(
            AuditEventType::TlsHandshakeFailed,
            "1.0.0.3",
            "u3",
            "/p3",
            "m3",
        );
        trail.record(
            AuditEventType::UnauthorizedOperation,
            "1.0.0.4",
            "u4",
            "/p4",
            "m4",
        );

        assert_eq!(trail.critical_count(), 2);
    }

    #[test]
    fn test_audit_trail_warning_count() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(
            AuditEventType::ExportViolation,
            "1.0.0.3",
            "u3",
            "/p3",
            "m3",
        );
        trail.record(
            AuditEventType::TlsHandshakeFailed,
            "1.0.0.4",
            "u4",
            "/p4",
            "m4",
        );

        assert_eq!(trail.warning_count(), 2);
    }

    #[test]
    fn test_audit_trail_clear() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        assert_eq!(trail.len(), 2);

        trail.clear();
        assert!(trail.is_empty());
        assert_eq!(trail.len(), 0);
    }

    #[test]
    fn test_audit_trail_records_by_type_unknown() {
        let mut trail = AuditTrail::new(AuditConfig::default());
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");

        let records = trail.records_by_type(&AuditEventType::TlsHandshakeFailed);
        assert!(records.is_empty());
    }

    #[test]
    fn test_audit_trail_min_severity_filters_events() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Critical,
            max_records: 1000,
            enabled: true,
        };
        let mut trail = AuditTrail::new(config);
        trail.record(AuditEventType::AuthSuccess, "1.0.0.1", "u1", "/p1", "m1");
        trail.record(AuditEventType::AuthFailure, "1.0.0.2", "u2", "/p2", "m2");
        trail.record(
            AuditEventType::TlsHandshakeFailed,
            "1.0.0.3",
            "u3",
            "/p3",
            "m3",
        );

        assert_eq!(trail.len(), 1);
        assert_eq!(trail.critical_count(), 1);
    }
}
