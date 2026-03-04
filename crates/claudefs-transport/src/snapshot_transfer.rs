//! Snapshot data transfer protocol for Raft snapshots and state transfers.
//!
//! Protocol tracker for transferring Raft snapshot data between nodes during
//! leader changes, new node catchup, or disaster recovery. Manages chunked
//! transfer with checkpointing so transfers can resume after network interruptions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Configuration for snapshot transfers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTransferConfig {
    /// Chunk size for transfer in bytes (default: 1MB).
    pub chunk_size_bytes: usize,
    /// Max concurrent transfers (default: 2).
    pub max_concurrent: usize,
    /// Inactivity timeout per transfer in ms (default: 60_000).
    pub transfer_timeout_ms: u64,
    /// Max retries per chunk before failing transfer (default: 3).
    pub max_chunk_retries: usize,
}

impl Default for SnapshotTransferConfig {
    fn default() -> Self {
        Self {
            chunk_size_bytes: 1_048_576,
            max_concurrent: 2,
            transfer_timeout_ms: 60_000,
            max_chunk_retries: 3,
        }
    }
}

/// Unique identifier for a snapshot transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(pub u64);

/// State of a chunk transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChunkTransferState {
    /// Chunk queued for transfer.
    Pending,
    /// Chunk currently in flight.
    InFlight,
    /// Receiver side: data arrived, waiting ACK.
    Received,
    /// Sender side: ACK received.
    Acked,
    /// Chunk transfer failed.
    Failed {
        /// Failure reason.
        reason: String,
    },
}

/// A single chunk within a snapshot transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferChunk {
    /// Index of this chunk in the snapshot.
    pub chunk_index: u64,
    /// Offset in bytes from start of snapshot.
    pub offset_bytes: u64,
    /// Size of this chunk in bytes.
    pub size_bytes: usize,
    /// CRC32 checksum of chunk data.
    pub checksum: u32,
    /// Current state of this chunk.
    pub state: ChunkTransferState,
    /// Number of times this chunk has been retried.
    pub retry_count: u32,
}

/// Metadata about a snapshot being transferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// Unique snapshot identifier.
    pub snapshot_id: u64,
    /// Source node for the transfer.
    pub source_node: [u8; 16],
    /// Destination node for the transfer.
    pub dest_node: [u8; 16],
    /// Total size of the snapshot in bytes.
    pub total_bytes: u64,
    /// Total number of chunks.
    pub chunk_count: u64,
    /// Which shard this snapshot belongs to (optional).
    pub shard_id: Option<u32>,
    /// Raft term of the snapshot.
    pub term: u64,
    /// Raft log index of the snapshot.
    pub index: u64,
}

/// Current state of a transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransferState {
    /// Transfer created but not yet started.
    Pending,
    /// Transfer in progress.
    InProgress {
        /// Number of chunks completed.
        chunks_done: u64,
        /// Total number of chunks.
        chunks_total: u64,
    },
    /// Transfer completed successfully.
    Completed {
        /// Total duration in ms.
        duration_ms: u64,
        /// Total bytes transferred.
        bytes_transferred: u64,
    },
    /// Transfer failed.
    Failed {
        /// Failure reason.
        reason: String,
        /// Number of chunks completed before failure.
        chunks_done: u64,
    },
    /// Transfer cancelled by user.
    Cancelled,
}

/// Error types for snapshot transfer operations.
#[derive(Debug, Error)]
pub enum TransferError {
    /// Transfer not found.
    #[error("transfer {0:?} not found")]
    TransferNotFound(TransferId),
    /// Transfer already exists.
    #[error("transfer {0:?} already exists")]
    TransferAlreadyExists(TransferId),
    /// Chunk not found.
    #[error("chunk {0} not found in transfer {1:?}")]
    ChunkNotFound(u64, TransferId),
    /// Max concurrent transfers exceeded.
    #[error("max concurrent transfers ({0}) exceeded")]
    MaxConcurrentExceeded(usize),
    /// Transfer is not in progress.
    #[error("transfer {0:?} not in progress")]
    TransferNotInProgress(TransferId),
}

/// Statistics for snapshot transfer operations.
pub struct SnapshotTransferStats {
    pub active_transfers: AtomicU64,
    pub total_initiated: AtomicU64,
    pub total_completed: AtomicU64,
    pub total_failed: AtomicU64,
    pub total_cancelled: AtomicU64,
    pub total_bytes_transferred: AtomicU64,
    pub total_chunk_retries: AtomicU64,
    pub total_stale_expired: AtomicU64,
}

