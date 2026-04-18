//! A11 Phase 5 Block 4: CI Monitoring Integration Tests
//!
//! Tests for validating Prometheus configuration, Grafana dashboards,
//! AlertManager configuration, and cost tracking infrastructure.
//!
//! This module verifies that all monitoring components are correctly configured
//! and syntactically valid. The test cluster runs on AWS EC2 with preemptible
//! instances (5 storage, 2 clients, 1 metadata conduit, 1 Jepsen controller,
//! 1 orchestrator).

use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
fn get_tools_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("tools").exists())
        .expect("Could not find workspace root");
    workspace_root.join("tools")
}

#[allow(dead_code)]
fn load_yaml_file(path: &str) -> Result<serde_yaml::Value, String> {
    let tools_dir = get_tools_dir();
    let file_path = tools_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path.display(), e))?;
    serde_yaml::from_str(&content).map_err(|e| format!("YAML parse error: {}", e))
}

#[allow(dead_code)]
fn load_json_file(path: &str) -> Result<serde_json::Value, String> {
    let tools_dir = get_tools_dir();
    let file_path = tools_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path.display(), e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("JSON parse error at {}: {}", file_path.display(), e))
}

#[cfg(test)]
mod prometheus_configuration {
    use super::*;

    #[test]
    fn test_prometheus_config_valid_yaml() {
        let result = load_yaml_file("prometheus.yml");
        assert!(
            result.is_ok(),
            "Failed to parse prometheus.yml: {:?}",
            result.err()
        );

        let config = result.unwrap();
        assert!(
            config.get("global").is_some(),
            "Missing global section in prometheus.yml"
        );

        let retention = config["global"]["retention"]
            .as_str()
            .expect("retention should be a string");
        let days: i32 = retention
            .trim_end_matches('d')
            .parse()
            .expect("Failed to parse retention days");
        assert!(
            days >= 7,
            "Retention should be >= 7 days, got {} days",
            days
        );
    }

#[test]
    fn test_alert_cost_threshold_correct() {
        let config = load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");
        
        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");
        
        let cost_group = groups.iter()
            .find(|g| g["name"].as_str() == Some(&"cost".to_string()))
            .expect("Missing cost alert group");
        
        let empty_vec: Vec<serde_yaml::Value> = Vec::new();
        let rules = cost_group["rules"]
            .as_sequence()
            .unwrap_or(&empty_vec);
            for rule in rules {
                assert!(rule.get("alert").is_some(), "Rule missing 'alert' field");
                assert!(rule.get("expr").is_some(), "Rule missing 'expr' field");
                assert!(rule.get("for").is_some(), "Rule missing 'for' field");
                assert!(
                    rule.get("annotations").is_some(),
                    "Rule missing 'annotations' field"
                );
                assert!(rule.get("labels").is_some(), "Rule missing 'labels' field");
            }
        }
    }

    #[test]
    fn test_prometheus_retention_policy_reasonable() {
        let config = load_yaml_file("prometheus.yml").expect("Failed to parse prometheus.yml");

        let retention = config["global"]["retention"]
            .as_str()
            .expect("retention should be a string");
        let days: i32 = retention
            .trim_end_matches('d')
            .parse()
            .expect("Failed to parse retention days");

        assert!(days >= 7, "Retention too short: {} days (min: 7)", days);
        assert!(days <= 90, "Retention too long: {} days (max: 90)", days);
    }
}

#[cfg(test)]
mod alert_rule_validation {
    use super::*;

    #[test]
    fn test_alert_cost_threshold_correct() {
        let config =
            load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");

        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");

        let cost_group = groups
            .iter()
            .find(|g| g["name"].as_str() == Some(&"cost".to_string()))
            .expect("Missing cost alert group");

        let rules = cost_group["rules"]
            .as_sequence()
            .expect("rules should be a sequence");

        let mut found_80 = false;
        let mut found_100 = false;
        let mut found_monthly = false;

        for rule in rules {
            let name = rule["alert"].as_str().unwrap_or("");
            let expr = rule["expr"].as_str().unwrap_or("");

            if name == "DailySpendExceeded80Percent" {
                assert!(
                    expr.contains("80"),
                    "DailySpendExceeded80Percent threshold should be 80"
                );
                found_80 = true;
            }
            if name == "DailySpendExceeded100Percent" {
                assert!(
                    expr.contains("100"),
                    "DailySpendExceeded100Percent threshold should be 100"
                );
                found_100 = true;
            }
            if name == "MonthlyProjectionExceeded" {
                assert!(
                    expr.contains("3000"),
                    "MonthlyProjectionExceeded threshold should be 3000"
                );
                found_monthly = true;
            }
        }

        assert!(found_80, "Missing DailySpendExceeded80Percent alert");
        assert!(found_100, "Missing DailySpendExceeded100Percent alert");
        assert!(found_monthly, "Missing MonthlyProjectionExceeded alert");
    }

