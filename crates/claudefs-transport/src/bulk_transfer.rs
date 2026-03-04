//! Parallel bulk data transfer state machine for large payload distribution.

use crate::routing::NodeId;
use serde::{Deserialize, Serialize};

/// Unique job identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub u64);

/// Unique chunk identifier within a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkId {
    /// The job this chunk belongs to.
    pub job_id: JobId,
    /// Zero-based chunk index.
    pub index: u32,
}

/// Per-chunk state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkState {
    /// Not yet sent.
    Pending,
    /// Sent, awaiting acknowledgement.
    InFlight,
    /// Successfully acknowledged.
    Acked,
    /// Failed after exhausting retries.
    Failed {
        /// Number of retries attempted.
        retries: u32,
    },
}

/// A single chunk to be transferred.
#[derive(Debug, Clone)]
pub struct TransferChunk {
    /// The chunk's unique identifier.
    pub id: ChunkId,
    /// Byte offset into the transfer payload.
    pub offset: u64,
    /// Chunk size in bytes.
    pub size: u32,
    /// Destination node.
    pub target: NodeId,
}

/// Configuration for a bulk transfer job.
#[derive(Debug, Clone)]
pub struct BulkTransferConfig {
    /// Chunk size in bytes. Default: 1MB (1 << 20).
    pub chunk_size: u32,
    /// Maximum number of in-flight chunks at once. Default: 8.
    pub max_in_flight: usize,
    /// Maximum retries per chunk before permanent failure. Default: 3.
    pub max_retries: u32,
}

impl Default for BulkTransferConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1 << 20,
            max_in_flight: 8,
            max_retries: 3,
        }
    }
}

/// Aggregate transfer statistics.
#[derive(Debug, Clone, Default)]
pub struct BulkTransferStats {
    /// Total chunks in this job.
    pub total_chunks: u32,
    /// Chunks successfully acknowledged.
    pub chunks_acked: u32,
    /// Chunks permanently failed.
    pub chunks_failed: u32,
    /// Chunks currently in-flight.
    pub chunks_in_flight: u32,
    /// Total bytes in this job.
    pub bytes_total: u64,
    /// Bytes in acknowledged chunks.
    pub bytes_acked: u64,
    /// Total retry attempts across all chunks.
    pub retries_total: u32,
}

/// Error type for bulk transfer operations.
#[derive(Debug, thiserror::Error)]
pub enum BulkTransferError {
    /// The given chunk ID is not part of this job.
    #[error("unknown chunk {0:?}")]
    UnknownChunk(ChunkId),
    /// The chunk exists but is not in the expected state.
    #[error("chunk {0:?} is not in-flight (state: {1:?})")]
    InvalidChunkState(ChunkId, ChunkState),
}

/// Bulk transfer job state machine.
pub struct BulkTransfer {
    /// The job identifier.
    pub job_id: JobId,
    config: BulkTransferConfig,
    chunks: Vec<(ChunkState, TransferChunk)>,
    retry_counts: Vec<u32>,
}

impl BulkTransfer {
    /// Creates a new bulk transfer job.
    ///
    /// Splits `total_bytes` into chunks according to `config.chunk_size`,
    /// assigning each chunk to a target node in round-robin order.
    /// If `targets` is empty, uses `NodeId::default()` for all chunks.
    pub fn new(
        job_id: JobId,
        total_bytes: u64,
        targets: Vec<NodeId>,
        config: BulkTransferConfig,
    ) -> Self {
        let chunk_size = config.chunk_size as u64;
        let num_chunks = if total_bytes == 0 {
            0
        } else {
            (total_bytes + chunk_size - 1) / chunk_size
        };

        let default_target = NodeId::default();
        let use_targets = if targets.is_empty() {
            vec![default_target]
        } else {
            targets
        };

        let chunks: Vec<(ChunkState, TransferChunk)> = (0..num_chunks)
            .map(|i| {
                let index = i as u32;
                let offset = i * chunk_size;
                let remaining = total_bytes.saturating_sub(offset);
                let size = if remaining >= chunk_size {
                    config.chunk_size
                } else {
                    remaining as u32
                };
                let target = use_targets[(i as usize) % use_targets.len()];

                (
                    ChunkState::Pending,
                    TransferChunk {
                        id: ChunkId { job_id, index },
                        offset,
                        size,
                        target,
                    },
                )
            })
            .collect();

        let retry_counts = vec![0u32; chunks.len()];

        Self {
            job_id,
            config,
            chunks,
            retry_counts,
        }
    }

