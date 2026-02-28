//! I/O engine bridge for io_uring operations.
//!
//! This module provides the async I/O bridge between the storage engine and the Linux io_uring subsystem.
//! It provides a trait-based abstraction ([`IoEngine`]) for block I/O with both real and mock implementations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, Deserialize};
use tokio::sync::Mutex as AsyncMutex;
use tracing::debug;

use crate::block::{BlockId, BlockRef, PlacementHint};
use crate::error::{StorageError, StorageResult};

/// Represents a pending I/O operation identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IoRequestId(pub u64);

/// Type of I/O operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoOpType {
    /// Read data from a block
    Read,
    /// Write data to a block
    Write,
    /// Flush/sync data to persistent storage
    Flush,
    /// Discard/trim a block range
    Discard,
}

/// Describes an I/O operation to be submitted.
#[derive(Debug, Clone)]
pub struct IoRequest {
    /// Unique request ID for tracking
    pub id: IoRequestId,
    /// Type of operation
    pub op: IoOpType,
    /// Target block reference
    pub block_ref: BlockRef,
    /// Data buffer for read/write operations
    pub data: Option<Vec<u8>>,
    /// FDP placement hint for writes
    pub placement_hint: Option<PlacementHint>,
}

/// Result of a completed I/O operation.
#[derive(Debug)]
pub struct IoCompletion {
    /// Request ID that completed
    pub id: IoRequestId,
    /// Operation type
    pub op: IoOpType,
    /// Result: number of bytes transferred on success, error on failure
    pub result: StorageResult<usize>,
    /// Data buffer (for reads, contains the read data)
    pub data: Option<Vec<u8>>,
}

/// I/O engine statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IoStats {
    /// Total read operations completed
    pub reads_completed: u64,
    /// Total write operations completed
    pub writes_completed: u64,
    /// Total flush operations completed
    pub flushes_completed: u64,
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
    /// Total errors encountered
    pub errors: u64,
    /// Current queue depth (pending operations)
    pub queue_depth: u32,
}

/// Trait for the I/O engine abstraction.
/// Implementations can be real io_uring or mock for testing.
pub trait IoEngine: Send + Sync {
    /// Submit a read operation for the given block.
    /// Returns the data read from the block.
    fn read_block(&self, block_ref: BlockRef) -> impl std::future::Future<Output = StorageResult<Vec<u8>>> + Send;

    /// Submit a write operation for the given block.
    /// The data length must match the block size.
    fn write_block(
        &self,
        block_ref: BlockRef,
        data: Vec<u8>,
        hint: Option<PlacementHint>,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    /// Flush all pending writes to persistent storage.
    fn flush(&self) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    /// Discard/trim a block range (TRIM command for NVMe).
    fn discard_block(&self, block_ref: BlockRef) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    /// Get current I/O statistics.
    fn stats(&self) -> IoStats;
}

/// Atomic counter for generating unique I/O request IDs.
#[derive(Debug)]
pub struct IoRequestIdGen {
    next: AtomicU64,
}

impl IoRequestIdGen {
    /// Create a new ID generator starting from 1.
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Generate the next unique request ID.
    pub fn next_id(&self) -> IoRequestId {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        IoRequestId(id)
    }
}

impl Default for IoRequestIdGen {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory mock I/O engine for testing.
/// Stores blocks in a HashMap.
pub struct MockIoEngine {
    blocks: AsyncMutex<HashMap<(u16, u64), Vec<u8>>>,
    stats: std::sync::Mutex<IoStats>,
}

impl MockIoEngine {
    /// Create a new mock I/O engine.
    pub fn new() -> Self {
        Self {
            blocks: AsyncMutex::new(HashMap::new()),
            stats: std::sync::Mutex::new(IoStats::default()),
        }
    }

    fn update_stats<F>(&self, f: F)
    where
        F: FnOnce(&mut IoStats),
    {
        if let Ok(mut stats) = self.stats.lock() {
            f(&mut stats);
        }
    }
}

impl Default for MockIoEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IoEngine for MockIoEngine {
    async fn read_block(&self, block_ref: BlockRef) -> StorageResult<Vec<u8>> {
        let key = (block_ref.id.device_idx, block_ref.id.offset);
        let expected_len = block_ref.size.as_bytes() as usize;

        debug!(
            "Mock read: device={}, offset={}, size={}",
            block_ref.id.device_idx, block_ref.id.offset, expected_len
        );

        let blocks = self.blocks.lock().await;

        match blocks.get(&key) {
            Some(data) if data.len() == expected_len => {
                let result = data.clone();
                drop(blocks);
                self.update_stats(|s| {
                    s.reads_completed += 1;
                    s.bytes_read += expected_len as u64;
                });
                Ok(result)
            }
            Some(_) => {
                drop(blocks);
                self.update_stats(|s| s.errors += 1);
                Err(StorageError::InvalidBlockSize {
                    requested: {
                        let blocks = self.blocks.lock().await;
                        blocks.get(&key).map(|d| d.len() as u64).unwrap_or(0)
                    },
                    valid_sizes: vec![4096, 65536, 1048576, 67108864],
                })
            }
            None => {
                drop(blocks);
                self.update_stats(|s| s.errors += 1);
                Err(StorageError::BlockNotFound {
                    block_id: BlockId::new(block_ref.id.device_idx, block_ref.id.offset),
                })
            }
        }
    }

