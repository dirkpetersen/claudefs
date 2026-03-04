//! Security tests for management RBAC, audit, compliance, live config, and rate limiting
//!
//! Part of A10 Phase 3: Mgmt subsystem security audit

#[cfg(test)]
mod tests {
    use claudefs_mgmt::audit_trail::{AuditEvent, AuditEventKind, AuditFilter, AuditTrail};
    use claudefs_mgmt::compliance::{
        ComplianceError, ComplianceRegistry, RetentionPolicy, RetentionRecord, RetentionStatus,
    };
    use claudefs_mgmt::live_config::{
        LiveConfigEntry, LiveConfigError, LiveConfigStore, ReloadStatus,
    };
    use claudefs_mgmt::rbac::{
        admin_role, operator_role, tenant_admin_role, viewer_role, Permission, RbacError,
        RbacRegistry, Role, User,
    };
    use claudefs_mgmt::security::{constant_time_eq, AuthRateLimiter};
    use std::collections::HashMap;

    // A. RBAC Security Tests

    #[test]
    fn test_rbac_admin_implies_all_permissions() {
        assert!(Permission::Admin.implies(&Permission::ViewCluster));
        assert!(Permission::Admin.implies(&Permission::ViewNodes));
        assert!(Permission::Admin.implies(&Permission::DrainNodes));
        assert!(Permission::Admin.implies(&Permission::ManageTiering));
        assert!(Permission::Admin.implies(&Permission::ManageSnapshots));
        assert!(Permission::Admin.implies(&Permission::ViewQuotas));
        assert!(Permission::Admin.implies(&Permission::ManageQuotas));
        assert!(Permission::Admin.implies(&Permission::ViewReplication));
        assert!(Permission::Admin.implies(&Permission::QueryAnalytics));
        assert!(Permission::Admin.implies(&Permission::ManageWebhooks));
        assert!(Permission::Admin.implies(&Permission::Admin));
    }

    #[test]
    fn test_rbac_non_admin_does_not_imply_admin() {
        assert!(!Permission::ViewCluster.implies(&Permission::Admin));
        assert!(!Permission::ViewNodes.implies(&Permission::Admin));
        assert!(!Permission::DrainNodes.implies(&Permission::Admin));
    }

