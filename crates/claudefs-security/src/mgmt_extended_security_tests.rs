//! Extended security tests for claudefs-mgmt: alerting, bootstrap, config sync, cost, health.
//!
//! Part of A10 Phase 10: Management extended security audit

#[cfg(test)]
mod tests {
    use claudefs_mgmt::alerting::{AlertRule, AlertSeverity, AlertState, Comparison};
    use claudefs_mgmt::cluster_bootstrap::{
        BootstrapConfig, BootstrapError, BootstrapManager, BootstrapState, NodeSpec,
    };
    use claudefs_mgmt::config_sync::{ConfigEntry, ConfigStore, ConfigVersion};
    use claudefs_mgmt::cost_tracker::{
        BudgetStatus, CostBudget, CostCategory, CostEntry, CostTracker,
    };
    use claudefs_mgmt::diagnostics::{
        CheckBuilder, DiagnosticCheck, DiagnosticLevel, DiagnosticReport, DiagnosticsRunner,
    };
    use claudefs_mgmt::health::{HealthStatus, NodeHealth};
    use claudefs_mgmt::node_scaling::{
        ClusterNode, NodeRole, NodeSpec as ScalingNodeSpec, NodeState,
    };

    fn make_alert_rule(threshold: f64, comparison: Comparison) -> AlertRule {
        AlertRule {
            name: "test_rule".to_string(),
            description: "Test alert rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold,
            comparison,
            for_secs: 60,
        }
    }

    fn make_bootstrap_config(
        cluster_name: &str,
        nodes: Vec<NodeSpec>,
        erasure_k: u8,
        erasure_m: u8,
    ) -> BootstrapConfig {
        BootstrapConfig {
            cluster_name: cluster_name.to_string(),
            site_id: "site1".to_string(),
            nodes,
            erasure_k,
            erasure_m,
        }
    }

