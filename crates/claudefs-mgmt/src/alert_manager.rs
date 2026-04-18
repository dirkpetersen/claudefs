use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

pub enum AlertType {
    Infrastructure,
    Performance,
    Capacity,
    Cost,
    Recovery,
}

impl AlertType {
    fn as_str(&self) -> &'static str {
        match self {
            AlertType::Infrastructure => "Infrastructure",
            AlertType::Performance => "Performance",
            AlertType::Capacity => "Capacity",
            AlertType::Cost => "Cost",
            AlertType::Recovery => "Recovery",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Infrastructure" => Some(AlertType::Infrastructure),
            "Performance" => Some(AlertType::Performance),
            "Capacity" => Some(AlertType::Capacity),
            "Cost" => Some(AlertType::Cost),
            "Recovery" => Some(AlertType::Recovery),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl AlertSeverity {
    fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "Info",
            AlertSeverity::Warning => "Warning",
            AlertSeverity::Critical => "Critical",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Info" => Some(AlertSeverity::Info),
            "Warning" => Some(AlertSeverity::Warning),
            "Critical" => Some(AlertSeverity::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AlertState {
    Active,
    Acknowledged,
    Resolved,
    Silenced,
}

impl AlertState {
    fn as_str(&self) -> &'static str {
        match self {
            AlertState::Active => "Active",
            AlertState::Acknowledged => "Acknowledged",
            AlertState::Resolved => "Resolved",
            AlertState::Silenced => "Silenced",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Active" => Some(AlertState::Active),
            "Acknowledged" => Some(AlertState::Acknowledged),
            "Resolved" => Some(AlertState::Resolved),
            "Silenced" => Some(AlertState::Silenced),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
    pub source: String,
    pub affected_resource: Option<String>,
    pub created_at: SystemTime,
    pub state: AlertState,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<SystemTime>,
    pub resolved_at: Option<SystemTime>,
    pub silenced_until: Option<SystemTime>,
    pub related_alerts: Vec<String>,
    pub metrics: serde_json::Value,
}

#[async_trait]
pub trait AlertCallback: Send + Sync {
    fn on_alert_created(&self, alert: &Alert);
    fn on_alert_acknowledged(&self, alert: &Alert, by: &str);
    fn on_alert_resolved(&self, alert: &Alert);
}

pub struct AlertManager {
    alerts: Arc<DashMap<String, Alert>>,
    db: Arc<Mutex<duckdb::Connection>>,
    callbacks: Arc<std::sync::Mutex<Vec<Box<dyn AlertCallback>>>>,
}

#[derive(Debug, Serialize)]
pub struct AlertStatistics {
    pub total_alerts: usize,
    pub active_count: usize,
    pub acknowledged_count: usize,
    pub resolved_count: usize,
    pub by_type: HashMap<String, usize>,
    pub by_severity: HashMap<String, usize>,
    pub recent_rate: f64,
}

impl AlertManager {
    pub async fn new(db_path: &str) -> anyhow::Result<Self> {
        let db = duckdb::Connection::open(db_path)?;

        db.execute(
            "CREATE TABLE IF NOT EXISTS alert_history (
                id UUID,
                alert_type VARCHAR,
                severity VARCHAR,
                title VARCHAR,
                message VARCHAR,
                source VARCHAR,
                affected_resource VARCHAR,
                created_at TIMESTAMP,
                state VARCHAR,
                acknowledged_by VARCHAR,
                acknowledged_at TIMESTAMP,
                resolved_at TIMESTAMP,
                silenced_until TIMESTAMP,
                metrics JSON,
                PRIMARY KEY (id)
            )",
            [],
        )?;

        db.execute(
            "CREATE TABLE IF NOT EXISTS alert_correlations (
                alert_id UUID,
                recovery_action_id VARCHAR,
                correlation_type VARCHAR,
                correlation_strength DECIMAL,
                created_at TIMESTAMP
            )",
            [],
        )?;

        let alerts = Arc::new(DashMap::new());

        let mut manager = Self {
            alerts: alerts.clone(),
            db: Arc::new(Mutex::new(db)),
            callbacks: Arc::new(std::sync::Mutex::new(Vec::new())),
        };

        manager.load_alerts_from_db().await?;

        Ok(manager)
    }

    async fn load_alerts_from_db(&self) -> anyhow::Result<()> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, alert_type, severity, title, message, source, affected_resource,
                    created_at, state, acknowledged_by, acknowledged_at, resolved_at,
                    silenced_until, metrics
             FROM alert_history
             WHERE state IN ('Active', 'Acknowledged', 'Silenced')"
        )?;

        let alert_rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let alert_type_str: String = row.get(1)?;
            let severity_str: String = row.get(2)?;
            let title: String = row.get(3)?;
            let message: String = row.get(4)?;
            let source: String = row.get(5)?;
            let affected_resource: Option<String> = row.get(6)?;
            let created_at: chrono::DateTime<Utc> = row.get(7)?;
            let state_str: String = row.get(8)?;
            let acknowledged_by: Option<String> = row.get(9)?;
            let acknowledged_at: Option<chrono::DateTime<Utc>> = row.get(10)?;
            let resolved_at: Option<chrono::DateTime<Utc>> = row.get(11)?;
            let silenced_until: Option<chrono::DateTime<Utc>> = row.get(12)?;
            let metrics_str: String = row.get(13)?;

            Ok((
                id,
                alert_type_str,
                severity_str,
                title,
                message,
                source,
                affected_resource,
                created_at,
                state_str,
                acknowledged_by,
                acknowledged_at,
                resolved_at,
                silenced_until,
                metrics_str,
            ))
        })?;

