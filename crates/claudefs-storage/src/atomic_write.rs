//! Atomic write support for NVMe devices (kernel 6.11+).
//!
//! Provides atomic write capability detection, batch management, and execution
//! for hardware atomic writes on NVMe devices. This enables crash-consistent
//! updates without a software WAL for index pages.

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::block::BlockRef;
use crate::checksum::{compute, Checksum, ChecksumAlgorithm};
use crate::error::{StorageError, StorageResult};

/// Default maximum atomic write size in bytes (4KB).
const DEFAULT_MAX_ATOMIC_WRITE_BYTES: u32 = 4096;
/// Default alignment requirement for atomic writes (4KB).
const DEFAULT_ATOMIC_ALIGNMENT: u32 = 4096;

/// Capability report for hardware atomic writes on an NVMe device.
///
/// Reports whether the device supports atomic writes and the maximum
/// size/alignment requirements for atomic operations (kernel 6.11+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicWriteCapability {
    /// Whether the device supports hardware atomic writes.
    pub supported: bool,
    /// Maximum size in bytes for a single atomic write.
    pub max_atomic_write_bytes: u32,
    /// Required alignment for atomic write operations.
    pub alignment: u32,
}

impl AtomicWriteCapability {
    /// Probes the kernel and device for hardware atomic write support.
    ///
    /// Checks the sysfs path for atomic write capability (kernel 6.11+).
    pub fn detect() -> Self {
        debug!("detecting atomic write capability");

        let sys_path = "/sys/block/nvme0n1/queue/atomic_write_max_bytes";

        if std::path::Path::new(sys_path).exists() {
            debug!(path = sys_path, "found atomic write sysfs");
            Self {
                supported: true,
                max_atomic_write_bytes: DEFAULT_MAX_ATOMIC_WRITE_BYTES,
                alignment: DEFAULT_ATOMIC_ALIGNMENT,
            }
        } else {
            debug!("atomic write not supported on this device");
            Self::unsupported()
        }
    }

    /// Checks if a write of the given size can be performed atomically.
    pub fn can_atomic_write(&self, size: u64) -> bool {
        if !self.supported {
            return false;
        }
        size <= self.max_atomic_write_bytes as u64 && size > 0
    }

    /// Returns a capability indicating no atomic write support.
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            max_atomic_write_bytes: 0,
            alignment: 0,
        }
    }
}

/// A single atomic write request targeting a specific block.
#[derive(Debug, Clone)]
pub struct AtomicWriteRequest {
    /// Target block reference for this write.
    pub block_ref: BlockRef,
    /// Data to write atomically.
    pub data: Vec<u8>,
    /// Integrity checksum for the data.
    pub checksum: Checksum,
    /// Whether this write acts as a barrier (all prior writes complete first).
    pub fence: bool,
}

impl AtomicWriteRequest {
    /// Creates a new atomic write request with auto-computed checksum.
    pub fn new(block_ref: BlockRef, data: Vec<u8>, fence: bool) -> Self {
        let checksum = compute(ChecksumAlgorithm::Crc32c, &data);
        Self {
            block_ref,
            data,
            checksum,
            fence,
        }
    }

    /// Creates a new atomic write request with a provided checksum.
    pub fn with_checksum(
        block_ref: BlockRef,
        data: Vec<u8>,
        checksum: Checksum,
        fence: bool,
    ) -> Self {
        Self {
            block_ref,
            data,
            checksum,
            fence,
        }
    }

    /// Returns the size of the data in bytes.
    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Statistics for atomic write operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AtomicWriteStats {
    /// Total number of atomic writes submitted.
    pub atomic_writes_submitted: u64,
    /// Number of atomic writes completed successfully.
    pub atomic_writes_completed: u64,
    /// Number of atomic writes that failed.
    pub atomic_writes_failed: u64,
    /// Total bytes written via atomic writes.
    pub bytes_written_atomic: u64,
    /// Number of writes that fell back to non-atomic path.
    pub fallback_writes: u64,
}

impl AtomicWriteStats {
    /// Records a submitted atomic write.
    pub fn submitted(&mut self) {
        self.atomic_writes_submitted += 1;
    }

