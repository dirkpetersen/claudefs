//! Snapshot transfer for new replica bootstrap.
//!
//! A new replica joining the cluster needs to receive a full state snapshot
//! before it can start streaming journal entries.

use crate::compression::CompressionAlgo;
use std::collections::HashMap;
use thiserror::Error;

/// Configuration for snapshot transfers.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Size of each snapshot chunk in bytes.
    pub chunk_size_bytes: usize,
    /// Compression algorithm for chunk data.
    pub compression: CompressionAlgo,
    /// Maximum concurrent chunk transfers.
    pub max_concurrent_chunks: usize,
    /// Transfer timeout in milliseconds.
    pub transfer_timeout_ms: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            chunk_size_bytes: 64 * 1024, // 64KB
            compression: CompressionAlgo::Lz4,
            max_concurrent_chunks: 4,
            transfer_timeout_ms: 300_000, // 5 minutes
        }
    }
}

/// A snapshot chunk (one piece of the full state transfer).
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotChunk {
    /// Unique ID for this snapshot session.
    pub snapshot_id: u64,
    /// 0-based chunk sequence.
    pub chunk_index: u32,
    /// Total chunks in snapshot.
    pub total_chunks: u32,
    /// Raw chunk data (compressed if config says so).
    pub data: Vec<u8>,
    /// Compression used.
    pub algo: CompressionAlgo,
    /// Checksum for this chunk.
    pub crc32: u32,
    /// True on last chunk.
    pub is_final: bool,
}

impl SnapshotChunk {
    /// Compute CRC32 approximation: sum all bytes in data mod 2^32.
    pub fn compute_crc32(data: &[u8]) -> u32 {
        data.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64)) as u32
    }

    /// Verify the chunk data matches its CRC32.
    pub fn verify_crc(&self) -> bool {
        Self::compute_crc32(&self.data) == self.crc32
    }

    /// Create a new chunk with computed CRC.
    pub fn new(
        snapshot_id: u64,
        chunk_index: u32,
        total_chunks: u32,
        data: Vec<u8>,
        algo: CompressionAlgo,
        is_final: bool,
    ) -> Self {
        let crc32 = Self::compute_crc32(&data);
        Self {
            snapshot_id,
            chunk_index,
            total_chunks,
            data,
            algo,
            crc32,
            is_final,
        }
    }
}

/// Snapshot metadata describing what was snapshotted.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotMeta {
    /// Unique snapshot ID.
    pub snapshot_id: u64,
    /// Source site ID.
    pub source_site_id: u64,
    /// Unix ms when snapshot started.
    pub taken_at_ms: u64,
    /// Per-shard last-included sequence.
    pub shard_cursors: HashMap<u32, u64>,
    /// Total bytes before compression.
    pub total_bytes_uncompressed: u64,
    /// Number of chunks.
    pub chunk_count: u32,
}

/// Phase of a snapshot transfer.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotPhase {
    /// Idle, no snapshot in progress.
    Idle,
    /// Preparing snapshot transfer.
    Preparing {
        /// The snapshot identifier.
        snapshot_id: u64,
    },
    /// Sending snapshot chunks.
    Sending {
        /// The snapshot identifier.
        snapshot_id: u64,
        /// Number of chunks sent so far.
        chunks_sent: u32,
        /// Total number of chunks to send.
        total_chunks: u32,
    },
    /// Receiving snapshot chunks.
    Receiving {
        /// The snapshot identifier.
        snapshot_id: u64,
        /// Number of chunks received so far.
        chunks_received: u32,
        /// Total number of chunks to receive.
        total_chunks: u32,
    },
    /// Verifying snapshot integrity.
    Verifying {
        /// The snapshot identifier.
        snapshot_id: u64,
    },
    /// Snapshot transfer complete.
    Complete {
        /// The snapshot identifier.
        snapshot_id: u64,
        /// Duration of the snapshot transfer in milliseconds.
        duration_ms: u64,
    },
    /// Snapshot transfer failed.
    Failed {
        /// The snapshot identifier.
        snapshot_id: u64,
        /// Reason for failure.
        reason: String,
    },
}

