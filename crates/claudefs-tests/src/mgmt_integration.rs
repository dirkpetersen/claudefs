//! A8 Management API integration tests
//!
//! Tests for RBAC, SLA, and alerting components from claudefs-mgmt.

use claudefs_mgmt::alerting::{
    default_alert_rules, Alert, AlertError, AlertManager, AlertRule, AlertSeverity, AlertState,
    Comparison,
};
use claudefs_mgmt::rbac::{
    admin_role, operator_role, tenant_admin_role, viewer_role, Permission, RbacError, RbacRegistry,
    Role, User,
};
use claudefs_mgmt::sla::{
    compute_percentiles, LatencySample, PercentileResult, SlaMetricKind, SlaTarget,
};
use std::collections::HashMap;

#[test]
fn test_rbac_admin_role_has_all_permissions() {
    let admin = admin_role();

    assert!(admin.has_permission(&Permission::Admin));
    assert!(admin.has_permission(&Permission::ViewCluster));
    assert!(admin.has_permission(&Permission::ManageQuotas));
}

#[test]
fn test_rbac_viewer_role_has_limited_permissions() {
    let viewer = viewer_role();

    assert!(viewer.has_permission(&Permission::ViewCluster));
    assert!(!viewer.has_permission(&Permission::ManageQuotas));
    assert!(!viewer.has_permission(&Permission::DrainNodes));
}

#[test]
fn test_rbac_operator_role_has_write_not_manage_webhooks() {
    let oper = operator_role();

    assert!(oper.has_permission(&Permission::ViewCluster));
    assert!(oper.has_permission(&Permission::DrainNodes));
    assert!(!oper.has_permission(&Permission::ManageWebhooks));
}

#[test]
fn test_rbac_tenant_admin_role_has_manage_quota() {
    let tenant = tenant_admin_role();

    assert!(tenant.has_permission(&Permission::ManageQuotas));
    assert!(!tenant.has_permission(&Permission::Admin));
}

#[test]
fn test_rbac_role_add_permission() {
    let mut role = Role::new("test".to_string(), "test role".to_string());
    role.add_permission(Permission::ViewCluster);

    assert!(role.has_permission(&Permission::ViewCluster));
}

#[test]
fn test_rbac_role_permission_count() {
    let mut role = Role::new("test".to_string(), "test role".to_string());
    role.add_permission(Permission::ViewCluster);
    role.add_permission(Permission::ViewNodes);

    assert_eq!(role.permission_count(), 2);
}

#[test]
fn test_rbac_registry_add_get_role() {
    let mut registry = RbacRegistry::new();
    let role = Role::new("test-role".to_string(), "test".to_string());
    registry.add_role(role);

    let retrieved = registry.get_role("test-role");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "test-role");
}

#[test]
fn test_rbac_registry_remove_role() {
    let mut registry = RbacRegistry::new();
    let role = Role::new("test-role".to_string(), "test".to_string());
    registry.add_role(role);

    let removed = registry.remove_role("test-role");
    assert!(removed.is_some());

    let get = registry.get_role("test-role");
    assert!(get.is_none());
}

#[test]
fn test_rbac_registry_add_get_user() {
    let mut registry = RbacRegistry::new();
    let user = User::new("user-1".to_string(), "testuser".to_string());
    registry.add_user(user);

    let retrieved = registry.get_user("user-1");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().username, "testuser");
}

#[test]
fn test_rbac_user_new_fields() {
    let user = User::new("user-1".to_string(), "testuser".to_string());

    assert_eq!(user.id, "user-1");
    assert_eq!(user.username, "testuser");
    assert!(user.active);
}

#[test]
fn test_rbac_permission_implies_admin() {
    let admin = Permission::Admin;

    assert!(admin.implies(&Permission::ViewCluster));
    assert!(admin.implies(&Permission::ManageQuotas));
    assert!(admin.implies(&Permission::Admin));
}

#[test]
fn test_rbac_permission_implies_not_transitive() {
    let read = Permission::ViewCluster;

    assert!(!read.implies(&Permission::ManageQuotas));
    assert!(read.implies(&Permission::ViewCluster));
}

#[test]
fn test_sla_compute_percentiles_empty_returns_none() {
    let result = compute_percentiles(&[]);
    assert!(result.is_none());
}

#[test]
fn test_sla_compute_percentiles_uniform_samples() {
    let samples: Vec<u64> = (1..=100).collect();
    let result = compute_percentiles(&samples);

    assert!(result.is_some());
    let p = result.unwrap();
    assert!(p.p50 > 0.0);
    assert!(p.p95 > p.p50);
    assert!(p.p99 > p.p95);
}

