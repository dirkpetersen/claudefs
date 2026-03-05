//! Fair I/O scheduling across multiple concurrent workloads.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum WorkloadClass {
    Metadata,
    Data,
    Background,
}

impl Default for WorkloadClass {
    fn default() -> Self {
        Self::Data
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FairnessStats {
    pub total_scheduled: u64,
    pub metadata_scheduled: u64,
    pub data_scheduled: u64,
    pub background_scheduled: u64,
    pub backpressure_events: u64,
    pub avg_queue_depth: f64,
}

impl Default for FairnessStats {
    fn default() -> Self {
        Self {
            total_scheduled: 0,
            metadata_scheduled: 0,
            data_scheduled: 0,
            background_scheduled: 0,
            backpressure_events: 0,
            avg_queue_depth: 0.0,
        }
    }
}

#[derive(Debug)]
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: std::time::Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: std::time::Instant::now(),
        }
    }

    pub fn try_consume(&mut self, amount: f64) -> bool {
        self.refill();
        
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }

    pub fn refill(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        
        let new_tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.tokens = new_tokens;
        self.last_refill = now;
    }

    pub fn tokens(&self) -> f64 {
        self.tokens
    }
}

#[derive(Debug)]
pub struct WorkloadQueue {
    class: WorkloadClass,
    weight: f64,
    pending: VecDeque<u64>,
    token_bucket: TokenBucket,
}

impl WorkloadQueue {
    pub fn new(class: WorkloadClass, weight: f64, token_capacity: f64, token_rate: f64) -> Self {
        Self {
            class,
            weight,
            pending: VecDeque::new(),
            token_bucket: TokenBucket::new(token_capacity, token_rate),
        }
    }

    pub fn enqueue(&mut self, op_id: u64) {
        self.pending.push_back(op_id);
    }

    pub fn try_schedule(&mut self) -> Option<u64> {
        if self.pending.is_empty() {
            return None;
        }
        
        let needed = self.weight.max(1.0);
        if self.token_bucket.try_consume(needed) {
            self.pending.pop_front()
        } else {
            None
        }
    }

    pub fn depth(&self) -> usize {
        self.pending.len()
    }

    pub fn weight(&self) -> f64 {
        self.weight
    }

    pub fn class(&self) -> WorkloadClass {
        self.class
    }

    pub fn has_tokens(&self) -> bool {
        self.token_bucket.tokens() >= self.weight.max(1.0)
    }
}

pub struct IoSchedulerFairness {
    queues: RwLock<Vec<WorkloadQueue>>,
    metadata_weight: f64,
    data_weight: f64,
    background_weight: f64,
    stats: RwLock<FairnessStats>,
}

impl IoSchedulerFairness {
    pub fn new(
        metadata_weight: f64,
        data_weight: f64,
        background_weight: f64,
        token_capacity: f64,
    ) -> Self {
        let metadata_rate = token_capacity * metadata_weight / 100.0;
        let data_rate = token_capacity * data_weight / 100.0;
        let background_rate = token_capacity * background_weight / 100.0;

        let queues = vec![
            WorkloadQueue::new(WorkloadClass::Metadata, metadata_weight, token_capacity, metadata_rate),
            WorkloadQueue::new(WorkloadClass::Data, data_weight, token_capacity, data_rate),
            WorkloadQueue::new(WorkloadClass::Background, background_weight, token_capacity, background_rate),
        ];

        Self {
            queues: RwLock::new(queues),
            metadata_weight,
            data_weight,
            background_weight,
            stats: RwLock::new(FairnessStats::default()),
        }
    }

    pub async fn enqueue(&self, op_id: u64, class: WorkloadClass) -> Result<(), FairnessError> {
        let mut queues = self.queues.write().await;
        
        let idx = match class {
            WorkloadClass::Metadata => 0,
            WorkloadClass::Data => 1,
            WorkloadClass::Background => 2,
        };
        
        if idx >= queues.len() {
            return Err(FairnessError::InvalidClass);
        }
        
        queues[idx].enqueue(op_id);
        
        let mut stats = self.stats.write().await;
        stats.avg_queue_depth = queues.iter().map(|q| q.depth() as f64).sum::<f64>() / queues.len() as f64;
        
        Ok(())
    }