/// Statistics for snapshot operations.
#[derive(Debug, Default, Clone)]
pub struct SnapshotStats {
    /// Number of snapshots sent.
    pub snapshots_sent: u64,
    /// Number of snapshots received.
    pub snapshots_received: u64,
    /// Number of snapshots failed.
    pub snapshots_failed: u64,
    /// Total bytes sent.
    pub total_bytes_sent: u64,
    /// Total bytes received.
    pub total_bytes_received: u64,
    /// Duration of last snapshot in ms.
    pub last_snapshot_duration_ms: u64,
}

/// Errors for snapshot operations.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// A snapshot is already in progress.
    #[error("snapshot already in progress")]
    AlreadyInProgress,
    /// Invalid chunk index.
    #[error("invalid chunk: expected {expected}, got {got}")]
    InvalidChunk {
        /// Expected chunk index.
        expected: u32,
        /// Received chunk index.
        got: u32,
    },
    /// Checksum mismatch.
    #[error("checksum mismatch on chunk {chunk_index}")]
    ChecksumMismatch {
        /// The chunk index that failed verification.
        chunk_index: u32,
    },
    /// Transfer timeout.
    #[error("timeout for snapshot {snapshot_id}")]
    Timeout {
        /// The snapshot that timed out.
        snapshot_id: u64,
    },
    /// Compression error.
    #[error("compression error: {0}")]
    CompressionError(String),
}

/// Manages snapshot transfer between sites.
#[derive(Debug)]
pub struct SnapshotManager {
    config: SnapshotConfig,
    local_site_id: u64,
    phase: SnapshotPhase,
    stats: SnapshotStats,
    start_time_ms: Option<u64>,
    received_chunks: Vec<SnapshotChunk>,
    current_snapshot_id: Option<u64>,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new(local_site_id: u64, config: SnapshotConfig) -> Self {
        Self {
            config,
            local_site_id,
            phase: SnapshotPhase::Idle,
            stats: SnapshotStats::default(),
            start_time_ms: None,
            received_chunks: Vec::new(),
            current_snapshot_id: None,
        }
    }

    /// Initiate sending a snapshot to a destination site.
    pub fn initiate_send(&mut self, _dest_site_id: u64) -> Result<SnapshotMeta, SnapshotError> {
        if !self.is_idle() {
            return Err(SnapshotError::AlreadyInProgress);
        }

        let snapshot_id = rand::random::<u64>();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let meta = SnapshotMeta {
            snapshot_id,
            source_site_id: self.local_site_id,
            taken_at_ms: now_ms,
            shard_cursors: HashMap::new(),
            total_bytes_uncompressed: 0,
            chunk_count: 0,
        };

        self.start_time_ms = Some(now_ms);
        self.current_snapshot_id = Some(snapshot_id);
        self.phase = SnapshotPhase::Preparing { snapshot_id };

        Ok(meta)
    }

    /// Initiate receiving a snapshot.
    pub fn initiate_receive(&mut self, meta: SnapshotMeta) -> Result<(), SnapshotError> {
        if !self.is_idle() {
            return Err(SnapshotError::AlreadyInProgress);
        }

        self.current_snapshot_id = Some(meta.snapshot_id);
        self.received_chunks.clear();
        self.start_time_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        );
        self.phase = SnapshotPhase::Receiving {
            snapshot_id: meta.snapshot_id,
            chunks_received: 0,
            total_chunks: meta.chunk_count,
        };

