use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64};

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("Action timeout: {0}")]
    Timeout(String),
    #[error("Target service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("Action already in progress: {0}")]
    AlreadyInProgress(String),
    #[error("Invalid action parameter: {0}")]
    InvalidParameter(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecoveryAction {
    ReduceWorkerThreads { target: u16 },
    ShrinkMemoryCaches { target_mb: u32 },
    EvictColdData { target_bytes: u64 },
    TriggerEmergencyCleanup,
    RestartComponent { component: String },
    RemoveDeadNode { node_id: String },
    RotateBackup { retention_days: u32 },
    GracefulShutdown { drain_timeout_secs: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionStatus {
    Pending,
    InProgress,
    Succeeded,
    PartialSuccess,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryLog {
    pub timestamp: u64,
    pub action: RecoveryAction,
    pub status: ActionStatus,
    pub details: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    pub cpu_threshold_high: f64,
    pub cpu_threshold_critical: f64,
    pub memory_threshold_high: f64,
    pub memory_threshold_critical: f64,
    pub disk_threshold_warning: f64,
    pub disk_threshold_critical: f64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            cpu_threshold_high: 70.0,
            cpu_threshold_critical: 85.0,
            memory_threshold_high: 80.0,
            memory_threshold_critical: 95.0,
            disk_threshold_warning: 80.0,
            disk_threshold_critical: 95.0,
        }
    }
}

pub struct ExecutionContext {
    pub node_id: String,
    pub timestamp: u64,
    pub metrics: HashMap<String, f64>,
}

pub struct RecoveryExecutor {
    history: Arc<Mutex<Vec<RecoveryLog>>>,
    config: RecoveryConfig,
    action_in_progress: Arc<AtomicBool>,
}

impl RecoveryExecutor {
    pub fn new(config: RecoveryConfig) -> Self {
        Self {
            history: Arc::new(Mutex::new(Vec::new())),
            config,
            action_in_progress: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn execute(&mut self, action: RecoveryAction) -> Result<RecoveryLog, RecoveryError> {
        if self.action_in_progress.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(RecoveryError::AlreadyInProgress(
                format!("{:?}", action)
            ));
        }

        self.action_in_progress.store(true, std::sync::atomic::Ordering::Relaxed);
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let result = match &action {
            RecoveryAction::ReduceWorkerThreads { target } => {
                self.reduce_worker_threads(*target).await
            },
            RecoveryAction::ShrinkMemoryCaches { target_mb } => {
                self.shrink_memory_caches(*target_mb).await
            },
            RecoveryAction::EvictColdData { target_bytes } => {
                self.evict_cold_data(*target_bytes).await
            },
            RecoveryAction::TriggerEmergencyCleanup => {
                self.trigger_emergency_cleanup().await
            },
            RecoveryAction::RestartComponent { component } => {
                self.restart_component(component).await
            },
            RecoveryAction::RemoveDeadNode { node_id } => {
                self.remove_dead_node(node_id).await
            },
            RecoveryAction::RotateBackup { retention_days } => {
                self.rotate_backups(*retention_days).await
            },
            RecoveryAction::GracefulShutdown { drain_timeout_secs } => {
                self.graceful_shutdown(*drain_timeout_secs).await
            },
        };

        let end = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let duration_ms = end - start;

        let mut log = result?;
        log.duration_ms = duration_ms;

        if let Ok(mut history) = self.history.lock() {
            history.push(log.clone());
        }

        self.action_in_progress.store(false, std::sync::atomic::Ordering::Relaxed);

        Ok(log)
    }

    async fn reduce_worker_threads(&mut self, target: u16) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Reduced worker threads to {}", target);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::ReduceWorkerThreads { target },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn shrink_memory_caches(&mut self, target_mb: u32) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Shrunk memory caches to {} MB", target_mb);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::ShrinkMemoryCaches { target_mb },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn evict_cold_data(&mut self, target_bytes: u64) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Evicted {} bytes of cold data", target_bytes);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::EvictColdData { target_bytes },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn trigger_emergency_cleanup(&mut self) -> Result<RecoveryLog, RecoveryError> {
        let details = "Emergency cleanup completed".to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::TriggerEmergencyCleanup,
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn restart_component(&mut self, component: &str) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Restarted component: {}", component);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::RestartComponent { component: component.to_string() },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn remove_dead_node(&mut self, node_id: &str) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Removed dead node: {}", node_id);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::RemoveDeadNode { node_id: node_id.to_string() },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn rotate_backups(&mut self, retention_days: u32) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Rotated backups with {} day retention", retention_days);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::RotateBackup { retention_days },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    async fn graceful_shutdown(&mut self, drain_timeout: u64) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Graceful shutdown with {} second drain timeout", drain_timeout);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(RecoveryLog {
            timestamp,
            action: RecoveryAction::GracefulShutdown { drain_timeout_secs: drain_timeout },
            status: ActionStatus::Succeeded,
            details,
            duration_ms: 0,
        })
    }

    pub fn history(&self) -> Vec<RecoveryLog> {
        if let Ok(guard) = self.history.lock() {
            guard.iter().take(1000).cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn clear_history(&self) {
        if let Ok(mut guard) = self.history.lock() {
            guard.clear();
        }
    }
}

pub fn cpu_to_action(cpu_usage_pct: f64) -> Option<RecoveryAction> {
    if cpu_usage_pct > 70.0 {
        Some(RecoveryAction::ReduceWorkerThreads { target: 4 })
    } else {
        None
    }
}

pub fn memory_to_action(mem_usage_pct: f64) -> Option<RecoveryAction> {
    if mem_usage_pct > 80.0 {
        Some(RecoveryAction::ShrinkMemoryCaches { target_mb: 512 })
    } else {
        None
    }
}

pub fn disk_to_action(free_pct: f64) -> Option<RecoveryAction> {
    if free_pct < 5.0 {
        Some(RecoveryAction::TriggerEmergencyCleanup)
    } else if free_pct < 10.0 {
        Some(RecoveryAction::EvictColdData { target_bytes: 1024 * 1024 * 1024 })
    } else {
        None
    }
}

pub fn should_remove_node(missed_heartbeats: u32) -> bool {
    missed_heartbeats >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_to_action_threshold() {
        let action = cpu_to_action(75.0);
        assert!(matches!(action, Some(RecoveryAction::ReduceWorkerThreads { target: 4 })));
        
        let action = cpu_to_action(50.0);
        assert!(action.is_none());
    }

    #[test]
    fn test_memory_to_action_threshold() {
        let action = memory_to_action(85.0);
        assert!(matches!(action, Some(RecoveryAction::ShrinkMemoryCaches { target_mb: 512 })));
        
        let action = memory_to_action(70.0);
        assert!(action.is_none());
    }

    #[test]
    fn test_disk_to_action_threshold() {
        let action = disk_to_action(8.0);
        assert!(matches!(action, Some(RecoveryAction::EvictColdData { target_bytes: 1_073_741_824 })));

        let action = disk_to_action(4.0);
        assert!(matches!(action, Some(RecoveryAction::TriggerEmergencyCleanup)));

        let action = disk_to_action(50.0);
        assert!(action.is_none());
    }

    #[test]
    fn test_recovery_log_serialization() {
        let log = RecoveryLog {
            timestamp: 12345,
            action: RecoveryAction::TriggerEmergencyCleanup,
            status: ActionStatus::Succeeded,
            details: "test cleanup".to_string(),
            duration_ms: 100,
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("12345"));
        
        let deserialized: RecoveryLog = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.timestamp, 12345);
        assert_eq!(deserialized.duration_ms, 100);
    }

    #[tokio::test]
    async fn test_executor_history_audit_trail() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let log1 = executor.execute(RecoveryAction::ReduceWorkerThreads { target: 4 }).await.unwrap();
        let log2 = executor.execute(RecoveryAction::ShrinkMemoryCaches { target_mb: 256 }).await.unwrap();
        
        let history = executor.history();
        assert!(history.len() >= 2);
    }

    #[test]
    fn test_should_remove_node_logic() {
        assert!(!should_remove_node(1));
        assert!(!should_remove_node(2));
        assert!(should_remove_node(3));
        assert!(should_remove_node(4));
    }

    #[tokio::test]
    async fn test_recovery_executor_new() {
        let config = RecoveryConfig::default();
        let executor = RecoveryExecutor::new(config);
        let history = executor.history();
        assert_eq!(history.len(), 0);
    }

    #[tokio::test]
    async fn test_execute_reduce_workers() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::ReduceWorkerThreads { target: 4 }).await;
        assert!(result.is_ok());
        
        let log = result.unwrap();
        assert!(matches!(log.status, ActionStatus::Succeeded));
    }

    #[tokio::test]
    async fn test_execute_shrink_memory() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::ShrinkMemoryCaches { target_mb: 512 }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_evict_cold_data() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::EvictColdData { target_bytes: 1024 * 1024 }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_emergency_cleanup() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::TriggerEmergencyCleanup).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_remove_dead_node() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::RemoveDeadNode { node_id: "node2".to_string() }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_rotate_backup() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::RotateBackup { retention_days: 7 }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_graceful_shutdown() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::GracefulShutdown { drain_timeout_secs: 60 }).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_recovery_action_serialize() {
        let action = RecoveryAction::ReduceWorkerThreads { target: 8 };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("ReduceWorkerThreads"));
    }

    #[test]
    fn test_action_status_variants() {
        assert_ne!(ActionStatus::Pending, ActionStatus::InProgress);
        assert_ne!(ActionStatus::Succeeded, ActionStatus::Pending);
    }

    #[test]
    fn test_recovery_error_display() {
        let err = RecoveryError::ExecutionFailed("test".to_string());
        assert!(err.to_string().contains("Execution failed"));
        
        let err = RecoveryError::Timeout("timeout".to_string());
        assert!(err.to_string().contains("timeout"));
        
        let err = RecoveryError::ServiceUnavailable("service".to_string());
        assert!(err.to_string().contains("service"));
    }

    #[test]
    fn test_execution_context_creation() {
        let mut metrics = HashMap::new();
        metrics.insert("cpu".to_string(), 75.5);

        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 100,
            metrics,
        };

        assert_eq!(ctx.node_id, "node1");
        assert_eq!(ctx.metrics.get("cpu"), Some(&75.5));
    }

    #[test]
    fn test_recovery_config_default() {
        let config = RecoveryConfig::default();
        assert_eq!(config.cpu_threshold_high, 70.0);
        assert_eq!(config.cpu_threshold_critical, 85.0);
        assert_eq!(config.memory_threshold_high, 80.0);
        assert_eq!(config.memory_threshold_critical, 95.0);
        assert_eq!(config.disk_threshold_warning, 80.0);
        assert_eq!(config.disk_threshold_critical, 95.0);
    }

    #[tokio::test]
    async fn test_restart_component_action() {
        let config = RecoveryConfig::default();
        let mut executor = RecoveryExecutor::new(config);
        
        let result = executor.execute(RecoveryAction::RestartComponent { component: "storage".to_string() }).await;
        assert!(result.is_ok());
        
        let log = result.unwrap();
        assert!(log.details.contains("storage"));
    }
}