    /// Returns up to `max_in_flight - current_in_flight` pending chunks,
    /// marking them as in-flight.
    pub fn next_to_send(&mut self) -> Vec<TransferChunk> {
        let in_flight_count = self
            .chunks
            .iter()
            .filter(|(state, _)| *state == ChunkState::InFlight)
            .count();

        let available = self.config.max_in_flight.saturating_sub(in_flight_count);

        let mut result = Vec::with_capacity(available);
        for (state, chunk) in &mut self.chunks {
            if result.len() >= available {
                break;
            }
            if *state == ChunkState::Pending {
                *state = ChunkState::InFlight;
                result.push(chunk.clone());
            }
        }

        result
    }

    /// Acknowledges successful transfer of a chunk.
    ///
    /// Returns an error if the chunk is unknown or not in-flight.
    pub fn ack(&mut self, chunk_id: ChunkId) -> Result<(), BulkTransferError> {
        let idx = self.find_chunk_index(chunk_id)?;
        let (state, _) = &self.chunks[idx];

        if *state != ChunkState::InFlight {
            return Err(BulkTransferError::InvalidChunkState(chunk_id, *state));
        }

        self.chunks[idx].0 = ChunkState::Acked;
        Ok(())
    }

    /// Reports a failed transfer attempt for a chunk.
    ///
    /// If retries have been exhausted, the chunk enters the Failed state.
    /// Otherwise, it returns to Pending for retry.
    ///
    /// Returns an error if the chunk is unknown or not in-flight.
    pub fn nack(&mut self, chunk_id: ChunkId) -> Result<(), BulkTransferError> {
        let idx = self.find_chunk_index(chunk_id)?;
        let (state, _) = &self.chunks[idx];

        if *state != ChunkState::InFlight {
            return Err(BulkTransferError::InvalidChunkState(chunk_id, *state));
        }

        self.retry_counts[idx] += 1;
        let retries = self.retry_counts[idx];

        if retries > self.config.max_retries {
            self.chunks[idx].0 = ChunkState::Failed { retries };
        } else {
            self.chunks[idx].0 = ChunkState::Pending;
        }

        Ok(())
    }

    /// Returns true if all chunks have been acknowledged.
    pub fn is_complete(&self) -> bool {
        self.chunks
            .iter()
            .all(|(state, _)| *state == ChunkState::Acked)
    }

    /// Returns true if any chunk has permanently failed.
    pub fn has_fatal_failure(&self) -> bool {
        self.chunks
            .iter()
            .any(|(state, _)| matches!(state, ChunkState::Failed { .. }))
    }

    /// Returns the list of chunk IDs that have permanently failed.
    pub fn fatal_failures(&self) -> Vec<ChunkId> {
        self.chunks
            .iter()
            .filter_map(|(state, chunk)| {
                if matches!(state, ChunkState::Failed { .. }) {
                    Some(chunk.id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Computes and returns aggregate statistics for this transfer job.
    pub fn stats(&self) -> BulkTransferStats {
        let mut stats = BulkTransferStats::default();

        stats.total_chunks = self.chunks.len() as u32;

        for (state, chunk) in &self.chunks {
            stats.bytes_total += chunk.size as u64;

            match state {
                ChunkState::Acked => {
                    stats.chunks_acked += 1;
                    stats.bytes_acked += chunk.size as u64;
                }
                ChunkState::Failed { retries } => {
                    stats.chunks_failed += 1;
                    stats.retries_total += retries;
                }
                ChunkState::InFlight => {
                    stats.chunks_in_flight += 1;
                }
                ChunkState::Pending => {}
            }
        }

        for &retry_count in &self.retry_counts {
            if !matches!(
                self.chunks
                    .iter()
                    .find(|(_, c)| c.id.index
                        == self
                            .retry_counts
                            .iter()
                            .position(|&r| r == retry_count)
                            .unwrap() as u32)
                    .map(|(s, _)| *s),
                Some(ChunkState::Failed { .. })
            ) {
                stats.retries_total += retry_count;
            }
        }

        stats.retries_total = self.retry_counts.iter().sum();

        stats
    }

    fn find_chunk_index(&self, chunk_id: ChunkId) -> Result<usize, BulkTransferError> {
        if chunk_id.job_id != self.job_id {
            return Err(BulkTransferError::UnknownChunk(chunk_id));
        }

        self.chunks
            .iter()
            .position(|(_, chunk)| chunk.id.index == chunk_id.index)
            .ok_or(BulkTransferError::UnknownChunk(chunk_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u64) -> NodeId {
        NodeId::new(n)
    }

    fn nodes(n: usize) -> Vec<NodeId> {
        (0..n as u64).map(NodeId::new).collect()
    }

    const MB: u64 = 1 << 20;

    #[test]
    fn test_new_job_single_chunk() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            ..Default::default()
        };
        let transfer = BulkTransfer::new(JobId(1), 100, nodes(2), config.clone());

        assert_eq!(transfer.chunks.len(), 1);
        assert_eq!(transfer.chunks[0].1.size, 100);
        assert_eq!(transfer.chunks[0].1.offset, 0);
        assert_eq!(transfer.chunks[0].1.target, node(0));
        assert_eq!(transfer.chunks[0].0, ChunkState::Pending);
    }

    #[test]
    fn test_new_job_exact_chunks() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            ..Default::default()
        };
        let transfer = BulkTransfer::new(JobId(1), 3 * MB, nodes(2), config.clone());

        assert_eq!(transfer.chunks.len(), 3);
        for (i, (state, chunk)) in transfer.chunks.iter().enumerate() {
            assert_eq!(*state, ChunkState::Pending);
            assert_eq!(chunk.size as u64, MB);
            assert_eq!(chunk.offset, i as u64 * MB);
        }
    }

    #[test]
    fn test_new_job_partial_last_chunk() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            ..Default::default()
        };
        let transfer = BulkTransfer::new(JobId(1), 2 * MB + 500, nodes(2), config.clone());

        assert_eq!(transfer.chunks.len(), 3);
        assert_eq!(transfer.chunks[0].1.size as u64, MB);
        assert_eq!(transfer.chunks[1].1.size as u64, MB);
        assert_eq!(transfer.chunks[2].1.size, 500);
        assert_eq!(transfer.chunks[2].1.offset, 2 * MB);
    }

    #[test]
    fn test_next_to_send_respects_max_in_flight() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            max_in_flight: 2,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 10 * MB, nodes(2), config);

