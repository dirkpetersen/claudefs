//! Request deduplication for identical read requests to avoid redundant I/O.

use std::sync::Arc;
use tokio::sync::RwLock;
use dashmap::DashMap;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct QueuePairId(pub u64);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ReadKey {
    pub qp_id: QueuePairId,
    pub lba: u64,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DedupStats {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub dedup_rate: f64,
}

impl Default for DedupStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            dedup_rate: 0.0,
        }
    }
}

pub struct RequestDeduplicator {
    inflight: Arc<DashMap<ReadKey, Arc<RwLock<Option<Result<Vec<u8>, String>>>>>>,
    stats: Arc<AtomicU64>,
    hit_count: Arc<AtomicU64>,
    miss_count: Arc<AtomicU64>,
}

impl RequestDeduplicator {
    pub fn new() -> Self {
        Self {
            inflight: Arc::new(DashMap::new()),
            stats: Arc::new(AtomicU64::new(0)),
            hit_count: Arc::new(AtomicU64::new(0)),
            miss_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn read_deduplicated<F>(&self, key: ReadKey, mut fetch_fn: F) -> Result<Vec<u8>, String>
    where
        F: FnMut() -> Result<Vec<u8>, String>,
    {
        self.stats.fetch_add(1, Ordering::Relaxed);

        let entry = self.inflight.entry(key).or_insert_with(|| {
            Arc::new(RwLock::new(None))
        });

        {
            let guard = entry.value().read().await;
            if let Some(ref result) = *guard {
                self.hit_count.fetch_add(1, Ordering::Relaxed);
                return result.clone();
            }
        }

        self.miss_count.fetch_add(1, Ordering::Relaxed);

        let mut guard = entry.value().write().await;
        if let Some(ref result) = *guard {
            return result.clone();
        }

        let result = fetch_fn();
        *guard = Some(result.clone());
        
        drop(guard);

        result
    }

    pub fn stats(&self) -> DedupStats {
        let total = self.stats.load(Ordering::Relaxed);
        let hits = self.hit_count.load(Ordering::Relaxed);
        let misses = self.miss_count.load(Ordering::Relaxed);
        
        let dedup_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };

        DedupStats {
            total_requests: total,
            cache_hits: hits,
            cache_misses: misses,
            dedup_rate,
        }
    }

    pub async fn clear(&self) {
        self.inflight.clear();
    }

    pub async fn invalidate(&self, key: &ReadKey) {
        self.inflight.remove(key);
    }

    pub fn pending_count(&self) -> usize {
        self.inflight.len()
    }
}

impl Default for RequestDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_dedup() -> RequestDeduplicator {
        RequestDeduplicator::new()
    }

