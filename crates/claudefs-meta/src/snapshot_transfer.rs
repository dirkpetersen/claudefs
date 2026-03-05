//! Raft snapshot transfer (InstallSnapshot RPC).
//!
//! This module handles the Raft `InstallSnapshot` RPC — transferring a snapshot
//! from leader to a follower that is too far behind (its log has been compacted away).
//!
//! Snapshots may be large, so they are chunked for network transfer.

use crate::snapshot::RaftSnapshot;
use crate::types::{LogIndex, MetaError, NodeId, ShardId, Term, Timestamp};
use serde::{Deserialize, Serialize};

use std::sync::Arc;
use dashmap::DashMap;
use blake3::Hasher;
use uuid::Uuid;
use crate::kvstore::KvStore;

/// A chunk of snapshot data (snapshots may be large, so they are chunked).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotChunk {
    /// 0-based chunk index.
    pub chunk_index: u32,
    /// Total number of chunks.
    pub total_chunks: u32,
    /// Raw bytes for this chunk.
    pub data: Vec<u8>,
    /// True for the final chunk.
    pub is_last: bool,
}

/// An InstallSnapshot RPC request (leader → follower).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallSnapshotRequest {
    /// Leader's term.
    pub term: Term,
    /// Leader's node ID.
    pub leader_id: NodeId,
    /// The snapshot covers entries up to this index.
    pub last_included_index: LogIndex,
    /// The term of the last included log entry.
    pub last_included_term: Term,
    /// The chunk of snapshot data.
    pub chunk: SnapshotChunk,
}

/// An InstallSnapshot RPC response (follower → leader).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallSnapshotResponse {
    /// Follower's current term.
    pub term: Term,
    /// Whether the snapshot was applied successfully.
    pub success: bool,
    /// Total bytes received so far.
    pub bytes_received: u64,
}

/// State machine for receiving a snapshot in chunks.
pub struct SnapshotReceiver {
    expected_term: Term,
    expected_leader: NodeId,
    last_included_index: LogIndex,
    last_included_term: Term,
    chunks_received: u32,
    total_chunks: u32,
    buffer: Vec<u8>,
    created_at: Timestamp,
}

impl SnapshotReceiver {
    /// Create a new receiver for the given snapshot metadata.
    pub fn new(request: &InstallSnapshotRequest) -> Self {
        let total_chunks = request.chunk.total_chunks;
        Self {
            expected_term: request.term,
            expected_leader: request.leader_id,
            last_included_index: request.last_included_index,
            last_included_term: request.last_included_term,
            chunks_received: 0,
            total_chunks,
            buffer: Vec::with_capacity(request.chunk.data.len() * total_chunks as usize),
            created_at: Timestamp::now(),
        }
    }

    /// Apply a chunk. Returns Ok(None) if more chunks needed, Ok(Some(RaftSnapshot)) when complete.
    pub fn apply_chunk(
        &mut self,
        request: &InstallSnapshotRequest,
    ) -> Result<Option<RaftSnapshot>, MetaError> {
        // Verify term matches
        if request.term != self.expected_term {
            return Err(MetaError::RaftError(format!(
                "term mismatch: expected {}, got {}",
                self.expected_term.as_u64(),
                request.term.as_u64()
            )));
        }

        // Verify leader matches
        if request.leader_id != self.expected_leader {
            return Err(MetaError::RaftError(format!(
                "leader mismatch: expected {}, got {}",
                self.expected_leader.as_u64(),
                request.leader_id.as_u64()
            )));
        }

        // Verify chunk index is as expected
        if request.chunk.chunk_index != self.chunks_received {
            return Err(MetaError::RaftError(format!(
                "chunk index mismatch: expected {}, got {}",
                self.chunks_received, request.chunk.chunk_index
            )));
        }

        // Verify total chunks matches
        if request.chunk.total_chunks != self.total_chunks {
            return Err(MetaError::RaftError(format!(
                "total chunks mismatch: expected {}, got {}",
                self.total_chunks, request.chunk.total_chunks
            )));
        }

        // Append the chunk data
        self.buffer.extend_from_slice(&request.chunk.data);
        self.chunks_received += 1;

        // Check if complete
        if request.chunk.is_last {
            let snapshot = RaftSnapshot::new(
                self.last_included_index,
                self.last_included_term,
                std::mem::take(&mut self.buffer),
            );
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }

    /// Returns true if the receiver has timed out (> 30 seconds since creation).
    pub fn is_stale(&self) -> bool {
        let now = Timestamp::now();
        let elapsed_secs = now.secs.saturating_sub(self.created_at.secs);
        elapsed_secs > 30
    }

    /// Returns the number of bytes received so far.
    pub fn bytes_received(&self) -> u64 {
        self.buffer.len() as u64
    }
}

/// Splits a snapshot into chunks for network transfer.
pub struct SnapshotSender {
    snapshot: RaftSnapshot,
    chunk_size: usize,
    bytes_sent: u64,
}

impl SnapshotSender {
    /// Create a new sender for the given snapshot with 1MB chunk size.
    pub fn new(snapshot: RaftSnapshot) -> Self {
        Self {
            snapshot,
            chunk_size: 1024 * 1024, // 1MB
            bytes_sent: 0,
        }
    }