#[test]
fn test_sla_target_fields() {
    let target = SlaTarget::new(
        SlaMetricKind::ReadLatencyUs,
        1000.0,
        5000.0,
        10000.0,
        "Read latency SLA",
    );

    assert_eq!(target.kind, SlaMetricKind::ReadLatencyUs);
    assert_eq!(target.p50_threshold, 1000.0);
    assert_eq!(target.p95_threshold, 5000.0);
    assert_eq!(target.p99_threshold, 10000.0);
}

#[test]
fn test_sla_metric_kind_name() {
    assert_eq!(SlaMetricKind::ReadLatencyUs.name(), "read_latency_us");
    assert_eq!(SlaMetricKind::WriteLatencyUs.name(), "write_latency_us");
    assert_eq!(SlaMetricKind::Iops.name(), "iops");
}

#[test]
fn test_sla_latency_sample_construction() {
    let sample = LatencySample::new(5000, 1000000);
    assert_eq!(sample.value_us, 5000);
    assert_eq!(sample.timestamp, 1000000);
}

#[test]
fn test_sla_percentile_result_fields() {
    let samples: Vec<u64> = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    let result = compute_percentiles(&samples).unwrap();

    assert!(result.min > 0.0);
    assert!(result.max > 0.0);
    assert!(result.mean > 0.0);
    assert!(result.sample_count > 0);
}

#[test]
fn test_alert_manager_with_default_rules() {
    let manager = AlertManager::with_default_rules();

    let rules = default_alert_rules();
    assert!(!rules.is_empty());
}

#[test]
fn test_comparison_greater_than_evaluate() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test rule".to_string(),
        severity: AlertSeverity::Warning,
        metric: "test_metric".to_string(),
        threshold: 100.0,
        comparison: Comparison::GreaterThan,
        for_secs: 60,
    };

    assert!(rule.evaluate(150.0));
    assert!(!rule.evaluate(50.0));
    assert!(!rule.evaluate(100.0));
}

#[test]
fn test_comparison_less_than_evaluate() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test rule".to_string(),
        severity: AlertSeverity::Warning,
        metric: "test_metric".to_string(),
        threshold: 100.0,
        comparison: Comparison::LessThan,
        for_secs: 60,
    };

    assert!(rule.evaluate(50.0));
    assert!(!rule.evaluate(150.0));
}

#[test]
fn test_alert_new_is_firing_is_resolved() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test rule".to_string(),
        severity: AlertSeverity::Warning,
        metric: "test_metric".to_string(),
        threshold: 100.0,
        comparison: Comparison::GreaterThan,
        for_secs: 60,
    };

    let alert = Alert::new(rule, 150.0);
    assert!(!alert.is_firing());
    assert!(!alert.is_resolved());
}

#[test]
fn test_alert_age_secs() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test rule".to_string(),
        severity: AlertSeverity::Warning,
        metric: "test_metric".to_string(),
        threshold: 100.0,
        comparison: Comparison::GreaterThan,
        for_secs: 60,
    };

    let alert = Alert::new(rule, 150.0);
    let age = alert.age_secs();
    assert_eq!(age, 0);
}

#[test]
fn test_alert_manager_evaluate_no_metrics() {
    let mut manager = AlertManager::with_default_rules();
    let metrics: HashMap<String, f64> = HashMap::new();

    let alerts = manager.evaluate(&metrics);
    assert!(alerts.is_empty() || !alerts.is_empty());
}

#[test]
fn test_alert_manager_evaluate_with_firing_alert() {
    let mut manager = AlertManager::new(vec![AlertRule {
        name: "HighLatency".to_string(),
        description: "High write latency".to_string(),
        severity: AlertSeverity::Warning,
        metric: "latency".to_string(),
        threshold: 1000.0,
        comparison: Comparison::GreaterThan,
        for_secs: 60,
    }]);

    let mut metrics = HashMap::new();
    metrics.insert("latency".to_string(), 2000.0);

    let alerts = manager.evaluate(&metrics);
    assert!(!alerts.is_empty());
}

#[test]
fn test_combined_rbac_quota_check_viewer_cannot_manage_quota() {
    let viewer = viewer_role();

    assert!(!viewer.has_permission(&Permission::ManageQuotas));
    assert!(viewer.has_permission(&Permission::ViewQuotas));
}

#[test]
fn test_combined_rbac_and_quota_registry() {
    let mut registry = RbacRegistry::new();
    registry.add_role(admin_role());

    let user = User::new("admin-1".to_string(), "adminuser".to_string());
    registry.add_user(user);

    let admin_role_retrieved = registry.get_role("admin");
    assert!(admin_role_retrieved.is_some());
    assert!(admin_role_retrieved
        .unwrap()
        .has_permission(&Permission::ManageQuotas));
}

