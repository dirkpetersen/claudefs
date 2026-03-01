//! Background async pipeline for similarity dedup and garbage collection.
//! Handles:
//! - Tier 2 similarity deduplication with delta compression
//! - Periodic GC sweeps
//! - Statistics reporting

use crate::dedupe::CasIndex;
use crate::error::ReduceError;
use crate::fingerprint::{ChunkHash, SuperFeatures};
use crate::gc::{GcConfig, GcEngine};
use crate::similarity::SimilarityIndex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, watch};
use tracing::debug;

/// A work item for the background processor.
#[derive(Debug)]
pub enum BackgroundTask {
    /// Process a new chunk: check for similarity matches, attempt delta compression.
    ProcessChunk {
        /// Chunk hash for CAS lookup.
        hash: ChunkHash,
        /// Super-features for similarity matching.
        features: SuperFeatures,
        /// Original chunk data (for delta compression reference lookups).
        data: Vec<u8>,
    },
    /// Run a GC cycle with the given reachable hashes.
    RunGc {
        /// Chunk hashes that are still reachable (in use).
        reachable: Vec<ChunkHash>,
    },
    /// Shutdown the background processor.
    Shutdown,
}

/// Configuration for the background processor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundConfig {
    /// Channel capacity for incoming tasks.
    pub channel_capacity: usize,
    /// Compression level for delta compression.
    pub delta_compression_level: i32,
    /// Minimum similarity count (3 means 3/4 features must match).
    pub similarity_threshold: usize,
    /// GC config for periodic sweeps.
    pub gc_config: GcConfig,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1000,
            delta_compression_level: 3,
            similarity_threshold: 3,
            gc_config: GcConfig::default(),
        }
    }
}

/// Statistics tracked by the background processor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundStats {
    /// Total chunks processed in background.
    pub chunks_processed: u64,
    /// Chunks where a similar match was found.
    pub similarity_hits: u64,
    /// Chunks where delta compression was applied.
    pub delta_compressed: u64,
    /// GC cycles run.
    pub gc_cycles: u64,
    /// Total chunks reclaimed by GC.
    pub chunks_reclaimed: u64,
    /// Bytes saved by delta compression (estimate).
    pub bytes_saved_delta: u64,
}

/// Handle to the background processor for sending tasks and reading stats.
pub struct BackgroundHandle {
    sender: mpsc::Sender<BackgroundTask>,
    stats: Arc<watch::Receiver<BackgroundStats>>,
}

impl BackgroundHandle {
    /// Send a task to the background processor.
    /// Returns Ok(()) if sent successfully, Err if the processor has shut down.
    pub async fn send(&self, task: BackgroundTask) -> Result<(), ReduceError> {
        self.sender
            .send(task)
            .await
            .map_err(|_| ReduceError::Io(std::io::Error::other("background processor shut down")))
    }

    /// Get a snapshot of current stats.
    pub fn stats(&self) -> BackgroundStats {
        self.stats.borrow().clone()
    }

    /// Check if the background processor is still running.
    pub fn is_running(&self) -> bool {
        !self.sender.is_closed()
    }
}

/// Background processor that handles similarity dedup and GC in async tasks.
pub struct BackgroundProcessor {
    config: BackgroundConfig,
    similarity_index: SimilarityIndex,
    cas: Arc<Mutex<CasIndex>>,
    stats_tx: watch::Sender<BackgroundStats>,
    stats: BackgroundStats,
}