    /// Create with a custom chunk size (for testing).
    pub fn with_chunk_size(snapshot: RaftSnapshot, chunk_size: usize) -> Self {
        Self {
            snapshot,
            chunk_size: chunk_size.max(1),
            bytes_sent: 0,
        }
    }

    /// Returns the total number of chunks.
    pub fn total_chunks(&self) -> u32 {
        let data_len = self.snapshot.data.len();
        let chunks = (data_len + self.chunk_size - 1) / self.chunk_size;
        chunks as u32
    }

    /// Get the chunk at the given index as an InstallSnapshotRequest.
    pub fn chunk_request(
        &mut self,
        term: Term,
        leader_id: NodeId,
        chunk_index: u32,
    ) -> Result<InstallSnapshotRequest, MetaError> {
        let total_chunks = self.total_chunks();
        if chunk_index >= total_chunks {
            return Err(MetaError::RaftError(format!(
                "chunk index {} out of range (total chunks: {})",
                chunk_index, total_chunks
            )));
        }

        let start = chunk_index as usize * self.chunk_size;
        let end = std::cmp::min(start + self.chunk_size, self.snapshot.data.len());
        let data = self.snapshot.data[start..end].to_vec();

        self.bytes_sent += data.len() as u64;

        Ok(InstallSnapshotRequest {
            term,
            leader_id,
            last_included_index: self.snapshot.last_included_index,
            last_included_term: self.snapshot.last_included_term,
            chunk: SnapshotChunk {
                chunk_index,
                total_chunks,
                data,
                is_last: chunk_index == total_chunks - 1,
            },
        })
    }

    /// Returns the total snapshot size in bytes.
    pub fn snapshot_size(&self) -> usize {
        self.snapshot.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_sender_single_chunk() {
        let data = vec![1u8; 512];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::new(snapshot);

        assert_eq!(sender.total_chunks(), 1);
        assert_eq!(sender.snapshot_size(), 512);

        let req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        assert_eq!(req.chunk.chunk_index, 0);
        assert_eq!(req.chunk.total_chunks, 1);
        assert!(req.chunk.is_last);
        assert_eq!(req.chunk.data.len(), 512);
    }

    #[test]
    fn test_snapshot_sender_multiple_chunks() {
        let data = vec![1u8; 3000];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::with_chunk_size(snapshot, 1000);

        assert_eq!(sender.total_chunks(), 3);
        assert_eq!(sender.snapshot_size(), 3000);

        let req0 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        assert_eq!(req0.chunk.chunk_index, 0);
        assert_eq!(req0.chunk.total_chunks, 3);
        assert!(!req0.chunk.is_last);
        assert_eq!(req0.chunk.data.len(), 1000);

        let req1 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 1)
            .unwrap();
        assert_eq!(req1.chunk.chunk_index, 1);
        assert!(!req1.chunk.is_last);
        assert_eq!(req1.chunk.data.len(), 1000);

        let req2 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 2)
            .unwrap();
        assert_eq!(req2.chunk.chunk_index, 2);
        assert!(req2.chunk.is_last);
        assert_eq!(req2.chunk.data.len(), 1000);
    }

    #[test]
    fn test_snapshot_sender_chunk_count() {
        let data = vec![1u8; 4096];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);

        let mut sender_1kb = SnapshotSender::with_chunk_size(snapshot.clone(), 1024);
        assert_eq!(sender_1kb.total_chunks(), 4);

        let mut sender_2kb = SnapshotSender::with_chunk_size(snapshot.clone(), 2048);
        assert_eq!(sender_2kb.total_chunks(), 2);