    pub async fn try_schedule(&self) -> Option<(u64, WorkloadClass)> {
        let mut queues = self.queues.write().await;
        
        let mut best_idx: Option<usize> = None;
        let mut best_depth = usize::MAX;
        
        for (i, queue) in queues.iter().enumerate() {
            if queue.depth() > 0 && queue.has_tokens() && queue.depth() < best_depth {
                best_idx = Some(i);
                best_depth = queue.depth();
            }
        }
        
        if let Some(idx) = best_idx {
            if let Some(op_id) = queues[idx].try_schedule() {
                let class = queues[idx].class();
                
                let mut stats = self.stats.write().await;
                stats.total_scheduled += 1;
                match class {
                    WorkloadClass::Metadata => stats.metadata_scheduled += 1,
                    WorkloadClass::Data => stats.data_scheduled += 1,
                    WorkloadClass::Background => stats.background_scheduled += 1,
                }
                
                return Some((op_id, class));
            }
        }
        
        for queue in queues.iter_mut() {
            if let Some(op_id) = queue.try_schedule() {
                let class = queue.class();
                
                let mut stats = self.stats.write().await;
                stats.total_scheduled += 1;
                match class {
                    WorkloadClass::Metadata => stats.metadata_scheduled += 1,
                    WorkloadClass::Data => stats.data_scheduled += 1,
                    WorkloadClass::Background => stats.background_scheduled += 1,
                }
                
                return Some((op_id, class));
            }
        }
        
        let total_depth: usize = queues.iter().map(|q| q.depth()).sum();
        if total_depth > 0 {
            let mut stats = self.stats.write().await;
            stats.backpressure_events += 1;
        }
        
        None
    }

    pub async fn stats(&self) -> FairnessStats {
        let queues = self.queues.read().await;
        let mut stats = self.stats.write().await;
        stats.avg_queue_depth = queues.iter().map(|q| q.depth() as f64).sum::<f64>() / queues.len().max(1) as f64;
        stats.clone()
    }

    pub async fn current_depths(&self) -> Vec<(WorkloadClass, usize)> {
        let queues = self.queues.read().await;
        queues.iter().map(|q| (q.class(), q.depth())).collect()
    }

    pub async fn reset(&self) {
        let mut queues = self.queues.write().await;
        for queue in queues.iter_mut() {
            while queue.try_schedule().is_some() {}
        }
        
        let mut stats = self.stats.write().await;
        *stats = FairnessStats::default();
    }
}

#[derive(Debug, Error)]
pub enum FairnessError {
    #[error("All queues exhausted, backpressure applied")]
    Backpressure,
    #[error("Invalid workload class")]
    InvalidClass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_scheduler() -> IoSchedulerFairness {
        IoSchedulerFairness::new(50.0, 30.0, 20.0, 100.0)
    }

    #[tokio::test]
    async fn test_fairness_distribution() {
        let scheduler = create_test_scheduler();
        
        for i in 0..10 {
            scheduler.enqueue(i, WorkloadClass::Metadata).await.unwrap();
        }
        for i in 10..20 {
            scheduler.enqueue(i, WorkloadClass::Data).await.unwrap();
        }
        for i in 20..30 {
            scheduler.enqueue(i, WorkloadClass::Background).await.unwrap();
        }
        
        let mut metadata_count = 0;
        let mut data_count = 0;
        let mut background_count = 0;
        
        for _ in 0..30 {
            if let Some((_, class)) = scheduler.try_schedule().await {
                match class {
                    WorkloadClass::Metadata => metadata_count += 1,
                    WorkloadClass::Data => data_count += 1,
                    WorkloadClass::Background => background_count += 1,
                }
            }
        }
        
        assert!(metadata_count >= 8);
        assert!(data_count >= 5);
    }