impl BackgroundProcessor {
    /// Create and start the background processor.
    /// Returns a BackgroundHandle for submitting tasks.
    pub fn start(
        config: BackgroundConfig,
        cas: Arc<Mutex<CasIndex>>,
    ) -> BackgroundHandle {
        let (task_tx, task_rx) = mpsc::channel(config.channel_capacity);
        let (stats_tx, stats_rx) = watch::channel(BackgroundStats::default());

        let processor = BackgroundProcessor {
            config,
            similarity_index: SimilarityIndex::new(),
            cas,
            stats_tx,
            stats: BackgroundStats::default(),
        };

        tokio::spawn(processor.run(task_rx));

        BackgroundHandle {
            sender: task_tx,
            stats: Arc::new(stats_rx),
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<BackgroundTask>) {
        while let Some(task) = rx.recv().await {
            match task {
                BackgroundTask::ProcessChunk { hash, features, data } => {
                    self.process_chunk(hash, features, data).await;
                }
                BackgroundTask::RunGc { reachable } => {
                    self.run_gc(reachable).await;
                }
                BackgroundTask::Shutdown => break,
            }
            let _ = self.stats_tx.send(self.stats.clone());
        }
    }

    async fn process_chunk(&mut self, hash: ChunkHash, features: SuperFeatures, _data: Vec<u8>) {
        self.stats.chunks_processed += 1;

        let similar_hash = self.similarity_index.find_similar(&features);

        if let Some(ref_hash) = similar_hash {
            self.stats.similarity_hits += 1;
            debug!(?hash, ?ref_hash, "Similarity match found (delta compression deferred to storage layer)");
        }

        self.similarity_index.insert(hash, features);
    }

    async fn run_gc(&mut self, reachable: Vec<ChunkHash>) {
        let mut gc = GcEngine::new(self.config.gc_config.clone());
        let mut cas = self.cas.lock().await;
        let gc_stats = gc.run_cycle(&mut cas, &reachable);
        self.stats.gc_cycles += 1;
        self.stats.chunks_reclaimed += gc_stats.chunks_reclaimed as u64;
        debug!(
            reclaimed = gc_stats.chunks_reclaimed,
            "GC cycle complete"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::super_features;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_send_process_chunk() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        let data = b"test data for background processing";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        handle
            .send(BackgroundTask::ProcessChunk {
                hash,
                features,
                data: data.to_vec(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 1);
    }

    #[tokio::test]
    async fn test_send_gc_task() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        handle
            .send(BackgroundTask::RunGc {
                reachable: vec![],
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = handle.stats();
        assert_eq!(stats.gc_cycles, 1);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        handle.send(BackgroundTask::Shutdown).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(!handle.is_running());
    }

    #[tokio::test]
    async fn test_similarity_hit() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        let data = b"hello world this is some test data for similarity detection";
        let features = super_features(data);

        let hash1 = ChunkHash(*blake3::hash(b"chunk1").as_bytes());
        handle
            .send(BackgroundTask::ProcessChunk {
                hash: hash1,
                features,
                data: data.to_vec(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut features2 = features;
        features2.0[3] = features2.0[3].wrapping_add(1);
        let hash2 = ChunkHash(*blake3::hash(b"chunk2").as_bytes());
        handle
            .send(BackgroundTask::ProcessChunk {
                hash: hash2,
                features: features2,
                data: data.to_vec(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 2);
        assert!(stats.similarity_hits >= 1);
    }

    #[tokio::test]
    async fn test_multiple_chunks() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        for i in 0..10 {
            let data = format!("test data chunk {}", i);
            let hash = ChunkHash(*blake3::hash(data.as_bytes()).as_bytes());
            let features = super_features(data.as_bytes());

            handle
                .send(BackgroundTask::ProcessChunk {
                    hash,
                    features,
                    data: data.into_bytes(),
                })
                .await
                .unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let stats = handle.stats();
        assert_eq!(stats.chunks_processed, 10);
    }

    #[tokio::test]
    async fn test_stats_update() {
        let cas = Arc::new(Mutex::new(CasIndex::new()));
        let config = BackgroundConfig::default();
        let handle = BackgroundProcessor::start(config, cas);

        let initial_stats = handle.stats();
        assert_eq!(initial_stats.chunks_processed, 0);

        let data = b"stats update test data";
        let hash = ChunkHash(*blake3::hash(data).as_bytes());
        let features = super_features(data);

        handle
            .send(BackgroundTask::ProcessChunk {
                hash,
                features,
                data: data.to_vec(),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let updated_stats = handle.stats();
        assert!(updated_stats.chunks_processed > 0);
    }
}