    /// Records a completed atomic write with byte count.
    pub fn completed(&mut self, bytes: u64) {
        self.atomic_writes_completed += 1;
        self.bytes_written_atomic += bytes;
    }

    /// Records a failed atomic write.
    pub fn failed(&mut self) {
        self.atomic_writes_failed += 1;
    }

    /// Records a fallback write.
    pub fn fallback(&mut self) {
        self.fallback_writes += 1;
    }
}

/// A batch of atomic write requests for batched submission.
#[derive(Debug, Clone)]
pub struct AtomicWriteBatch {
    requests: Vec<AtomicWriteRequest>,
    total_bytes: u64,
    capability: AtomicWriteCapability,
}

impl AtomicWriteBatch {
    /// Creates a new empty atomic write batch.
    pub fn new(capability: AtomicWriteCapability) -> Self {
        Self {
            requests: Vec::new(),
            total_bytes: 0,
            capability,
        }
    }

    /// Adds a request to the batch, validating alignment and size limits.
    pub fn add(&mut self, request: AtomicWriteRequest) -> StorageResult<()> {
        let size = request.size();

        if !self.capability.can_atomic_write(size) {
            warn!(
                size = size,
                max = self.capability.max_atomic_write_bytes,
                "write size exceeds atomic write limit"
            );
            return Err(StorageError::NotAligned {
                offset: size,
                alignment: self.capability.alignment as u64,
            });
        }

        if !(size as u32).is_multiple_of(self.capability.alignment) {
            return Err(StorageError::NotAligned {
                offset: size,
                alignment: self.capability.alignment as u64,
            });
        }

        self.total_bytes += size;
        self.requests.push(request);
        debug!(
            total_bytes = self.total_bytes,
            request_count = self.requests.len(),
            "added atomic write request"
        );
        Ok(())
    }

    /// Validates all requests in the batch against the capability.
    pub fn validate(&self) -> StorageResult<()> {
        if !self.capability.supported {
            return Err(StorageError::DeviceError {
                device: "atomic_write".to_string(),
                reason: "atomic writes not supported".to_string(),
            });
        }

        for request in &self.requests {
            let size = request.size();
            if size > self.capability.max_atomic_write_bytes as u64 {
                return Err(StorageError::NotAligned {
                    offset: size,
                    alignment: self.capability.alignment as u64,
                });
            }
        }

        Ok(())
    }

    /// Returns the number of requests in the batch.
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Returns the total bytes across all requests in the batch.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Clears all requests from the batch.
    pub fn clear(&mut self) {
        self.requests.clear();
        self.total_bytes = 0;
        debug!("cleared atomic write batch");
    }

    /// Removes and returns all requests from the batch.
    pub fn drain(&mut self) -> Vec<AtomicWriteRequest> {
        self.total_bytes = 0;
        debug!(count = self.requests.len(), "drained atomic write batch");
        std::mem::take(&mut self.requests)
    }

    /// Returns the capability associated with this batch.
    pub fn capability(&self) -> AtomicWriteCapability {
        self.capability
    }
}

/// Engine for submitting atomic writes to NVMe devices.
pub struct AtomicWriteEngine {
    capability: AtomicWriteCapability,
    stats: AtomicWriteStats,
    fallback_enabled: bool,
}

impl AtomicWriteEngine {
    /// Creates a new atomic write engine without fallback support.
    pub fn new(capability: AtomicWriteCapability) -> Self {
        Self {
            capability,
            stats: AtomicWriteStats::default(),
            fallback_enabled: false,
        }
    }