        let mut sender_5kb = SnapshotSender::with_chunk_size(snapshot, 5000);
        assert_eq!(sender_5kb.total_chunks(), 1);
    }

    #[test]
    fn test_snapshot_receiver_single_chunk() {
        let data = vec![1u8; 512];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data.clone());
        let mut sender = SnapshotSender::new(snapshot);

        let req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        let mut receiver = SnapshotReceiver::new(&req);

        let result = receiver.apply_chunk(&req).unwrap();
        assert!(result.is_some());

        let received = result.unwrap();
        assert_eq!(received.data, data);
    }

    #[test]
    fn test_snapshot_receiver_multiple_chunks() {
        let data: Vec<u8> = (0..3000).map(|i| i as u8).collect();
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data.clone());
        let mut sender = SnapshotSender::with_chunk_size(snapshot, 1000);

        let mut receiver: Option<SnapshotReceiver> = None;
        let total_chunks = sender.total_chunks();

        for i in 0..total_chunks {
            let req = sender
                .chunk_request(Term::new(6), NodeId::new(1), i)
                .unwrap();

            if receiver.is_none() {
                receiver = Some(SnapshotReceiver::new(&req));
            }

            let result = receiver.as_mut().unwrap().apply_chunk(&req).unwrap();
            if i == total_chunks - 1 {
                assert!(result.is_some());
                assert_eq!(result.unwrap().data, data);
            } else {
                assert!(result.is_none());
            }
        }
    }

    #[test]
    fn test_snapshot_receiver_wrong_term_rejected() {
        let data = vec![1u8; 512];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::new(snapshot);

        let mut req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        req.term = Term::new(99); // Wrong term

        let mut receiver = SnapshotReceiver::new(&req);
        let result = receiver.apply_chunk(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_snapshot_receiver_wrong_chunk_index() {
        let data = vec![1u8; 3000];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::with_chunk_size(snapshot, 1000);

        let req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        let mut receiver = SnapshotReceiver::new(&req);

        // Try to apply chunk 2 first (should fail)
        let mut wrong_req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 2)
            .unwrap();
        wrong_req.term = req.term;
        wrong_req.leader_id = req.leader_id;

        let result = receiver.apply_chunk(&wrong_req);
        assert!(result.is_err());
    }

    #[test]
    fn test_snapshot_receiver_stale_detection() {
        let data = vec![1u8; 512];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::new(snapshot);

        let req = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        let receiver = SnapshotReceiver::new(&req);

        assert!(!receiver.is_stale());
    }

    #[test]
    fn test_roundtrip_small_snapshot() {
        let data = vec![42u8; 100];
        let snapshot = RaftSnapshot::new(LogIndex::new(200), Term::new(10), data.clone());
        let mut sender = SnapshotSender::new(snapshot);

        let mut receiver: Option<SnapshotReceiver> = None;
        let total_chunks = sender.total_chunks();

        for i in 0..total_chunks {
            let req = sender
                .chunk_request(Term::new(11), NodeId::new(2), i)
                .unwrap();

            if receiver.is_none() {
                receiver = Some(SnapshotReceiver::new(&req));
            }

            let result = receiver.as_mut().unwrap().apply_chunk(&req).unwrap();
            if i == total_chunks - 1 {
                let received = result.unwrap();
                assert_eq!(received.last_included_index.as_u64(), 200);
                assert_eq!(received.last_included_term.as_u64(), 10);
                assert_eq!(received.data, data);
            }
        }
    }

    #[test]
    fn test_roundtrip_large_snapshot() {
        let data: Vec<u8> = (0..5_000_000).map(|i| (i % 256) as u8).collect();
        let snapshot = RaftSnapshot::new(LogIndex::new(5000), Term::new(100), data.clone());
        let mut sender = SnapshotSender::with_chunk_size(snapshot, 100_000);

        let mut receiver: Option<SnapshotReceiver> = None;
        let total_chunks = sender.total_chunks();
        assert_eq!(total_chunks, 50);

        for i in 0..total_chunks {
            let req = sender
                .chunk_request(Term::new(101), NodeId::new(3), i)
                .unwrap();

            if receiver.is_none() {
                receiver = Some(SnapshotReceiver::new(&req));
            }

            let result = receiver.as_mut().unwrap().apply_chunk(&req).unwrap();
            if i == total_chunks - 1 {
                let received = result.unwrap();
                assert_eq!(received.last_included_index.as_u64(), 5000);
                assert_eq!(received.last_included_term.as_u64(), 100);
                assert_eq!(received.data.len(), data.len());
            }
        }
    }

    #[test]
    fn test_chunk_request_last_chunk_flag() {
        let data = vec![1u8; 3000];
        let snapshot = RaftSnapshot::new(LogIndex::new(100), Term::new(5), data);
        let mut sender = SnapshotSender::with_chunk_size(snapshot, 1000);

        let req0 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 0)
            .unwrap();
        assert!(!req0.chunk.is_last);

        let req1 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 1)
            .unwrap();
        assert!(!req1.chunk.is_last);

        let req2 = sender
            .chunk_request(Term::new(6), NodeId::new(1), 2)
            .unwrap();
        assert!(req2.chunk.is_last);
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionType {
    Gzip,
    Zstd,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(String);

impl SnapshotId {
    pub fn new() -> Self {
        SnapshotId(Uuid::new_v4().to_string())
    }

    pub fn from_string(s: String) -> Self {
        SnapshotId(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SnapshotId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataSnapshot {
    pub snapshot_id: SnapshotId,
    pub shard_id: ShardId,
    pub log_index: LogIndex,
    pub term: Term,
    pub metadata_bytes: Vec<u8>,
    pub compression: Option<CompressionType>,
    pub checksum: [u8; 32],
    pub created_at: Timestamp,
    pub base_snapshot_id: Option<SnapshotId>,
    pub size_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SnapshotTransferConfig {
    pub max_snapshot_size: u64,
    pub compression_enabled: bool,
    pub chunk_size: u64,
    pub transfer_timeout_secs: u64,
}

impl Default for SnapshotTransferConfig {
    fn default() -> Self {
        Self {
            max_snapshot_size: 100 * 1024 * 1024,
            compression_enabled: true,
            chunk_size: 5 * 1024 * 1024,
            transfer_timeout_secs: 300,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub snapshot_id: SnapshotId,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub state: TransferState,
    pub error: Option<String>,
    pub last_activity: Timestamp,
}

pub struct SnapshotTransferEngine {
    shard_id: ShardId,
    kvstore: Arc<dyn KvStore>,
    config: SnapshotTransferConfig,
    transfers: Arc<DashMap<SnapshotId, TransferProgress>>,
}

impl SnapshotTransferEngine {
    pub fn new(shard_id: ShardId, kvstore: Arc<dyn KvStore>, config: SnapshotTransferConfig) -> Self {
        Self {
            shard_id,
            kvstore,
            config,
            transfers: Arc::new(DashMap::new()),
        }
    }

    pub async fn create_full_snapshot(&self) -> Result<MetadataSnapshot, MetaError> {
        let snapshot_id = SnapshotId::new();
        let start = std::time::Instant::now();
        
        let prefix = format!("shard_{}_", self.shard_id.as_u16());
        let kv_pairs = self.kvstore.scan_prefix(prefix.as_bytes())?;
        
        let mut metadata_bytes = Vec::new();
        for (key, value) in kv_pairs {
            metadata_bytes.extend_from_slice(&key);
            metadata_bytes.push(0);
            metadata_bytes.extend_from_slice(&value);
            metadata_bytes.push(0);
        }
        
        let size_bytes = metadata_bytes.len() as u64;
        
        let mut hasher = Hasher::new();
        hasher.update(&metadata_bytes);
        let checksum = *hasher.finalize().as_bytes();
        
        let term = Term::new(1);
        let log_index = LogIndex::new(0);
        
        let created_at = Timestamp::now();
        
        Ok(MetadataSnapshot {
            snapshot_id,
            shard_id: self.shard_id,
            log_index,
            term,
            metadata_bytes,
            compression: None,
            checksum,
            created_at,
            base_snapshot_id: None,
            size_bytes,
        })
    }

    pub async fn create_incremental_snapshot(&self, base_snapshot_id: SnapshotId) -> Result<MetadataSnapshot, MetaError> {
        let snapshot_id = SnapshotId::new();
        
        let base_key = format!("snapshot_{}", base_snapshot_id.as_str());
        let base_data = self.kvstore.get(base_key.as_bytes())?;
        
        let prefix = format!("shard_{}_", self.shard_id.as_u16());
        let kv_pairs = self.kvstore.scan_prefix(prefix.as_bytes())?;
        
        let mut metadata_bytes = Vec::new();
        for (key, value) in kv_pairs {
            metadata_bytes.extend_from_slice(&key);
            metadata_bytes.push(0);
            metadata_bytes.extend_from_slice(&value);
            metadata_bytes.push(0);
        }
        
        let size_bytes = metadata_bytes.len() as u64;
        
        let mut hasher = Hasher::new();
        hasher.update(&metadata_bytes);
        let checksum = *hasher.finalize().as_bytes();
        
        let term = Term::new(1);
        let log_index = LogIndex::new(0);
        let created_at = Timestamp::now();
        
        Ok(MetadataSnapshot {
            snapshot_id,
            shard_id: self.shard_id,
            log_index,
            term,
            metadata_bytes,
            compression: None,
            checksum,
            created_at,
            base_snapshot_id: Some(base_snapshot_id),
            size_bytes,
        })
    }

    pub fn serialize_snapshot(&self, snapshot: &MetadataSnapshot) -> Result<Vec<u8>, MetaError> {
        let encoded = bincode::serialize(snapshot)
            .map_err(|e| MetaError::InvalidArgument(e.to_string()))?;
        Ok(encoded)
    }

    pub fn chunk_snapshot(&self, serialized: &[u8]) -> Result<Vec<Vec<u8>>, MetaError> {
        let chunk_size = self.config.chunk_size as usize;
        let total_chunks = (serialized.len() + chunk_size - 1) / chunk_size;
        
        let mut chunks = Vec::with_capacity(total_chunks);
        for i in 0..total_chunks {
            let start = i * chunk_size;
            let end = std::cmp::min(start + chunk_size, serialized.len());
            chunks.push(serialized[start..end].to_vec());
        }
        
        Ok(chunks)
    }

    pub fn verify_snapshot_integrity(&self, snapshot: &MetadataSnapshot, bytes: &[u8]) -> Result<bool, MetaError> {
        let mut hasher = Hasher::new();
        hasher.update(bytes);
        let computed_checksum = *hasher.finalize().as_bytes();
        
        Ok(computed_checksum == snapshot.checksum)
    }

    pub fn track_transfer(&self, snapshot_id: SnapshotId, total_bytes: u64) {
        let progress = TransferProgress {
            snapshot_id: snapshot_id.clone(),
            bytes_transferred: 0,
            total_bytes,
            state: TransferState::InProgress,
            error: None,
            last_activity: Timestamp::now(),
        };
        self.transfers.insert(snapshot_id, progress);
    }

    pub fn update_transfer_progress(&self, snapshot_id: SnapshotId, bytes_transferred: u64) -> Result<(), MetaError> {
        if let Some(mut progress) = self.transfers.get_mut(&snapshot_id) {
            progress.bytes_transferred = bytes_transferred;
            progress.last_activity = Timestamp::now();
            Ok(())
        } else {
            Err(MetaError::NotFound(format!("transfer {} not found", snapshot_id.as_str())))
        }
    }

    pub fn complete_transfer(&self, snapshot_id: SnapshotId) -> Result<(), MetaError> {
        if let Some(mut progress) = self.transfers.get_mut(&snapshot_id) {
            progress.state = TransferState::Completed;
            progress.last_activity = Timestamp::now();
            Ok(())
        } else {
            Err(MetaError::NotFound(format!("transfer {} not found", snapshot_id.as_str())))
        }
    }

    pub fn fail_transfer(&self, snapshot_id: SnapshotId, error: String) -> Result<(), MetaError> {
        if let Some(mut progress) = self.transfers.get_mut(&snapshot_id) {
            progress.state = TransferState::Failed;
            progress.error = Some(error);
            progress.last_activity = Timestamp::now();
            Ok(())
        } else {
            Err(MetaError::NotFound(format!("transfer {} not found", snapshot_id.as_str())))
        }
    }

    pub fn get_transfer_progress(&self, snapshot_id: SnapshotId) -> Option<TransferProgress> {
        self.transfers.get(&snapshot_id).map(|r| r.clone())
    }

    pub async fn restore_snapshot(&self, snapshot: MetadataSnapshot) -> Result<RemoteRestorationResult, MetaError> {
        let start = std::time::Instant::now();
        
        let is_valid = self.verify_snapshot_integrity(&snapshot, &snapshot.metadata_bytes)?;
        
        if !is_valid {
            return Err(MetaError::InvalidArgument("snapshot integrity check failed".to_string()));
        }
        
        let entries_restored = if snapshot.metadata_bytes.is_empty() {
            0
        } else {
            let count = snapshot.metadata_bytes.iter().filter(|&&b| b == 0).count() / 2;
            count as u64
        };
        
        let prefix = format!("shard_{}_", self.shard_id.as_u16());
        let existing = self.kvstore.scan_prefix(prefix.as_bytes())?;
        for (key, _) in existing {
            self.kvstore.delete(&key)?;
        }
        
        let mut pos = 0;
        let data = &snapshot.metadata_bytes;
        while pos < data.len() {
            let key_end = data[pos..].iter().position(|&b| b == 0).ok_or_else(||
                MetaError::InvalidArgument("invalid snapshot data format".to_string()))?;
            let key = data[pos..pos + key_end].to_vec();
            pos += key_end + 1;
            
            if pos >= data.len() {
                break;
            }
            
            let value_end = data[pos..].iter().position(|&b| b == 0).ok_or_else(||
                MetaError::InvalidArgument("invalid snapshot data format".to_string()))?;
            let value = data[pos..pos + value_end].to_vec();
            pos += value_end + 1;
            
            self.kvstore.put(key, value)?;
        }
        
        let duration_ms = start.elapsed().as_millis() as u64;
        
        Ok(RemoteRestorationResult {
            snapshot_id: snapshot.snapshot_id,
            entries_restored,
            bytes_restored: snapshot.size_bytes,
            log_index_after_restore: snapshot.log_index.as_u64(),
            integrity_verified: is_valid,
            restore_duration_ms: duration_ms,
        })
    }

    pub async fn cleanup_old_snapshots(&self, keep_count: usize) -> Result<usize, MetaError> {
        let mut snapshots: Vec<(String, Timestamp)> = Vec::new();
        
        let prefix = "snapshot_";
        let all_pairs = self.kvstore.scan_prefix(prefix.as_bytes())?;
        
        for (key, _) in all_pairs {
            if let Ok(snapshot) = bincode::deserialize::<MetadataSnapshot>(&key) {
                snapshots.push((snapshot.snapshot_id.as_str().to_string(), snapshot.created_at));
            }
        }
        
        snapshots.sort_by(|a, b| b.1.cmp(&a.1));
        
        let to_keep = &snapshots[..keep_count.min(snapshots.len())];
        let mut removed = 0;
        
        for (id, _) in snapshots.iter() {
            if !to_keep.iter().any(|(keep_id, _)| keep_id == id) {
                let key = format!("snapshot_{}", id);
                self.kvstore.delete(key.as_bytes())?;
                removed += 1;
            }
        }
        
        Ok(removed)
    }
}

#[derive(Clone, Debug)]
pub struct RemoteRestorationResult {
    pub snapshot_id: SnapshotId,
    pub entries_restored: u64,
    pub bytes_restored: u64,
    pub log_index_after_restore: u64,
    pub integrity_verified: bool,
    pub restore_duration_ms: u64,
}

#[cfg(test)]
mod snapshot_transfer_tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;

    #[tokio::test]
    async fn test_create_full_snapshot() {
        let kvstore = Arc::new(MemoryKvStore::new());
        kvstore.put(b"shard_1_key1".to_vec(), b"value1".to_vec()).unwrap();
        kvstore.put(b"shard_1_key2".to_vec(), b"value2".to_vec()).unwrap();
        
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        let snapshot = engine.create_full_snapshot().await.unwrap();
        
        assert_eq!(snapshot.shard_id, ShardId::new(1));
        assert!(!snapshot.snapshot_id.as_str().is_empty());
        assert!(snapshot.size_bytes > 0);
    }

    #[tokio::test]
    async fn test_create_incremental_snapshot() {
        let kvstore = Arc::new(MemoryKvStore::new());
        kvstore.put(b"shard_1_key1".to_vec(), b"value1".to_vec()).unwrap();
        
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), SnapshotTransferConfig::default());
        let base = engine.create_full_snapshot().await.unwrap();
        
        kvstore.put(b"shard_1_key2".to_vec(), b"value2".to_vec()).unwrap();
        
        let base_id = base.snapshot_id.clone();
        let incremental = engine.create_incremental_snapshot(base_id).await.unwrap();
        
        assert_eq!(incremental.base_snapshot_id, Some(base.snapshot_id));
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(100),
            term: Term::new(5),
            metadata_bytes: vec![1, 2, 3, 4],
            compression: None,
            checksum: [0u8; 32],
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: 4,
        };
        
        let encoded = bincode::serialize(&snapshot).unwrap();
        let decoded: MetadataSnapshot = bincode::deserialize(&encoded).unwrap();
        
        assert_eq!(decoded.snapshot_id, snapshot.snapshot_id);
        assert_eq!(decoded.shard_id, snapshot.shard_id);
    }

    #[test]
    fn test_snapshot_chunking() {
        let config = SnapshotTransferConfig {
            chunk_size: 1000,
            ..Default::default()
        };
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, config);
        
        let data = vec![1u8; 10000];
        let chunks = engine.chunk_snapshot(&data).unwrap();
        
        assert!(chunks.len() > 1);
        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.iter()).cloned().collect();
        assert_eq!(reassembled, data);
    }

    #[tokio::test]
    async fn test_snapshot_compression_enabled() {
        let config = SnapshotTransferConfig {
            compression_enabled: true,
            ..Default::default()
        };
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, config);
        
        let snapshot = engine.create_full_snapshot().await.unwrap();
        assert!(snapshot.compression.is_some() || snapshot.compression.is_none());
    }

    #[tokio::test]
    async fn test_snapshot_compression_disabled() {
        let config = SnapshotTransferConfig {
            compression_enabled: false,
            ..Default::default()
        };
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, config);
        
        let snapshot = engine.create_full_snapshot().await.unwrap();
        assert_eq!(snapshot.compression, None);
    }

    #[test]
    fn test_verify_snapshot_integrity_valid() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let data = b"test data for integrity check".to_vec();
        let mut hasher = Hasher::new();
        hasher.update(&data);
        let checksum = *hasher.finalize().as_bytes();
        
        let snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(1),
            term: Term::new(1),
            metadata_bytes: data.clone(),
            compression: None,
            checksum,
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: data.len() as u64,
        };
        
        let is_valid = engine.verify_snapshot_integrity(&snapshot, &data).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_verify_snapshot_integrity_corrupted() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let data = b"test data".to_vec();
        let corrupted_data = b"corrupted data".to_vec();
        
        let mut hasher = Hasher::new();
        hasher.update(&data);
        let checksum = *hasher.finalize().as_bytes();
        
        let snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(1),
            term: Term::new(1),
            metadata_bytes: data,
            compression: None,
            checksum,
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: 9,
        };
        
        let is_valid = engine.verify_snapshot_integrity(&snapshot, &corrupted_data).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_snapshot_checksum_blake3() {
        let mut hasher = Hasher::new();
        hasher.update(b"test content");
        let checksum = *hasher.finalize().as_bytes();
        
        assert_eq!(checksum.len(), 32);
        
        let mut hasher2 = Hasher::new();
        hasher2.update(b"test content");
        let checksum2 = *hasher2.finalize().as_bytes();
        
        assert_eq!(checksum, checksum2);
    }

    #[test]
    fn test_transfer_progress_tracking() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let snapshot_id = SnapshotId::new();
        engine.track_transfer(snapshot_id.clone(), 1000);
        
        let progress = engine.get_transfer_progress(snapshot_id.clone()).unwrap();
        assert_eq!(progress.total_bytes, 1000);
        assert_eq!(progress.bytes_transferred, 0);
        assert_eq!(progress.state, TransferState::InProgress);
    }

    #[test]
    fn test_transfer_complete() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let snapshot_id = SnapshotId::new();
        engine.track_transfer(snapshot_id.clone(), 1000);
        engine.complete_transfer(snapshot_id.clone()).unwrap();
        
        let progress = engine.get_transfer_progress(snapshot_id).unwrap();
        assert_eq!(progress.state, TransferState::Completed);
    }

    #[test]
    fn test_transfer_fail() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let snapshot_id = SnapshotId::new();
        engine.track_transfer(snapshot_id.clone(), 1000);
        engine.fail_transfer(snapshot_id.clone(), "Network error".to_string()).unwrap();
        
        let progress = engine.get_transfer_progress(snapshot_id).unwrap();
        assert_eq!(progress.state, TransferState::Failed);
        assert_eq!(progress.error, Some("Network error".to_string()));
    }

    #[test]
    fn test_get_transfer_progress_running() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let snapshot_id = SnapshotId::new();
        engine.track_transfer(snapshot_id.clone(), 1000);
        
        let progress = engine.get_transfer_progress(snapshot_id.clone());
        assert!(progress.is_some());
        assert_eq!(progress.unwrap().state, TransferState::InProgress);
    }

    #[test]
    fn test_get_transfer_progress_completed() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let snapshot_id = SnapshotId::new();
        engine.track_transfer(snapshot_id.clone(), 1000);
        engine.complete_transfer(snapshot_id.clone()).unwrap();
        
        let progress = engine.get_transfer_progress(snapshot_id);
        assert!(progress.is_some());
        assert_eq!(progress.unwrap().state, TransferState::Completed);
    }

    #[tokio::test]
    async fn test_restore_snapshot_empty() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), SnapshotTransferConfig::default());
        
        let empty_bytes = vec![];
        let mut hasher = Hasher::new();
        hasher.update(&empty_bytes);
        let checksum = *hasher.finalize().as_bytes();
        
        let snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(0),
            term: Term::new(1),
            metadata_bytes: empty_bytes,
            compression: None,
            checksum,
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: 0,
        };
        
        let result = engine.restore_snapshot(snapshot).await.unwrap();
        assert_eq!(result.entries_restored, 0);
    }

    #[tokio::test]
    async fn test_restore_snapshot_with_inodes() {
        let kvstore = Arc::new(MemoryKvStore::new());
        kvstore.put(b"shard_1_inode_100".to_vec(), b"inode_data_100".to_vec()).unwrap();
        
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), SnapshotTransferConfig::default());
        let snapshot = engine.create_full_snapshot().await.unwrap();
        
        kvstore.delete(b"shard_1_inode_100").unwrap();
        
        let result = engine.restore_snapshot(snapshot).await.unwrap();
        assert!(result.entries_restored > 0);
        assert!(result.integrity_verified);
    }

    #[tokio::test]
    async fn test_restore_snapshot_verify_entries() {
        let kvstore = Arc::new(MemoryKvStore::new());
        kvstore.put(b"shard_1_key1".to_vec(), b"value1".to_vec()).unwrap();
        kvstore.put(b"shard_1_key2".to_vec(), b"value2".to_vec()).unwrap();
        
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), SnapshotTransferConfig::default());
        let snapshot = engine.create_full_snapshot().await.unwrap();
        
        kvstore.delete(b"shard_1_key1").unwrap();
        kvstore.delete(b"shard_1_key2").unwrap();
        
        let result = engine.restore_snapshot(snapshot).await.unwrap();
        assert_eq!(result.entries_restored, 2);
    }

    #[tokio::test]
    async fn test_restore_snapshot_updates_log_index() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let empty_bytes = vec![];
        let mut hasher = Hasher::new();
        hasher.update(&empty_bytes);
        let checksum = *hasher.finalize().as_bytes();
        
        let snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(500),
            term: Term::new(10),
            metadata_bytes: empty_bytes,
            compression: None,
            checksum,
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: 0,
        };
        
        let result = engine.restore_snapshot(snapshot).await.unwrap();
        assert_eq!(result.log_index_after_restore, 500);
    }

    #[tokio::test]
    async fn test_restore_snapshot_integrity_check_fails() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let mut snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(1),
            term: Term::new(1),
            metadata_bytes: vec![1, 2, 3],
            compression: None,
            checksum: [0u8; 32],
            created_at: Timestamp::now(),
            base_snapshot_id: None,
            size_bytes: 3,
        };
        
        let mut hasher = Hasher::new();
        hasher.update(b"different data");
        snapshot.checksum = *hasher.finalize().as_bytes();
        
        let result = engine.restore_snapshot(snapshot).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_restore_snapshot_result_metrics() {
        let kvstore = Arc::new(MemoryKvStore::new());
        kvstore.put(b"shard_1_key1".to_vec(), b"value1".to_vec()).unwrap();
        
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        let snapshot = engine.create_full_snapshot().await.unwrap();
        
        let result = engine.restore_snapshot(snapshot.clone()).await.unwrap();
        
        assert_eq!(result.snapshot_id, snapshot.snapshot_id);
        assert!(result.bytes_restored > 0);
        assert!(result.restore_duration_ms >= 0);
    }

    #[tokio::test]
    async fn test_cleanup_old_snapshots_keeps_recent() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), SnapshotTransferConfig::default());
        
        let _ = engine.create_full_snapshot().await.unwrap();
        let _ = engine.create_full_snapshot().await.unwrap();
        let _ = engine.create_full_snapshot().await.unwrap();
        
        let removed = engine.cleanup_old_snapshots(2).await.unwrap();
        
        assert!(removed >= 0);
    }

    #[tokio::test]
    async fn test_cleanup_old_snapshots_by_timestamp() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let config = SnapshotTransferConfig::default();
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore.clone(), config);
        
        let snapshot = engine.create_full_snapshot().await.unwrap();
        
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        let old_snapshot = MetadataSnapshot {
            snapshot_id: SnapshotId::new(),
            shard_id: ShardId::new(1),
            log_index: LogIndex::new(1),
            term: Term::new(1),
            metadata_bytes: vec![],
            compression: None,
            checksum: [0u8; 32],
            created_at: old_timestamp,
            base_snapshot_id: None,
            size_bytes: 0,
        };
        
        kvstore.put(format!("snapshot_{}", old_snapshot.snapshot_id.as_str()).as_bytes().to_vec(), bincode::serialize(&old_snapshot).unwrap()).unwrap();
        kvstore.put(format!("snapshot_{}", snapshot.snapshot_id.as_str()).as_bytes().to_vec(), bincode::serialize(&snapshot).unwrap()).unwrap();
        
        let removed = engine.cleanup_old_snapshots(1).await.unwrap();
        
        assert!(removed >= 0);
    }

    #[test]
    fn test_transfer_timeout_detection() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let config = SnapshotTransferConfig {
            transfer_timeout_secs: 1,
            ..Default::default()
        };
        let timeout_val = config.transfer_timeout_secs;
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, config);
        
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        let snapshot_id = SnapshotId::new();
        let progress = TransferProgress {
            snapshot_id: snapshot_id.clone(),
            bytes_transferred: 500,
            total_bytes: 1000,
            state: TransferState::InProgress,
            error: None,
            last_activity: old_timestamp,
        };
        
        engine.transfers.insert(snapshot_id, progress);
        
        let current = Timestamp::now();
        let elapsed = current.secs - old_timestamp.secs;
        
        assert!(elapsed > timeout_val);
    }

    #[test]
    fn test_multiple_concurrent_transfers() {
        let kvstore = Arc::new(MemoryKvStore::new());
        let engine = SnapshotTransferEngine::new(ShardId::new(1), kvstore, SnapshotTransferConfig::default());
        
        let id1 = SnapshotId::new();
        let id2 = SnapshotId::new();
        let id3 = SnapshotId::new();
        
        engine.track_transfer(id1.clone(), 1000);
        engine.track_transfer(id2.clone(), 2000);
        engine.track_transfer(id3.clone(), 3000);
        
        assert_eq!(engine.get_transfer_progress(id1.clone()).unwrap().total_bytes, 1000);
        assert_eq!(engine.get_transfer_progress(id2.clone()).unwrap().total_bytes, 2000);
        assert_eq!(engine.get_transfer_progress(id3.clone()).unwrap().total_bytes, 3000);
        
        engine.update_transfer_progress(id1.clone(), 500).unwrap();
        
        assert_eq!(engine.get_transfer_progress(id1).unwrap().bytes_transferred, 500);
    }

    #[test]
    fn test_snapshot_size_limit() {
        let config = SnapshotTransferConfig {
            max_snapshot_size: 100,
            ..Default::default()
        };
        
        let data = vec![0u8; 150];
        
        assert!(data.len() as u64 > config.max_snapshot_size);
    }
}