impl SnapshotTransferStats {
    pub fn new() -> Self {
        Self {
            active_transfers: AtomicU64::new(0),
            total_initiated: AtomicU64::new(0),
            total_completed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_cancelled: AtomicU64::new(0),
            total_bytes_transferred: AtomicU64::new(0),
            total_chunk_retries: AtomicU64::new(0),
            total_stale_expired: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> SnapshotTransferStatsSnapshot {
        SnapshotTransferStatsSnapshot {
            active_transfers: self.active_transfers.load(Ordering::Relaxed),
            total_initiated: self.total_initiated.load(Ordering::Relaxed),
            total_completed: self.total_completed.load(Ordering::Relaxed),
            total_failed: self.total_failed.load(Ordering::Relaxed),
            total_cancelled: self.total_cancelled.load(Ordering::Relaxed),
            total_bytes_transferred: self.total_bytes_transferred.load(Ordering::Relaxed),
            total_chunk_retries: self.total_chunk_retries.load(Ordering::Relaxed),
            total_stale_expired: self.total_stale_expired.load(Ordering::Relaxed),
        }
    }
}

impl Default for SnapshotTransferStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of transfer statistics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTransferStatsSnapshot {
    /// Number of currently active transfers.
    pub active_transfers: u64,
    /// Total transfers initiated.
    pub total_initiated: u64,
    /// Total transfers completed successfully.
    pub total_completed: u64,
    /// Total transfers that failed.
    pub total_failed: u64,
    /// Total transfers cancelled.
    pub total_cancelled: u64,
    /// Total bytes transferred.
    pub total_bytes_transferred: u64,
    /// Total chunk retry attempts.
    pub total_chunk_retries: u64,
    /// Total transfers expired due to inactivity.
    pub total_stale_expired: u64,
}

struct Transfer {
    id: TransferId,
    meta: SnapshotMeta,
    state: TransferState,
    chunks: Vec<TransferChunk>,
    created_at_ms: u64,
    last_progress_ms: u64,
    next_chunk_index: u64,
}

impl Transfer {
    fn new(id: TransferId, meta: SnapshotMeta, chunk_size_bytes: usize, now_ms: u64) -> Self {
        let actual_chunk_count = meta.chunk_count as usize;

        let chunks: Vec<TransferChunk> = (0..actual_chunk_count)
            .map(|i| {
                let offset = (i * chunk_size_bytes) as u64;
                let size = (meta.total_bytes.saturating_sub(offset) as usize).min(chunk_size_bytes);
                TransferChunk {
                    chunk_index: i as u64,
                    offset_bytes: offset,
                    size_bytes: size,
                    checksum: 0,
                    state: ChunkTransferState::Pending,
                    retry_count: 0,
                }
            })
            .collect();

        Self {
            id,
            meta,
            state: TransferState::Pending,
            chunks,
            created_at_ms: now_ms,
            last_progress_ms: now_ms,
            next_chunk_index: 0,
        }
    }
}

/// The snapshot transfer manager.
pub struct SnapshotTransferManager {
    config: SnapshotTransferConfig,
    transfers: RwLock<HashMap<TransferId, Transfer>>,
    next_transfer_id: AtomicU64,
    stats: Arc<SnapshotTransferStats>,
}

impl SnapshotTransferManager {
    /// Create a new snapshot transfer manager with the given configuration.
    pub fn new(config: SnapshotTransferConfig) -> Self {
        Self {
            config: config.clone(),
            transfers: RwLock::new(HashMap::new()),
            next_transfer_id: AtomicU64::new(1),
            stats: Arc::new(SnapshotTransferStats::new()),
        }
    }

    /// Initiate a new transfer (sender side).
    pub fn initiate_transfer(
        &self,
        meta: SnapshotMeta,
        now_ms: u64,
    ) -> Result<TransferId, TransferError> {
        let active = {
            let transfers = self
                .transfers
                .read()
                .map_err(|_| TransferError::MaxConcurrentExceeded(0))?;
            transfers
                .values()
                .filter(|t| {
                    matches!(
                        t.state,
                        TransferState::Pending | TransferState::InProgress { .. }
                    )
                })
                .count()
        };

        if active >= self.config.max_concurrent {
            return Err(TransferError::MaxConcurrentExceeded(
                self.config.max_concurrent,
            ));
        }

        let id = TransferId(self.next_transfer_id.fetch_add(1, Ordering::Relaxed));

        let mut transfers = self
            .transfers
            .write()
            .map_err(|_| TransferError::MaxConcurrentExceeded(0))?;

        if transfers.contains_key(&id) {
            return Err(TransferError::TransferAlreadyExists(id));
        }

        let transfer = Transfer::new(id, meta, self.config.chunk_size_bytes, now_ms);
        transfers.insert(id, transfer);

        self.stats.active_transfers.fetch_add(1, Ordering::Relaxed);
        self.stats.total_initiated.fetch_add(1, Ordering::Relaxed);

        Ok(id)
    }

    /// Mark a chunk as sent (sender side advances to next chunk).
    pub fn mark_chunk_sent(
        &self,
        transfer_id: TransferId,
        chunk_index: u64,
        now_ms: u64,
    ) -> Result<(), TransferError> {
        let mut transfers = self
            .transfers
            .write()
            .map_err(|_| TransferError::TransferNotFound(transfer_id))?;

        let transfer = transfers
            .get_mut(&transfer_id)
            .ok_or(TransferError::TransferNotFound(transfer_id))?;

        match &mut transfer.state {
            TransferState::Pending => {
                transfer.state = TransferState::InProgress {
                    chunks_done: 0,
                    chunks_total: transfer.chunks.len() as u64,
                };
            }
            TransferState::InProgress { .. } => {}
            _ => return Err(TransferError::TransferNotInProgress(transfer_id)),
        }

        let chunk = transfer
            .chunks
            .get_mut(chunk_index as usize)
            .ok_or(TransferError::ChunkNotFound(chunk_index, transfer_id))?;

        chunk.state = ChunkTransferState::InFlight;
        transfer.last_progress_ms = now_ms;

        Ok(())
    }

    /// Mark a chunk as ACKed by receiver.
    pub fn mark_chunk_acked(
        &self,
        transfer_id: TransferId,
        chunk_index: u64,
        now_ms: u64,
    ) -> Result<(), TransferError> {
        let mut transfers = self
            .transfers
            .write()
            .map_err(|_| TransferError::TransferNotFound(transfer_id))?;

        let transfer = transfers
            .get_mut(&transfer_id)
            .ok_or(TransferError::TransferNotFound(transfer_id))?;

        let chunk = transfer
            .chunks
            .get_mut(chunk_index as usize)
            .ok_or(TransferError::ChunkNotFound(chunk_index, transfer_id))?;

        chunk.state = ChunkTransferState::Acked;
        transfer.last_progress_ms = now_ms;

        if let TransferState::InProgress {
            ref mut chunks_done,
            chunks_total,
        } = transfer.state
        {
            *chunks_done += 1;

            if *chunks_done >= chunks_total {
                let duration = now_ms.saturating_sub(transfer.created_at_ms);
                let bytes = transfer.meta.total_bytes;

                transfer.state = TransferState::Completed {
                    duration_ms: duration,
                    bytes_transferred: bytes,
                };

                self.stats.active_transfers.fetch_sub(1, Ordering::Relaxed);
                self.stats.total_completed.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .total_bytes_transferred
                    .fetch_add(bytes, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    /// Mark a chunk as failed, triggering retry or transfer failure.
    pub fn mark_chunk_failed(
        &self,
        transfer_id: TransferId,
        chunk_index: u64,
        reason: String,
        now_ms: u64,
    ) -> Result<(), TransferError> {
        let mut transfers = self
            .transfers
            .write()
            .map_err(|_| TransferError::TransferNotFound(transfer_id))?;

        let transfer = transfers
            .get_mut(&transfer_id)
            .ok_or(TransferError::TransferNotFound(transfer_id))?;

        let chunk = transfer
            .chunks
            .get_mut(chunk_index as usize)
            .ok_or(TransferError::ChunkNotFound(chunk_index, transfer_id))?;

        chunk.retry_count += 1;

        if chunk.retry_count > self.config.max_chunk_retries as u32 {
            chunk.state = ChunkTransferState::Failed {
                reason: reason.clone(),
            };

            let chunks_done = transfer
                .chunks
                .iter()
                .filter(|c| matches!(c.state, ChunkTransferState::Acked))
                .count() as u64;
            transfer.state = TransferState::Failed {
                reason,
                chunks_done,
            };

            self.stats.active_transfers.fetch_sub(1, Ordering::Relaxed);
            self.stats.total_failed.fetch_add(1, Ordering::Relaxed);
        } else {
            chunk.state = ChunkTransferState::Pending;
            self.stats
                .total_chunk_retries
                .fetch_add(1, Ordering::Relaxed);
        }

        transfer.last_progress_ms = now_ms;

        Ok(())
    }

    /// Get the next chunk to send for a transfer (returns None if waiting for ACKs or complete).
    pub fn next_chunk_to_send(&self, transfer_id: TransferId) -> Option<TransferChunk> {
        let transfers = match self.transfers.read() {
            Ok(t) => t,
            Err(_) => return None,
        };

        let transfer = transfers.get(&transfer_id)?;

        if !matches!(
            transfer.state,
            TransferState::Pending | TransferState::InProgress { .. }
        ) {
            return None;
        }

        for chunk in &transfer.chunks {
            if matches!(chunk.state, ChunkTransferState::Pending) {
                return Some(chunk.clone());
            }
        }

        None
    }

    /// Expire stale transfers that have not progressed within transfer_timeout_ms.
    pub fn expire_stale(&self, now_ms: u64) -> Vec<TransferId> {
        let mut expired = Vec::new();

        let mut transfers = match self.transfers.write() {
            Ok(t) => t,
            Err(_) => return expired,
        };

        let timeout = self.config.transfer_timeout_ms;

        for (id, transfer) in transfers.iter_mut() {
            if matches!(
                transfer.state,
                TransferState::Pending | TransferState::InProgress { .. }
            ) {
                if now_ms.saturating_sub(transfer.last_progress_ms) > timeout {
                    let chunks_done = transfer
                        .chunks
                        .iter()
                        .filter(|c| matches!(c.state, ChunkTransferState::Acked))
                        .count() as u64;
                    transfer.state = TransferState::Failed {
                        reason: "transfer timeout".to_string(),
                        chunks_done,
                    };

                    self.stats.active_transfers.fetch_sub(1, Ordering::Relaxed);
                    self.stats
                        .total_stale_expired
                        .fetch_add(1, Ordering::Relaxed);

                    expired.push(*id);
                }
            }
        }

        expired
    }

    /// Cancel a transfer.
    pub fn cancel_transfer(&self, transfer_id: TransferId) -> Result<(), TransferError> {
        let mut transfers = self
            .transfers
            .write()
            .map_err(|_| TransferError::TransferNotFound(transfer_id))?;

        let transfer = transfers
            .get_mut(&transfer_id)
            .ok_or(TransferError::TransferNotFound(transfer_id))?;

        match &transfer.state {
            TransferState::Pending | TransferState::InProgress { .. } => {
                transfer.state = TransferState::Cancelled;
                self.stats.active_transfers.fetch_sub(1, Ordering::Relaxed);
                self.stats.total_cancelled.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            _ => Err(TransferError::TransferNotInProgress(transfer_id)),
        }
    }

    /// Get state of a transfer.
    pub fn transfer_state(&self, transfer_id: TransferId) -> Option<TransferState> {
        let transfers = match self.transfers.read() {
            Ok(t) => t,
            Err(_) => return None,
        };

        transfers.get(&transfer_id).map(|t| t.state.clone())
    }

    /// Get stats snapshot.
    pub fn stats(&self) -> SnapshotTransferStatsSnapshot {
        self.stats.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node_id(seed: u8) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = seed;
        id
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn make_meta() -> SnapshotMeta {
        SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 1_000_000,
            chunk_count: 10,
            shard_id: Some(1),
            term: 5,
            index: 100,
        }
    }

    #[test]
    fn test_create_manager() {
        let config = SnapshotTransferConfig::default();
        let manager = SnapshotTransferManager::new(config);
        let stats = manager.stats();

        assert_eq!(stats.active_transfers, 0);
    }

    #[test]
    fn test_initiate_single_transfer() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms());
        assert!(id.is_ok());

        let stats = manager.stats();
        assert_eq!(stats.total_initiated, 1);
        assert_eq!(stats.active_transfers, 1);
    }

    #[test]
    fn test_max_concurrent_limit() {
        let config = SnapshotTransferConfig {
            max_concurrent: 2,
            ..Default::default()
        };
        let manager = SnapshotTransferManager::new(config);

        manager.initiate_transfer(make_meta(), now_ms()).unwrap();
        manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let result = manager.initiate_transfer(make_meta(), now_ms());
        assert!(matches!(
            result,
            Err(TransferError::MaxConcurrentExceeded(_))
        ));
    }

    #[test]
    fn test_transfer_not_found() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let result = manager.transfer_state(TransferId(999));
        assert!(result.is_none());
    }

    #[test]
    fn test_mark_chunk_sent() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let result = manager.mark_chunk_sent(id, 0, now_ms());
        assert!(result.is_ok());

        let state = manager.transfer_state(id).unwrap();
        assert!(matches!(state, TransferState::InProgress { .. }));
    }

    #[test]
    fn test_mark_chunk_acked() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();
        let result = manager.mark_chunk_acked(id, 0, now_ms());
        assert!(result.is_ok());
    }

    #[test]
    fn test_all_chunks_acked_completes_transfer() {
        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 100_000,
            chunk_count: 1,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();

        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();
        manager.mark_chunk_acked(id, 0, now_ms()).unwrap();

        let state = manager.transfer_state(id).unwrap();
        assert!(matches!(state, TransferState::Completed { .. }));
    }

    #[test]
    fn test_chunk_not_found() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let result = manager.mark_chunk_acked(id, 999, now_ms());
        assert!(matches!(result, Err(TransferError::ChunkNotFound(_, _))));
    }

    #[test]
    fn test_mark_chunk_failed_triggers_retry() {
        let config = SnapshotTransferConfig {
            max_chunk_retries: 2,
            ..Default::default()
        };
        let manager = SnapshotTransferManager::new(config);

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();

        manager
            .mark_chunk_failed(id, 0, "network error".to_string(), now_ms())
            .unwrap();
        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();

        let result = manager.mark_chunk_failed(id, 0, "network error".to_string(), now_ms());
        assert!(result.is_ok());
    }

    #[test]
    fn test_chunk_fails_after_max_retries() {
        let config = SnapshotTransferConfig {
            max_chunk_retries: 1,
            ..Default::default()
        };
        let manager = SnapshotTransferManager::new(config);

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();
        manager
            .mark_chunk_failed(id, 0, "network error".to_string(), now_ms())
            .unwrap();
        manager
            .mark_chunk_failed(id, 0, "network error".to_string(), now_ms())
            .unwrap();

        let state = manager.transfer_state(id).unwrap();
        assert!(matches!(state, TransferState::Failed { .. }));
    }

    #[test]
    fn test_next_chunk_to_send() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let chunk = manager.next_chunk_to_send(id);
        assert!(chunk.is_some());
        assert_eq!(chunk.unwrap().chunk_index, 0);
    }

