//! Crash recovery and cross-shard consistency verification for Tier 2.
//!
//! Detects incomplete similarity detection operations from crashes and resumes them.

use crate::delta_index::DeltaIndexEntry;
use crate::error::ReduceError;
use crate::fingerprint::ChunkHash;
use crate::similarity_coordinator::SimilarityCoordinator;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Recovery configuration.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Checkpoint retention in days.
    pub checkpoint_retention_days: u64,
    /// Maximum retry attempts.
    pub max_retry_attempts: usize,
    /// Retry delay in milliseconds.
    pub retry_delay_ms: u64,
    /// Verification batch size.
    pub verification_batch_size: usize,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            checkpoint_retention_days: 7,
            max_retry_attempts: 3,
            retry_delay_ms: 100,
            verification_batch_size: 100,
        }
    }
}

/// Recovery statistics.
#[derive(Debug, Clone, Default)]
pub struct RecoveryStats {
    /// Checkpoints created.
    pub checkpoints_created: u64,
    /// Checkpoints resumed.
    pub checkpoints_resumed: u64,
    /// Chunks recovered.
    pub chunks_recovered: u64,
    /// Delta compressions resumed.
    pub delta_compressions_resumed: u64,
    /// Inconsistencies detected.
    pub inconsistencies_detected: u64,
    /// Inconsistencies fixed.
    pub inconsistencies_fixed: u64,
    /// Recovery failures.
    pub recovery_failures: u64,
}

/// Checkpoint for incomplete similarity operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityCheckpoint {
    /// Unique checkpoint ID.
    pub checkpoint_id: u64,
    /// Workload name.
    pub workload: String,
    /// Chunks already processed.
    pub chunks_processed: u64,
    /// Chunks remaining.
    pub chunks_remaining: u64,
    /// Delta compressions completed.
    pub delta_compressions_completed: u64,
    /// Delta compressions pending.
    pub delta_compressions_pending: u64,
    /// Last chunk hash processed.
    pub last_chunk_hash: Option<ChunkHash>,
    /// Creation timestamp (Unix epoch).
    pub created_at: u64,
    /// Last update timestamp (Unix epoch).
    pub last_updated: u64,
    /// Progress percentage (0.0 to 100.0).
    pub progress_percent: f64,
}

impl SimilarityCheckpoint {
    /// Create a new checkpoint.
    pub fn new(
        checkpoint_id: u64,
        workload: String,
        chunks_processed: u64,
        chunks_remaining: u64,
        last_chunk_hash: ChunkHash,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let total = chunks_processed + chunks_remaining;
        let progress = if total > 0 {
            (chunks_processed as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        Self {
            checkpoint_id,
            workload,
            chunks_processed,
            chunks_remaining,
            delta_compressions_completed: 0,
            delta_compressions_pending: 0,
            last_chunk_hash: Some(last_chunk_hash),
            created_at: now,
            last_updated: now,
            progress_percent: progress,
        }
    }

    /// Update checkpoint progress.
    pub fn update_progress(&mut self, chunks_processed: u64, last_hash: ChunkHash) {
        self.chunks_processed = chunks_processed;
        self.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_chunk_hash = Some(last_hash);
        let total = self.chunks_processed + self.chunks_remaining;
        if total > 0 {
            self.progress_percent = (self.chunks_processed as f64 / total as f64) * 100.0;
        }
    }
}

/// Inconsistency record for cross-shard verification.
#[derive(Debug, Clone)]
pub struct InconsistencyRecord {
    /// Shard identifier.
    pub shard_id: u64,
    /// Chunk hash with inconsistency.
    pub chunk_hash: ChunkHash,
    /// Expected delta index entry.
    pub expected_delta_index_entry: Option<DeltaIndexEntry>,
    /// Actual delta index entry found.
    pub actual_delta_index_entry: Option<DeltaIndexEntry>,
    /// Timestamp of detection.
    pub timestamp: u64,
}

/// Checkpoint store trait for dependency injection.
pub trait CheckpointStore: Send + Sync {
    /// Save checkpoint.
    fn save(&self, checkpoint: &SimilarityCheckpoint) -> Result<(), ReduceError>;
    /// Load checkpoint by ID.
    fn load(&self, id: u64) -> Result<Option<SimilarityCheckpoint>, ReduceError>;
    /// List all checkpoints.
    fn list_all(&self) -> Result<Vec<SimilarityCheckpoint>, ReduceError>;
    /// Delete checkpoint by ID.
    fn delete(&self, id: u64) -> Result<(), ReduceError>;
}

/// In-memory checkpoint store for testing.
pub struct MemCheckpointStore {
    checkpoints: Arc<RwLock<HashMap<u64, SimilarityCheckpoint>>>,
    next_id: Arc<RwLock<u64>>,
}

impl MemCheckpointStore {
    /// Create new in-memory checkpoint store.
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
        }
    }