    #[test]
    fn test_rbac_inactive_user_denied() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());

        let mut user = User::new("user1".to_string(), "alice".to_string());
        user.active = false;
        registry.add_user(user);
        registry.assign_role("user1", "admin").unwrap();

        let result = registry.check_permission("user1", &Permission::ViewCluster);
        assert!(matches!(result, Err(RbacError::PermissionDenied { .. })));
    }

    #[test]
    fn test_rbac_assign_role_to_nonexistent_user() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());

        let result = registry.assign_role("nonexistent", "admin");
        assert!(matches!(result, Err(RbacError::UserNotFound(_))));
    }

    #[test]
    fn test_rbac_assign_nonexistent_role() {
        let mut registry = RbacRegistry::new();
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        let result = registry.assign_role("user1", "nonexistent_role");
        assert!(matches!(result, Err(RbacError::RoleNotFound(_))));
    }

    #[test]
    fn test_rbac_removed_user_not_accessible() {
        let mut registry = RbacRegistry::new();
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        registry.remove_user("user1");

        let result = registry.get_user("user1");
        assert!(result.is_none());
    }

    #[test]
    fn test_rbac_duplicate_role_assignment() {
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());
        let user = User::new("user1".to_string(), "alice".to_string());
        registry.add_user(user);

        registry.assign_role("user1", "admin").unwrap();
        registry.assign_role("user1", "admin").unwrap();

        let user = registry.get_user("user1").unwrap();
        let admin_count = user.roles.iter().filter(|r| *r == "admin").count();
        assert_eq!(admin_count, 1);
    }

    // B. Audit Trail Security Tests

    #[test]
    fn test_audit_record_returns_incrementing_ids() {
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
        let id3 = trail.record(
            "user3",
            "10.0.0.3",
            AuditEventKind::ConfigChange,
            "/config",
            "change",
            true,
        );

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[test]
    fn test_audit_filter_by_user() {
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

        let filter = AuditFilter {
            user: Some("alice".to_string()),
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.user == "alice"));
    }

    #[test]
    fn test_audit_filter_by_kind() {
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
    fn test_audit_empty_filter_returns_all() {
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

        let filter = AuditFilter::new();
        let results = trail.query(&filter);

        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_audit_success_only_filter() {
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
            AuditEventKind::Logout,
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
            success_only: true,
            ..Default::default()
        };
        let results = trail.query(&filter);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.success));
    }

    // C. Compliance Security Tests

    #[test]
    fn test_compliance_worm_record_status_active() {
        let registry = ComplianceRegistry::new();
        let now: u64 = 1000 * 86400000;

        let policy = RetentionPolicy {
            policy_id: "worm_policy".to_string(),
            name: "WORM Policy".to_string(),
            retention_days: 30,
            worm_enabled: true,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry
            .register_file("/data/file.txt", "worm_policy", now)
            .unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.status(now), RetentionStatus::Locked);
    }

    #[test]
    fn test_compliance_expired_record_status() {
        let registry = ComplianceRegistry::new();
        let now: u64 = 1000 * 86400000;

        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        let expired_time = now + 31 * 86400000;
        assert_eq!(record.status(expired_time), RetentionStatus::Expired);
    }

    #[test]
    fn test_compliance_duplicate_policy_rejected() {
        let registry = ComplianceRegistry::new();

        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();

        let result = registry.add_policy(policy);
        assert!(matches!(
            result,
            Err(ComplianceError::PolicyAlreadyExists(_))
        ));
    }

    #[test]
    fn test_compliance_register_file_unknown_policy() {
        let registry = ComplianceRegistry::new();
        let now: u64 = 1000 * 86400000;

        let result = registry.register_file("/data/file.txt", "nonexistent", now);
        assert!(matches!(result, Err(ComplianceError::PolicyNotFound(_))));
    }

    #[test]
    fn test_compliance_days_remaining_calculation() {
        let registry = ComplianceRegistry::new();
        let now: u64 = 1000 * 86400000;

        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        let days = record.days_remaining(now);
        assert_eq!(days, 30);

        let half_way = now + 15 * 86400000;
        let days_half = record.days_remaining(half_way);
        assert_eq!(days_half, 15);
    }

    // D. Live Config Security Tests

    #[test]
    fn test_live_config_set_and_get_roundtrip() {
        let store = LiveConfigStore::new();

        store
            .set("test_key", "test_value", "test description")
            .unwrap();
        let entry = store.get("test_key").unwrap();

        assert_eq!(entry.key, "test_key");
        assert_eq!(entry.value, "test_value");
        assert_eq!(entry.description, "test description");
    }

    #[test]
    fn test_live_config_get_nonexistent_key_error() {
        let store = LiveConfigStore::new();

        let result = store.get("nonexistent");
        assert!(matches!(result, Err(LiveConfigError::NotFound(_))));
    }

    #[test]
    fn test_live_config_remove_key() {
        let store = LiveConfigStore::new();

        store.set("removeme", "value", "desc").unwrap();
        store.remove("removeme").unwrap();

        let result = store.get("removeme");
        assert!(matches!(result, Err(LiveConfigError::NotFound(_))));
    }

    #[test]
    fn test_live_config_version_increments() {
        let store = LiveConfigStore::new();

        let v0 = store.version();
        store.set("key1", "value1", "desc1").unwrap();
        let v1 = store.version();
        store.set("key2", "value2", "desc2").unwrap();
        let v2 = store.version();

        assert!(v1 > v0);
        assert!(v2 > v1);
    }

    #[test]
    fn test_live_config_reload_updates_existing() {
        let store = LiveConfigStore::new();

        store.set("existing", "old_value", "old desc").unwrap();

        let mut new_entries = HashMap::new();
        new_entries.insert(
            "existing".to_string(),
            ("new_value".to_string(), "new desc".to_string()),
        );
        new_entries.insert(
            "new_key".to_string(),
            ("new_value".to_string(), "new desc".to_string()),
        );

        let status = store.reload(new_entries);

        assert!(matches!(status, ReloadStatus::Success { .. }));
        let entry = store.get("existing").unwrap();
        assert_eq!(entry.value, "new_value");
    }

    // E. Rate Limiter Security Tests

    #[test]
    fn test_rate_limiter_locks_after_threshold() {
        let limiter = AuthRateLimiter::new();

        for _ in 0..4 {
            assert!(!limiter.record_failure("192.168.1.1"));
        }

        let is_locked = limiter.record_failure("192.168.1.1");
        assert!(is_locked);
        assert!(limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn test_rate_limiter_different_ips_independent() {
        let limiter = AuthRateLimiter::new();

        for _ in 0..5 {
            limiter.record_failure("192.168.1.1");
        }

        assert!(limiter.is_rate_limited("192.168.1.1"));
        assert!(!limiter.is_rate_limited("192.168.1.2"));
        assert!(!limiter.is_rate_limited("10.0.0.1"));
    }

    #[test]
    fn test_rate_limiter_constant_time_eq_works() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "xyz"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }
}