    fn ts(days_ago: u64) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - (days_ago * 86400)
    }

    mod category1_alerting_diagnostics {
        use super::*;

        #[test]
        fn test_alert_rule_evaluate_boundary() {
            let rule = make_alert_rule(100.0, Comparison::GreaterThan);
            assert!(
                !rule.evaluate(100.0),
                "FINDING-MGMT-EXT-01: 100.0 is NOT greater than 100.0 (boundary)"
            );
            assert!(rule.evaluate(100.1), "100.1 IS greater than 100.0");
        }

        #[test]
        fn test_alert_rule_nan_handling() {
            let rule = make_alert_rule(100.0, Comparison::GreaterThan);
            let result = rule.evaluate(f64::NAN);
            assert!(
                !result,
                "FINDING-MGMT-EXT-02: NaN comparison always returns false per IEEE 754"
            );
        }

        #[test]
        fn test_alert_severity_ordering() {
            fn severity_order(s: &AlertSeverity) -> u8 {
                match s {
                    AlertSeverity::Info => 0,
                    AlertSeverity::Warning => 1,
                    AlertSeverity::Critical => 2,
                }
            }
            assert!(
                severity_order(&AlertSeverity::Info) < severity_order(&AlertSeverity::Warning),
                "FINDING-MGMT-EXT-03: Info < Warning"
            );
            assert!(
                severity_order(&AlertSeverity::Warning) < severity_order(&AlertSeverity::Critical),
                "Warning < Critical"
            );
            assert!(
                severity_order(&AlertSeverity::Info) < severity_order(&AlertSeverity::Critical),
                "Info < Critical"
            );
        }

        #[test]
        fn test_diagnostic_report_is_healthy() {
            let report = DiagnosticReport {
                checks: vec![
                    DiagnosticCheck {
                        name: "check1".to_string(),
                        level: DiagnosticLevel::Info,
                        passed: true,
                        message: "OK".to_string(),
                        duration_ms: 10,
                    },
                    DiagnosticCheck {
                        name: "check2".to_string(),
                        level: DiagnosticLevel::Warning,
                        passed: false,
                        message: "Warning".to_string(),
                        duration_ms: 10,
                    },
                ],
                total_duration_ms: 20,
                generated_at_ms: 1000,
            };
            assert!(
                report.is_healthy(),
                "Report with passing checks should be healthy"
            );

            let report_critical = DiagnosticReport {
                checks: vec![DiagnosticCheck {
                    name: "check1".to_string(),
                    level: DiagnosticLevel::Critical,
                    passed: false,
                    message: "FAIL".to_string(),
                    duration_ms: 10,
                }],
                total_duration_ms: 10,
                generated_at_ms: 1000,
            };
            assert!(
                !report_critical.is_healthy(),
                "FINDING-MGMT-EXT-04: Critical failure makes report unhealthy"
            );
        }

        #[test]
        fn test_diagnostic_check_builder() {
            let pass_check = CheckBuilder::new("test_pass").pass("All good", 50);
            assert_eq!(pass_check.name, "test_pass");
            assert!(pass_check.passed);
            assert_eq!(pass_check.message, "All good");

            let fail_check = CheckBuilder::new("test_fail")
                .level(DiagnosticLevel::Error)
                .fail("Something failed", 100);
            assert_eq!(fail_check.name, "test_fail");
            assert!(!fail_check.passed);
            assert_eq!(fail_check.message, "Something failed");
            assert_eq!(fail_check.level, DiagnosticLevel::Error);
        }
    }

    mod category2_cluster_bootstrap {
        use super::*;

        #[test]
        fn test_bootstrap_empty_cluster_name() {
            let config = make_bootstrap_config(
                "",
                vec![NodeSpec {
                    node_id: "n1".to_string(),
                    address: "192.168.1.1".to_string(),
                    role: "storage".to_string(),
                    capacity_gb: 1000,
                }],
                4,
                2,
            );
            let manager = BootstrapManager::new(config);
            let result = manager.start();
            assert!(
                result.is_err(),
                "FINDING-MGMT-EXT-05: Empty cluster_name should fail validation"
            );
            if let Err(BootstrapError::InvalidConfig(msg)) = result {
                assert!(
                    msg.contains("cluster_name"),
                    "Error should mention cluster_name"
                );
            }
        }

        #[test]
        fn test_bootstrap_invalid_erasure_params() {
            let config = make_bootstrap_config(
                "test",
                vec![NodeSpec {
                    node_id: "n1".to_string(),
                    address: "192.168.1.1".to_string(),
                    role: "storage".to_string(),
                    capacity_gb: 1000,
                }],
                1,
                0,
            );
            let manager = BootstrapManager::new(config);
            let result = manager.start();
            assert!(
                result.is_err(),
                "FINDING-MGMT-EXT-06: erasure_k=1 should fail (k must be >= 2)"
            );
            if let Err(BootstrapError::InvalidConfig(msg)) = result {
                assert!(
                    msg.contains("erasure_k") || msg.contains("2"),
                    "Error should mention erasure_k"
                );
            }

            let config2 = make_bootstrap_config(
                "test",
                vec![NodeSpec {
                    node_id: "n1".to_string(),
                    address: "192.168.1.1".to_string(),
                    role: "storage".to_string(),
                    capacity_gb: 1000,
                }],
                4,
                0,
            );
            let manager2 = BootstrapManager::new(config2);
            let result2 = manager2.start();
            assert!(
                result2.is_err(),
                "FINDING-MGMT-EXT-07: erasure_m=0 should fail (m must be >= 1)"
            );
        }

        #[test]
        fn test_bootstrap_state_transitions() {
            let config = make_bootstrap_config(
                "test",
                vec![NodeSpec {
                    node_id: "n1".to_string(),
                    address: "192.168.1.1".to_string(),
                    role: "storage".to_string(),
                    capacity_gb: 1000,
                }],
                4,
                2,
            );
            let manager = BootstrapManager::new(config);
            assert_eq!(
                manager.state(),
                BootstrapState::Uninitialized,
                "Initial state is Uninitialized"
            );
            manager.start().unwrap();
            assert_eq!(
                manager.state(),
                BootstrapState::InProgress,
                "FINDING-MGMT-EXT-08: After start(), state is InProgress"
            );
            manager.complete().unwrap();
            assert_eq!(
                manager.state(),
                BootstrapState::Complete,
                "After complete(), state is Complete"
            );
        }

        #[test]
        fn test_bootstrap_empty_nodes() {
            let config = make_bootstrap_config("test", vec![], 4, 2);
            let manager = BootstrapManager::new(config);
            let result = manager.start();
            assert!(
                result.is_err(),
                "FINDING-MGMT-EXT-09: Empty nodes list should fail"
            );
            if let Err(BootstrapError::InvalidConfig(msg)) = result {
                assert!(msg.contains("nodes"), "Error should mention nodes");
            }
        }

        #[test]
        fn test_bootstrap_duplicate_node_registration() {
            let config = make_bootstrap_config(
                "test",
                vec![NodeSpec {
                    node_id: "n1".to_string(),
                    address: "192.168.1.1".to_string(),
                    role: "storage".to_string(),
                    capacity_gb: 1000,
                }],
                4,
                2,
            );
            let manager = BootstrapManager::new(config);
            manager.start().unwrap();
            manager.register_node("n1").unwrap();
            let result = manager.register_node("n1");
            assert!(
                result.is_err(),
                "FINDING-MGMT-EXT-10: Duplicate node_id registration is rejected"
            );
            if let Err(BootstrapError::NodeAlreadyRegistered(id)) = result {
                assert_eq!(id, "n1");
            }
        }
    }

    mod category3_config_sync {
        use super::*;

        #[test]
        fn test_config_store_put_get_roundtrip() {
            let mut store = ConfigStore::new();
            let version = store.put("test", "hello", "admin");
            assert!(version.version > 0, "Version should be > 0 after put");
            let entry = store.get("test");
            assert!(entry.is_some(), "Entry should exist after put");
            assert_eq!(entry.unwrap().value, "hello");
        }

        #[test]
        fn test_config_store_version_increments() {
            let mut store = ConfigStore::new();
            store.put("key1", "value1", "author1");
            let v1 = store.current_version();
            store.put("key2", "value2", "author2");
            let v2 = store.current_version();
            store.put("key3", "value3", "author3");
            let v3 = store.current_version();
            assert!(
                v1 < v2,
                "FINDING-MGMT-EXT-11: Version should increment monotonically"
            );
            assert!(v2 < v3);
            assert_eq!(v3, 3);
        }

        #[test]
        fn test_config_store_delete() {
            let mut store = ConfigStore::new();
            store.put("test", "hello", "admin");
            assert!(
                store.get("test").is_some(),
                "Entry should exist before delete"
            );

            let delete_result = store.delete("test");
            assert!(delete_result, "Delete of existing key should return true");
            assert!(
                store.get("test").is_none(),
                "Entry should not exist after delete"
            );

            let delete_nonexistent = store.delete("nonexistent");
            assert!(
                !delete_nonexistent,
                "FINDING-MGMT-EXT-12: Delete of non-existent key returns false"
            );
        }

        #[test]
        fn test_config_store_entries_since() {
            let mut store = ConfigStore::new();
            store.put("a", "1", "author");
            store.put("b", "2", "author");
            store.put("c", "3", "author");
            store.put("d", "4", "author");
            store.put("e", "5", "author");

            let entries = store.entries_since(3);
            assert_eq!(
                entries.len(),
                2,
                "FINDING-MGMT-EXT-13: Should return entries with version > 3"
            );
            assert!(entries.iter().all(|e| e.version.version > 3));
        }

        #[test]
        fn test_config_store_empty_key() {
            let mut store = ConfigStore::new();
            store.put("", "empty_key_value", "admin");
            let entry = store.get("");
            assert!(
                entry.is_some(),
                "FINDING-MGMT-EXT-14: Empty key appears to be accepted"
            );
            assert_eq!(entry.unwrap().value, "empty_key_value");
        }
    }

    mod category4_cost_tracking {
        use super::*;

        #[test]
        fn test_cost_tracker_total() {
            let budget = CostBudget {
                daily_limit_usd: 100.0,
                monthly_limit_usd: 3000.0,
            };
            let tracker = CostTracker::new(budget);
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 10.0,
                resource_id: "vm-1".to_string(),
                timestamp: ts(0),
            });
            tracker.record(CostEntry {
                category: CostCategory::Storage,
                amount_usd: 20.5,
                resource_id: "disk-1".to_string(),
                timestamp: ts(0),
            });
            tracker.record(CostEntry {
                category: CostCategory::Network,
                amount_usd: 30.0,
                resource_id: "nic-1".to_string(),
                timestamp: ts(0),
            });
            assert!(
                (tracker.total_cost() - 60.5).abs() < 0.001,
                "Total should be 60.5"
            );
        }

        #[test]
        fn test_cost_tracker_budget_exceeded() {
            let budget = CostBudget {
                daily_limit_usd: 50.0,
                monthly_limit_usd: 1500.0,
            };
            let tracker = CostTracker::new(budget);
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 25.0,
                resource_id: "vm-1".to_string(),
                timestamp: ts(0),
            });
            tracker.record(CostEntry {
                category: CostCategory::Storage,
                amount_usd: 20.0,
                resource_id: "disk-1".to_string(),
                timestamp: ts(0),
            });
            tracker.record(CostEntry {
                category: CostCategory::Network,
                amount_usd: 15.0,
                resource_id: "nic-1".to_string(),
                timestamp: ts(0),
            });
            let status = tracker.budget_status(ts(0));
            assert_eq!(
                status,
                BudgetStatus::Exceeded,
                "FINDING-MGMT-EXT-15: 60.0 > 50.0 limit = Exceeded"
            );
        }

        #[test]
        fn test_cost_tracker_negative_cost() {
            let budget = CostBudget {
                daily_limit_usd: 100.0,
                monthly_limit_usd: 3000.0,
            };
            let tracker = CostTracker::new(budget);
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 10.0,
                resource_id: "vm-1".to_string(),
                timestamp: ts(0),
            });
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: -5.0,
                resource_id: "vm-1".to_string(),
                timestamp: ts(0),
            });
            let total = tracker.total_cost();
            assert!(
                (total - 5.0).abs() < 0.001,
                "FINDING-MGMT-EXT-16: Negative costs reduce apparent spend"
            );
        }

        #[test]
        fn test_cost_tracker_daily_total() {
            let budget = CostBudget {
                daily_limit_usd: 100.0,
                monthly_limit_usd: 3000.0,
            };
            let tracker = CostTracker::new(budget);
            let today = ts(0);
            let yesterday = ts(1);

            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 50.0,
                resource_id: "vm-1".to_string(),
                timestamp: today,
            });
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 30.0,
                resource_id: "vm-2".to_string(),
                timestamp: yesterday,
            });

            let daily = tracker.daily_total(today);
            assert!(
                (daily - 50.0).abs() < 0.001,
                "FINDING-MGMT-EXT-17: Only today's entries counted"
            );
        }

        #[test]
        fn test_cost_budget_status_thresholds() {
            let budget = CostBudget {
                daily_limit_usd: 100.0,
                monthly_limit_usd: 3000.0,
            };
            let tracker = CostTracker::new(budget);
            let now = ts(0);

            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 70.0,
                resource_id: "vm-1".to_string(),
                timestamp: now,
            });
            assert_eq!(
                tracker.budget_status(now),
                BudgetStatus::Ok,
                "70% < 75% = Ok"
            );

            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 15.0,
                resource_id: "vm-2".to_string(),
                timestamp: now,
            });
            assert_eq!(
                tracker.budget_status(now),
                BudgetStatus::Warning,
                "85% >= 75% = Warning"
            );

            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 11.0,
                resource_id: "vm-3".to_string(),
                timestamp: now,
            });
            assert_eq!(
                tracker.budget_status(now),
                BudgetStatus::Critical,
                "96% >= 90% = Critical"
            );

            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 5.0,
                resource_id: "vm-4".to_string(),
                timestamp: now,
            });
            assert_eq!(
                tracker.budget_status(now),
                BudgetStatus::Exceeded,
                "FINDING-MGMT-EXT-18: 101% >= 100% = Exceeded"
            );
        }
    }

    mod category5_health_scaling {
        use super::*;

        #[test]
        fn test_node_health_capacity_percent() {
            let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
            node.capacity_total = 1000;
            node.capacity_used = 800;
            let pct = node.capacity_percent();
            assert!((pct - 80.0).abs() < 0.001, "Capacity should be 80%");

            node.capacity_total = 0;
            let pct_zero = node.capacity_percent();
            assert!(
                (pct_zero - 0.0).abs() < 0.001,
                "FINDING-MGMT-EXT-19: capacity_total=0 returns 0.0 (no division by zero)"
            );
        }

        #[test]
        fn test_node_health_capacity_thresholds() {
            let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
            node.capacity_total = 1000;
            node.capacity_used = 790;
            assert!(
                !node.is_capacity_warning(),
                "79% should not trigger warning"
            );
            assert!(
                !node.is_capacity_critical(),
                "79% should not trigger critical"
            );

            node.capacity_used = 810;
            assert!(
                node.is_capacity_warning(),
                "FINDING-MGMT-EXT-20: 81% triggers warning (>80%)"
            );
            assert!(
                !node.is_capacity_critical(),
                "81% should not trigger critical"
            );

            node.capacity_used = 960;
            assert!(node.is_capacity_warning(), "96% triggers warning");
            assert!(
                node.is_capacity_critical(),
                "FINDING-MGMT-EXT-21: 96% triggers critical (>95%)"
            );
        }

        #[test]
        fn test_node_scaling_state_transitions() {
            assert!(
                NodeState::Joining.can_transition_to(&NodeState::Active),
                "Joining -> Active is valid"
            );
            assert!(
                NodeState::Active.can_transition_to(&NodeState::Draining),
                "Active -> Draining is valid"
            );
            assert!(
                NodeState::Draining.can_transition_to(&NodeState::Drained),
                "Draining -> Drained is valid"
            );
            assert!(
                NodeState::Drained.can_transition_to(&NodeState::Decommissioned),
                "Drained -> Decommissioned is valid"
            );

            assert!(
                !NodeState::Drained.can_transition_to(&NodeState::Active),
                "FINDING-MGMT-EXT-22: Drained -> Active is invalid"
            );
            assert!(
                !NodeState::Decommissioned.can_transition_to(&NodeState::Active),
                "FINDING-MGMT-EXT-23: Decommissioned -> anything is invalid"
            );
            assert!(
                !NodeState::Decommissioned.can_transition_to(&NodeState::Joining),
                "Decommissioned -> Joining is invalid"
            );
        }

        #[test]
        fn test_node_role_predicates() {
            assert!(
                NodeRole::Storage.is_storage(),
                "Storage.is_storage() = true"
            );
            assert!(
                !NodeRole::Metadata.is_storage(),
                "Metadata.is_storage() = false"
            );
            assert!(
                NodeRole::StorageAndMetadata.is_storage(),
                "StorageAndMetadata.is_storage() = true"
            );

            assert!(
                NodeRole::Metadata.is_metadata(),
                "Metadata.is_metadata() = true"
            );
            assert!(
                !NodeRole::Storage.is_metadata(),
                "Storage.is_metadata() = false"
            );
            assert!(
                NodeRole::StorageAndMetadata.is_metadata(),
                "FINDING-MGMT-EXT-24: StorageAndMetadata is both storage and metadata"
            );
        }

        #[test]
        fn test_node_health_stale_detection() {
            let mut node = NodeHealth::new("node1".to_string(), "192.168.1.1".to_string());
            let t = 1000u64;
            node.last_seen = t;

            assert!(
                node.is_stale(t + 100, 60),
                "FINDING-MGMT-EXT-25: 100s > 60s threshold = stale"
            );
            assert!(
                !node.is_stale(t + 50, 60),
                "50s <= 60s threshold = not stale"
            );
            assert!(
                !node.is_stale(t + 60, 60),
                "60s <= 60s threshold = not stale"
            );
            assert!(node.is_stale(t + 61, 60), "61s > 60s threshold = stale");
        }
    }
}
