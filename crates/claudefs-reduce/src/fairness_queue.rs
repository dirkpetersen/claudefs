//! Fairness Queue for weighted tenant write scheduling.
//!
//! Provides priority-based queueing that prevents tenant starvation.

use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::tenant_isolator::TenantId;
use crate::error::ReduceError;

/// Configuration for fairness queue behavior.
pub struct FairnessQueueConfig {
    /// Maximum number of writes in queue (default 10000)
    pub max_queue_depth: usize,
    /// Batch timeout in milliseconds (default 100)
    pub batch_timeout_ms: u64,
    /// Priority boost percentage for high-utilization tenants (default 10%)
    pub priority_boost_percent: f64,
}

impl Default for FairnessQueueConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 10000,
            batch_timeout_ms: 100,
            priority_boost_percent: 10.0,
        }
    }
}

/// A queued write operation with priority metadata.
#[derive(Debug, Clone)]
pub struct QueuedWrite {
    /// Tenant identifier
    pub tenant_id: TenantId,
    /// Size of the write in bytes
    pub write_size: u64,
    /// Priority value (higher % quota used = lower priority = need fairness)
    pub priority: f64,
    /// Time when the write was enqueued
    pub enqueued_at: Instant,
    /// Unique identifier for this write
    pub write_id: u64,
}

impl Ord for QueuedWrite {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.partial_cmp(&other.priority).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialOrd for QueuedWrite {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for QueuedWrite {}

impl PartialEq for QueuedWrite {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.write_id == other.write_id
    }
}

/// Queue metrics for monitoring.
#[derive(Debug, Default, Clone)]
pub struct QueueMetrics {
    /// Total writes enqueued since creation
    pub total_enqueued: u64,
    /// Total writes dequeued since creation
    pub total_dequeued: u64,
    /// Total writes that expired without being processed
    pub total_expired: u64,
    /// Current number of items in queue
    pub current_depth: usize,
}

/// Fairness queue for managing concurrent tenant writes with weighted priority.
pub struct FairnessQueue {
    queue: Arc<Mutex<BinaryHeap<QueuedWrite>>>,
    config: FairnessQueueConfig,
    metrics: Arc<Mutex<QueueMetrics>>,
    tenant_counts: Arc<Mutex<HashMap<TenantId, usize>>>,
    write_counter: Arc<Mutex<u64>>,
}

impl FairnessQueue {
    /// Create a new FairnessQueue with the given configuration.
    pub fn new(config: FairnessQueueConfig) -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            config,
            metrics: Arc::new(Mutex::new(QueueMetrics::default())),
            tenant_counts: Arc::new(Mutex::new(HashMap::new())),
            write_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Enqueue a write operation for a tenant.
    pub async fn enqueue(&self, tenant_id: TenantId, write_size: u64, priority: f64) -> Result<(), ReduceError> {
        let mut metrics = self.metrics.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        if metrics.current_depth >= self.config.max_queue_depth {
            return Err(ReduceError::InvalidInput("Queue at max capacity".to_string()));
        }

        let mut counter = self.write_counter.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        *counter += 1;
        let write_id = *counter;

        let write = QueuedWrite {
            tenant_id,
            write_size,
            priority,
            enqueued_at: Instant::now(),
            write_id,
        };

        let tenant_id = write.tenant_id;

        let mut tenant_counts = self.tenant_counts.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        *tenant_counts.entry(tenant_id).or_insert(0) += 1;

        let mut queue = self.queue.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        queue.push(write);
        
        metrics.total_enqueued += 1;
        metrics.current_depth = queue.len();
        
        debug!("Enqueued write for tenant {:?}, depth={}", tenant_id, metrics.current_depth);
        Ok(())
    }

