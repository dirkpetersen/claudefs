//! Device timeout handling with retry logic and degradation tracking.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock};
use serde::Serialize;

use crate::nvme_passthrough::QueuePairId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandType {
    Read,
    Write,
    Flush,
    Deallocate,
}

impl Default for CommandType {
    fn default() -> Self {
        Self::Read
    }
}

#[derive(Debug, Clone)]
pub struct OpMetadata {
    pub cmd_id: u64,
    pub submitted_at: u64,
    pub retry_count: u32,
    pub op_type: CommandType,
}

#[derive(Debug, Clone)]
pub struct TimedOutOp {
    pub op_id: u64,
    pub metadata: OpMetadata,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TimeoutStats {
    pub pending_ops: u64,
    pub timeout_count: u64,
    pub retry_count: u64,
    pub degraded_count: u64,
    pub p99_latency_ms: u64,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: Vec<u64>,
    pub degradation_threshold: u32,
    pub degradation_window_s: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            max_retries: 3,
            retry_backoff_ms: vec![50, 100, 200, 500],
            degradation_threshold: 3,
            degradation_window_s: 60,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TimeoutError {
    #[error("Operation {0} exceeded max retries")]
    MaxRetriesExceeded(u64),
    #[error("Operation {0} not found")]
    OpNotFound(u64),
    #[error("Device {:?} is degraded", .0)]
    DeviceDegraded(QueuePairId),
    #[error("Invalid backoff index {0}")]
    InvalidBackoffIndex(usize),
}

#[derive(Debug)]
pub struct TimeoutHandler {
    qp_id: QueuePairId,
    pending_ops: Arc<DashMap<u64, OpMetadata>>,
    completed_latencies: Arc<RwLock<VecDeque<u64>>>,
    timeout_counts: Arc<RwLock<VecDeque<u64>>>,
    degraded: RwLock<bool>,
    stats: Mutex<TimeoutStats>,
    config: TimeoutConfig,
    op_counter: RwLock<u64>,
    start_time: Instant,
}

impl TimeoutHandler {
    pub fn new(qp_id: QueuePairId, config: TimeoutConfig) -> Self {
        Self {
            qp_id,
            pending_ops: Arc::new(DashMap::new()),
            completed_latencies: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            timeout_counts: Arc::new(RwLock::new(VecDeque::with_capacity(60))),
            degraded: RwLock::new(false),
            stats: Mutex::new(TimeoutStats::default()),
            config,
            op_counter: RwLock::new(0),
            start_time: Instant::now(),
        }
    }

    pub async fn track(&self, op_type: CommandType) -> u64 {
        let mut counter = self.op_counter.write().await;
        *counter += 1;
        let op_id = *counter;

        let metadata = OpMetadata {
            cmd_id: op_id,
            submitted_at: self.start_time.elapsed().as_millis() as u64,
            retry_count: 0,
            op_type,
        };

        self.pending_ops.insert(op_id, metadata);

        let mut stats = self.stats.lock().await;
        stats.pending_ops = self.pending_ops.len() as u64;

        op_id
    }

    pub async fn complete(&self, op_id: u64) -> Result<(), TimeoutError> {
        let metadata = self.pending_ops.remove(&op_id)
            .ok_or(TimeoutError::OpNotFound(op_id))
            .map(|m| m.1)?;

        let now = self.start_time.elapsed().as_millis() as u64;
        let elapsed_ms = now.saturating_sub(metadata.submitted_at);

        let mut latencies = self.completed_latencies.write().await;
        latencies.push_back(elapsed_ms);
        if latencies.len() > 1000 {
            latencies.pop_front();
        }

        let mut stats = self.stats.lock().await;
        stats.pending_ops = self.pending_ops.len() as u64;

        Ok(())
    }

    pub async fn check_timeouts(&self) -> Vec<TimedOutOp> {
        let timeout_duration_ms = self.config.timeout_ms;
        let now = self.start_time.elapsed().as_millis() as u64;
        let mut timed_out = Vec::new();

        let ops_to_check: Vec<(u64, OpMetadata)> = self.pending_ops.iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        for (op_id, metadata) in ops_to_check {
            let elapsed_ms = now.saturating_sub(metadata.submitted_at);
            
            if elapsed_ms > timeout_duration_ms {
                self.pending_ops.remove(&op_id);
                
                timed_out.push(TimedOutOp {
                    op_id,
                    metadata,
                    elapsed_ms,
                });

                let mut stats = self.stats.lock().await;
                stats.timeout_count += 1;
                stats.pending_ops = self.pending_ops.len() as u64;
            }
        }

        if !timed_out.is_empty() {
            self.update_degradation_status().await;
        }

        timed_out
    }

    pub fn get_backoff_delay(&self, retry_count: u32) -> Duration {
        let idx = retry_count as usize;
        if idx < self.config.retry_backoff_ms.len() {
            Duration::from_millis(self.config.retry_backoff_ms[idx])
        } else {
            Duration::from_millis(*self.config.retry_backoff_ms.last().unwrap_or(&500))
        }
    }

    pub async fn is_degraded(&self) -> bool {
        *self.degraded.read().await
    }

    pub async fn stats(&self) -> TimeoutStats {
        let mut stats = self.stats.lock().await;
        
        let latencies = self.completed_latencies.read().await;
        let latency_vec: Vec<u64> = latencies.iter().copied().collect();
        
        stats.avg_latency_ms = if latency_vec.is_empty() {
            0
        } else {
            latency_vec.iter().sum::<u64>() / latency_vec.len() as u64
        };
        
        stats.p99_latency_ms = Self::calculate_p99(&latency_vec);
        
        stats.clone()
    }

    pub async fn reset(&self) {
        self.pending_ops.clear();
        
        {
            let mut latencies = self.completed_latencies.write().await;
            latencies.clear();
        }
        
        {
            let mut timeout_counts = self.timeout_counts.write().await;
            timeout_counts.clear();
        }
        
        {
            let mut degraded = self.degraded.write().await;
            *degraded = false;
        }
        
        {
            let mut stats = self.stats.lock().await;
            *stats = TimeoutStats::default();
        }
        
        {
            let mut counter = self.op_counter.write().await;
            *counter = 0;
        }
    }

    fn calculate_p99(latencies: &[u64]) -> u64 {
        if latencies.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = latencies.iter().copied().collect();
        sorted.sort();
        let idx = ((sorted.len() as f64) * 0.99) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    async fn update_degradation_status(&self) {
        let now = self.start_time.elapsed().as_secs() as u64;
        
        let mut timeout_counts = self.timeout_counts.write().await;
        timeout_counts.push_back(now);
        
        if timeout_counts.len() > self.config.degradation_window_s as usize {
            timeout_counts.pop_front();
        }

        let window_start = now.saturating_sub(self.config.degradation_window_s);
        let recent_timeouts = timeout_counts.iter()
            .filter(|&&t| t >= window_start)
            .count() as u32;

        let mut degraded = self.degraded.write().await;
        
        if recent_timeouts >= self.config.degradation_threshold {
            if !*degraded {
                let mut stats = self.stats.lock().await;
                stats.degraded_count += 1;
            }
            *degraded = true;
        } else {
            *degraded = false;
        }
    }

    pub async fn pending_count(&self) -> u64 {
        self.pending_ops.len() as u64
    }

    pub fn qp_id(&self) -> QueuePairId {
        self.qp_id
    }

    pub async fn clear_old_timeouts(&self, older_than_ms: u64) -> usize {
        let now = self.start_time.elapsed().as_millis() as u64;
        let cutoff = now.saturating_sub(older_than_ms);
        let mut removed = 0;

        let ops_to_check: Vec<u64> = self.pending_ops.iter()
            .map(|entry| *entry.key())
            .collect();

        for op_id in ops_to_check {
            if let Some(metadata) = self.pending_ops.get(&op_id) {
                if metadata.submitted_at < cutoff {
                    self.pending_ops.remove(&op_id);
                    removed += 1;
                }
            }
        }

        let mut stats = self.stats.lock().await;
        stats.pending_ops = self.pending_ops.len() as u64;

        removed
    }

    pub async fn retry_operation(&self, op_id: u64) -> Result<OpMetadata, TimeoutError> {
        let mut metadata = self.pending_ops.get(&op_id)
            .ok_or(TimeoutError::OpNotFound(op_id))?
            .value()
            .clone();

        if metadata.retry_count >= self.config.max_retries {
            return Err(TimeoutError::MaxRetriesExceeded(op_id));
        }

        metadata.retry_count += 1;
        metadata.submitted_at = self.start_time.elapsed().as_millis() as u64;

        self.pending_ops.insert(op_id, metadata.clone());

        let mut stats = self.stats.lock().await;
        stats.retry_count += 1;

        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    fn create_test_handler() -> TimeoutHandler {
        TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 100,
                max_retries: 3,
                retry_backoff_ms: vec![10, 20, 50],
                degradation_threshold: 3,
                degradation_window_s: 1,
            },
        )
    }

    #[tokio::test]
    async fn test_track_operation() {
        let handler = create_test_handler();
        let op_id = handler.track(CommandType::Read).await;
        assert!(op_id > 0);
        assert_eq!(handler.pending_count().await, 1);
    }

    #[tokio::test]
    async fn test_complete_operation() {
        let handler = create_test_handler();
        let op_id = handler.track(CommandType::Write).await;
        
        handler.complete(op_id).await.unwrap();
        
        let stats = handler.stats().await;
        assert!(stats.avg_latency_ms > 0);
    }

    #[tokio::test]
    async fn test_timeout_detection() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 10,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        );
        
        let _op_id = handler.track(CommandType::Read).await;
        
        tokio::time::sleep(Duration::from_millis(15)).await;
        
        let timeouts = handler.check_timeouts().await;
        assert_eq!(timeouts.len(), 1);
    }