    async fn write_block(
        &self,
        block_ref: BlockRef,
        data: Vec<u8>,
        _hint: Option<PlacementHint>,
    ) -> StorageResult<()> {
        let key = (block_ref.id.device_idx, block_ref.id.offset);
        let expected_len = block_ref.size.as_bytes() as usize;

        debug!(
            "Mock write: device={}, offset={}, size={}, data_len={}",
            block_ref.id.device_idx,
            block_ref.id.offset,
            expected_len,
            data.len()
        );

        if data.len() != expected_len {
            self.update_stats(|s| s.errors += 1);
            return Err(StorageError::InvalidBlockSize {
                requested: data.len() as u64,
                valid_sizes: vec![4096, 65536, 1048576, 67108864],
            });
        }

        let mut blocks = self.blocks.lock().await;
        blocks.insert(key, data);
        drop(blocks);

        self.update_stats(|s| {
            s.writes_completed += 1;
            s.bytes_written += expected_len as u64;
        });

        Ok(())
    }

    async fn flush(&self) -> StorageResult<()> {
        debug!("Mock flush: syncing all pending writes");
        self.update_stats(|s| s.flushes_completed += 1);
        Ok(())
    }

    async fn discard_block(&self, block_ref: BlockRef) -> StorageResult<()> {
        let key = (block_ref.id.device_idx, block_ref.id.offset);

        debug!(
            "Mock discard: device={}, offset={}",
            block_ref.id.device_idx, block_ref.id.offset
        );

        let mut blocks = self.blocks.lock().await;
        blocks.remove(&key);

        Ok(())
    }

    fn stats(&self) -> IoStats {
        self.stats
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockId;
    use crate::BlockSize;

    #[tokio::test]
    async fn test_mock_write_read_roundtrip() {
        let engine = MockIoEngine::new();
        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let data = vec![0xAB; 4096];

        engine
            .write_block(block_ref, data.clone(), None)
            .await
            .unwrap();

        let read_data = engine.read_block(block_ref).await.unwrap();
        assert_eq!(read_data, data);
    }

    #[tokio::test]
    async fn test_mock_read_nonexistent() {
        let engine = MockIoEngine::new();
        let block_ref = BlockRef {
            id: BlockId::new(0, 999),
            size: BlockSize::B4K,
        };

        let result = engine.read_block(block_ref).await;
        assert!(matches!(result, Err(StorageError::BlockNotFound { .. })));
    }

    #[tokio::test]
    async fn test_mock_write_wrong_size() {
        let engine = MockIoEngine::new();
        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let wrong_size_data = vec![0xAB; 100];

        let result = engine.write_block(block_ref, wrong_size_data, None).await;
        assert!(matches!(result, Err(StorageError::InvalidBlockSize { .. })));
    }

    #[tokio::test]
    async fn test_mock_discard() {
        let engine = MockIoEngine::new();
        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let data = vec![0xAB; 4096];

        engine
            .write_block(block_ref, data, None)
            .await
            .unwrap();
        engine.discard_block(block_ref).await.unwrap();

        let result = engine.read_block(block_ref).await;
        assert!(matches!(result, Err(StorageError::BlockNotFound { .. })));
    }

    #[tokio::test]
    async fn test_mock_flush() {
        let engine = MockIoEngine::new();

        engine.flush().await.unwrap();

        let stats = engine.stats();
        assert_eq!(stats.flushes_completed, 1);
    }

    #[tokio::test]
    async fn test_io_stats_tracking() {
        let engine = MockIoEngine::new();
        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let data = vec![0xAB; 4096];

        engine
            .write_block(block_ref, data.clone(), None)
            .await
            .unwrap();
        engine.read_block(block_ref).await.unwrap();
        engine.flush().await.unwrap();

        let stats = engine.stats();
        assert_eq!(stats.writes_completed, 1);
        assert_eq!(stats.reads_completed, 1);
        assert_eq!(stats.flushes_completed, 1);
        assert_eq!(stats.bytes_written, 4096);
        assert_eq!(stats.bytes_read, 4096);
    }

    #[tokio::test]
    async fn test_request_id_gen() {
        let gen = IoRequestIdGen::new();

        let id1 = gen.next_id();
        let id2 = gen.next_id();
        let id3 = gen.next_id();

        assert_eq!(id1.0, 1);
        assert_eq!(id2.0, 2);
        assert_eq!(id3.0, 3);
        assert!(id1 != id2);
        assert!(id2 != id3);
    }

    #[tokio::test]
    async fn test_multiple_block_sizes() {
        let engine = MockIoEngine::new();

        let block_4k = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let block_64k = BlockRef {
            id: BlockId::new(0, 1),
            size: BlockSize::B64K,
        };
        let block_1m = BlockRef {
            id: BlockId::new(0, 2),
            size: BlockSize::B1M,
        };

        let data_4k = vec![0x11; 4096];
        let data_64k = vec![0x22; 65536];
        let data_1m = vec![0x33; 1048576];

        engine
            .write_block(block_4k, data_4k.clone(), None)
            .await
            .unwrap();
        engine
            .write_block(block_64k, data_64k.clone(), None)
            .await
            .unwrap();
        engine
            .write_block(block_1m, data_1m.clone(), None)
            .await
            .unwrap();

        assert_eq!(engine.read_block(block_4k).await.unwrap(), data_4k);
        assert_eq!(engine.read_block(block_64k).await.unwrap(), data_64k);
        assert_eq!(engine.read_block(block_1m).await.unwrap(), data_1m);
    }
}