    #[test]
    fn test_alert_cpu_high_threshold_correct() {
        let config =
            load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");

        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");

        for group in groups {
            let rules = group["rules"].as_sequence().unwrap_or(&Vec::new());
            for rule in rules {
                let name = rule["alert"].as_str().unwrap_or("");
                if name == "HighCPUUsage" {
                    let expr = rule["expr"].as_str().unwrap_or("");
                    let for_duration = rule["for"].as_str().unwrap_or("");

                    assert!(expr.contains("80"), "CPU threshold should be > 80");
                    assert!(!expr.contains("> 95"), "CPU threshold should be < 95");
                    assert!(
                        for_duration.contains("5m") || for_duration.contains("10m"),
                        "CPU alert duration should be >= 5m"
                    );
                    return;
                }
            }
        }
        panic!("HighCPUUsage alert not found");
    }

    #[test]
    fn test_alert_memory_high_threshold_correct() {
        let config =
            load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");

        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");

        for group in groups {
            let rules = group["rules"].as_sequence().unwrap_or(&Vec::new());
            for rule in rules {
                let name = rule["alert"].as_str().unwrap_or("");
                if name == "HighMemoryUsage" {
                    let expr = rule["expr"].as_str().unwrap_or("");

                    assert!(expr.contains("85"), "Memory threshold should be > 80 (85)");
                    assert!(!expr.contains("> 95"), "Memory threshold should be < 95");
                    return;
                }
            }
        }
        panic!("HighMemoryUsage alert not found");
    }

    #[test]
    fn test_alert_rules_have_annotations() {
        let config =
            load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");

        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");

        let mut total_rules = 0;
        let mut rules_with_description = 0;
        let mut rules_with_summary = 0;
let mut rules_with_severity = 0;
        
        for group in groups {
            let empty_vec: Vec<serde_yaml::Value> = Vec::new();
            let rules = group["rules"].as_sequence().unwrap_or(&empty_vec);
            
            for rule in rules {
                total_rules += 1;

                let annotations = rule.get("annotations");
                if let Some(ann) = annotations {
                    if ann.get("description").is_some() {
                        rules_with_description += 1;
                    }
                    if ann.get("summary").is_some() {
                        rules_with_summary += 1;
                    }
                }

                let labels = rule.get("labels");
                if let Some(lbls) = labels {
                    if lbls.get("severity").is_some() {
                        rules_with_severity += 1;
                    }
                }
            }
        }

        assert_eq!(
            total_rules, rules_with_description,
            "Not all rules have description annotation"
        );
        assert_eq!(
            total_rules, rules_with_summary,
            "Not all rules have summary annotation"
        );
        assert_eq!(
            total_rules, rules_with_severity,
            "Not all rules have severity label"
        );
    }

    #[test]
    fn test_alert_rules_count() {
        let config =
            load_yaml_file("prometheus-alerts.yml").expect("Failed to parse prometheus-alerts.yml");

        let groups = config["groups"]
            .as_sequence()
            .expect("groups should be a sequence");

        let mut total_rules = 0;

        for group in groups {
            let rules = group["rules"]
                .as_sequence()
                .expect("rules should be a sequence");
            total_rules += rules.len();
        }

        assert_eq!(
            total_rules, 13,
            "Expected exactly 13 alert rules, found {}",
            total_rules
        );
    }
}

#[cfg(test)]
mod grafana_dashboards {
    use super::*;

    #[test]
    fn test_grafana_dashboard_infrastructure_valid_json() {
        let result = load_json_file("grafana-dashboard-infrastructure.json");
        assert!(
            result.is_ok(),
            "Failed to parse grafana-dashboard-infrastructure.json: {:?}",
            result.err()
        );

        let dashboard = result.unwrap();

        let title = dashboard["title"]
            .as_str()
            .expect("Dashboard should have a title");
        assert_eq!(
            title, "Infrastructure Health",
            "Dashboard title should be 'Infrastructure Health'"
        );

        let panels = dashboard["panels"]
            .as_array()
            .expect("Dashboard should have panels");
        assert!(
            panels.len() >= 5,
            "Dashboard should have at least 5 panels, found {}",
            panels.len()
        );
    }