    #[tokio::test]
    async fn test_retry_logic() {
        let handler = create_test_handler();
        
        let op_id = handler.track(CommandType::Read).await;
        
        let _ = handler.retry_operation(op_id).await;
        
        let result = handler.retry_operation(op_id).await;
        assert!(result.is_ok());
        
        let result2 = handler.retry_operation(op_id).await;
        assert!(result2.is_ok());
        
        let result3 = handler.retry_operation(op_id).await;
        assert!(result3.is_err());
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let handler = create_test_handler();
        
        assert_eq!(handler.get_backoff_delay(0), Duration::from_millis(10));
        assert_eq!(handler.get_backoff_delay(1), Duration::from_millis(20));
        assert_eq!(handler.get_backoff_delay(2), Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 100,
                max_retries: 1,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        );
        
        let op_id = handler.track(CommandType::Read).await;
        
        let _ = handler.retry_operation(op_id).await.unwrap();
        
        let result = handler.retry_operation(op_id).await;
        assert!(matches!(result, Err(TimeoutError::MaxRetriesExceeded(_))));
    }

    #[tokio::test]
    async fn test_degradation_threshold() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 10,
                max_retries: 1,
                retry_backoff_ms: vec![10],
                degradation_threshold: 3,
                degradation_window_s: 1,
            },
        );
        
        for _ in 0..3 {
            let op_id = handler.track(CommandType::Read).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
            handler.check_timeouts().await;
        }
        
        assert!(handler.is_degraded().await);
    }

    #[tokio::test]
    async fn test_concurrent_ops_tracking() {
        let handler = Arc::new(create_test_handler());
        
        let mut handles = vec![];
        
        for i in 0..5 {
            let handler = Arc::clone(&handler);
            let handle = tokio::spawn(async move {
                handler.track(CommandType::Read).await
            });
            handles.push(handle);
        }
        
        let mut op_ids = vec![];
        for handle in handles {
            op_ids.push(handle.await.unwrap());
        }
        
        assert_eq!(handler.pending_count().await, 5);
        
        for op_id in op_ids {
            handler.complete(op_id).await.unwrap();
        }
        
        assert_eq!(handler.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_concurrent_ops_timeout() {
        let handler = Arc::new(TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 20,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        ));
        
        let mut handles = vec![];
        
        for _ in 0..3 {
            let handler = Arc::clone(&handler);
            let handle = tokio::spawn(async move {
                handler.track(CommandType::Write).await
            });
            handles.push(handle);
        }
        
        for handle in handles {
            handle.await.unwrap();
        }
        
        tokio::time::sleep(Duration::from_millis(30)).await;
        
        let timeouts = handler.check_timeouts().await;
        assert_eq!(timeouts.len(), 3);
    }

    #[tokio::test]
    async fn test_concurrent_complete_and_timeout() {
        let handler = Arc::new(TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 50,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        ));
        
        let op1 = handler.track(CommandType::Read).await;
        let op2 = handler.track(CommandType::Write).await;
        
        let handler_clone = Arc::clone(&handler);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            handler_clone.complete(op1).await.unwrap();
        });
        
        tokio::time::sleep(Duration::from_millis(60)).await;
        
        let timeouts = handler.check_timeouts().await;
        assert_eq!(timeouts.len(), 1);
        assert_eq!(timeouts[0].op_id, op2);
    }

    #[tokio::test]
    async fn test_concurrent_retry_and_complete() {
        let handler = Arc::new(create_test_handler());
        
        let op_id = handler.track(CommandType::Read).await;
        
        let handler_clone = Arc::clone(&handler);
        let retry_handle = tokio::spawn(async move {
            handler_clone.retry_operation(op_id).await
        });
        
        let handler_clone2 = Arc::clone(&handler);
        let complete_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            handler_clone2.complete(op_id).await
        });
        
        let retry_result = retry_handle.await.unwrap();
        let complete_result = complete_handle.await.unwrap();
        
        assert!(retry_result.is_ok() || complete_result.is_ok());
    }

    #[tokio::test]
    async fn test_concurrent_check_timeouts() {
        let handler = Arc::new(TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 10,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        ));
        
        for _ in 0..10 {
            handler.track(CommandType::Read).await;
        }
        
        let handler_clone = Arc::clone(&handler);
        let handle1 = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            handler_clone.check_timeouts().await
        });
        
        let handler_clone2 = Arc::clone(&handler);
        let handle2 = tokio::spawn(async move {
            handler_clone2.check_timeouts().await
        });
        
        let result1 = handle1.await.unwrap();
        let result2 = handle2.await.unwrap();
        
        assert!(result1.len() == 10 || result2.len() == 10);
    }

    #[tokio::test]
    async fn test_histogram_accuracy() {
        let handler = create_test_handler();
        
        for i in 0..100 {
            let op_id = handler.track(CommandType::Read).await;
            tokio::time::sleep(Duration::from_micros(i * 10)).await;
            handler.complete(op_id).await.unwrap();
        }
        
        let stats = handler.stats().await;
        assert!(stats.avg_latency_ms > 0);
        assert!(stats.p99_latency_ms > stats.avg_latency_ms);
    }

    #[tokio::test]
    async fn test_latency_p99_calculation() {
        let handler = create_test_handler();
        
        for _ in 0..100 {
            let op_id = handler.track(CommandType::Read).await;
            handler.complete(op_id).await.unwrap();
        }
        
        let stats = handler.stats().await;
        assert!(stats.p99_latency_ms > 0);
    }

    #[tokio::test]
    async fn test_recovery_after_timeout() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 10,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 3,
                degradation_window_s: 1,
            },
        );
        
        for _ in 0..3 {
            let op_id = handler.track(CommandType::Read).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
            handler.check_timeouts().await;
        }
        
        assert!(handler.is_degraded().await);
        
        tokio::time::sleep(Duration::from_millis(1100)).await;
        
        let timeouts = handler.check_timeouts().await;
        assert!(timeouts.is_empty());
        
        assert!(!handler.is_degraded().await);
    }

    #[tokio::test]
    async fn test_multiple_devices_independent() {
        let handler1 = TimeoutHandler::new(QueuePairId(0), TimeoutConfig::default());
        let handler2 = TimeoutHandler::new(QueuePairId(1), TimeoutConfig::default());
        
        let op_id1 = handler1.track(CommandType::Read).await;
        let op_id2 = handler2.track(CommandType::Write).await;
        
        handler1.complete(op_id1).await.unwrap();
        
        assert_eq!(handler1.pending_count().await, 0);
        assert_eq!(handler2.pending_count().await, 1);
        
        assert!(!handler1.is_degraded().await);
        assert!(!handler2.is_degraded().await);
    }

    #[tokio::test]
    async fn test_backpressure_on_high_timeout_rate() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 10,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 2,
                degradation_window_s: 1,
            },
        );
        
        for _ in 0..5 {
            let op_id = handler.track(CommandType::Read).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
            handler.check_timeouts().await;
        }
        
        assert!(handler.is_degraded().await);
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = TimeoutConfig::default();
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_backoff_ms, vec![50, 100, 200, 500]);
        assert_eq!(config.degradation_threshold, 3);
        assert_eq!(config.degradation_window_s, 60);
    }

    #[tokio::test]
    async fn test_pending_ops_tracking() {
        let handler = create_test_handler();
        
        assert_eq!(handler.pending_count().await, 0);
        
        let op1 = handler.track(CommandType::Read).await;
        let op2 = handler.track(CommandType::Write).await;
        
        assert_eq!(handler.pending_count().await, 2);
        
        handler.complete(op1).await.unwrap();
        
        assert_eq!(handler.pending_count().await, 1);
        
        handler.complete(op2).await.unwrap();
        
        assert_eq!(handler.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_stats_reset() {
        let handler = create_test_handler();
        
        for _ in 0..5 {
            let op_id = handler.track(CommandType::Read).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            handler.check_timeouts().await;
        }
        
        let stats_before = handler.stats().await;
        assert!(stats_before.timeout_count > 0);
        
        handler.reset().await;
        
        let stats_after = handler.stats().await;
        assert_eq!(stats_after.pending_ops, 0);
        assert_eq!(stats_after.timeout_count, 0);
    }

    #[tokio::test]
    async fn test_pending_count() {
        let handler = create_test_handler();
        
        assert_eq!(handler.pending_count().await, 0);
        
        for i in 0..10 {
            handler.track(CommandType::Read).await;
            assert_eq!(handler.pending_count().await, i + 1);
        }
    }

    #[tokio::test]
    async fn test_clear_old_timeouts() {
        let handler = TimeoutHandler::new(
            QueuePairId(0),
            TimeoutConfig {
                timeout_ms: 100,
                max_retries: 3,
                retry_backoff_ms: vec![10],
                degradation_threshold: 10,
                degradation_window_s: 60,
            },
        );
        
        for _ in 0..5 {
            handler.track(CommandType::Read).await;
        }
        
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        let removed = handler.clear_old_timeouts(50).await;
        assert_eq!(removed, 5);
        assert_eq!(handler.pending_count().await, 0);
    }
}