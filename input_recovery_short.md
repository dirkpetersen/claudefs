# Phase 4 Block 3: recovery_actions.rs

Implement `crates/claudefs-mgmt/src/recovery_actions.rs` (450 lines):

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Action timeout")]
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecoveryAction {
    ReduceWorkerThreads { target: u16 },
    ShrinkMemoryCaches { target_mb: u32 },
    EvictColdData { target_bytes: u64 },
    TriggerEmergencyCleanup,
    RemoveDeadNode { node_id: String },
    RotateBackup { retention_days: u32 },
    GracefulShutdown { drain_timeout_secs: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionStatus {
    Pending,
    InProgress,
    Success,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryLog {
    pub timestamp: u64,
    pub action: RecoveryAction,
    pub status: ActionStatus,
    pub details: String,
}

pub struct RecoveryExecutor {
    logs: Vec<RecoveryLog>,
}

pub struct ExecutionContext {
    pub node_id: String,
    pub timestamp: u64,
    pub metrics: HashMap<String, f64>,
}

impl RecoveryExecutor {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
        }
    }

    pub fn get_logs(&self) -> &[RecoveryLog] {
        &self.logs
    }

    pub fn get_last_action(&self) -> Option<&RecoveryLog> {
        self.logs.last()
    }

    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    async fn log_action(&mut self, action: RecoveryAction, status: ActionStatus, details: String) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.logs.push(RecoveryLog {
            timestamp,
            action,
            status,
            details,
        });
    }

    pub async fn execute(&mut self, action: RecoveryAction, _ctx: &ExecutionContext) -> Result<RecoveryLog, RecoveryError> {
        self.log_action(action.clone(), ActionStatus::InProgress, "Starting".to_string()).await;

        match &action {
            RecoveryAction::ReduceWorkerThreads { target } => {
                self.execute_reduce_workers(*target).await
            },
            RecoveryAction::ShrinkMemoryCaches { target_mb } => {
                self.execute_shrink_memory(*target_mb).await
            },
            RecoveryAction::EvictColdData { target_bytes } => {
                let (_, details) = self.execute_evict_cold_data(*target_bytes).await?;
                self.log_action(action, ActionStatus::Success, details).await;
                Ok(self.logs.last().unwrap().clone())
            },
            RecoveryAction::TriggerEmergencyCleanup => {
                let (_, details) = self.execute_emergency_cleanup().await?;
                self.log_action(action, ActionStatus::Success, details).await;
                Ok(self.logs.last().unwrap().clone())
            },
            RecoveryAction::RemoveDeadNode { node_id } => {
                self.execute_remove_dead_node(node_id).await
            },
            RecoveryAction::RotateBackup { retention_days } => {
                self.execute_rotate_backup(*retention_days).await
            },
            RecoveryAction::GracefulShutdown { drain_timeout_secs } => {
                self.execute_graceful_shutdown(*drain_timeout_secs).await
            },
        }
    }

    async fn execute_reduce_workers(&mut self, target: u16) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Reduced worker threads to {}", target);
        self.log_action(RecoveryAction::ReduceWorkerThreads { target }, ActionStatus::Success, details).await;
        Ok(self.logs.last().unwrap().clone())
    }

    async fn execute_shrink_memory(&mut self, target_mb: u32) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Shrank memory caches to {} MB", target_mb);
        self.log_action(RecoveryAction::ShrinkMemoryCaches { target_mb }, ActionStatus::Success, details).await;
        Ok(self.logs.last().unwrap().clone())
    }

    async fn execute_evict_cold_data(&self, target_bytes: u64) -> Result<(u64, String), RecoveryError> {
        Ok((target_bytes, format!("Evicted {} bytes", target_bytes)))
    }

    async fn execute_emergency_cleanup(&self) -> Result<(u64, String), RecoveryError> {
        Ok((1024, "Emergency cleanup completed".to_string()))
    }

    async fn execute_remove_dead_node(&mut self, node_id: &str) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Removed dead node {}", node_id);
        self.log_action(RecoveryAction::RemoveDeadNode { node_id: node_id.to_string() }, ActionStatus::Success, details).await;
        Ok(self.logs.last().unwrap().clone())
    }

    async fn execute_rotate_backup(&mut self, retention_days: u32) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Rotated backups with {} day retention", retention_days);
        self.log_action(RecoveryAction::RotateBackup { retention_days }, ActionStatus::Success, details).await;
        Ok(self.logs.last().unwrap().clone())
    }

    async fn execute_graceful_shutdown(&mut self, drain_timeout_secs: u64) -> Result<RecoveryLog, RecoveryError> {
        let details = format!("Graceful shutdown with {} second drain timeout", drain_timeout_secs);
        self.log_action(RecoveryAction::GracefulShutdown { drain_timeout_secs }, ActionStatus::Success, details).await;
        Ok(self.logs.last().unwrap().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_recovery_executor_new() {
        let executor = RecoveryExecutor::new();
        assert_eq!(executor.get_logs().len(), 0);
    }

    #[tokio::test]
    async fn test_execute_reduce_workers() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::ReduceWorkerThreads { target: 4 },
            &ctx
        ).await;

        assert!(result.is_ok());
        assert_eq!(executor.get_logs().len(), 2); // InProgress + Success
    }

    #[tokio::test]
    async fn test_execute_shrink_memory() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::ShrinkMemoryCaches { target_mb: 512 },
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_evict_cold_data() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::EvictColdData { target_bytes: 1024 * 1024 },
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_emergency_cleanup() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::TriggerEmergencyCleanup,
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_remove_dead_node() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::RemoveDeadNode { node_id: "node2".to_string() },
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_rotate_backup() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::RotateBackup { retention_days: 7 },
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_graceful_shutdown() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let result = executor.execute(
            RecoveryAction::GracefulShutdown { drain_timeout_secs: 60 },
            &ctx
        ).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_get_logs_empty() {
        let executor = RecoveryExecutor::new();
        assert_eq!(executor.get_logs().len(), 0);
    }

    #[test]
    fn test_get_last_action_none() {
        let executor = RecoveryExecutor::new();
        assert!(executor.get_last_action().is_none());
    }

    #[test]
    fn test_clear_logs() {
        let mut executor = RecoveryExecutor::new();
        executor.logs.push(RecoveryLog {
            timestamp: 0,
            action: RecoveryAction::TriggerEmergencyCleanup,
            status: ActionStatus::Success,
            details: "test".to_string(),
        });
        assert_eq!(executor.get_logs().len(), 1);
        executor.clear_logs();
        assert_eq!(executor.get_logs().len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_actions() {
        let mut executor = RecoveryExecutor::new();
        let ctx = ExecutionContext {
            node_id: "node1".to_string(),
            timestamp: 0,
            metrics: HashMap::new(),
        };

        let _ = executor.execute(RecoveryAction::ReduceWorkerThreads { target: 4 }, &ctx).await;
        let _ = executor.execute(RecoveryAction::ShrinkMemoryCaches { target_mb: 256 }, &ctx).await;

        assert!(executor.get_logs().len() >= 2);
    }

    #[test]
    fn test_recovery_action_serialize() {
        let action = RecoveryAction::ReduceWorkerThreads { target: 8 };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("ReduceWorkerThreads"));
    }

    #[test]
    fn test_recovery_log_serialize() {
        let log = RecoveryLog {
            timestamp: 12345,
            action: RecoveryAction::TriggerEmergencyCleanup,
            status: ActionStatus::Success,
            details: "test cleanup".to_string(),
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("12345"));
    }

    #[test]
    fn test_action_status_variants() {
        assert_ne!(ActionStatus::Pending, ActionStatus::InProgress);
        assert_ne!(ActionStatus::Success, ActionStatus::Pending);
    }

    #[test]
    fn test_recovery_error_display() {
        let err = RecoveryError::ExecutionFailed("test".to_string());
        assert!(err.to_string().contains("Execution failed"));
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
}
```

Compile with `cargo build -p claudefs-mgmt`, all tests must pass with `#[tokio::test]`.
