use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AlertError {
    #[error("Rule evaluation error: {0}")]
    Evaluation(String),
    #[error("Notification error: {0}")]
    Notification(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertState {
    Ok,
    Firing,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub description: String,
    pub severity: AlertSeverity,
    pub metric: String,
    pub threshold: f64,
    pub comparison: Comparison,
    pub for_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Comparison {
    GreaterThan,
    LessThan,
    Equal,
}

impl AlertRule {
    pub fn evaluate(&self, metric_value: f64) -> bool {
        match self.comparison {
            Comparison::GreaterThan => metric_value > self.threshold,
            Comparison::LessThan => metric_value < self.threshold,
            Comparison::Equal => (metric_value - self.threshold).abs() < f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub rule: AlertRule,
    pub state: AlertState,
    pub value: f64,
    pub firing_since: Option<u64>,
    pub resolved_at: Option<u64>,
    pub message: String,
    pub labels: HashMap<String, String>,
}

impl Alert {
    pub fn new(rule: AlertRule, value: f64) -> Self {
        let message = if value != 0.0 {
            format!("{}: {} (value: {})", rule.name, rule.description, value)
        } else {
            format!("{}: {} (no data)", rule.name, rule.description)
        };

        Self {
            rule,
            state: AlertState::Ok,
            value,
            firing_since: None,
            resolved_at: None,
            message,
            labels: HashMap::new(),
        }
    }

    pub fn is_firing(&self) -> bool {
        self.state == AlertState::Firing
    }

    pub fn is_resolved(&self) -> bool {
        self.state == AlertState::Resolved
    }

    pub fn age_secs(&self) -> u64 {
        match self.firing_since {
            Some(start) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                now.saturating_sub(start)
            }
            None => 0,
        }
    }
}

pub fn default_alert_rules() -> Vec<AlertRule> {
    vec![
        AlertRule {
            name: "NodeOffline".to_string(),
            description: "Storage node is offline".to_string(),
            severity: AlertSeverity::Critical,
            metric: "nodes_healthy".to_string(),
            threshold: 1.0,
            comparison: Comparison::LessThan,
            for_secs: 60,
        },
        AlertRule {
            name: "HighReplicationLag".to_string(),
            description: "Replication lag exceeds 60 seconds".to_string(),
            severity: AlertSeverity::Warning,
            metric: "replication_lag_secs".to_string(),
            threshold: 60.0,
            comparison: Comparison::GreaterThan,
            for_secs: 30,
        },
        AlertRule {
            name: "HighCapacityUsage".to_string(),
            description: "Cluster capacity usage exceeds 90%".to_string(),
            severity: AlertSeverity::Critical,
            metric: "capacity_used_ratio".to_string(),
            threshold: 0.90,
            comparison: Comparison::GreaterThan,
            for_secs: 300,
        },
        AlertRule {
            name: "HighWriteLatency".to_string(),
            description: "Write latency p99 exceeds 10ms".to_string(),
            severity: AlertSeverity::Warning,
            metric: "latency_write_us_p99".to_string(),
            threshold: 10000.0,
            comparison: Comparison::GreaterThan,
            for_secs: 120,
        },
    ]
}

pub struct AlertManager {
    rules: Vec<AlertRule>,
    active_alerts: HashMap<String, Alert>,
}

impl AlertManager {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            active_alerts: HashMap::new(),
        }
    }

    pub fn with_default_rules() -> Self {
        Self::new(default_alert_rules())
    }

    pub fn evaluate(&mut self, metrics: &HashMap<String, f64>) -> Vec<Alert> {
        let mut changed = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        for rule in &self.rules {
            let metric_value = metrics.get(&rule.metric).copied().unwrap_or(0.0);
            let condition_met = rule.evaluate(metric_value);

            let alert = self
                .active_alerts
                .entry(rule.name.clone())
                .or_insert_with(|| Alert::new(rule.clone(), metric_value));

            let previous_state = alert.state.clone();
            alert.value = metric_value;
            alert.rule = rule.clone();
            alert.message = format!(
                "{}: {} (value: {:.2})",
                rule.name, rule.description, metric_value
            );

            if condition_met {
                if alert.firing_since.is_none() {
                    alert.firing_since = Some(now);
                }
                alert.state = AlertState::Firing;
            } else if alert.state == AlertState::Firing {
                alert.state = AlertState::Resolved;
                alert.resolved_at = Some(now);
            } else {
                alert.state = AlertState::Ok;
            }

            if alert.state != previous_state {
                changed.push(alert.clone());
            }
        }

        changed
    }

    pub fn firing_alerts(&self) -> Vec<&Alert> {
        self.active_alerts
            .values()
            .filter(|a| a.is_firing())
            .collect()
    }

    pub fn all_alerts(&self) -> Vec<&Alert> {
        self.active_alerts.values().collect()
    }

    pub fn alert_count_by_severity(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();

        for alert in self.active_alerts.values() {
            if alert.is_firing() {
                let severity_key = match alert.rule.severity {
                    AlertSeverity::Info => "info",
                    AlertSeverity::Warning => "warning",
                    AlertSeverity::Critical => "critical",
                };
                *counts.entry(severity_key.to_string()).or_insert(0) += 1;
            }
        }

        counts
    }

    pub fn gc_resolved(&mut self, max_age_secs: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.active_alerts.retain(|_, alert| {
            if let Some(resolved_at) = alert.resolved_at {
                now - resolved_at < max_age_secs
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_rule_evaluate_greater_than_true() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        assert!(rule.evaluate(11.0));
    }

    #[test]
    fn test_alert_rule_evaluate_greater_than_false_equal() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        assert!(!rule.evaluate(10.0));
    }

    #[test]
    fn test_alert_rule_evaluate_greater_than_false_below() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        assert!(!rule.evaluate(9.0));
    }

    #[test]
    fn test_alert_rule_evaluate_less_than_true() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::LessThan,
            for_secs: 60,
        };

        assert!(rule.evaluate(9.0));
    }

    #[test]
    fn test_alert_rule_evaluate_less_than_false() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::LessThan,
            for_secs: 60,
        };

        assert!(!rule.evaluate(10.0));
    }

    #[test]
    fn test_alert_rule_evaluate_equal_true() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::Equal,
            for_secs: 60,
        };

        assert!(rule.evaluate(10.0));
    }

    #[test]
    fn test_alert_rule_evaluate_equal_false() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::Equal,
            for_secs: 60,
        };

        assert!(!rule.evaluate(11.0));
    }

    #[test]
    fn test_alert_new_ok_state() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        let alert = Alert::new(rule.clone(), 5.0);

        assert_eq!(alert.state, AlertState::Ok);
    }

    #[test]
    fn test_alert_is_firing_false_for_ok() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        let alert = Alert::new(rule, 5.0);

        assert!(!alert.is_firing());
    }

    #[test]
    fn test_alert_is_resolved_false_for_ok() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        let alert = Alert::new(rule, 5.0);

        assert!(!alert.is_resolved());
    }

    #[test]
    fn test_alert_age_secs_zero_when_not_firing() {
        let rule = AlertRule {
            name: "Test".to_string(),
            description: "Test rule".to_string(),
            severity: AlertSeverity::Warning,
            metric: "test_metric".to_string(),
            threshold: 10.0,
            comparison: Comparison::GreaterThan,
            for_secs: 60,
        };

        let alert = Alert::new(rule, 5.0);

        assert_eq!(alert.age_secs(), 0);
    }

    #[test]
    fn test_alert_manager_with_default_rules() {
        let manager = AlertManager::with_default_rules();

        assert_eq!(manager.rules.len(), 4);
    }

    #[test]
    fn test_alert_manager_evaluate_all_ok() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 3.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.5),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        let changed = manager.evaluate(&metrics);

        assert!(changed.is_empty() || changed.iter().all(|a| a.state != AlertState::Firing));
    }

    #[test]
    fn test_alert_manager_evaluate_fires_node_offline() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 0.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.5),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        let changed = manager.evaluate(&metrics);

        let node_offline_fired = changed.iter().any(|a| a.rule.name == "NodeOffline");
        assert!(node_offline_fired);
    }

    #[test]
    fn test_alert_manager_evaluate_fires_high_capacity() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 3.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.95),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        let changed = manager.evaluate(&metrics);

        let capacity_fired = changed.iter().any(|a| a.rule.name == "HighCapacityUsage");
        assert!(capacity_fired);
    }

    #[test]
    fn test_alert_manager_firing_alerts() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 0.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.95),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        manager.evaluate(&metrics);

        let firing = manager.firing_alerts();
        assert!(!firing.is_empty());
    }

    #[test]
    fn test_alert_manager_alert_count_by_severity() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 0.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.95),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        manager.evaluate(&metrics);

        let counts = manager.alert_count_by_severity();

        assert!(*counts.get("critical").unwrap_or(&0) > 0);
    }

    #[test]
    fn test_alert_manager_gc_resolved() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 3.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.5),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        manager.evaluate(&metrics);

        let firing = manager.firing_alerts().len();

        manager.gc_resolved(3600);

        assert_eq!(manager.firing_alerts().len(), firing);
    }

    #[test]
    fn test_default_alert_rules_returns_4() {
        let rules = default_alert_rules();
        assert_eq!(rules.len(), 4);
    }

    #[test]
    fn test_default_alert_rules_have_names() {
        let rules = default_alert_rules();
        for rule in &rules {
            assert!(!rule.name.is_empty());
        }
    }

    #[test]
    fn test_default_alert_rules_have_descriptions() {
        let rules = default_alert_rules();
        for rule in &rules {
            assert!(!rule.description.is_empty());
        }
    }

    #[test]
    fn test_alert_manager_new() {
        let rules = default_alert_rules();
        let manager = AlertManager::new(rules);

        assert_eq!(manager.rules.len(), 4);
    }

    #[test]
    fn test_alert_all_alerts() {
        let mut manager = AlertManager::with_default_rules();

        let metrics = HashMap::from([
            ("nodes_healthy".to_string(), 3.0),
            ("replication_lag_secs".to_string(), 10.0),
            ("capacity_used_ratio".to_string(), 0.5),
            ("latency_write_us_p99".to_string(), 5000.0),
        ]);

        manager.evaluate(&metrics);

        let all = manager.all_alerts();
        assert_eq!(all.len(), 4);
    }
}