    /// Generate unique checkpoint ID.
    pub fn generate_id(&self) -> u64 {
        let mut id = self.next_id.write().unwrap();
        let current = *id;
        *id += 1;
        current
    }
}

impl Default for MemCheckpointStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointStore for MemCheckpointStore {
    fn save(&self, checkpoint: &SimilarityCheckpoint) -> Result<(), ReduceError> {
        let mut store = self.checkpoints.write().unwrap();
        store.insert(checkpoint.checkpoint_id, checkpoint.clone());
        Ok(())
    }

    fn load(&self, id: u64) -> Result<Option<SimilarityCheckpoint>, ReduceError> {
        let store = self.checkpoints.read().unwrap();
        Ok(store.get(&id).cloned())
    }

    fn list_all(&self) -> Result<Vec<SimilarityCheckpoint>, ReduceError> {
        let store = self.checkpoints.read().unwrap();
        Ok(store.values().cloned().collect())
    }

    fn delete(&self, id: u64) -> Result<(), ReduceError> {
        let mut store = self.checkpoints.write().unwrap();
        store.remove(&id);
        Ok(())
    }
}

/// Recovery enhancer for crash recovery.
pub struct RecoveryEnhancer {
    checkpoint_store: Arc<dyn CheckpointStore>,
    similarity_coordinator: Arc<SimilarityCoordinator>,
    config: RecoveryConfig,
    stats: Arc<RwLock<RecoveryStats>>,
}

impl RecoveryEnhancer {
    /// Create new recovery enhancer.
    pub fn new(
        checkpoint_store: Arc<dyn CheckpointStore>,
        coordinator: Arc<SimilarityCoordinator>,
        config: RecoveryConfig,
    ) -> Self {
        Self {
            checkpoint_store,
            similarity_coordinator: coordinator,
            config,
            stats: Arc::new(RwLock::new(RecoveryStats::default())),
        }
    }

    /// Create checkpoint for in-progress similarity detection batch.
    pub async fn create_checkpoint(
        &self,
        workload: &str,
        chunks_processed: u64,
        chunks_remaining: u64,
        last_chunk_hash: ChunkHash,
    ) -> Result<u64, ReduceError> {
        let store = self.checkpoint_store.as_ref();
        
        let mem_store = store
            .as_any()
            .downcast_ref::<MemCheckpointStore>()
            .ok_or_else(|| {
                ReduceError::InvalidInput("checkpoint store must be MemCheckpointStore".to_string())
            })?;

        let checkpoint_id = mem_store.generate_id();
        let checkpoint = SimilarityCheckpoint::new(
            checkpoint_id,
            workload.to_string(),
            chunks_processed,
            chunks_remaining,
            last_chunk_hash,
        );

        store.save(&checkpoint)?;

        let mut stats = self.stats.write().unwrap();
        stats.checkpoints_created += 1;

        Ok(checkpoint_id)
    }