        Ok(())
    }

    /// Process a received chunk, returns the next expected chunk index if any.
    pub fn process_chunk(
        &mut self,
        chunk: SnapshotChunk,
    ) -> Result<Option<SnapshotChunk>, SnapshotError> {
        let current_snapshot = match self.phase {
            SnapshotPhase::Receiving {
                snapshot_id,
                chunks_received,
                total_chunks,
            } => {
                if chunk.snapshot_id != snapshot_id {
                    return Err(SnapshotError::InvalidChunk {
                        expected: chunks_received,
                        got: chunk.chunk_index,
                    });
                }
                if chunk.chunk_index != chunks_received {
                    return Err(SnapshotError::InvalidChunk {
                        expected: chunks_received,
                        got: chunk.chunk_index,
                    });
                }
                (snapshot_id, chunks_received, total_chunks)
            }
            _ => return Err(SnapshotError::AlreadyInProgress),
        };

        if !chunk.verify_crc() {
            return Err(SnapshotError::ChecksumMismatch {
                chunk_index: chunk.chunk_index,
            });
        }

        self.stats.total_bytes_received += chunk.data.len() as u64;
        self.received_chunks.push(chunk);

        let chunks_received = current_snapshot.1 + 1;
        let total_chunks = current_snapshot.2;

        if chunks_received >= total_chunks {
            self.phase = SnapshotPhase::Verifying {
                snapshot_id: current_snapshot.0,
            };
            Ok(None)
        } else {
            self.phase = SnapshotPhase::Receiving {
                snapshot_id: current_snapshot.0,
                chunks_received,
                total_chunks,
            };
            Ok(Some(SnapshotChunk {
                snapshot_id: current_snapshot.0,
                chunk_index: chunks_received,
                total_chunks,
                data: vec![],
                algo: CompressionAlgo::None,
                crc32: 0,
                is_final: false,
            }))
        }
    }

    /// Complete sending a snapshot.
    pub fn complete_send(&mut self) -> Result<(), SnapshotError> {
        let snapshot_id = match self.phase {
            SnapshotPhase::Sending {
                snapshot_id,
                chunks_sent,
                total_chunks,
            } if chunks_sent == total_chunks => snapshot_id,
            _ => return Err(SnapshotError::AlreadyInProgress),
        };

        let duration_ms = self
            .start_time_ms
            .map(|start| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64
                    - start
            })
            .unwrap_or(0);

        self.stats.snapshots_sent += 1;
        self.stats.last_snapshot_duration_ms = duration_ms;

        self.phase = SnapshotPhase::Complete {
            snapshot_id,
            duration_ms,
        };

        Ok(())
    }

    /// Complete a receive operation.
    pub fn complete_receive(&mut self) -> Result<(), SnapshotError> {
        let snapshot_id = match self.phase {
            SnapshotPhase::Verifying { snapshot_id } => snapshot_id,
            _ => return Err(SnapshotError::AlreadyInProgress),
        };

        let duration_ms = self
            .start_time_ms
            .map(|start| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64
                    - start
            })
            .unwrap_or(0);

        self.stats.snapshots_received += 1;
        self.stats.last_snapshot_duration_ms = duration_ms;
        self.received_chunks.clear();

        self.phase = SnapshotPhase::Complete {
            snapshot_id,
            duration_ms,
        };

        Ok(())
    }

    /// Check if the manager is idle.
    pub fn is_idle(&self) -> bool {
        matches!(self.phase, SnapshotPhase::Idle)
    }

    /// Get the current phase.
    pub fn phase(&self) -> &SnapshotPhase {
        &self.phase
    }

    /// Get statistics.
    pub fn stats(&self) -> &SnapshotStats {
        &self.stats
    }

    /// Get the config.
    pub fn config(&self) -> &SnapshotConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn arb_chunk_data()(size in 1..1024usize) -> Vec<u8> {
            (0..size).map(|_| rand::random::<u8>()).collect()
        }
    }

    proptest! {
        #[test]
        fn test_chunk_crc_roundtrip(data in arb_chunk_data()) {
            let crc = SnapshotChunk::compute_crc32(&data);
            let chunk = SnapshotChunk::new(
                1, 0, 1,
                data.clone(),
                CompressionAlgo::None,
                true
            );
            assert_eq!(chunk.crc32, crc);
            assert!(chunk.verify_crc());
        }

        #[test]
        fn test_chunk_verify_fails_on_corruption(data in arb_chunk_data()) {
            let mut chunk = SnapshotChunk::new(
                1, 0, 1,
                data,
                CompressionAlgo::None,
                true
            );
            chunk.data[0] = !chunk.data[0];
            assert!(!chunk.verify_crc());
        }

        #[test]
        fn test_phase_transitions(snapshot_id: u64) {
            let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

            // Idle -> Preparing
            assert!(manager.is_idle());
            let meta = manager.initiate_send(2).unwrap();
            assert_eq!(meta.snapshot_id, snapshot_id);
            assert!(!manager.is_idle());

            // Preparing -> Sending
            if let SnapshotPhase::Preparing { .. } = manager.phase().clone() {
                manager.phase = SnapshotPhase::Sending {
                    snapshot_id,
                    chunks_sent: 0,
                    total_chunks: 1,
                };
            }

            // Sending -> Complete
            manager.complete_send().unwrap();
            assert!(matches!(manager.phase(), SnapshotPhase::Complete { .. }));
        }

        #[test]
        fn test_stats_accumulation() {
            let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

            manager.initiate_send(2).unwrap();
            manager.phase = SnapshotPhase::Sending {
                snapshot_id: 1,
                chunks_sent: 1,
                total_chunks: 1,
            };
            manager.stats.total_bytes_sent = 1000;
            manager.complete_send().unwrap();

            assert_eq!(manager.stats.snapshots_sent, 1);
            assert_eq!(manager.stats.total_bytes_sent, 1000);
        }

        #[test]
        fn test_invalid_chunk_index(snapshot_id: u64) {
            let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

            let meta = SnapshotMeta {
                snapshot_id,
                source_site_id: 1,
                taken_at_ms: 1000,
                shard_cursors: HashMap::new(),
                total_bytes_uncompressed: 1000,
                chunk_count: 3,
            };
            manager.initiate_receive(meta).unwrap();

            // Send chunk 1 first instead of 0
            let chunk = SnapshotChunk::new(snapshot_id, 1, 3, vec![1,2,3], CompressionAlgo::None, false);
            let result = manager.process_chunk(chunk);
            assert!(matches!(result, Err(SnapshotError::InvalidChunk { .. })));
        }

        #[test]
        fn test_wrong_snapshot_id() {
            let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

            let meta = SnapshotMeta {
                snapshot_id: 100,
                source_site_id: 1,
                taken_at_ms: 1000,
                shard_cursors: HashMap::new(),
                total_bytes_uncompressed: 1000,
                chunk_count: 3,
            };
            manager.initiate_receive(meta).unwrap();

            // Send chunk with different snapshot_id
            let chunk = SnapshotChunk::new(999, 0, 3, vec![1,2,3], CompressionAlgo::None, false);
            let result = manager.process_chunk(chunk);
            assert!(matches!(result, Err(SnapshotError::InvalidChunk { .. })));
        }
    }

    #[test]
    fn test_snapshot_manager_new() {
        let manager = SnapshotManager::new(42, SnapshotConfig::default());
        assert_eq!(manager.local_site_id, 42);
        assert!(manager.is_idle());
        assert!(matches!(manager.phase(), SnapshotPhase::Idle));
        let default_stats = SnapshotStats::default();
        assert_eq!(manager.stats(), &default_stats);
    }

    #[test]
    fn test_initiate_send() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());
        let meta = manager.initiate_send(2).unwrap();

        assert_eq!(meta.source_site_id, 1);
        assert!(matches!(manager.phase(), SnapshotPhase::Preparing { .. }));
    }

    #[test]
    fn test_initiate_send_already_in_progress() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());
        manager.initiate_send(2).unwrap();

        let result = manager.initiate_send(3);
        assert!(matches!(result, Err(SnapshotError::AlreadyInProgress)));
    }

    #[test]
    fn test_initiate_receive() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

        let meta = SnapshotMeta {
            snapshot_id: 42,
            source_site_id: 2,
            taken_at_ms: 1000,
            shard_cursors: HashMap::new(),
            total_bytes_uncompressed: 5000,
            chunk_count: 10,
        };

        manager.initiate_receive(meta).unwrap();
        assert!(matches!(manager.phase(), SnapshotPhase::Receiving { .. }));
    }

    #[test]
    fn test_process_chunk_in_order() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

        let meta = SnapshotMeta {
            snapshot_id: 100,
            source_site_id: 2,
            taken_at_ms: 1000,
            shard_cursors: HashMap::new(),
            total_bytes_uncompressed: 300,
            chunk_count: 3,
        };
        manager.initiate_receive(meta).unwrap();

        let chunk0 = SnapshotChunk::new(100, 0, 3, vec![1, 2, 3], CompressionAlgo::None, false);
        let chunk1 = SnapshotChunk::new(100, 1, 3, vec![4, 5, 6], CompressionAlgo::None, false);
        let chunk2 = SnapshotChunk::new(100, 2, 3, vec![7, 8, 9], CompressionAlgo::None, true);

        manager.process_chunk(chunk0).unwrap();
        manager.process_chunk(chunk1).unwrap();
        manager.process_chunk(chunk2).unwrap();

        assert!(matches!(manager.phase(), SnapshotPhase::Verifying { .. }));
        assert_eq!(manager.stats.total_bytes_received, 9);
    }

    #[test]
    fn test_checksum_mismatch() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

        let meta = SnapshotMeta {
            snapshot_id: 100,
            source_site_id: 2,
            taken_at_ms: 1000,
            shard_cursors: HashMap::new(),
            total_bytes_uncompressed: 100,
            chunk_count: 1,
        };
        manager.initiate_receive(meta).unwrap();

        let mut chunk = SnapshotChunk::new(100, 0, 1, vec![1, 2, 3], CompressionAlgo::None, true);
        chunk.crc32 = 0; // Wrong CRC

        let result = manager.process_chunk(chunk);
        assert!(matches!(
            result,
            Err(SnapshotError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_complete_send() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

        manager.initiate_send(2).unwrap();
        manager.phase = SnapshotPhase::Sending {
            snapshot_id: 1,
            chunks_sent: 1,
            total_chunks: 1,
        };
        manager.stats.total_bytes_sent = 500;

        manager.complete_send().unwrap();

        assert!(matches!(manager.phase(), SnapshotPhase::Complete { .. }));
        assert_eq!(manager.stats.snapshots_sent, 1);
    }

    #[test]
    fn test_complete_receive() {
        let mut manager = SnapshotManager::new(1, SnapshotConfig::default());

        let meta = SnapshotMeta {
            snapshot_id: 100,
            source_site_id: 2,
            taken_at_ms: 1000,
            shard_cursors: HashMap::new(),
            total_bytes_uncompressed: 100,
            chunk_count: 1,
        };
        manager.initiate_receive(meta).unwrap();

        let chunk = SnapshotChunk::new(100, 0, 1, vec![1, 2, 3], CompressionAlgo::None, true);
        manager.process_chunk(chunk).unwrap();
        manager.complete_receive().unwrap();

        assert!(matches!(manager.phase(), SnapshotPhase::Complete { .. }));
        assert_eq!(manager.stats.snapshots_received, 1);
    }

    #[test]
    fn test_default_config() {
        let config = SnapshotConfig::default();
        assert_eq!(config.chunk_size_bytes, 64 * 1024);
        assert_eq!(config.compression, CompressionAlgo::Lz4);
        assert_eq!(config.max_concurrent_chunks, 4);
        assert_eq!(config.transfer_timeout_ms, 300_000);
    }

    #[test]
    fn test_chunk_fields() {
        let chunk = SnapshotChunk::new(123, 5, 10, vec![1, 2, 3], CompressionAlgo::Zstd, true);
        assert_eq!(chunk.snapshot_id, 123);
        assert_eq!(chunk.chunk_index, 5);
        assert_eq!(chunk.total_chunks, 10);
        assert_eq!(chunk.data, vec![1, 2, 3]);
        assert_eq!(chunk.algo, CompressionAlgo::Zstd);
        assert!(chunk.is_final);
    }

    #[test]
    fn test_snapshot_meta_fields() {
        let mut cursors = HashMap::new();
        cursors.insert(0, 100);
        cursors.insert(1, 200);

        let meta = SnapshotMeta {
            snapshot_id: 42,
            source_site_id: 1,
            taken_at_ms: 1000000,
            shard_cursors: cursors.clone(),
            total_bytes_uncompressed: 50000,
            chunk_count: 100,
        };

        assert_eq!(meta.snapshot_id, 42);
        assert_eq!(meta.source_site_id, 1);
        assert_eq!(meta.taken_at_ms, 1000000);
        assert_eq!(meta.shard_cursors, cursors);
        assert_eq!(meta.total_bytes_uncompressed, 50000);
        assert_eq!(meta.chunk_count, 100);
    }
}