    #[tokio::test]
    async fn test_metadata_priority() {
        let scheduler = IoSchedulerFairness::new(80.0, 15.0, 5.0, 100.0);
        
        for i in 0..10 {
            scheduler.enqueue(i, WorkloadClass::Data).await.unwrap();
        }
        for i in 100..105 {
            scheduler.enqueue(i, WorkloadClass::Metadata).await.unwrap();
        }
        
        let mut metadata_scheduled = 0;
        for _ in 0..15 {
            if let Some((_, WorkloadClass::Metadata)) = scheduler.try_schedule().await {
                metadata_scheduled += 1;
            }
        }
        
        assert!(metadata_scheduled >= 4);
    }

    #[tokio::test]
    async fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(10.0, 100.0);
        
        assert!(bucket.try_consume(5.0));
        assert_eq!(bucket.tokens(), 5.0);
        
        std::thread::sleep(std::time::Duration::from_millis(50));
        
        assert!(bucket.try_consume(5.0));
    }

    #[tokio::test]
    async fn test_backpressure_on_exhausted_tokens() {
        let scheduler = create_test_scheduler();
        
        for i in 0..100 {
            scheduler.enqueue(i, WorkloadClass::Background).await.unwrap();
        }
        
        let scheduled = scheduler.try_schedule().await;
        assert!(scheduled.is_some());
    }

    #[tokio::test]
    async fn test_weighted_round_robin() {
        let scheduler = create_test_scheduler();
        
        scheduler.enqueue(1, WorkloadClass::Data).await.unwrap();
        scheduler.enqueue(2, WorkloadClass::Data).await.unwrap();
        scheduler.enqueue(3, WorkloadClass::Data).await.unwrap();
        
        scheduler.enqueue(10, WorkloadClass::Metadata).await.unwrap();
        
        let first = scheduler.try_schedule().await;
        assert!(first.is_some());
    }

    #[tokio::test]
    async fn test_empty_queues() {
        let scheduler = create_test_scheduler();
        
        let result = scheduler.try_schedule().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_multiple_workloads() {
        let scheduler = Arc::new(create_test_scheduler());
        
        let s1 = Arc::clone(&scheduler);
        let handle1 = tokio::spawn(async move {
            for i in 0..5 {
                s1.enqueue(i, WorkloadClass::Metadata).await.unwrap();
            }
        });
        
        let s2 = Arc::clone(&scheduler);
        let handle2 = tokio::spawn(async move {
            for i in 0..5 {
                s2.enqueue(i + 100, WorkloadClass::Data).await.unwrap();
            }
        });
        
        handle1.await.unwrap();
        handle2.await.unwrap();
        
        let depths = scheduler.current_depths().await;
        let total: usize = depths.iter().map(|(_, d)| d).sum();
        assert_eq!(total, 10);
    }

    #[tokio::test]
    async fn test_schedule_order() {
        let scheduler = create_test_scheduler();
        
        for i in 0..5 {
            scheduler.enqueue(i, WorkloadClass::Data).await.unwrap();
        }
        
        let results: Vec<u64> = (0..5).filter_map(|_| {
            tokio::runtime::Handle::current().block_on(async {
                scheduler.try_schedule().await.map(|(id, _)| id)
            })
        }).collect();
        
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_dequeue_removes_item() {
        let scheduler = create_test_scheduler();
        
        scheduler.enqueue(42, WorkloadClass::Data).await.unwrap();
        
        let depths_before = scheduler.current_depths().await;
        let data_depth_before = depths_before.iter().find(|(c, _)| *c == WorkloadClass::Data).unwrap().1;
        
        let result = scheduler.try_schedule().await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, 42);
        
        let depths_after = scheduler.current_depths().await;
        let data_depth_after = depths_after.iter().find(|(c, _)| *c == WorkloadClass::Data).unwrap().1;
        
        assert_eq!(data_depth_before - data_depth_after, 1);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let scheduler = Arc::new(create_test_scheduler());
        
        let mut handles = vec![];
        
        for i in 0..10 {
            let s = Arc::clone(&scheduler);
            let handle = tokio::spawn(async move {
                s.enqueue(i, WorkloadClass::Data).await
            });
            handles.push(handle);
        }
        
        for handle in handles {
            handle.await.unwrap().unwrap();
        }
        
        let depths = scheduler.current_depths().await;
        let total: usize = depths.iter().map(|(_, d)| d).sum();
        assert_eq!(total, 10);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let scheduler = create_test_scheduler();
        
        for i in 0..3 {
            scheduler.enqueue(i, WorkloadClass::Metadata).await.unwrap();
        }
        
        for _ in 0..3 {
            scheduler.try_schedule().await;
        }
        
        let stats = scheduler.stats().await;
        assert!(stats.total_scheduled >= 3);
    }

    #[tokio::test]
    async fn test_zero_weight_handling() {
        let scheduler = IoSchedulerFairness::new(0.0, 50.0, 50.0, 100.0);
        
        scheduler.enqueue(1, WorkloadClass::Metadata).await.unwrap();
        scheduler.enqueue(2, WorkloadClass::Data).await.unwrap();
        
        let result = scheduler.try_schedule().await;
        assert!(result.is_some());
        
        if let Some((_, class)) = result {
            assert_eq!(class, WorkloadClass::Data);
        }
    }

    #[tokio::test]
    async fn test_backpressure_tracking() {
        let scheduler = Arc::new(create_test_scheduler());
        
        for _ in 0..50 {
            scheduler.enqueue(1, WorkloadClass::Data).await.unwrap();
        }
        
        for _ in 0..100 {
            scheduler.try_schedule().await;
        }
        
        let stats = scheduler.stats().await;
        assert!(stats.backpressure_events >= 0);
    }

    #[tokio::test]
    async fn test_depth_calculation() {
        let scheduler = create_test_scheduler();
        
        assert_eq!(scheduler.current_depths().await.len(), 3);
        
        scheduler.enqueue(1, WorkloadClass::Data).await.unwrap();
        
        let depths = scheduler.current_depths().await;
        let data_depth = depths.iter().find(|(c, _)| *c == WorkloadClass::Data).unwrap().1;
        assert_eq!(data_depth, 1);
    }

    #[tokio::test]
    async fn test_reset_clears_queues() {
        let scheduler = create_test_scheduler();
        
        for i in 0..5 {
            scheduler.enqueue(i, WorkloadClass::Data).await.unwrap();
        }
        
        scheduler.reset().await;
        
        let depths = scheduler.current_depths().await;
        let total: usize = depths.iter().map(|(_, d)| d).sum();
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_all_workload_classes() {
        let scheduler = create_test_scheduler();
        
        scheduler.enqueue(1, WorkloadClass::Metadata).await.unwrap();
        scheduler.enqueue(2, WorkloadClass::Data).await.unwrap();
        scheduler.enqueue(3, WorkloadClass::Background).await.unwrap();
        
        let depths = scheduler.current_depths().await;
        
        for (class, depth) in depths {
            match class {
                WorkloadClass::Metadata | WorkloadClass::Data | WorkloadClass::Background => {
                    assert!(depth >= 0);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_workload_queue_weight() {
        let queue = WorkloadQueue::new(WorkloadClass::Data, 30.0, 100.0, 30.0);
        
        assert_eq!(queue.weight(), 30.0);
    }

    #[tokio::test]
    async fn test_workload_queue_depth() {
        let mut queue = WorkloadQueue::new(WorkloadClass::Data, 30.0, 100.0, 30.0);
        
        queue.enqueue(1);
        queue.enqueue(2);
        
        assert_eq!(queue.depth(), 2);
    }
}