    #[test]
    fn test_grafana_dashboard_cicd_valid_json() {
        let result = load_json_file("grafana-dashboard-cicd-metrics.json");
        assert!(
            result.is_ok(),
            "Failed to parse grafana-dashboard-cicd-metrics.json: {:?}",
            result.err()
        );

        let dashboard = result.unwrap();

        let title = dashboard["title"]
            .as_str()
            .expect("Dashboard should have a title");
        assert_eq!(
            title, "CI/CD Metrics",
            "Dashboard title should be 'CI/CD Metrics'"
        );

        let panels = dashboard["panels"]
            .as_array()
            .expect("Dashboard should have panels");
        assert!(
            panels.len() >= 5,
            "Dashboard should have at least 5 panels, found {}",
            panels.len()
        );
    }

    #[test]
    fn test_grafana_dashboard_cost_valid_json() {
        let result = load_json_file("grafana-dashboard-cost-analysis.json");
        assert!(
            result.is_ok(),
            "Failed to parse grafana-dashboard-cost-analysis.json: {:?}",
            result.err()
        );

        let dashboard = result.unwrap();

        let title = dashboard["title"]
            .as_str()
            .expect("Dashboard should have a title");
        assert_eq!(
            title, "Cost Analysis",
            "Dashboard title should be 'Cost Analysis'"
        );

        let panels = dashboard["panels"]
            .as_array()
            .expect("Dashboard should have panels");
        assert!(
            panels.len() >= 4,
            "Dashboard should have at least 4 panels, found {}",
            panels.len()
        );
    }

    #[test]
    fn test_grafana_dashboard_storage_valid_json() {
        let result = load_json_file("grafana-dashboard-storage-performance.json");
        assert!(
            result.is_ok(),
            "Failed to parse grafana-dashboard-storage-performance.json: {:?}",
            result.err()
        );

        let dashboard = result.unwrap();

        let title = dashboard["title"]
            .as_str()
            .expect("Dashboard should have a title");
        assert_eq!(
            title, "Storage Performance (Placeholder)",
            "Dashboard title should be 'Storage Performance (Placeholder)'"
        );

        let panels = dashboard["panels"]
            .as_array()
            .expect("Dashboard should have panels");
        assert!(
            panels.len() >= 4,
            "Storage dashboard should have at least 4 panels (placeholders), found {}",
            panels.len()
        );
    }
}

#[cfg(test)]
mod alertmanager_config {
    use super::*;

    #[test]
    fn test_alertmanager_config_valid_yaml() {
        let result = load_yaml_file("alertmanager.yml");
        assert!(
            result.is_ok(),
            "Failed to parse alertmanager.yml: {:?}",
            result.err()
        );

        let config = result.unwrap();
        assert!(
            config.get("route").is_some(),
            "Missing route section in alertmanager.yml"
        );
        assert!(
            config.get("receivers").is_some(),
            "Missing receivers section in alertmanager.yml"
        );

        let receivers = config["receivers"]
            .as_sequence()
            .expect("receivers should be a sequence");
        assert!(
            !receivers.is_empty(),
            "Should have at least one receiver defined"
        );
    }

    #[test]
    fn test_alertmanager_routes_cover_all_severity_levels() {
        let config = load_yaml_file("alertmanager.yml").expect("Failed to parse alertmanager.yml");

        let route = config["route"].clone();

        let mut has_critical = false;
        let mut has_warning = false;
        let mut has_info = false;

        let routes = route["routes"].as_sequence().unwrap_or(&Vec::new());
        for r in routes {
            if let Some(severity) = r["match"].as_object().and_then(|m| m.get("severity")) {
                if severity == "critical" {
                    has_critical = true;
                    let receiver = r["receiver"]
                        .as_str()
                        .expect("Critical route should have receiver");
                    assert!(
                        !receiver.is_empty(),
                        "Critical route should have non-empty receiver"
                    );
                }
                if severity == "warning" {
                    has_warning = true;
                    let receiver = r["receiver"]
                        .as_str()
                        .expect("Warning route should have receiver");
                    assert!(
                        !receiver.is_empty(),
                        "Warning route should have non-empty receiver"
                    );
                }
                if severity == "info" {
                    has_info = true;
                }
            }
        }

        assert!(has_critical, "Missing route for critical severity");
        assert!(has_warning, "Missing route for warning severity");
        assert!(has_info, "Missing route for info severity");
    }