    #[tokio::test]
    async fn test_no_dedup_when_unique() {
        let dedup = create_test_dedup();
        
        let key1 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let key2 = ReadKey { qp_id: QueuePairId(0), lba: 200, length: 4096 };
        
        let result1 = dedup.read_deduplicated(key1, || Ok(vec![1, 2, 3])).await;
        let result2 = dedup.read_deduplicated(key2, || Ok(vec![4, 5, 6])).await;
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 2);
    }

    #[tokio::test]
    async fn test_dedup_identical_reads() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let result1 = dedup.read_deduplicated(key, || Ok(vec![1, 2, 3, 4, 5])).await;
        let result2 = dedup.read_deduplicated(key, || Ok(vec![])).await;
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap(), result2.unwrap());
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_hits, 1);
    }

    #[tokio::test]
    async fn test_dedup_concurrent_requests() {
        let dedup = Arc::new(create_test_dedup());
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let mut handles = vec![];
        for _ in 0..5 {
            let d = Arc::clone(&dedup);
            let handle = tokio::spawn(async move {
                d.read_deduplicated(key, || Ok(vec![1, 2, 3])).await
            });
            handles.push(handle);
        }
        
        let mut results = vec![];
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        
        for result in &results {
            assert!(result.is_ok());
            assert_eq!(result.as_ref().unwrap(), &vec![1, 2, 3]);
        }
        
        let stats = dedup.stats();
        assert!(stats.cache_hits >= 4);
    }

    #[tokio::test]
    async fn test_different_lbas_no_dedup() {
        let dedup = create_test_dedup();
        
        let key1 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let key2 = ReadKey { qp_id: QueuePairId(0), lba: 200, length: 4096 };
        
        let _ = dedup.read_deduplicated(key1, || Ok(vec![1])).await;
        let _ = dedup.read_deduplicated(key2, || Ok(vec![2])).await;
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 2);
        assert_eq!(stats.cache_hits, 0);
    }

    #[tokio::test]
    async fn test_different_lengths_no_dedup() {
        let dedup = create_test_dedup();
        
        let key1 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let key2 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 8192 };
        
        let _ = dedup.read_deduplicated(key1, || Ok(vec![1])).await;
        let _ = dedup.read_deduplicated(key2, || Ok(vec![2])).await;
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 2);
    }

    #[tokio::test]
    async fn test_result_sharing() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let mut counter = 0;
        
        for _ in 0..3 {
            let result = dedup.read_deduplicated(key, || {
                counter += 1;
                Ok(vec![counter])
            }).await;
            
            assert!(result.is_ok());
        }
        
        assert_eq!(counter, 1);
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_hits, 2);
    }

    #[tokio::test]
    async fn test_error_handling() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let result = dedup.read_deduplicated(key, || Err("IO error".to_string())).await;
        assert!(result.is_err());
        
        let result2 = dedup.read_deduplicated(key, || Ok(vec![])).await;
        assert!(result2.is_err());
    }

    #[tokio::test]
    async fn test_multiple_qp_ids() {
        let dedup = create_test_dedup();
        
        let key1 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let key2 = ReadKey { qp_id: QueuePairId(1), lba: 100, length: 4096 };
        
        let _ = dedup.read_deduplicated(key1, || Ok(vec![1])).await;
        let _ = dedup.read_deduplicated(key2, || Ok(vec![2])).await;
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 2);
    }

    #[tokio::test]
    async fn test_concurrent_different_keys() {
        let dedup = Arc::new(create_test_dedup());
        
        let mut handles = vec![];
        
        for i in 0..5 {
            let d = Arc::clone(&dedup);
            let handle = tokio::spawn(async move {
                let key = ReadKey { qp_id: QueuePairId(0), lba: 100 + i as u64, length: 4096 };
                d.read_deduplicated(key, || Ok(vec![i as u8])).await
            });
            handles.push(handle);
        }
        
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 5);
    }

    #[tokio::test]
    async fn test_rapid_sequence_requests() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        for _ in 0..10 {
            let _ = dedup.read_deduplicated(key, || Ok(vec![1, 2, 3])).await;
        }
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 1);
        assert_eq!(stats.cache_hits, 9);
    }

    #[tokio::test]
    async fn test_large_data_dedup() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let large_data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        
        let result1 = dedup.read_deduplicated(key, || Ok(large_data.clone())).await;
        let result2 = dedup.read_deduplicated(key, || Ok(vec![])).await;
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap(), result2.unwrap());
    }

    #[tokio::test]
    async fn test_partial_overlap_no_dedup() {
        let dedup = create_test_dedup();
        
        let key1 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let key2 = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 2048 };
        
        let _ = dedup.read_deduplicated(key1, || Ok(vec![1])).await;
        let _ = dedup.read_deduplicated(key2, || Ok(vec![2])).await;
        
        let stats = dedup.stats();
        assert_eq!(stats.cache_misses, 2);
    }

    #[tokio::test]
    async fn test_clear() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let _ = dedup.read_deduplicated(key, || Ok(vec![1])).await;
        
        assert_eq!(dedup.pending_count(), 1);
        
        dedup.clear().await;
        
        assert_eq!(dedup.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_invalidate() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        let _ = dedup.read_deduplicated(key, || Ok(vec![1])).await;
        
        assert_eq!(dedup.pending_count(), 1);
        
        dedup.invalidate(&key).await;
        
        assert_eq!(dedup.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_dedup_stats_tracking() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        for _ in 0..3 {
            let _ = dedup.read_deduplicated(key, || Ok(vec![1])).await;
        }
        
        let stats = dedup.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.cache_hits, 2);
        assert!(stats.dedup_rate > 0.5);
    }

    #[tokio::test]
    async fn test_write_invalidation() {
        let dedup = create_test_dedup();
        
        let read_key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let _ = dedup.read_deduplicated(read_key, || Ok(vec![1])).await;
        dedup.invalidate(&read_key).await;
        
        let result = dedup.read_deduplicated(read_key, || Ok(vec![2])).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![2]);
    }

    #[tokio::test]
    async fn test_dedup_after_error() {
        let dedup = create_test_dedup();
        
        let key = ReadKey { qp_id: QueuePairId(0), lba: 100, length: 4096 };
        
        let _ = dedup.read_deduplicated(key, || Err("error".to_string())).await;
        
        let result = dedup.read_deduplicated(key, || Ok(vec![1])).await;
        
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_mixed_keys_and_qp() {
        let dedup = Arc::new(create_test_dedup());
        
        let mut handles = vec![];
        
        for qp in 0..3u64 {
            for lba in 0..3u64 {
                let d = Arc::clone(&dedup);
                let handle = tokio::spawn(async move {
                    let key = ReadKey { 
                        qp_id: QueuePairId(qp), 
                        lba: 100 + lba * 100, 
                        length: 4096 
                    };
                    d.read_deduplicated(key, || Ok(vec![qp as u8, lba as u8])).await
                });
                handles.push(handle);
            }
        }
        
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
        
        let stats = dedup.stats();
        assert_eq!(stats.total_requests, 9);
    }
}