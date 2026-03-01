use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DrainError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Drain already in progress for node: {0}")]
    AlreadyDraining(String),
    #[error("Drain cancelled for node: {0}")]
    Cancelled(String),
    #[error("Insufficient capacity for drain: need {needed} bytes, available {available} bytes")]
    InsufficientCapacity { needed: u64, available: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrainPhase {
    Pending,
    Calculating,
    Migrating,
    Reconstructing,
    AwaitingConnections,
    Complete,
    Cancelled,
    Failed(String),
}

impl DrainPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(self, DrainPhase::Complete | DrainPhase::Cancelled | DrainPhase::Failed(_))
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            DrainPhase::Calculating
                | DrainPhase::Migrating
                | DrainPhase::Reconstructing
                | DrainPhase::AwaitingConnections
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrainProgress {
    pub node_id: String,
    pub phase: DrainPhase,
    pub total_bytes: u64,
    pub migrated_bytes: u64,
    pub total_shards: u64,
    pub migrated_shards: u64,
    pub started_at: u64,
    pub updated_at: u64,
    pub estimated_complete_secs: Option<u64>,
    pub errors: Vec<String>,
}

impl DrainProgress {
    pub fn new(node_id: String, total_bytes: u64, total_shards: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            node_id,
            phase: DrainPhase::Pending,
            total_bytes,
            migrated_bytes: 0,
            total_shards,
            migrated_shards: 0,
            started_at: now,
            updated_at: now,
            estimated_complete_secs: None,
            errors: Vec::new(),
        }
    }

    pub fn percent_complete(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.migrated_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn migration_rate_bps(&self) -> f64 {
        let elapsed = self.updated_at.saturating_sub(self.started_at);
        if elapsed == 0 {
            return 0.0;
        }
        self.migrated_bytes as f64 / elapsed as f64
    }

    pub fn remaining_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.migrated_bytes)
    }

    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    pub fn is_complete(&self) -> bool {
        self.phase == DrainPhase::Complete
    }
}

pub struct DrainManager {
    active_drains: Arc<RwLock<HashMap<String, DrainProgress>>>,
    completed_drains: Arc<RwLock<Vec<DrainProgress>>>,
}