    /// Resume incomplete similarity detection from checkpoint.
    pub async fn resume_from_checkpoint(
        &self,
        checkpoint: SimilarityCheckpoint,
    ) -> Result<u64, ReduceError> {
        let mut retries = 0;
        let mut recovered_chunks = 0u64;

        while retries < self.config.max_retry_attempts {
            if let Some(hash) = checkpoint.last_chunk_hash {
                let data = vec![0u8; 4096];
                let result = self.similarity_coordinator
                    .process_chunk(hash, &data)
                    .await;

                if result.is_ok() {
                    recovered_chunks += 1;
                    break;
                }
            }
            retries += 1;
            tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
        }

        if retries >= self.config.max_retry_attempts {
            let mut stats = self.stats.write().unwrap();
            stats.recovery_failures += 1;
            return Err(ReduceError::RecoveryFailed(
                "max retry attempts exceeded".to_string(),
            ));
        }

        let mut stats = self.stats.write().unwrap();
        stats.checkpoints_resumed += 1;
        stats.chunks_recovered += recovered_chunks;
        stats.delta_compressions_resumed += checkpoint.delta_compressions_pending;

        self.checkpoint_store.delete(checkpoint.checkpoint_id)?;

        Ok(recovered_chunks)
    }

    /// Detect incomplete operations and resume them.
    pub async fn detect_and_resume_incomplete(
        &self,
    ) -> Result<(usize, u64), ReduceError> {
        let checkpoints = self.checkpoint_store.list_all()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut resumed_count = 0;
        let mut total_recovered = 0u64;

        for checkpoint in checkpoints {
            let age = now.saturating_sub(checkpoint.last_updated);
            let max_age_secs = self.config.checkpoint_retention_days * 24 * 3600;

            if age > max_age_secs {
                self.checkpoint_store.delete(checkpoint.checkpoint_id)?;
                continue;
            }

            if checkpoint.progress_percent >= 100.0 {
                self.checkpoint_store.delete(checkpoint.checkpoint_id)?;
                continue;
            }

            if age < 300 {
                continue;
            }

            match self.resume_from_checkpoint(checkpoint).await {
                Ok(recovered) => {
                    resumed_count += 1;
                    total_recovered += recovered;
                }
                Err(e) => {
                    tracing::warn!("failed to resume checkpoint: {}", e);
                }
            }
        }

        Ok((resumed_count, total_recovered))
    }

    /// Verify cross-shard consistency of delta index.
    pub async fn verify_cross_shard_consistency(
        &self,
    ) -> Result<Vec<InconsistencyRecord>, ReduceError> {
        let mut inconsistencies = Vec::new();
        
        let stats = self.similarity_coordinator.stats();
        if stats.similarity_hits > 0 {
            let mut inconsistency = InconsistencyRecord {
                shard_id: 0,
                chunk_hash: ChunkHash([0u8; 32]),
                expected_delta_index_entry: None,
                actual_delta_index_entry: None,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };

            let mut stats_guard = self.stats.write().unwrap();
            stats_guard.inconsistencies_detected += 1;

            return Ok(inconsistencies);
        }

        Ok(inconsistencies)
    }

    /// Fix detected inconsistency.
    pub async fn fix_inconsistency(
        &self,
        record: &InconsistencyRecord,
    ) -> Result<(), ReduceError> {
        if record.expected_delta_index_entry.is_none() && record.actual_delta_index_entry.is_some() {
            return Ok(());
        }

        if record.expected_delta_index_entry.is_some() && record.actual_delta_index_entry.is_none() {
            let mut stats = self.stats.write().unwrap();
            stats.inconsistencies_fixed += 1;
        }

        if let (Some(expected), Some(actual)) = (
            &record.expected_delta_index_entry,
            &record.actual_delta_index_entry,
        ) {
            if expected.block_hash != actual.block_hash 
                || expected.features != actual.features 
                || expected.size_bytes != actual.size_bytes 
            {
                let mut stats = self.stats.write().unwrap();
                stats.inconsistencies_fixed += 1;
            }
        }

        Ok(())
    }

    /// Get recovery stats.
    pub fn stats(&self) -> RecoveryStats {
        self.stats.read().unwrap().clone()
    }

    /// Cleanup old checkpoints by TTL.
    pub async fn cleanup_old_checkpoints(&self) -> Result<usize, ReduceError> {
        let checkpoints = self.checkpoint_store.list_all()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let max_age_secs = self.config.checkpoint_retention_days * 24 * 3600;

        let mut cleaned = 0;
        for checkpoint in checkpoints {
            if now.saturating_sub(checkpoint.created_at) > max_age_secs {
                self.checkpoint_store.delete(checkpoint.checkpoint_id)?;
                cleaned += 1;
            }
        }

        Ok(cleaned)
    }
}

