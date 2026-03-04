//! Raft snapshot transfer (InstallSnapshot RPC).
//!
//! This module handles the Raft `InstallSnapshot` RPC — transferring a snapshot
//! from leader to a follower that is too far behind (its log has been compacted away).
//!
//! Snapshots may be large, so they are chunked for network transfer.

use crate::snapshot::RaftSnapshot;
use crate::types::{LogIndex, MetaError, NodeId, Term, Timestamp};
use serde::{Deserialize, Serialize};

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