        for alert_result in alert_rows {
            let (
                id,
                alert_type_str,
                severity_str,
                title,
                message,
                source,
                affected_resource,
                created_at,
                state_str,
                acknowledged_by,
                acknowledged_at,
                resolved_at,
                silenced_until,
                metrics_str,
            ) = alert_result?;

            let alert_type = AlertType::from_str(&alert_type_str).unwrap_or(AlertType::Infrastructure);
            let severity = AlertSeverity::from_str(&severity_str).unwrap_or(AlertSeverity::Info);
            let state = AlertState::from_str(&state_str).unwrap_or(AlertState::Active);

            let created_at_sys = SystemTime::from(created_at);
            let acknowledged_at_sys = acknowledged_at.map(SystemTime::from);
            let resolved_at_sys = resolved_at.map(SystemTime::from);
            let silenced_until_sys = silenced_until.map(SystemTime::from);

            let metrics: serde_json::Value = serde_json::from_str(&metrics_str).unwrap_or(serde_json::Value::Null);

            let alert = Alert {
                id,
                alert_type,
                severity,
                title,
                message,
                source,
                affected_resource,
                created_at: created_at_sys,
                state,
                acknowledged_by,
                acknowledged_at: acknowledged_at_sys,
                resolved_at: resolved_at_sys,
                silenced_until: silenced_until_sys,
                related_alerts: Vec::new(),
                metrics,
            };

            self.alerts.insert(alert.id.clone(), alert);
        }