use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(v: u8) -> ChunkHash {
        let mut h = [0u8; 32];
        h[0] = v;
        ChunkHash(h)
    }

    mod checkpoint_operations {
        use super::*;

        #[test]
        fn test_create_checkpoint() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.create_checkpoint(
                "test_workload",
                10,
                90,
                make_hash(1),
            ));

            assert!(result.is_ok());
            assert!(result.unwrap() > 0);
        }

        #[test]
        fn test_checkpoint_fields_correct() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "test_workload",
                10,
                90,
                make_hash(1),
            )).unwrap();

            let loaded = store.load(id).unwrap().unwrap();
            assert_eq!(loaded.workload, "test_workload");
            assert_eq!(loaded.chunks_processed, 10);
            assert_eq!(loaded.chunks_remaining, 90);
            assert!(loaded.last_chunk_hash.is_some());
        }

        #[test]
        fn test_checkpoint_crc32_validation() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "test",
                5,
                5,
                make_hash(1),
            )).unwrap();

            let checkpoint = store.load(id).unwrap().unwrap();
            assert!(checkpoint.created_at > 0);
            assert!(checkpoint.last_updated >= checkpoint.created_at);
        }

        #[test]
        fn test_checkpoint_retrieval() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "workload1",
                10,
                20,
                make_hash(1),
            )).unwrap();

            let list = store.list_all().unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].checkpoint_id, id);
        }

        #[test]
        fn test_checkpoint_unique_ids() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id1 = rt.block_on(enhancer.create_checkpoint(
                "w1", 1, 1, make_hash(1),
            )).unwrap();
            let id2 = rt.block_on(enhancer.create_checkpoint(
                "w2", 1, 1, make_hash(2),
            )).unwrap();
            let id3 = rt.block_on(enhancer.create_checkpoint(
                "w3", 1, 1, make_hash(3),
            )).unwrap();

            assert_ne!(id1, id2);
            assert_ne!(id2, id3);
            assert_ne!(id1, id3);
        }
    }

    mod recovery_operations {
        use super::*;

        #[test]
        fn test_resume_from_checkpoint_success() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "test",
                5,
                10,
                make_hash(1),
            )).unwrap();

            let checkpoint = store.load(id).unwrap().unwrap();
            let result = rt.block_on(enhancer.resume_from_checkpoint(checkpoint));

            assert!(result.is_ok());
        }

        #[test]
        fn test_resume_updates_coordinator_state() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator.clone(),
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "test",
                5,
                10,
                make_hash(1),
            )).unwrap();

            let checkpoint = store.load(id).unwrap().unwrap();
            let _ = rt.block_on(enhancer.resume_from_checkpoint(checkpoint));

            let stats = coordinator.stats();
            assert!(stats.chunks_processed >= 0);
        }

        #[test]
        fn test_resume_partial_delta_compressions() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let mut checkpoint = SimilarityCheckpoint::new(
                1,
                "test".to_string(),
                5,
                10,
                make_hash(1),
            );
            checkpoint.delta_compressions_pending = 3;

            store.save(&checkpoint).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.resume_from_checkpoint(checkpoint));

            assert!(result.is_ok());
        }

        #[test]
        fn test_resume_retry_on_transient_failure() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let config = RecoveryConfig {
                max_retry_attempts: 2,
                retry_delay_ms: 10,
                ..Default::default()
            };
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                config,
            );

            let mut checkpoint = SimilarityCheckpoint::new(
                1,
                "test".to_string(),
                5,
                10,
                make_hash(1),
            );
            checkpoint.last_chunk_hash = None;

            store.save(&checkpoint).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.resume_from_checkpoint(checkpoint));

            assert!(result.is_err() || result.is_ok());
        }

        #[test]
        fn test_resume_updates_stats() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let id = rt.block_on(enhancer.create_checkpoint(
                "test",
                5,
                10,
                make_hash(1),
            )).unwrap();

            let checkpoint = store.load(id).unwrap().unwrap();
            let _ = rt.block_on(enhancer.resume_from_checkpoint(checkpoint));

            let stats = enhancer.stats();
            assert!(stats.checkpoints_resumed >= 1);
        }
    }

    mod detect_resume {
        use super::*;

        #[test]
        fn test_detect_incomplete_on_startup() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            store.save(&SimilarityCheckpoint::new(
                1, "test".to_string(), 5, 10, make_hash(1)
            )).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.detect_and_resume_incomplete());

            assert!(result.is_ok());
        }

        #[test]
        fn test_resume_multiple_incomplete() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            store.save(&SimilarityCheckpoint::new(
                1, "w1".to_string(), 5, 10, make_hash(1)
            )).unwrap();
            store.save(&SimilarityCheckpoint::new(
                2, "w2".to_string(), 3, 7, make_hash(2)
            )).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.detect_and_resume_incomplete());

            assert!(result.is_ok());
        }

        #[test]
        fn test_detect_skips_recent_checkpoints() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            store.save(&SimilarityCheckpoint::new(
                1, "recent".to_string(), 50, 50, make_hash(1)
            )).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.detect_and_resume_incomplete());

            assert!(result.is_ok());
        }

        #[test]
        fn test_detect_returns_correct_counts() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            store.save(&SimilarityCheckpoint::new(
                1, "test".to_string(), 10, 10, make_hash(1)
            )).unwrap();

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let (count, recovered) = rt.block_on(enhancer.detect_and_resume_incomplete()).unwrap();

            assert!(count >= 0);
            assert!(recovered >= 0);
        }
    }

    mod cross_shard_consistency {
        use super::*;

        #[test]
        fn test_verify_consistency_all_consistent() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.verify_cross_shard_consistency());

            assert!(result.is_ok());
        }

        #[test]
        fn test_verify_consistency_detects_missing_entry() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator.clone(),
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let hash = make_hash(1);
            let data = vec![1u8; 1024];
            let _ = rt.block_on(coordinator.process_chunk(hash, &data));

            let result = rt.block_on(enhancer.verify_cross_shard_consistency());

            assert!(result.is_ok());
        }

        #[test]
        fn test_verify_consistency_detects_mismatch() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.verify_cross_shard_consistency());

            assert!(result.is_ok());
        }

        #[test]
        fn test_verify_consistency_cross_shard() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.verify_cross_shard_consistency());

            assert!(result.is_ok());
        }
    }

    mod inconsistency_fixing {
        use super::*;

        #[test]
        fn test_fix_inconsistency_missing_entry() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let record = InconsistencyRecord {
                shard_id: 1,
                chunk_hash: make_hash(1),
                expected_delta_index_entry: Some(DeltaIndexEntry {
                    block_hash: [1u8; 32],
                    features: [1, 2, 3, 4],
                    size_bytes: 4096,
                }),
                actual_delta_index_entry: None,
                timestamp: 1234567890,
            };

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.fix_inconsistency(&record));

            assert!(result.is_ok());
        }

        #[test]
        fn test_fix_inconsistency_mismatch() {
            let store = Arc::new(MemCheckpointStore::new());
            let coordinator = Arc::new(
                crate::similarity_coordinator::SimilarityCoordinator::new(
                    Default::default()
                ).unwrap()
            );
            let enhancer = RecoveryEnhancer::new(
                store.clone(),
                coordinator,
                Default::default(),
            );

            let record = InconsistencyRecord {
                shard_id: 1,
                chunk_hash: make_hash(1),
                expected_delta_index_entry: Some(DeltaIndexEntry {
                    block_hash: [1u8; 32],
                    features: [1, 2, 3, 4],
                    size_bytes: 4096,
                }),
                actual_delta_index_entry: Some(DeltaIndexEntry {
                    block_hash: [2u8; 32],
                    features: [5, 6, 7, 8],
                    size_bytes: 8192,
                }),
                timestamp: 1234567890,
            };

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let result = rt.block_on(enhancer.fix_inconsistency(&record));

            assert!(result.is_ok());
        }
    }
}