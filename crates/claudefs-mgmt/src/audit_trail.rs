use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_EVENTS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuditEventKind {
    Login,
    Logout,
    TokenCreate,
    TokenRevoke,
    QuotaChange,
    RoleAssign,
    RoleRevoke,
    NodeDrain,
    SnapshotCreate,
    SnapshotDelete,
    MigrationStart,
    MigrationAbort,
    ConfigChange,
    AdminCommand,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub id: u64,
    pub timestamp: u64,
    pub user: String,
    pub ip: String,
    pub kind: AuditEventKind,
    pub resource: String,
    pub detail: String,
    pub success: bool,
}

impl AuditEvent {
    pub fn new(
        id: u64,
        timestamp: u64,
        user: String,
        ip: String,
        kind: AuditEventKind,
        resource: String,
        detail: String,
        success: bool,
    ) -> Self {
        Self {
            id,
            timestamp,
            user,
            ip,
            kind,
            resource,
            detail,
            success,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AuditFilter {
    pub user: Option<String>,
    pub kind: Option<AuditEventKind>,
    pub since_ts: Option<u64>,
    pub until_ts: Option<u64>,
    pub success_only: bool,
}

impl AuditFilter {
    pub fn new() -> Self {
        Self {
            user: None,
            kind: None,
            since_ts: None,
            until_ts: None,
            success_only: false,
        }
    }

    pub fn matches(&self, event: &AuditEvent) -> bool {
        if let Some(ref u) = self.user {
            if &event.user != u {
                return false;
            }
        }
        if let Some(ref k) = self.kind {
            if &event.kind != k {
                return false;
            }
        }
        if let Some(since) = self.since_ts {
            if event.timestamp < since {
                return false;
            }
        }
        if let Some(until) = self.until_ts {
            if event.timestamp > until {
                return false;
            }
        }
        if self.success_only && !event.success {
            return false;
        }
        true
    }
}

pub struct AuditTrail {
    events: Mutex<VecDeque<AuditEvent>>,
    next_id: Mutex<u64>,
}

impl AuditTrail {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENTS)),
            next_id: Mutex::new(1),
        }
    }

    pub fn record(
        &self,
        user: &str,
        ip: &str,
        kind: AuditEventKind,
        resource: &str,
        detail: &str,
        success: bool,
    ) -> u64 {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let id = {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            id
        };

        let event = AuditEvent::new(
            id,
            timestamp,
            user.to_string(),
            ip.to_string(),
            kind,
            resource.to_string(),
            detail.to_string(),
            success,
        );

        let mut events = self.events.lock().unwrap();
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(event);

        id
    }

    pub fn query(&self, filter: &AuditFilter) -> Vec<AuditEvent> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect()
    }

    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_increments_event_count() {
        let trail = AuditTrail::new();
        assert_eq!(trail.event_count(), 0);

        trail.record(
            "user1",
            "192.168.1.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        assert_eq!(trail.event_count(), 1);

        trail.record(
            "user2",
            "192.168.1.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        assert_eq!(trail.event_count(), 2);

        trail.record(
            "user3",
            "192.168.1.3",
            AuditEventKind::ConfigChange,
            "/config",
            "update",
            true,
        );
        assert_eq!(trail.event_count(), 3);
    }

    #[test]
    fn test_record_assigns_sequential_ids() {
        let trail = AuditTrail::new();

        let id1 = trail.record(
            "user1",
            "192.168.1.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        let id2 = trail.record(
            "user2",
            "192.168.1.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        let id3 = trail.record(
            "user3",
            "192.168.1.3",
            AuditEventKind::ConfigChange,
            "/config",
            "update",
            true,
        );

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[test]
    fn test_query_user_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record("bob", "10.0.0.2", AuditEventKind::Login, "/", "login", true);
        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        trail.record(
            "charlie",
            "10.0.0.3",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );

        let filter = AuditFilter {
            user: Some("alice".to_string()),
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.user == "alice"));
    }

    #[test]
    fn test_query_kind_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record(
            "user4",
            "10.0.0.4",
            AuditEventKind::ConfigChange,
            "/config",
            "change",
            true,
        );

        let filter = AuditFilter {
            kind: Some(AuditEventKind::Login),
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|e| matches!(e.kind, AuditEventKind::Login)));
    }

    #[test]
    fn test_query_time_range_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::ConfigChange,
            "/config",
            "change",
            true,
        );

        let all_events = trail.query(&AuditFilter::new());
        assert_eq!(all_events.len(), 3);

        let ts1 = all_events[0].timestamp;
        let ts2 = all_events[1].timestamp;
        let ts3 = all_events[2].timestamp;

        let mid = (ts1 + ts3) / 2;
        let filter = AuditFilter {
            since_ts: Some(ts1),
            until_ts: Some(mid),
            ..Default::default()
        };
        let results = trail.query(&filter);

        let count_in_range = all_events
            .iter()
            .filter(|e| e.timestamp >= ts1 && e.timestamp <= mid)
            .count();
        assert_eq!(results.len(), count_in_range);
    }

    #[test]
    fn test_query_success_only_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Login,
            "/",
            "failed login",
            false,
        );
        trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        trail.record(
            "user4",
            "10.0.0.4",
            AuditEventKind::Login,
            "/",
            "failed",
            false,
        );

        let filter = AuditFilter {
            success_only: true,
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.success));
    }

    #[test]
    fn test_ring_buffer_wraps_at_10000() {
        let trail = AuditTrail::new();

        for i in 0..10050 {
            trail.record(
                &format!("user{}", i),
                "10.0.0.1",
                AuditEventKind::Login,
                "/",
                "login",
                true,
            );
        }

        assert_eq!(trail.event_count(), MAX_EVENTS);

        let filter = AuditFilter::new();
        let results = trail.query(&filter);

        let first_event_id = results.first().map(|e| e.id);
        assert!(first_event_id.is_some());

        assert!(first_event_id.unwrap() > 50);
    }

    #[test]
    fn test_multiple_events_from_different_users() {
        let trail = AuditTrail::new();

        let users = vec!["alice", "bob", "charlie", "dave", "eve"];
        let kinds = vec![
            AuditEventKind::Login,
            AuditEventKind::TokenCreate,
            AuditEventKind::QuotaChange,
            AuditEventKind::RoleAssign,
            AuditEventKind::SnapshotCreate,
        ];

        for (i, user) in users.iter().enumerate() {
            trail.record(
                user,
                &format!("10.0.0.{}", i + 1),
                kinds[i].clone(),
                &format!("/resource{}", i),
                "detail",
                true,
            );
        }

        assert_eq!(trail.event_count(), 5);

        let filter = AuditFilter::new();
        let results = trail.query(&filter);
        assert_eq!(results.len(), 5);

        let user_names: Vec<String> = results.iter().map(|e| e.user.clone()).collect();
        assert!(user_names.contains(&"alice".to_string()));
        assert!(user_names.contains(&"bob".to_string()));
        assert!(user_names.contains(&"charlie".to_string()));
    }

    #[test]
    fn test_audit_filter_new_returns_all_pass() {
        let filter = AuditFilter::new();

        assert!(filter.user.is_none());
        assert!(filter.kind.is_none());
        assert!(filter.since_ts.is_none());
        assert!(filter.until_ts.is_none());
        assert!(!filter.success_only);

        let trail = AuditTrail::new();
        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            false,
        );

        let results = trail.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_empty_trail_query_returns_empty_vec() {
        let trail = AuditTrail::new();

        let filter = AuditFilter::new();
        let results = trail.query(&filter);

        assert!(results.is_empty());

        let user_filter = AuditFilter {
            user: Some("nonexistent".to_string()),
            ..Default::default()
        };
        let results2 = trail.query(&user_filter);
        assert!(results2.is_empty());
    }

    #[test]
    fn test_all_event_kinds() {
        let trail = AuditTrail::new();

        let kinds = vec![
            AuditEventKind::Login,
            AuditEventKind::Logout,
            AuditEventKind::TokenCreate,
            AuditEventKind::TokenRevoke,
            AuditEventKind::QuotaChange,
            AuditEventKind::RoleAssign,
            AuditEventKind::RoleRevoke,
            AuditEventKind::NodeDrain,
            AuditEventKind::SnapshotCreate,
            AuditEventKind::SnapshotDelete,
            AuditEventKind::MigrationStart,
            AuditEventKind::MigrationAbort,
            AuditEventKind::ConfigChange,
            AuditEventKind::AdminCommand,
        ];

        for (i, kind) in kinds.iter().enumerate() {
            trail.record("admin", "10.0.0.1", kind.clone(), "/", "action", true);
        }

        assert_eq!(trail.event_count(), 14);

        let filter = AuditFilter::new();
        let results = trail.query(&filter);
        assert_eq!(results.len(), 14);
    }

    #[test]
    fn test_combined_filters() {
        let trail = AuditTrail::new();

        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login1",
            true,
        );
        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::ConfigChange,
            "/config",
            "change1",
            true,
        );
        trail.record(
            "bob",
            "10.0.0.2",
            AuditEventKind::Login,
            "/",
            "login2",
            true,
        );
        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login3",
            false,
        );
        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );

        let filter = AuditFilter {
            user: Some("alice".to_string()),
            kind: Some(AuditEventKind::Login),
            success_only: true,
            ..Default::default()
        };

        let results = trail.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].user, "alice");
        assert!(matches!(results[0].kind, AuditEventKind::Login));
        assert!(results[0].success);
    }

    #[test]
    fn test_record_returns_event_id() {
        let trail = AuditTrail::new();

        let id1 = trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        let id2 = trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        let filter = AuditFilter::new();
        let results = trail.query(&filter);

        let found = results.iter().any(|e| e.id == id1);
        assert!(found);
    }

    #[test]
    fn test_since_ts_exclusive() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );

        let filter = AuditFilter {
            since_ts: Some(u64::MAX),
            ..Default::default()
        };
        let results = trail.query(&filter);
        assert!(results.is_empty());
    }

    #[test]
    fn test_until_ts_exclusive() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );

        let filter = AuditFilter {
            until_ts: Some(0),
            ..Default::default()
        };
        let results = trail.query(&filter);
        assert!(results.is_empty());
    }

    #[test]
    fn test_event_fields_populated() {
        let trail = AuditTrail::new();

        let id = trail.record(
            "testuser",
            "192.168.100.50",
            AuditEventKind::TokenCreate,
            "/tokens",
            "created token for api access",
            true,
        );

        let filter = AuditFilter::new();
        let results = trail.query(&filter);

        assert_eq!(results.len(), 1);
        let event = &results[0];

        assert_eq!(event.id, id);
        assert_eq!(event.user, "testuser");
        assert_eq!(event.ip, "192.168.100.50");
        assert!(matches!(event.kind, AuditEventKind::TokenCreate));
        assert_eq!(event.resource, "/tokens");
        assert_eq!(event.detail, "created token for api access");
        assert!(event.success);
        assert!(event.timestamp > 0);
    }

    #[test]
    fn test_failure_events_recorded() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "invalid password",
            false,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::TokenRevoke,
            "/tokens",
            "not authorized",
            false,
        );

        let filter = AuditFilter {
            success_only: false,
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(!results[0].success);
        assert!(!results[1].success);
    }

    #[test]
    fn test_event_kind_equality() {
        let k1 = AuditEventKind::Login;
        let k2 = AuditEventKind::Login;
        let k3 = AuditEventKind::Logout;

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_audit_filter_default() {
        let default_filter = AuditFilter::default();

        assert!(default_filter.user.is_none());
        assert!(default_filter.kind.is_none());
        assert!(!default_filter.success_only);
    }

    #[test]
    fn test_audit_trail_default() {
        let trail: AuditTrail = Default::default();

        assert_eq!(trail.event_count(), 0);

        let filter = AuditFilter::new();
        let results = trail.query(&filter);
        assert!(results.is_empty());
    }

    #[test]
    fn test_sequential_ids_never_repeat() {
        let trail = AuditTrail::new();

        let mut ids = Vec::new();
        for i in 0..100 {
            let id = trail.record(
                &format!("user{}", i),
                "10.0.0.1",
                AuditEventKind::Login,
                "/",
                "login",
                true,
            );
            ids.push(id);
        }

        let unique_ids: std::collections::HashSet<u64> = ids.into_iter().collect();
        assert_eq!(unique_ids.len(), 100);
    }

    #[test]
    fn test_query_with_resource_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::ConfigChange,
            "/etc/config",
            "change1",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::ConfigChange,
            "/etc/other",
            "change2",
            true,
        );
        trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::ConfigChange,
            "/etc/config",
            "change3",
            true,
        );

        let filter = AuditFilter {
            kind: Some(AuditEventKind::ConfigChange),
            ..Default::default()
        };
        let results = trail.query(&filter);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_audit_event_clone() {
        let event = AuditEvent::new(
            1,
            1000000,
            "testuser".to_string(),
            "10.0.0.1".to_string(),
            AuditEventKind::Login,
            "/".to_string(),
            "login".to_string(),
            true,
        );

        let cloned = event.clone();
        assert_eq!(event.id, cloned.id);
        assert_eq!(event.user, cloned.user);
        assert_eq!(event.ip, cloned.ip);
        assert_eq!(event.kind, cloned.kind);
        assert_eq!(event.resource, cloned.resource);
        assert_eq!(event.detail, cloned.detail);
        assert_eq!(event.success, cloned.success);
    }

    #[test]
    fn test_filter_kind_and_user_combined() {
        let trail = AuditTrail::new();

        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "login",
            true,
        );
        trail.record("bob", "10.0.0.2", AuditEventKind::Login, "/", "login", true);
        trail.record(
            "alice",
            "10.0.0.1",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );
        trail.record(
            "bob",
            "10.0.0.2",
            AuditEventKind::Logout,
            "/",
            "logout",
            true,
        );

        let filter = AuditFilter {
            user: Some("alice".to_string()),
            kind: Some(AuditEventKind::Login),
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].user, "alice");
    }

    #[test]
    fn test_large_batch_record_performance() {
        let trail = AuditTrail::new();

        let start = std::time::Instant::now();
        for i in 0..1000 {
            trail.record(
                &format!("user{}", i % 10),
                "10.0.0.1",
                AuditEventKind::AdminCommand,
                "/",
                "command",
                true,
            );
        }
        let elapsed = start.elapsed();

        assert_eq!(trail.event_count(), 1000);
        assert!(elapsed.as_millis() < 100);
    }

    #[test]
    fn test_mixed_success_failure_filter() {
        let trail = AuditTrail::new();

        trail.record(
            "user1",
            "10.0.0.1",
            AuditEventKind::Login,
            "/",
            "success",
            true,
        );
        trail.record(
            "user2",
            "10.0.0.2",
            AuditEventKind::Login,
            "/",
            "failed",
            false,
        );
        trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::Login,
            "/",
            "success",
            true,
        );
        trail.record(
            "user4",
            "10.0.0.4",
            AuditEventKind::Login,
            "/",
            "failed",
            false,
        );

        let filter = AuditFilter {
            success_only: false,
            kind: Some(AuditEventKind::Login),
            ..Default::default()
        };
        let results = trail.query(&filter);
        assert_eq!(results.len(), 4);

        let success_filter = AuditFilter {
            success_only: true,
            kind: Some(AuditEventKind::Login),
            ..Default::default()
        };
        let success_results = trail.query(&success_filter);
        assert_eq!(success_results.len(), 2);
    }
}