impl DrainManager {
    pub fn new() -> Self {
        Self {
            active_drains: Arc::new(RwLock::new(HashMap::new())),
            completed_drains: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn start_drain(
        &self,
        node_id: String,
        total_bytes: u64,
        total_shards: u64,
    ) -> Result<DrainProgress, DrainError> {
        let mut drains = self.active_drains.write().await;
        if drains.contains_key(&node_id) {
            return Err(DrainError::AlreadyDraining(node_id));
        }
        let progress = DrainProgress::new(node_id.clone(), total_bytes, total_shards);
        drains.insert(node_id, progress.clone());
        Ok(progress)
    }

    pub async fn update_progress(
        &self,
        node_id: &str,
        migrated_bytes: u64,
        migrated_shards: u64,
        phase: DrainPhase,
    ) -> Result<(), DrainError> {
        let mut drains = self.active_drains.write().await;
        let progress = drains
            .get_mut(node_id)
            .ok_or_else(|| DrainError::NodeNotFound(node_id.to_string()))?;
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        progress.migrated_bytes = migrated_bytes;
        progress.migrated_shards = migrated_shards;
        progress.phase = phase;
        progress.updated_at = now;
        
        if progress.total_bytes > 0 && progress.migration_rate_bps() > 0.0 {
            let remaining = progress.remaining_bytes() as f64;
            let rate = progress.migration_rate_bps();
            progress.estimated_complete_secs = Some((remaining / rate) as u64);
        }
        
        Ok(())
    }

    pub async fn complete_drain(&self, node_id: &str) -> Result<DrainProgress, DrainError> {
        let mut drains = self.active_drains.write().await;
        let progress = drains
            .remove(node_id)
            .ok_or_else(|| DrainError::NodeNotFound(node_id.to_string()))?;
        
        let mut completed = self.completed_drains.write().await;
        let mut completed_progress = progress;
        completed_progress.phase = DrainPhase::Complete;
        completed.push(completed_progress.clone());
        
        Ok(completed_progress)
    }

    pub async fn cancel_drain(&self, node_id: &str) -> Result<(), DrainError> {
        let mut drains = self.active_drains.write().await;
        let progress = drains
            .get_mut(node_id)
            .ok_or_else(|| DrainError::NodeNotFound(node_id.to_string()))?;
        
        progress.phase = DrainPhase::Cancelled;
        
        Ok(())
    }

    pub async fn get_progress(&self, node_id: &str) -> Option<DrainProgress> {
        let drains = self.active_drains.read().await;
        drains.get(node_id).cloned()
    }

    pub async fn active_drains(&self) -> Vec<DrainProgress> {
        let drains = self.active_drains.read().await;
        drains.values().cloned().collect()
    }

    pub async fn completed_drains(&self) -> Vec<DrainProgress> {
        let drains = self.completed_drains.read().await;
        drains.clone()
    }

    pub async fn is_draining(&self, node_id: &str) -> bool {
        let drains = self.active_drains.read().await;
        drains.contains_key(node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_drain_phase_terminal() {
        assert!(DrainPhase::Complete.is_terminal());
        assert!(DrainPhase::Cancelled.is_terminal());
        assert!(DrainPhase::Failed("err".to_string()).is_terminal());
        assert!(!DrainPhase::Pending.is_terminal());
        assert!(!DrainPhase::Migrating.is_terminal());
    }

    #[tokio::test]
    async fn test_drain_phase_active() {
        assert!(DrainPhase::Calculating.is_active());
        assert!(DrainPhase::Migrating.is_active());
        assert!(DrainPhase::Reconstructing.is_active());
        assert!(DrainPhase::AwaitingConnections.is_active());
        assert!(!DrainPhase::Complete.is_active());
        assert!(!DrainPhase::Pending.is_active());
    }

    #[tokio::test]
    async fn test_drain_progress_new() {
        let progress = DrainProgress::new("node1".to_string(), 1000, 10);
        assert_eq!(progress.node_id, "node1");
        assert_eq!(progress.phase, DrainPhase::Pending);
        assert_eq!(progress.total_bytes, 1000);
        assert_eq!(progress.migrated_bytes, 0);
    }

    #[tokio::test]
    async fn test_percent_complete_zero() {
        let progress = DrainProgress::new("node1".to_string(), 1000, 10);
        assert_eq!(progress.percent_complete(), 0.0);
    }

    #[tokio::test]
    async fn test_percent_complete_full() {
        let mut progress = DrainProgress::new("node1".to_string(), 1000, 10);
        progress.migrated_bytes = 1000;
        assert_eq!(progress.percent_complete(), 100.0);
    }

    #[tokio::test]
    async fn test_percent_complete_halfway() {
        let mut progress = DrainProgress::new("node1".to_string(), 1000, 10);
        progress.migrated_bytes = 500;
        assert!((progress.percent_complete() - 50.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_migration_rate_no_elapsed() {
        let progress = DrainProgress::new("node1".to_string(), 1000, 10);
        assert_eq!(progress.migration_rate_bps(), 0.0);
    }

    #[tokio::test]
    async fn test_remaining_bytes() {
        let mut progress = DrainProgress::new("node1".to_string(), 1000, 10);
        progress.migrated_bytes = 300;
        assert_eq!(progress.remaining_bytes(), 700);
    }

    #[tokio::test]
    async fn test_add_error() {
        let mut progress = DrainProgress::new("node1".to_string(), 1000, 10);
        progress.add_error("error 1".to_string());
        progress.add_error("error 2".to_string());
        assert_eq!(progress.errors.len(), 2);
    }

    #[tokio::test]
    async fn test_is_complete() {
        let mut progress = DrainProgress::new("node1".to_string(), 1000, 10);
        assert!(!progress.is_complete());
        progress.phase = DrainPhase::Complete;
        assert!(progress.is_complete());
    }

    #[tokio::test]
    async fn test_drain_manager_start_drain() {
        let manager = DrainManager::new();
        let result = manager.start_drain("node1".to_string(), 1000, 10).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_drain_manager_already_draining() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        let result = manager.start_drain("node1".to_string(), 1000, 10).await;
        assert!(matches!(result, Err(DrainError::AlreadyDraining(_))));
    }

    #[tokio::test]
    async fn test_update_progress() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        manager
            .update_progress("node1", 500, 5, DrainPhase::Migrating)
            .await
            .unwrap();
        let progress = manager.get_progress("node1").await.unwrap();
        assert_eq!(progress.migrated_bytes, 500);
        assert_eq!(progress.migrated_shards, 5);
    }

    #[tokio::test]
    async fn test_complete_drain() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        manager
            .update_progress("node1", 1000, 10, DrainPhase::Migrating)
            .await
            .unwrap();
        let result = manager.complete_drain("node1").await;
        assert!(result.is_ok());
        assert!(manager.get_progress("node1").await.is_none());
    }

    #[tokio::test]
    async fn test_cancel_drain() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        manager.cancel_drain("node1").await.unwrap();
        let progress = manager.get_progress("node1").await.unwrap();
        assert_eq!(progress.phase, DrainPhase::Cancelled);
    }

    #[tokio::test]
    async fn test_get_progress_unknown() {
        let manager = DrainManager::new();
        let result = manager.get_progress("unknown").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_active_drains() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        manager.start_drain("node2".to_string(), 2000, 20).await.unwrap();
        let drains = manager.active_drains().await;
        assert_eq!(drains.len(), 2);
    }

    #[tokio::test]
    async fn test_is_draining() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        assert!(manager.is_draining("node1").await);
        assert!(!manager.is_draining("node2").await);
        assert!(!manager.is_draining("unknown").await);
    }

    #[tokio::test]
    async fn test_complete_moves_to_completed() {
        let manager = DrainManager::new();
        manager.start_drain("node1".to_string(), 1000, 10).await.unwrap();
        manager
            .update_progress("node1", 1000, 10, DrainPhase::Migrating)
            .await
            .unwrap();
        manager.complete_drain("node1").await.unwrap();
        let completed = manager.completed_drains().await;
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].node_id, "node1");
    }
}