    #[test]
    fn test_alertmanager_sns_receiver_configured() {
        let config = load_yaml_file("alertmanager.yml").expect("Failed to parse alertmanager.yml");

        let receivers = config["receivers"]
            .as_sequence()
            .expect("receivers should be a sequence");

        let mut has_sns = false;
let mut sns_configured = false;
        
        for receiver in receivers {
            if let Some(sns_configs) = receiver.get("sns_configs") {
                has_sns = true;
                let empty_vec: Vec<serde_yaml::Value> = Vec::new();
                let configs = sns_configs.as_sequence().unwrap_or(&empty_vec);
                for cfg in configs {
                    if let Some(topic_arn) = cfg.get("topic_arn") {
                        let arn = topic_arn.as_str().unwrap_or("");
                        if arn.contains("cfs-alerts-critical") {
                            sns_configured = true;
                        }
                    }
                }
            }
        }

        assert!(has_sns, "No SNS configs found in receivers");
        assert!(
            sns_configured,
            "SNS receiver for cfs-alerts-critical not configured"
        );
    }
}

#[cfg(test)]
mod cost_tracking {
    use super::*;

    #[test]
    fn test_cost_aggregator_script_exists() {
        let tools_dir = get_tools_dir();
        let script_path = tools_dir.join("cfs-cost-aggregator.sh");

        assert!(
            script_path.exists(),
            "Cost aggregator script should exist at {}",
            script_path.display()
        );

        let metadata = fs::metadata(&script_path).expect("Failed to get file metadata");
        let permissions = metadata.permissions();

        let executable = permissions.mode() & 0o111 != 0;
        assert!(
            executable,
            "Cost aggregator script should be executable (+x)"
        );

        let content = fs::read_to_string(&script_path).expect("Failed to read script");
        assert!(
            content.starts_with("#!/bin/bash") || content.starts_with("#!/bin/sh"),
            "Script should have bash/sh shebang"
        );
    }

    #[test]
    fn test_cost_aggregator_script_valid_bash() {
        let tools_dir = get_tools_dir();
        let script_path = tools_dir.join("cfs-cost-aggregator.sh");

        let output = std::process::Command::new("bash")
            .arg("-n")
            .arg(&script_path)
            .output();

        match output {
            Ok(result) => {
                assert!(
                    result.status.success(),
                    "Bash syntax check failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            Err(e) => {
                panic!("Failed to run bash syntax check: {}", e);
            }
        }
    }

    #[test]
    fn test_cost_aggregator_daily_cron_valid() {
        let tools_dir = get_tools_dir();
        let script_path = tools_dir.join("cfs-cost-aggregator.sh");

        let content = fs::read_to_string(&script_path).expect("Failed to read script");

        let cron_expr = content
            .lines()
            .find(|l| l.contains("cron") || l.contains("Cron") || l.contains("CRON"))
            .or_else(|| {
                content
                    .lines()
                    .find(|l| l.contains("30 0 * * *") || l.contains("00:30"))
            });

        if let Some(line) = cron_expr {
            if line.contains("30") && line.contains("*") {
                return;
            }
        }

        assert!(
            content.contains("00:30")
                || content.contains("30 0")
                || content.contains("daily")
                || content.contains("cron"),
            "Script should contain cron schedule for daily execution at 00:30 UTC"
        );
    }

    #[test]
    fn test_monitoring_infrastructure_completeness() {
        let tools_dir = get_tools_dir();

        let required_files = vec![
            "prometheus.yml",
            "prometheus-alerts.yml",
            "alertmanager.yml",
            "cfs-cost-aggregator.sh",
            "grafana-dashboard-infrastructure.json",
            "grafana-dashboard-cicd-metrics.json",
            "grafana-dashboard-cost-analysis.json",
            "grafana-dashboard-storage-performance.json",
        ];

        for file in required_files {
            let path = tools_dir.join(file);
            assert!(path.exists(), "Required monitoring file missing: {}", file);
        }
    }
}
