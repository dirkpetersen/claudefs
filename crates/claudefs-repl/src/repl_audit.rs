//! Audit trail for replication events (compliance).

use crate::conflict_resolver::SiteId;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Kinds of audit events tracked for compliance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuditEventKind {
    /// Replication operation started.
    ReplicationStarted,
    /// Replication operation completed successfully.
    ReplicationCompleted,
    /// Replication operation failed.
    ReplicationFailed,
    /// Conflict detected between sites.
    ConflictDetected,
    /// Conflict was resolved.
    ConflictResolved,
    /// Site connected successfully.
    SiteConnected,
    /// Site disconnected.
    SiteDisconnected,
    /// TLS handshake completed successfully.
    TlsHandshakeCompleted,
    /// Rate limit was exceeded.
    RateLimitExceeded,
    /// Journal garbage collection completed.
    JournalGcCompleted,
}

/// An audit event recorded in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique identifier for this event.
    pub event_id: u64,
    /// The kind of event.
    pub kind: AuditEventKind,
    /// The site associated with this event.
    pub site_id: SiteId,
    /// Timestamp in nanoseconds since epoch.
    pub timestamp_ns: u64,
    /// Additional details about the event.
    pub details: String,
    /// Optional operator identifier.
    pub operator_id: Option<String>,
}

/// Filter for querying audit events.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    /// Filter by event kind.
    pub kind: Option<AuditEventKind>,
    /// Filter by site ID.
    pub site_id: Option<SiteId>,
    /// Filter events from this timestamp (inclusive).
    pub since_ns: Option<u64>,
    /// Filter events until this timestamp (inclusive).
    pub until_ns: Option<u64>,
}

/// Audit log for compliance tracking.
pub struct AuditLog {
    /// Recorded audit events.
    events: Vec<AuditEvent>,
    /// Next event ID to assign.
    next_id: u64,
}