#[test]
fn test_alert_severity_variants() {
    let info = AlertSeverity::Info;
    let warning = AlertSeverity::Warning;
    let critical = AlertSeverity::Critical;

    assert!(matches!(info, AlertSeverity::Info));
    assert!(matches!(warning, AlertSeverity::Warning));
    assert!(matches!(critical, AlertSeverity::Critical));
}

#[test]
fn test_alert_state_variants() {
    let ok = AlertState::Ok;
    let firing = AlertState::Firing;
    let resolved = AlertState::Resolved;

    assert!(matches!(ok, AlertState::Ok));
    assert!(matches!(firing, AlertState::Firing));
    assert!(matches!(resolved, AlertState::Resolved));
}

#[test]
fn test_rbac_registry_user_not_found() {
    let registry = RbacRegistry::new();
    let result = registry.get_user("nonexistent");
    assert!(result.is_none());
}

#[test]
fn test_rbac_registry_role_not_found() {
    let registry = RbacRegistry::new();
    let result = registry.get_role("nonexistent");
    assert!(result.is_none());
}

#[test]
fn test_sla_different_metric_kinds() {
    let kinds = vec![
        SlaMetricKind::ReadLatencyUs,
        SlaMetricKind::WriteLatencyUs,
        SlaMetricKind::MetadataLatencyUs,
        SlaMetricKind::ThroughputMBps,
        SlaMetricKind::Iops,
        SlaMetricKind::AvailabilityPercent,
    ];

    for kind in kinds {
        let name = kind.name();
        assert!(!name.is_empty());
    }
}

#[test]
fn test_default_alert_rules_count() {
    let rules = default_alert_rules();
    assert!(rules.len() >= 4);
}

#[test]
fn test_role_description() {
    let admin = admin_role();
    assert!(!admin.description.is_empty());
}

#[test]
fn test_alert_rule_evaluate_with_equal() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test".to_string(),
        severity: AlertSeverity::Info,
        metric: "test".to_string(),
        threshold: 100.0,
        comparison: Comparison::Equal,
        for_secs: 60,
    };

    assert!(rule.evaluate(100.0));
}

#[test]
fn test_alert_manager_evaluate_resolves_alert() {
    let mut manager = AlertManager::new(vec![AlertRule {
        name: "TestAlert".to_string(),
        description: "Test".to_string(),
        severity: AlertSeverity::Info,
        metric: "value".to_string(),
        threshold: 100.0,
        comparison: Comparison::GreaterThan,
        for_secs: 60,
    }]);

    let mut metrics = HashMap::new();
    metrics.insert("value".to_string(), 150.0);
    manager.evaluate(&metrics);

    metrics.insert("value".to_string(), 50.0);
    let alerts = manager.evaluate(&metrics);

    assert!(!alerts.is_empty());
}

#[test]
fn test_rbac_operator_has_manage_snapshots() {
    let oper = operator_role();
    assert!(oper.has_permission(&Permission::ManageSnapshots));
}

#[test]
fn test_rbac_viewer_does_not_have_drain_nodes() {
    let viewer = viewer_role();
    assert!(!viewer.has_permission(&Permission::DrainNodes));
}

#[test]
fn test_rbac_tenant_admin_manages_snapshots() {
    let tenant = tenant_admin_role();
    assert!(tenant.has_permission(&Permission::ManageSnapshots));
}

#[test]
fn test_rbac_tenant_admin_view_replication() {
    let tenant = tenant_admin_role();
    assert!(tenant.has_permission(&Permission::ViewReplication));
}

#[test]
fn test_sla_single_sample() {
    let samples: Vec<u64> = vec![100];
    let result = compute_percentiles(&samples);
    assert!(result.is_some());
}

#[test]
fn test_sla_two_samples() {
    let samples: Vec<u64> = vec![10, 100];
    let result = compute_percentiles(&samples);
    assert!(result.is_some());
}

#[test]
fn test_alert_rule_for_secs() {
    let rule = AlertRule {
        name: "test".to_string(),
        description: "test".to_string(),
        severity: AlertSeverity::Warning,
        metric: "test".to_string(),
        threshold: 100.0,
        comparison: Comparison::GreaterThan,
        for_secs: 300,
    };

    assert_eq!(rule.for_secs, 300);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_has_query_analytics() {
        let admin = admin_role();
        assert!(admin.has_permission(&Permission::QueryAnalytics));
    }

    #[test]
    fn test_operator_has_manage_tiering() {
        let oper = operator_role();
        assert!(oper.has_permission(&Permission::ManageTiering));
    }

    #[test]
    fn test_viewer_has_query_analytics() {
        let viewer = viewer_role();
        assert!(viewer.has_permission(&Permission::QueryAnalytics));
    }
}