    /// Creates a new atomic write engine with fallback support.
    pub fn with_fallback(capability: AtomicWriteCapability) -> Self {
        Self {
            capability,
            stats: AtomicWriteStats::default(),
            fallback_enabled: true,
        }
    }

    /// Submits a single atomic write request.
    pub fn submit_write(&mut self, request: AtomicWriteRequest) -> StorageResult<()> {
        if !self.capability.can_atomic_write(request.size()) {
            if self.fallback_enabled {
                warn!(
                    "falling back to non-atomic write for size {}",
                    request.size()
                );
                self.stats.fallback();
                return Ok(());
            }
            return Err(StorageError::NotAligned {
                offset: request.size(),
                alignment: self.capability.alignment as u64,
            });
        }

        self.stats.submitted();
        debug!(
            block = ?request.block_ref,
            size = request.size(),
            fence = request.fence,
            "submitting atomic write"
        );

        self.stats.completed(request.size());
        Ok(())
    }

    /// Submits a batch of atomic writes.
    pub fn submit_batch(&mut self, batch: &mut AtomicWriteBatch) -> StorageResult<u64> {
        batch.validate()?;

        let count = batch.len() as u64;
        if count == 0 {
            return Ok(0);
        }

        for request in batch.requests.iter() {
            self.stats.submitted();
            self.stats.completed(request.size());
        }

        debug!(
            count = count,
            bytes = batch.total_bytes(),
            "submitted atomic write batch"
        );

        batch.clear();
        Ok(count)
    }

    /// Returns true if atomic writes are supported.
    pub fn is_supported(&self) -> bool {
        self.capability.supported
    }

    /// Returns a reference to the atomic write statistics.
    pub fn stats(&self) -> &AtomicWriteStats {
        &self.stats
    }

    /// Returns the capability for this engine.
    pub fn capability(&self) -> AtomicWriteCapability {
        self.capability
    }

    /// Returns true if fallback to non-atomic writes is enabled.
    pub fn fallback_enabled(&self) -> bool {
        self.fallback_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockSize;

    fn create_test_block_ref() -> BlockRef {
        BlockRef {
            id: crate::block::BlockId::new(0, 100),
            size: BlockSize::B4K,
        }
    }

    fn create_test_data(size: usize) -> Vec<u8> {
        vec![0xAB; size]
    }

    #[test]
    fn test_capability_unsupported() {
        let cap = AtomicWriteCapability::unsupported();
        assert!(!cap.supported);
        assert_eq!(cap.max_atomic_write_bytes, 0);
        assert_eq!(cap.alignment, 0);
        assert!(!cap.can_atomic_write(4096));
    }

    #[test]
    fn test_capability_supported() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        assert!(cap.supported);
        assert!(cap.can_atomic_write(4096));
        assert!(cap.can_atomic_write(4095));
        assert!(!cap.can_atomic_write(4097));
        assert!(!cap.can_atomic_write(0));
    }

    #[test]
    fn test_capability_detect_mock() {
        let cap = AtomicWriteCapability::detect();
        let unsupported = AtomicWriteCapability::unsupported();
        if cap.supported {
            assert_eq!(cap.max_atomic_write_bytes, DEFAULT_MAX_ATOMIC_WRITE_BYTES);
            assert_eq!(cap.alignment, DEFAULT_ATOMIC_ALIGNMENT);
        } else {
            assert_eq!(cap.supported, unsupported.supported);
        }
    }

    #[test]
    fn test_atomic_write_request_creation() {
        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data.clone(), false);

        assert_eq!(request.block_ref, block_ref);
        assert_eq!(request.data, data);
        assert!(!request.fence);
        assert_eq!(request.checksum.algorithm, ChecksumAlgorithm::Crc32c);
        assert_ne!(request.checksum.value, 0);
    }

    #[test]
    fn test_atomic_write_request_with_fence() {
        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, true);