        Ok(())
    }

    pub async fn register_callback(&mut self, callback: Box<dyn AlertCallback>) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.push(callback);
        }
    }

    pub async fn create_alert(
        &self,
        alert_type: AlertType,
        severity: AlertSeverity,
        title: String,
        message: String,
        source: String,
        affected_resource: Option<String>,
    ) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        let created_at = SystemTime::now();

        let alert = Alert {
            id: id.clone(),
            alert_type,
            severity,
            title,
            message,
            source,
            affected_resource,
            created_at,
            state: AlertState::Active,
            acknowledged_by: None,
            acknowledged_at: None,
            resolved_at: None,
            silenced_until: None,
            related_alerts: Vec::new(),
            metrics: serde_json::Value::Null,
        };

        self.alerts.insert(id.clone(), alert.clone());

        self.persist_alert(&alert).await?;

        if let Ok(callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter() {
                callback.on_alert_created(&alert);
            }
        }

        Ok(id)
    }

    async fn persist_alert(&self, alert: &Alert) -> anyhow::Result<()> {
        let db = self.db.lock().await;
        let created_at_dt = Utc.timestamp_opt(
            alert.created_at
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            0,
        ).unwrap();

        let acknowledged_at_str = alert.acknowledged_at.map(|t| {
            let dt = Utc.timestamp_opt(
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                0,
            ).unwrap();
            dt.to_string()
        });

        let resolved_at_str = alert.resolved_at.map(|t| {
            let dt = Utc.timestamp_opt(
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                0,
            ).unwrap();
            dt.to_string()
        });

        let silenced_until_str = alert.silenced_until.map(|t| {
            let dt = Utc.timestamp_opt(
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                0,
            ).unwrap();
            dt.to_string()
        });

        let metrics_str = serde_json::to_string(&alert.metrics)?;

        db.execute(
            "INSERT OR REPLACE INTO alert_history 
             (id, alert_type, severity, title, message, source, affected_resource,
              created_at, state, acknowledged_by, acknowledged_at, resolved_at,
              silenced_until, metrics)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                &alert.id,
                alert.alert_type.as_str(),
                alert.severity.as_str(),
                &alert.title,
                &alert.message,
                &alert.source,
                &alert.affected_resource.clone().unwrap_or_default(),
                &created_at_dt.to_string(),
                alert.state.as_str(),
                &alert.acknowledged_by.clone().unwrap_or_default(),
                &acknowledged_at_str.unwrap_or_default(),
                &resolved_at_str.unwrap_or_default(),
                &silenced_until_str.unwrap_or_default(),
                &metrics_str,
            ],
        )?;

        Ok(())
    }

    pub async fn acknowledge_alert(&self, alert_id: &str, acknowledged_by: String) -> anyhow::Result<()> {
        let mut alert = self.alerts.get(alert_id).ok_or_else(|| anyhow::anyhow!("Alert not found"))?;
        
        alert.state = AlertState::Acknowledged;
        alert.acknowledged_by = Some(acknowledged_by.clone());
        alert.acknowledged_at = Some(SystemTime::now());

        let alert_clone = alert.clone();
        drop(alert);

        self.persist_alert(&alert_clone).await?;

        if let Ok(callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter() {
                callback.on_alert_acknowledged(&alert_clone, &acknowledged_by);
            }
        }

        Ok(())
    }

    pub async fn resolve_alert(&self, alert_id: &str) -> anyhow::Result<()> {
        let mut alert = self.alerts.get(alert_id).ok_or_else(|| anyhow::anyhow!("Alert not found"))?;
        
        alert.state = AlertState::Resolved;
        alert.resolved_at = Some(SystemTime::now());

        let alert_clone = alert.clone();
        drop(alert);

        self.persist_alert(&alert_clone).await?;

        if let Ok(callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter() {
                callback.on_alert_resolved(&alert_clone);
            }
        }

        Ok(())
    }

    pub async fn silence_alert(&self, alert_id: &str, duration: Duration) -> anyhow::Result<()> {
        let mut alert = self.alerts.get(alert_id).ok_or_else(|| anyhow::anyhow!("Alert not found"))?;
        
        let now = SystemTime::now();
        alert.silenced_until = Some(now.checked_add(duration).unwrap_or(now));

        let alert_clone = alert.clone();
        drop(alert);

        self.persist_alert(&alert_clone).await?;

        Ok(())
    }

    pub async fn get_active_alerts(&self) -> anyhow::Result<Vec<Alert>> {
        let now = SystemTime::now();
        let alerts: Vec<Alert> = self.alerts
            .iter()
            .filter(|a| {
                match a.state {
                    AlertState::Active | AlertState::Acknowledged => true,
                    AlertState::Silenced => {
                        if let Some(until) = a.silenced_until {
                            now > until
                        } else {
                            false
                        }
                    }
                    AlertState::Resolved => false,
                }
            })
            .map(|a| a.clone())
            .collect();
        Ok(alerts)
    }

    pub async fn get_alerts_by_type(&self, alert_type: AlertType) -> anyhow::Result<Vec<Alert>> {
        let alerts: Vec<Alert> = self.alerts
            .iter()
            .filter(|a| a.alert_type == alert_type)
            .map(|a| a.clone())
            .collect();
        Ok(alerts)
    }

    pub async fn get_alerts_by_severity(&self, severity: AlertSeverity) -> anyhow::Result<Vec<Alert>> {
        let alerts: Vec<Alert> = self.alerts
            .iter()
            .filter(|a| a.severity == severity)
            .map(|a| a.clone())
            .collect();
        Ok(alerts)
    }

    pub async fn get_alerts_by_resource(&self, resource: &str) -> anyhow::Result<Vec<Alert>> {
        let alerts: Vec<Alert> = self.alerts
            .iter()
            .filter(|a| a.affected_resource.as_deref() == Some(resource))
            .map(|a| a.clone())
            .collect();
        Ok(alerts)
    }

    pub async fn get_recent_alerts(&self, limit: usize) -> anyhow::Result<Vec<Alert>> {
        let mut alerts: Vec<Alert> = self.alerts
            .iter()
            .map(|a| a.clone())
            .collect();
        
        alerts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        alerts.truncate(limit);
        
        Ok(alerts)
    }

    pub async fn get_alert_history(&self, from: SystemTime, to: SystemTime) -> anyhow::Result<Vec<Alert>> {
        let alerts: Vec<Alert> = self.alerts
            .iter()
            .filter(|a| a.created_at >= from && a.created_at <= to)
            .map(|a| a.clone())
            .collect();
        Ok(alerts)
    }

    pub async fn find_correlated_alerts(&self, alert_id: &str) -> anyhow::Result<Vec<Alert>> {
        let alert = self.alerts.get(alert_id).ok_or_else(|| anyhow::anyhow!("Alert not found"))?;
        
        let related_ids: Vec<String> = alert.related_alerts.clone();
        let source = alert.source.clone();
        let affected_resource = alert.affected_resource.clone();

        let mut correlated: Vec<Alert> = Vec::new();

        for a in self.alerts.iter() {
            if a.id == alert_id {
                continue;
            }
            if related_ids.contains(&a.id) {
                correlated.push(a.clone());
            } else if a.source == source && a.id != alert_id {
                correlated.push(a.clone());
            } else if let Some(ref ar) = affected_resource {
                if a.affected_resource.as_deref() == Some(ar) && a.id != alert_id {
                    correlated.push(a.clone());
                }
            }
        }

        Ok(correlated)
    }

    pub async fn get_alert_statistics(&self) -> anyhow::Result<AlertStatistics> {
        let now = SystemTime::now();
        let five_minutes_ago = now.checked_sub(Duration::from_secs(300)).unwrap_or(now);

        let mut total = 0;
        let mut active_count = 0;
        let mut acknowledged_count = 0;
        let mut resolved_count = 0;
        let mut by_type: HashMap<String, usize> = HashMap::new();
        let mut by_severity: HashMap<String, usize> = HashMap::new();
        let mut recent_count = 0;

        for alert in self.alerts.iter() {
            total += 1;
            
            *by_type.entry(alert.alert_type.as_str().to_string()).or_insert(0) += 1;
            *by_severity.entry(alert.severity.as_str().to_string()).or_insert(0) += 1;

            match alert.state {
                AlertState::Active | AlertState::Silenced => active_count += 1,
                AlertState::Acknowledged => acknowledged_count += 1,
                AlertState::Resolved => resolved_count += 1,
            }

            if alert.created_at >= five_minutes_ago {
                recent_count += 1;
            }
        }

        let recent_rate = recent_count as f64 / 5.0;

        Ok(AlertStatistics {
            total_alerts: total,
            active_count,
            acknowledged_count,
            resolved_count,
            by_type,
            by_severity,
            recent_rate,
        })
    }

    pub async fn correlate_alert_with_recovery(&self, alert_id: &str, recovery_action_id: String) -> anyhow::Result<()> {
        let alert = self.alerts.get(alert_id).ok_or_else(|| anyhow::anyhow!("Alert not found"))?;
        
        if !alert.related_alerts.contains(&recovery_action_id) {
            let mut mutable_alert = alert.clone();
            mutable_alert.related_alerts.push(recovery_action_id);
            self.alerts.insert(alert_id.to_string(), mutable_alert);
        }

        let db = self.db.lock().await;
        let now_dt = Utc::now();
        
        db.execute(
            "INSERT INTO alert_correlations (alert_id, recovery_action_id, correlation_type, correlation_strength, created_at)
             VALUES (?, ?, 'recovery', 1.0, ?)",
            [alert_id, &recovery_action_id, &now_dt.to_string()],
        )?;

        Ok(())
    }

    pub async fn alert_node_down(&self, node_id: &str) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Critical,
            format!("Node {} is down", node_id),
            format!("Storage node {} has become unreachable", node_id),
            "health_monitor".to_string(),
            Some(node_id.to_string()),
        ).await
    }

    pub async fn alert_node_high_cpu(&self, node_id: &str, cpu_percent: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            format!("High CPU on node {}", node_id),
            format!("Node {} CPU usage at {:.1}%", node_id, cpu_percent),
            "health_monitor".to_string(),
            Some(node_id.to_string()),
        ).await
    }

    pub async fn alert_node_high_memory(&self, node_id: &str, mem_percent: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            format!("High memory on node {}", node_id),
            format!("Node {} memory usage at {:.1}%", node_id, mem_percent),
            "health_monitor".to_string(),
            Some(node_id.to_string()),
        ).await
    }

    pub async fn alert_network_partition(&self, partitions: usize) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Critical,
            "Network partition detected".to_string(),
            format!("Detected {} network partitions", partitions),
            "health_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_read_latency_high(&self, p99_ms: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Performance,
            AlertSeverity::Warning,
            "High read latency".to_string(),
            format!("Read latency p99 at {:.2}ms", p99_ms),
            "performance_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_write_latency_high(&self, p99_ms: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Performance,
            AlertSeverity::Warning,
            "High write latency".to_string(),
            format!("Write latency p99 at {:.2}ms", p99_ms),
            "performance_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_iops_degradation(&self, current: f64, baseline: f64) -> anyhow::Result<String> {
        let pct = ((baseline - current) / baseline * 100.0).round();
        self.create_alert(
            AlertType::Performance,
            AlertSeverity::Warning,
            "IOPS degradation detected".to_string(),
            format!("Current IOPS {:.0} is {:.0}% below baseline {:.0}", current, pct, baseline),
            "performance_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_cache_hit_rate_low(&self, hit_rate: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Performance,
            AlertSeverity::Warning,
            "Low cache hit rate".to_string(),
            format!("Cache hit rate at {:.1}%", hit_rate),
            "cache_manager".to_string(),
            None,
        ).await
    }

    pub async fn alert_flash_utilization_high(&self, percent: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Capacity,
            AlertSeverity::Warning,
            "High flash utilization".to_string(),
            format!("Flash storage utilization at {:.1}%", percent),
            "capacity_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_s3_utilization_high(&self, percent: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Capacity,
            AlertSeverity::Warning,
            "High S3 utilization".to_string(),
            format!("S3 storage utilization at {:.1}%", percent),
            "capacity_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_flash_nearly_full(&self, days_to_full: u32) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Capacity,
            AlertSeverity::Critical,
            "Flash nearly full".to_string(),
            format!("Flash storage will be full in approximately {} days", days_to_full),
            "capacity_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_budget_exceeded(&self, amount: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Cost,
            AlertSeverity::Warning,
            "Budget exceeded".to_string(),
            format!("Cost exceeded budget by ${:.2}", amount),
            "cost_tracker".to_string(),
            None,
        ).await
    }

    pub async fn alert_cost_anomaly(&self, deviation_percent: f64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Cost,
            AlertSeverity::Warning,
            "Cost anomaly detected".to_string(),
            format!("Cost deviation at {:.1}% from expected", deviation_percent),
            "cost_tracker".to_string(),
            None,
        ).await
    }

    pub async fn alert_recovery_action_failed(&self, action_type: &str, reason: &str) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Recovery,
            AlertSeverity::Critical,
            format!("Recovery action {} failed", action_type),
            format!("Recovery action {} failed: {}", action_type, reason),
            "recovery_executor".to_string(),
            None,
        ).await
    }

    pub async fn alert_replication_lag_high(&self, lag_ms: u64) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Recovery,
            AlertSeverity::Warning,
            "High replication lag".to_string(),
            format!("Replication lag at {}ms", lag_ms),
            "replication_monitor".to_string(),
            None,
        ).await
    }

    pub async fn alert_data_inconsistency(&self, affected_segments: usize) -> anyhow::Result<String> {
        self.create_alert(
            AlertType::Recovery,
            AlertSeverity::Critical,
            "Data inconsistency detected".to_string(),
            format!("Data inconsistency in {} segments", affected_segments),
            "consistency_checker".to_string(),
            None,
        ).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestCallback {
        created: Arc<Mutex<bool>>,
        acknowledged: Arc<Mutex<bool>>,
        resolved: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl AlertCallback for TestCallback {
        fn on_alert_created(&self, _alert: &Alert) {
            let created = self.created.clone();
            tokio::spawn(async move {
                let mut c = created.lock().await;
                *c = true;
            });
        }
        fn on_alert_acknowledged(&self, _alert: &Alert, _by: &str) {
            let acknowledged = self.acknowledged.clone();
            tokio::spawn(async move {
                let mut a = acknowledged.lock().await;
                *a = true;
            });
        }
        fn on_alert_resolved(&self, _alert: &Alert) {
            let resolved = self.resolved.clone();
            tokio::spawn(async move {
                let mut r = resolved.lock().await;
                *r = true;
            });
        }
    }

    #[tokio::test]
    async fn test_alert_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let id = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Critical,
            "Test Alert".to_string(),
            "Test message".to_string(),
            "test_source".to_string(),
            Some("node1".to_string()),
        ).await.unwrap();

        assert!(!id.is_empty());
        assert!(manager.alerts.contains_key(&id));

        let db = manager.db.lock().await;
        let mut stmt = db.prepare("SELECT COUNT(*) FROM alert_history WHERE id = ?").unwrap();
        let count: i64 = stmt.query_row([&id], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_alert_state_transitions() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let id = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        let alert = manager.alerts.get(&id).unwrap();
        assert_eq!(alert.state, AlertState::Active);

        manager.acknowledge_alert(&id, "admin".to_string()).await.unwrap();
        
        let alert = manager.alerts.get(&id).unwrap();
        assert_eq!(alert.state, AlertState::Acknowledged);
        assert_eq!(alert.acknowledged_by, Some("admin".to_string()));
        assert!(alert.acknowledged_at.is_some());

        manager.resolve_alert(&id).await.unwrap();
        
        let alert = manager.alerts.get(&id).unwrap();
        assert_eq!(alert.state, AlertState::Resolved);
        assert!(alert.resolved_at.is_some());
    }

    #[tokio::test]
    async fn test_alert_acknowledgment() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let mut manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let created_flag = Arc::new(Mutex::new(false));
        let ack_flag = Arc::new(Mutex::new(false));
        
        let callback = Box::new(TestCallback {
            created: created_flag.clone(),
            acknowledged: ack_flag.clone(),
            resolved: Arc::new(Mutex::new(false)),
        });
        
        manager.register_callback(callback).await;

        let id = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        let created = *created_flag.lock().await;
        assert!(created);

        manager.acknowledge_alert(&id, "user".to_string()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        let acknowledged = *ack_flag.lock().await;
        assert!(acknowledged);
    }

    #[tokio::test]
    async fn test_alert_silencing() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let id = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        manager.silence_alert(&id, Duration::from_secs(60)).await.unwrap();

        let active = manager.get_active_alerts().await.unwrap();
        assert!(active.is_empty());

        tokio::time::sleep(Duration::from_secs(61)).await;

        let active = manager.get_active_alerts().await.unwrap();
        assert_eq!(active.len(), 1);
    }

    #[tokio::test]
    async fn test_get_alerts_by_severity() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Info,
            "Info alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Critical,
            "Critical alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        let critical = manager.get_alerts_by_severity(AlertSeverity::Critical).await.unwrap();
        assert_eq!(critical.len(), 1);

        let info = manager.get_alerts_by_severity(AlertSeverity::Info).await.unwrap();
        assert_eq!(info.len(), 1);
    }

    #[tokio::test]
    async fn test_get_alerts_by_type() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Infra alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        manager.create_alert(
            AlertType::Performance,
            AlertSeverity::Warning,
            "Perf alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        let infra = manager.get_alerts_by_type(AlertType::Infrastructure).await.unwrap();
        assert_eq!(infra.len(), 1);
        assert!(matches!(infra[0].alert_type, AlertType::Infrastructure));

        let perf = manager.get_alerts_by_type(AlertType::Performance).await.unwrap();
        assert_eq!(perf.len(), 1);
        assert!(matches!(perf[0].alert_type, AlertType::Performance));
    }

    #[tokio::test]
    async fn test_alert_correlation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let id1 = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Alert 1".to_string(),
            "Test".to_string(),
            "test".to_string(),
            Some("node1".to_string()),
        ).await.unwrap();

        let id2 = manager.create_alert(
            AlertType::Infrastructure,
            AlertSeverity::Warning,
            "Alert 2".to_string(),
            "Test".to_string(),
            "test".to_string(),
            Some("node1".to_string()),
        ).await.unwrap();

        let mut alert1 = manager.alerts.get(&id1).unwrap().clone();
        alert1.related_alerts.push(id2.clone());
        manager.alerts.insert(id1.clone(), alert1);

        let correlated = manager.find_correlated_alerts(&id1).await.unwrap();
        assert!(correlated.len() >= 1);
    }

    #[tokio::test]
    async fn test_alert_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        
        let manager1 = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let id = manager1.create_alert(
            AlertType::Capacity,
            AlertSeverity::Warning,
            "Capacity alert".to_string(),
            "Test".to_string(),
            "test".to_string(),
            None,
        ).await.unwrap();

        drop(manager1);

        let manager2 = AlertManager::new(db_path.to_str().unwrap()).await.unwrap();
        
        let alert = manager2.alerts.get(&id);
        assert!(alert.is_some());
    }
}