        let chunks = transfer.next_to_send();
        assert_eq!(chunks.len(), 2);

        let more = transfer.next_to_send();
        assert!(more.is_empty());
    }

    #[test]
    fn test_next_to_send_empty_when_all_in_flight() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            max_in_flight: 3,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 3 * MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        assert_eq!(chunks.len(), 3);

        let more = transfer.next_to_send();
        assert!(more.is_empty());
    }

    #[test]
    fn test_next_to_send_returns_pending_only() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            max_in_flight: 8,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 5 * MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        assert_eq!(chunks.len(), 5);

        for (state, _) in &transfer.chunks {
            assert_eq!(*state, ChunkState::InFlight);
        }
    }

    #[test]
    fn test_ack_completes_chunk() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.ack(chunk_id).unwrap();

        assert_eq!(transfer.chunks[0].0, ChunkState::Acked);
    }

    #[test]
    fn test_ack_all_chunks_completes_job() {
        let mut transfer = BulkTransfer::new(JobId(1), 3 * MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        for chunk in &chunks {
            transfer.ack(chunk.id).unwrap();
        }

        assert!(transfer.is_complete());
    }

    #[test]
    fn test_nack_retries() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.nack(chunk_id).unwrap();

        assert_eq!(transfer.chunks[0].0, ChunkState::Pending);
        assert_eq!(transfer.retry_counts[0], 1);
    }

    #[test]
    fn test_nack_max_retries_fails_permanently() {
        let config = BulkTransferConfig {
            max_retries: 3,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        for _ in 0..4 {
            transfer.nack(chunk_id).unwrap();
            if !matches!(transfer.chunks[0].0, ChunkState::Failed { .. }) {
                let _ = transfer.next_to_send();
            }
        }

        assert!(matches!(
            transfer.chunks[0].0,
            ChunkState::Failed { retries: 4 }
        ));
    }

    #[test]
    fn test_nack_then_resend() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.nack(chunk_id).unwrap();

        let resend = transfer.next_to_send();
        assert_eq!(resend.len(), 1);
        assert_eq!(resend[0].id, chunk_id);
    }

    #[test]
    fn test_is_complete_false_initially() {
        let transfer = BulkTransfer::new(JobId(1), 5 * MB, nodes(1), Default::default());

        assert!(!transfer.is_complete());
    }

    #[test]
    fn test_is_complete_true_when_all_acked() {
        let mut transfer = BulkTransfer::new(JobId(1), 2 * MB, nodes(1), Default::default());

        for (state, _) in &mut transfer.chunks {
            *state = ChunkState::Acked;
        }

        assert!(transfer.is_complete());
    }

    #[test]
    fn test_has_fatal_failure_false_initially() {
        let transfer = BulkTransfer::new(JobId(1), 5 * MB, nodes(1), Default::default());

        assert!(!transfer.has_fatal_failure());
    }

    #[test]
    fn test_has_fatal_failure_true_after_max_retries() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        for _ in 0..4 {
            transfer.nack(chunk_id).unwrap();
            if !matches!(transfer.chunks[0].0, ChunkState::Failed { .. }) {
                let _ = transfer.next_to_send();
            }
        }

        assert!(transfer.has_fatal_failure());
    }

    #[test]
    fn test_fatal_failures_list() {
        let config = BulkTransferConfig {
            max_retries: 0,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 2 * MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        transfer.nack(chunks[0].id).unwrap();
        transfer.nack(chunks[1].id).unwrap();

        let failures = transfer.fatal_failures();
        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn test_stats_total_chunks() {
        let transfer = BulkTransfer::new(JobId(1), 5 * MB, nodes(1), Default::default());

        let stats = transfer.stats();
        assert_eq!(stats.total_chunks, 5);
    }

    #[test]
    fn test_stats_bytes_total() {
        let transfer = BulkTransfer::new(JobId(1), 5 * MB, nodes(1), Default::default());

        let stats = transfer.stats();
        assert_eq!(stats.bytes_total, 5 * MB);
    }

    #[test]
    fn test_stats_bytes_acked_increments() {
        let mut transfer = BulkTransfer::new(JobId(1), 2 * MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        transfer.ack(chunks[0].id).unwrap();

        let stats = transfer.stats();
        assert_eq!(stats.bytes_acked, MB);
        assert_eq!(stats.chunks_acked, 1);
    }

    #[test]
    fn test_stats_chunks_in_flight() {
        let mut transfer = BulkTransfer::new(JobId(1), 3 * MB, nodes(1), Default::default());

        let _ = transfer.next_to_send();

        let stats = transfer.stats();
        assert_eq!(stats.chunks_in_flight, 3);
    }

    #[test]
    fn test_stats_retries_total() {
        let mut transfer = BulkTransfer::new(JobId(1), 2 * MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        transfer.nack(chunks[0].id).unwrap();
        let _ = transfer.next_to_send();
        transfer.nack(chunks[0].id).unwrap();
        transfer.nack(chunks[1].id).unwrap();

        let stats = transfer.stats();
        assert_eq!(stats.retries_total, 3);
    }

    #[test]
    fn test_ack_unknown_chunk_error() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let unknown_id = ChunkId {
            job_id: JobId(1),
            index: 999,
        };

        let result = transfer.ack(unknown_id);
        assert!(matches!(result, Err(BulkTransferError::UnknownChunk(_))));
    }

    #[test]
    fn test_nack_unknown_chunk_error() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let unknown_id = ChunkId {
            job_id: JobId(1),
            index: 999,
        };

        let result = transfer.nack(unknown_id);
        assert!(matches!(result, Err(BulkTransferError::UnknownChunk(_))));
    }

    #[test]
    fn test_ack_non_in_flight_error() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunk_id = transfer.chunks[0].1.id;

        let result = transfer.ack(chunk_id);
        assert!(matches!(
            result,
            Err(BulkTransferError::InvalidChunkState(_, _))
        ));
    }

    #[test]
    fn test_round_robin_target_assignment() {
        let transfer = BulkTransfer::new(
            JobId(1),
            4 * MB,
            vec![node(10), node(20), node(30)],
            Default::default(),
        );

        assert_eq!(transfer.chunks[0].1.target, node(10));
        assert_eq!(transfer.chunks[1].1.target, node(20));
        assert_eq!(transfer.chunks[2].1.target, node(30));
        assert_eq!(transfer.chunks[3].1.target, node(10));
    }

    #[test]
    fn test_chunk_offsets_contiguous() {
        let transfer = BulkTransfer::new(JobId(1), 3 * MB, nodes(1), Default::default());

        assert_eq!(transfer.chunks[0].1.offset, 0);
        assert_eq!(transfer.chunks[1].1.offset, MB);
        assert_eq!(transfer.chunks[2].1.offset, 2 * MB);
    }

    #[test]
    fn test_last_chunk_correct_size() {
        let config = BulkTransferConfig {
            chunk_size: MB as u32,
            ..Default::default()
        };
        let transfer = BulkTransfer::new(JobId(1), 2 * MB + 12345, nodes(1), config);

        assert_eq!(transfer.chunks[2].1.size, 12345);
    }

    #[test]
    fn test_retry_flow_end_to_end() {
        let config = BulkTransferConfig {
            max_retries: 2,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.nack(chunk_id).unwrap();
        let _ = transfer.next_to_send();
        transfer.nack(chunk_id).unwrap();
        let _ = transfer.next_to_send();
        transfer.nack(chunk_id).unwrap();

        assert!(transfer.has_fatal_failure());
        assert!(transfer.next_to_send().is_empty());
    }

    #[test]
    fn test_large_job_1000_chunks() {
        let config = BulkTransferConfig {
            chunk_size: 1000,
            max_in_flight: 50,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 1000 * 1000, nodes(4), config);

        let mut all_sent = Vec::new();

        while !transfer.is_complete() && !transfer.has_fatal_failure() {
            let chunks = transfer.next_to_send();
            for chunk in &chunks {
                all_sent.push(chunk.id);
                transfer.ack(chunk.id).unwrap();
            }
        }

        assert!(transfer.is_complete());
        assert_eq!(all_sent.len(), 1000);
    }

    #[test]
    fn test_default_config_values() {
        let config = BulkTransferConfig::default();

        assert_eq!(config.chunk_size, 1 << 20);
        assert_eq!(config.max_in_flight, 8);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_empty_targets_uses_default_node() {
        let transfer = BulkTransfer::new(JobId(1), 3 * MB, vec![], Default::default());

        for (_, chunk) in &transfer.chunks {
            assert_eq!(chunk.target, NodeId::default());
        }
    }

    #[test]
    fn test_max_in_flight_8() {
        let mut transfer = BulkTransfer::new(JobId(1), 20 * MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        assert_eq!(chunks.len(), 8);
    }

    #[test]
    fn test_zero_byte_transfer() {
        let transfer = BulkTransfer::new(JobId(1), 0, nodes(1), Default::default());

        assert_eq!(transfer.chunks.len(), 0);
        assert!(transfer.is_complete());
    }

    #[test]
    fn test_nack_on_failed_chunk_returns_error() {
        let config = BulkTransferConfig {
            max_retries: 1,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.nack(chunk_id).unwrap();
        let _ = transfer.next_to_send();
        transfer.nack(chunk_id).unwrap();

        let result = transfer.nack(chunk_id);
        assert!(matches!(
            result,
            Err(BulkTransferError::InvalidChunkState(_, _))
        ));
    }

    #[test]
    fn test_ack_wrong_job_id() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let wrong_job_chunk = ChunkId {
            job_id: JobId(999),
            index: 0,
        };

        let result = transfer.ack(wrong_job_chunk);
        assert!(matches!(result, Err(BulkTransferError::UnknownChunk(_))));
    }

    #[test]
    fn test_partial_acknowledgement_stats() {
        let mut transfer = BulkTransfer::new(JobId(1), 4 * MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        transfer.ack(chunks[0].id).unwrap();
        transfer.ack(chunks[1].id).unwrap();

        let stats = transfer.stats();
        assert_eq!(stats.chunks_acked, 2);
        assert_eq!(stats.chunks_in_flight, 2);
        assert_eq!(stats.bytes_acked, 2 * MB);
    }

    #[test]
    fn test_multiple_targets_distribution() {
        let transfer = BulkTransfer::new(
            JobId(1),
            100 * MB,
            vec![node(1), node(2), node(3), node(4), node(5)],
            Default::default(),
        );

        let mut counts = [0usize; 5];
        for (_, chunk) in &transfer.chunks {
            let target = chunk.target.as_u64() as usize;
            if target >= 1 && target <= 5 {
                counts[target - 1] += 1;
            }
        }

        for &count in &counts {
            assert!(count >= 19 && count <= 21);
        }
    }

    #[test]
    fn test_nack_preserves_retry_count_on_pending() {
        let mut transfer = BulkTransfer::new(JobId(1), MB, nodes(1), Default::default());

        let chunks = transfer.next_to_send();
        let chunk_id = chunks[0].id;

        transfer.nack(chunk_id).unwrap();
        assert_eq!(transfer.retry_counts[0], 1);

        let _ = transfer.next_to_send();
        transfer.nack(chunk_id).unwrap();
        assert_eq!(transfer.retry_counts[0], 2);
    }

    #[test]
    fn test_stats_failed_chunks() {
        let config = BulkTransferConfig {
            max_retries: 0,
            ..Default::default()
        };
        let mut transfer = BulkTransfer::new(JobId(1), 2 * MB, nodes(1), config);

        let chunks = transfer.next_to_send();
        transfer.nack(chunks[0].id).unwrap();

        let stats = transfer.stats();
        assert_eq!(stats.chunks_failed, 1);
    }
}