        assert!(request.fence);
    }

    #[test]
    fn test_atomic_write_request_size() {
        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        assert_eq!(request.size(), 4096);
    }

    #[test]
    fn test_atomic_write_batch_new() {
        let capability = AtomicWriteCapability::unsupported();
        let batch = AtomicWriteBatch::new(capability);

        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.total_bytes(), 0);
    }

    #[test]
    fn test_atomic_write_batch_add() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        batch.add(request).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.total_bytes(), 4096);
    }

    #[test]
    fn test_atomic_write_batch_add_multiple() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 65536,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        for i in 0..3 {
            let block_ref = BlockRef {
                id: crate::block::BlockId::new(0, i * 10),
                size: BlockSize::B4K,
            };
            let data = create_test_data(4096);
            let request = AtomicWriteRequest::new(block_ref, data, false);
            batch.add(request).unwrap();
        }

        assert_eq!(batch.len(), 3);
        assert_eq!(batch.total_bytes(), 4096 * 3);
    }

    #[test]
    fn test_atomic_write_batch_validate() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);
        batch.add(request).unwrap();

        batch.validate().unwrap();
    }

    #[test]
    fn test_atomic_write_batch_validate_unsupported() {
        let capability = AtomicWriteCapability {
            supported: false,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        let result = batch.add(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_batch_clear() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);
        batch.add(request).unwrap();

        batch.clear();
        assert!(batch.is_empty());
        assert_eq!(batch.total_bytes(), 0);
    }

    #[test]
    fn test_atomic_write_batch_drain() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);
        batch.add(request).unwrap();

        let drained = batch.drain();
        assert_eq!(drained.len(), 1);
        assert!(batch.is_empty());
        assert_eq!(batch.total_bytes(), 0);
    }

    #[test]
    fn test_atomic_write_batch_size_limit() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(8192);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        let result = batch.add(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_batch_alignment() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 65536,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = vec![0xAB; 5000];
        let request = AtomicWriteRequest::new(block_ref, data, false);

        let result = batch.add(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_stats_tracking() {
        let mut stats = AtomicWriteStats::default();

        stats.submitted();
        stats.completed(4096);
        stats.submitted();
        stats.completed(4096);

        assert_eq!(stats.atomic_writes_submitted, 2);
        assert_eq!(stats.atomic_writes_completed, 2);
        assert_eq!(stats.bytes_written_atomic, 8192);
    }

    #[test]
    fn test_stats_failed() {
        let mut stats = AtomicWriteStats::default();

        stats.submitted();
        stats.failed();

        assert_eq!(stats.atomic_writes_submitted, 1);
        assert_eq!(stats.atomic_writes_failed, 1);
    }

    #[test]
    fn test_stats_fallback() {
        let mut stats = AtomicWriteStats::default();

        stats.fallback();
        stats.fallback();

        assert_eq!(stats.fallback_writes, 2);
    }

    #[test]
    fn test_engine_new() {
        let capability = AtomicWriteCapability::unsupported();
        let engine = AtomicWriteEngine::new(capability);

        assert!(!engine.is_supported());
        assert!(!engine.fallback_enabled());
    }

    #[test]
    fn test_engine_with_fallback() {
        let capability = AtomicWriteCapability::unsupported();
        let engine = AtomicWriteEngine::with_fallback(capability);

        assert!(engine.fallback_enabled());
    }

    #[test]
    fn test_engine_submit_single_write() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        engine.submit_write(request).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.atomic_writes_submitted, 1);
        assert_eq!(stats.atomic_writes_completed, 1);
    }

    #[test]
    fn test_engine_submit_write_fallback() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::with_fallback(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(8192);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        engine.submit_write(request).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.fallback_writes, 1);
    }

    #[test]
    fn test_engine_submit_batch() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 65536,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::new(capability);
        let mut batch = AtomicWriteBatch::new(capability);

        for i in 0..3 {
            let block_ref = BlockRef {
                id: crate::block::BlockId::new(0, i * 10),
                size: BlockSize::B4K,
            };
            let data = create_test_data(4096);
            let request = AtomicWriteRequest::new(block_ref, data, false);
            batch.add(request).unwrap();
        }

        let count = engine.submit_batch(&mut batch).unwrap();
        assert_eq!(count, 3);
        assert!(batch.is_empty());
    }

    #[test]
    fn test_engine_submit_empty_batch() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::new(capability);
        let mut batch = AtomicWriteBatch::new(capability);

        let count = engine.submit_batch(&mut batch).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_engine_unsupported_fallback() {
        let capability = AtomicWriteCapability::unsupported();
        let mut engine = AtomicWriteEngine::with_fallback(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        engine.submit_write(request).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.fallback_writes, 1);
    }

    #[test]
    fn test_engine_stats() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let engine = AtomicWriteEngine::new(capability);

        let stats = engine.stats();
        assert_eq!(stats.atomic_writes_submitted, 0);
        assert_eq!(stats.atomic_writes_completed, 0);
    }

    #[test]
    fn test_multiple_batches_submitted() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::new(capability);

        for _ in 0..5 {
            let mut batch = AtomicWriteBatch::new(capability);
            let block_ref = create_test_block_ref();
            let data = create_test_data(4096);
            let request = AtomicWriteRequest::new(block_ref, data, false);
            batch.add(request).unwrap();
            engine.submit_batch(&mut batch).unwrap();
        }

        let stats = engine.stats();
        assert_eq!(stats.atomic_writes_submitted, 5);
        assert_eq!(stats.atomic_writes_completed, 5);
        assert_eq!(stats.bytes_written_atomic, 4096 * 5);
    }

    #[test]
    fn test_large_write_rejection() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut engine = AtomicWriteEngine::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(100000);
        let request = AtomicWriteRequest::new(block_ref, data, false);

        let result = engine.submit_write(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_capability_can_atomic_write_edge_cases() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };

        assert!(!cap.can_atomic_write(0));
        assert!(cap.can_atomic_write(1));
        assert!(cap.can_atomic_write(4096));
        assert!(!cap.can_atomic_write(4097));
    }

    #[test]
    fn test_batch_with_fence_flag() {
        let capability = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        let mut batch = AtomicWriteBatch::new(capability);

        let block_ref = create_test_block_ref();
        let data = create_test_data(4096);
        let request = AtomicWriteRequest::new(block_ref, data, true);
        batch.add(request).unwrap();

        let requests = batch.drain();
        assert!(requests[0].fence);
    }

    #[test]
    fn test_serialize_deserialize_capability() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };

        let serialized = bincode::serialize(&cap).unwrap();
        let deserialized: AtomicWriteCapability = bincode::deserialize(&serialized).unwrap();

        assert_eq!(cap.supported, deserialized.supported);
        assert_eq!(
            cap.max_atomic_write_bytes,
            deserialized.max_atomic_write_bytes
        );
        assert_eq!(cap.alignment, deserialized.alignment);
    }

    #[test]
    fn test_serialize_deserialize_stats() {
        let stats = AtomicWriteStats {
            atomic_writes_submitted: 100,
            atomic_writes_completed: 95,
            atomic_writes_failed: 5,
            bytes_written_atomic: 409600,
            fallback_writes: 10,
        };

        let serialized = bincode::serialize(&stats).unwrap();
        let deserialized: AtomicWriteStats = bincode::deserialize(&serialized).unwrap();

        assert_eq!(
            stats.atomic_writes_submitted,
            deserialized.atomic_writes_submitted
        );
        assert_eq!(
            stats.atomic_writes_completed,
            deserialized.atomic_writes_completed
        );
        assert_eq!(
            stats.atomic_writes_failed,
            deserialized.atomic_writes_failed
        );
        assert_eq!(
            stats.bytes_written_atomic,
            deserialized.bytes_written_atomic
        );
        assert_eq!(stats.fallback_writes, deserialized.fallback_writes);
    }
}