    #[test]
    fn test_next_chunk_returns_none_when_all_sent() {
        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 100_000,
            chunk_count: 1,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();
        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();

        let chunk = manager.next_chunk_to_send(id);
        assert!(chunk.is_none());
    }

    #[test]
    fn test_cancel_transfer() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let result = manager.cancel_transfer(id);
        assert!(result.is_ok());

        let state = manager.transfer_state(id).unwrap();
        assert!(matches!(state, TransferState::Cancelled));
    }

    #[test]
    fn test_cancel_completed_transfer_fails() {
        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 100_000,
            chunk_count: 1,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();
        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();
        manager.mark_chunk_acked(id, 0, now_ms()).unwrap();

        let result = manager.cancel_transfer(id);
        assert!(matches!(
            result,
            Err(TransferError::TransferNotInProgress(_))
        ));
    }

    #[test]
    fn test_expire_stale_transfers() {
        let config = SnapshotTransferConfig {
            transfer_timeout_ms: 100,
            ..Default::default()
        };
        let manager = SnapshotTransferManager::new(config);

        let id = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let expired = manager.expire_stale(now_ms() + 50);

        assert!(expired.is_empty());
    }

    #[test]
    fn test_expire_stale_removes_timed_out() {
        let config = SnapshotTransferConfig {
            transfer_timeout_ms: 100,
            ..Default::default()
        };
        let manager = SnapshotTransferManager::new(config);

        let _ = manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let expired = manager.expire_stale(now_ms() + 200);

        assert!(!expired.is_empty());

        let stats = manager.stats();
        assert_eq!(stats.total_stale_expired, 1);
    }

    #[test]
    fn test_stats_snapshot() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        manager.initiate_transfer(make_meta(), now_ms()).unwrap();

        let snapshot = manager.stats();

        assert_eq!(snapshot.total_initiated, 1);
        assert_eq!(snapshot.active_transfers, 1);
    }

    #[test]
    fn test_config_defaults() {
        let config = SnapshotTransferConfig::default();

        assert_eq!(config.chunk_size_bytes, 1_048_576);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.transfer_timeout_ms, 60_000);
        assert_eq!(config.max_chunk_retries, 3);
    }

    #[test]
    fn test_chunk_transfer_state_variants() {
        let pending = ChunkTransferState::Pending;
        let in_flight = ChunkTransferState::InFlight;
        let received = ChunkTransferState::Received;
        let acked = ChunkTransferState::Acked;
        let failed = ChunkTransferState::Failed {
            reason: "error".to_string(),
        };

        assert!(matches!(pending, ChunkTransferState::Pending));
        assert!(matches!(in_flight, ChunkTransferState::InFlight));
        assert!(matches!(received, ChunkTransferState::Received));
        assert!(matches!(acked, ChunkTransferState::Acked));
        assert!(matches!(failed, ChunkTransferState::Failed { .. }));
    }

    #[test]
    fn test_transfer_state_variants() {
        let pending = TransferState::Pending;
        let in_progress = TransferState::InProgress {
            chunks_done: 5,
            chunks_total: 10,
        };
        let completed = TransferState::Completed {
            duration_ms: 1000,
            bytes_transferred: 1_000_000,
        };
        let failed = TransferState::Failed {
            reason: "error".to_string(),
            chunks_done: 3,
        };
        let cancelled = TransferState::Cancelled;

        assert!(matches!(pending, TransferState::Pending));
        assert!(matches!(in_progress, TransferState::InProgress { .. }));
        assert!(matches!(completed, TransferState::Completed { .. }));
        assert!(matches!(failed, TransferState::Failed { .. }));
        assert!(matches!(cancelled, TransferState::Cancelled));
    }

    #[test]
    fn test_transfer_id_equality() {
        let id1 = TransferId(100);
        let id2 = TransferId(100);
        let id3 = TransferId(200);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_snapshot_meta_fields() {
        let meta = make_meta();

        assert_eq!(meta.snapshot_id, 1);
        assert_eq!(meta.total_bytes, 1_000_000);
        assert_eq!(meta.chunk_count, 10);
        assert_eq!(meta.shard_id, Some(1));
        assert_eq!(meta.term, 5);
        assert_eq!(meta.index, 100);
    }

    #[test]
    fn test_transfer_chunk_fields() {
        let chunk = TransferChunk {
            chunk_index: 5,
            offset_bytes: 5_242_880,
            size_bytes: 1_048_576,
            checksum: 0x12345678,
            state: ChunkTransferState::Pending,
            retry_count: 0,
        };

        assert_eq!(chunk.chunk_index, 5);
        assert_eq!(chunk.state, ChunkTransferState::Pending);
        assert_eq!(chunk.retry_count, 0);
    }

    #[test]
    fn test_concurrent_initiate_operations() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig {
            max_concurrent: 5,
            ..Default::default()
        });

        let meta = make_meta();

        for _ in 0..5 {
            let result = manager.initiate_transfer(meta.clone(), now_ms());
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_single_chunk_transfer() {
        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 500_000,
            chunk_count: 1,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();

        let chunk = manager.next_chunk_to_send(id);
        assert!(chunk.is_some());

        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();

        let chunk = manager.next_chunk_to_send(id);
        assert!(chunk.is_none());

        manager.mark_chunk_acked(id, 0, now_ms()).unwrap();

        let state = manager.transfer_state(id).unwrap();
        if let TransferState::Completed {
            bytes_transferred, ..
        } = state
        {
            assert_eq!(bytes_transferred, 500_000);
        } else {
            panic!("expected Completed");
        }
    }

    #[test]
    fn test_many_chunks_transfer() {
        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 100_000_000,
            chunk_count: 100,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();

        for i in 0..100 {
            manager.mark_chunk_sent(id, i, now_ms()).unwrap();
            manager.mark_chunk_acked(id, i, now_ms()).unwrap();
        }

        let state = manager.transfer_state(id).unwrap();
        assert!(matches!(state, TransferState::Completed { .. }));
    }

    #[test]
    fn test_stats_bytes_transferred() {
        let meta = SnapshotMeta {
            snapshot_id: 1,
            source_node: make_node_id(1),
            dest_node: make_node_id(2),
            total_bytes: 100_000,
            chunk_count: 1,
            shard_id: Some(1),
            term: 5,
            index: 100,
        };

        let manager = SnapshotTransferManager::new(SnapshotTransferConfig::default());

        let id = manager.initiate_transfer(meta, now_ms()).unwrap();
        manager.mark_chunk_sent(id, 0, now_ms()).unwrap();
        manager.mark_chunk_acked(id, 0, now_ms()).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.total_completed, 1);
        assert!(stats.total_bytes_transferred > 0);
    }
}