    /// Dequeue the highest priority write operation.
    pub async fn dequeue(&self) -> Result<Option<QueuedWrite>, ReduceError> {
        let mut queue = self.queue.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        let mut metrics = self.metrics.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        let mut tenant_counts = self.tenant_counts.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;

        if queue.is_empty() {
            return Ok(None);
        }

        let now = Instant::now();
        let mut expired = Vec::new();
        
        while let Some(write) = queue.pop() {
            let age = now.duration_since(write.enqueued_at);
            if age > Duration::from_secs(600) {
                expired.push(write);
                continue;
            }
            
            *tenant_counts.entry(write.tenant_id).or_insert(0) = tenant_counts.get(&write.tenant_id).map(|c| c - 1).unwrap_or(0);
            
            metrics.total_dequeued += 1;
            metrics.current_depth = queue.len();
            
            for e in expired {
                queue.push(e);
            }
            
            return Ok(Some(write));
        }

        metrics.total_expired += expired.len() as u64;
        metrics.current_depth = queue.len();
        
        Ok(None)
    }

    /// Get the number of writes queued for a specific tenant.
    pub async fn get_queue_depth(&self, tenant_id: TenantId) -> Result<usize, ReduceError> {
        let tenant_counts = self.tenant_counts.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        Ok(*tenant_counts.get(&tenant_id).unwrap_or(&0))
    }

    /// Get the total number of writes in the queue.
    pub async fn get_total_depth(&self) -> Result<usize, ReduceError> {
        let queue = self.queue.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        Ok(queue.len())
    }

    /// Get current queue metrics.
    pub fn get_metrics(&self) -> QueueMetrics {
        self.metrics.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Clear all writes for a specific tenant.
    pub async fn clear_tenant(&self, tenant_id: TenantId) -> Result<(), ReduceError> {
        let mut queue = self.queue.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        let mut tenant_counts = self.tenant_counts.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let mut remaining = BinaryHeap::new();
        while let Some(write) = queue.pop() {
            if write.tenant_id == tenant_id {
                continue;
            }
            remaining.push(write);
        }
        
        *queue = remaining;
        tenant_counts.remove(&tenant_id);
        
        let mut metrics = self.metrics.lock().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        metrics.current_depth = queue.len();
        
        info!("Cleared queue for tenant {:?}", tenant_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 1000, 50.0).await.unwrap();
        
        let result = queue.dequeue().await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().tenant_id, TenantId(1));
    }

    #[tokio::test]
    async fn test_empty_dequeue() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        let result = queue.dequeue().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 1000, 90.0).await.unwrap();
        queue.enqueue(TenantId(2), 1000, 10.0).await.unwrap();
        
        let result = queue.dequeue().await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().tenant_id, TenantId(2));
    }

    #[tokio::test]
    async fn test_tenant_queue_depth() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 1000, 50.0).await.unwrap();
        queue.enqueue(TenantId(1), 1000, 50.0).await.unwrap();
        
        let depth = queue.get_queue_depth(TenantId(1)).await.unwrap();
        assert_eq!(depth, 2);
    }

    #[tokio::test]
    async fn test_clear_tenant() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 1000, 50.0).await.unwrap();
        queue.enqueue(TenantId(2), 1000, 50.0).await.unwrap();
        
        queue.clear_tenant(TenantId(1)).await.unwrap();
        
        let depth = queue.get_queue_depth(TenantId(1)).await.unwrap();
        assert_eq!(depth, 0);
        
        let total = queue.get_total_depth().await.unwrap();
        assert_eq!(total, 1);
    }

    #[tokio::test]
    async fn test_max_capacity() {
        let mut config = FairnessQueueConfig::default();
        config.max_queue_depth = 2;
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 100, 50.0).await.unwrap();
        queue.enqueue(TenantId(2), 100, 50.0).await.unwrap();
        
        let result = queue.enqueue(TenantId(3), 100, 50.0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metrics() {
        let config = FairnessQueueConfig::default();
        let queue = FairnessQueue::new(config);
        
        queue.enqueue(TenantId(1), 100, 50.0).await.unwrap();
        queue.dequeue().await.unwrap();
        
        let metrics = queue.get_metrics();
        assert_eq!(metrics.total_enqueued, 1);
        assert_eq!(metrics.total_dequeued, 1);
    }
}