impl AuditLog {
    /// Create a new empty audit log.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_id: 0,
        }
    }

    /// Record a new audit event.
    ///
    /// Returns the event ID of the recorded event.
    pub fn record(
        &mut self,
        kind: AuditEventKind,
        site_id: SiteId,
        timestamp_ns: u64,
        details: impl Into<String>,
        operator_id: Option<String>,
    ) -> u64 {
        let event_id = self.next_id;
        info!(event_id = event_id, kind = ?kind, site_id = site_id.0, "audit event recorded");
        let event = AuditEvent {
            event_id,
            kind,
            site_id,
            timestamp_ns,
            details: details.into(),
            operator_id,
        };
        self.events.push(event);
        self.next_id += 1;
        event_id
    }

    /// Query events matching the given filter.
    ///
    /// Returns events that match all non-None filter fields.
    pub fn query(&self, filter: &AuditFilter) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| {
                if let Some(ref kind) = filter.kind {
                    if &e.kind != kind {
                        return false;
                    }
                }
                if let Some(site_id) = filter.site_id {
                    if e.site_id != site_id {
                        return false;
                    }
                }
                if let Some(since_ns) = filter.since_ns {
                    if e.timestamp_ns < since_ns {
                        return false;
                    }
                }
                if let Some(until_ns) = filter.until_ns {
                    if e.timestamp_ns > until_ns {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get the total number of events in the log.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get all events for a specific site.
    pub fn events_for_site(&self, site_id: SiteId) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| e.site_id == site_id)
            .collect()
    }

    /// Get the latest n events.
    ///
    /// Returns the last n events, or all events if there are fewer than n.
    pub fn latest_n(&self, n: usize) -> Vec<&AuditEvent> {
        let start = if n >= self.events.len() {
            0
        } else {
            self.events.len() - n
        };
        self.events[start..].iter().collect()
    }

    /// Remove events older than the given timestamp.
    ///
    /// Removes all events with timestamp_ns < before_ns.
    pub fn clear_before(&mut self, before_ns: u64) {
        let original_len = self.events.len();
        self.events.retain(|e| e.timestamp_ns >= before_ns);
        let removed = original_len - self.events.len();
        if removed > 0 {
            info!(removed_events = removed, "audit log GC completed");
        }
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(n: u64) -> u64 {
        n * 1_000_000_000
    }

    #[test]
    fn clear_before_keeps_recent() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(5),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            SiteId(1),
            ts(10),
            "c",
            None,
        );

        log.clear_before(ts(6));
        assert_eq!(log.event_count(), 1);
    }

    #[test]
    fn new_audit_log_empty() {
        let log = AuditLog::new();
        assert_eq!(log.event_count(), 0);
    }

    #[test]
    fn record_returns_event_id() {
        let mut log = AuditLog::new();
        let id = log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "test",
            None,
        );
        assert_eq!(id, 0);
    }

    #[test]
    fn record_increments_id() {
        let mut log = AuditLog::new();
        let id1 = log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "test1",
            None,
        );
        let id2 = log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(2),
            "test2",
            None,
        );
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
    }

    #[test]
    fn record_stores_event() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(42),
            ts(5),
            "replication details",
            None,
        );
        assert_eq!(log.event_count(), 1);
        let events = log.events_for_site(SiteId(42));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].details, "replication details");
    }

    #[test]
    fn query_by_kind() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(2),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            SiteId(1),
            ts(3),
            "c",
            None,
        );

        let filter = AuditFilter {
            kind: Some(AuditEventKind::ReplicationStarted),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, AuditEventKind::ReplicationStarted);
    }

    #[test]
    fn query_by_site_id() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(2),
            ts(2),
            "b",
            None,
        );

        let filter = AuditFilter {
            site_id: Some(SiteId(2)),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].site_id, SiteId(2));
    }

    #[test]
    fn query_by_since_ns() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(5),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(10),
            "c",
            None,
        );

        let filter = AuditFilter {
            since_ns: Some(ts(5)),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_by_until_ns() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(5),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(10),
            "c",
            None,
        );

        let filter = AuditFilter {
            until_ns: Some(ts(5)),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_combined_filters() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(5),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(10),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(2),
            ts(15),
            "c",
            None,
        );

        let filter = AuditFilter {
            kind: Some(AuditEventKind::ReplicationStarted),
            site_id: Some(SiteId(1)),
            since_ns: Some(ts(3)),
            until_ns: Some(ts(8)),
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].site_id, SiteId(1));
    }

    #[test]
    fn query_empty_filter_returns_all() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(2),
            ts(2),
            "b",
            None,
        );

        let filter = AuditFilter::default();
        let results = log.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn event_count() {
        let mut log = AuditLog::new();
        assert_eq!(log.event_count(), 0);
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(2),
            "b",
            None,
        );
        assert_eq!(log.event_count(), 2);
    }

    #[test]
    fn events_for_site() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(2),
            ts(2),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            SiteId(1),
            ts(3),
            "c",
            None,
        );

        let events = log.events_for_site(SiteId(1));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn latest_n_returns_last_n() {
        let mut log = AuditLog::new();
        for i in 0..10 {
            log.record(
                AuditEventKind::ReplicationStarted,
                SiteId(1),
                ts(i),
                format!("event {}", i),
                None,
            );
        }

        let results = log.latest_n(3);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].details, "event 7");
        assert_eq!(results[1].details, "event 8");
        assert_eq!(results[2].details, "event 9");
    }

    #[test]
    fn latest_n_fewer_than_n() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(2),
            "b",
            None,
        );

        let results = log.latest_n(10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn latest_n_zero() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );

        let results = log.latest_n(0);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn clear_before_removes_old() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "a",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            SiteId(1),
            ts(5),
            "b",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            SiteId(1),
            ts(10),
            "c",
            None,
        );

        log.clear_before(ts(5));
        assert_eq!(log.event_count(), 2);
    }

    #[test]
    fn operator_id_stored() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            SiteId(1),
            ts(1),
            "test",
            Some("operator-123".to_string()),
        );

        let events = log.events_for_site(SiteId(1));
        assert_eq!(events[0].operator_id.as_deref(), Some("operator-123"));
    }

    #[test]
    fn details_stored() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ConflictDetected,
            SiteId(1),
            ts(1),
            "file123 conflicted at offset 4096",
            None,
        );

        let events = log.events_for_site(SiteId(1));
        assert_eq!(events[0].details, "file123 conflicted at offset 4096");
    }

    #[test]
    fn audit_event_serialize() {
        let event = AuditEvent {
            event_id: 42,
            kind: AuditEventKind::TlsHandshakeCompleted,
            site_id: SiteId(123),
            timestamp_ns: 1_000_000_000,
            details: "TLSv1.3 established".to_string(),
            operator_id: Some("admin".to_string()),
        };

        let serialized = bincode::serialize(&event).unwrap();
        let deserialized: AuditEvent = bincode::deserialize(&serialized).unwrap();

        assert_eq!(event, deserialized);
    }

    #[test]
    fn audit_filter_default() {
        let filter = AuditFilter::default();
        assert!(filter.kind.is_none());
        assert!(filter.site_id.is_none());
        assert!(filter.since_ns.is_none());
        assert!(filter.until_ns.is_none());
    }

    #[test]
    fn multiple_sites_tracked() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::SiteConnected,
            SiteId(1),
            ts(1),
            "site 1 connected",
            None,
        );
        log.record(
            AuditEventKind::SiteConnected,
            SiteId(2),
            ts(2),
            "site 2 connected",
            None,
        );
        log.record(
            AuditEventKind::SiteDisconnected,
            SiteId(1),
            ts(3),
            "site 1 disconnected",
            None,
        );
        log.record(
            AuditEventKind::SiteConnected,
            SiteId(3),
            ts(4),
            "site 3 connected",
            None,
        );

        assert_eq!(log.events_for_site(SiteId(1)).len(), 2);
        assert_eq!(log.events_for_site(SiteId(2)).len(), 1);
        assert_eq!(log.events_for_site(SiteId(3)).len(), 1);
    }
}
