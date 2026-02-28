//! Real io_uring-based I/O engine for NVMe block operations.
//!
//! This module provides a production-ready I/O engine using Linux io_uring
//! for high-performance asynchronous NVMe operations with O_DIRECT passthrough.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{IoSlice, SeekFrom};
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use io_uring::opcode::{Fallocate, Fsync, Read, Write, Flush};
use io_uring::types::Timespec;
use io_uring::{IoUring, Queue, SubmissionQueue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task;
use tracing::{debug, error, info, warn};

use crate::block::{BlockId, BlockRef, BlockSize, PlacementHint};
use crate::error::{StorageError, StorageResult};
use crate::io_uring_bridge::{IoEngine, IoStats};

/// Configuration for the UringIoEngine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UringConfig {
    /// Submission queue depth (number of concurrent operations). Default: 256.
    pub queue_depth: u32,
    /// Use O_DIRECT for NVMe passthrough (bypass page cache). Default: true.
    pub direct_io: bool,
    /// Use IORING_SETUP_IOPOLL for interrupt-less polling. Default: false.
    pub io_poll: bool,
    /// Use IORING_SETUP_SQPOLL for kernel-side submission polling. Default: false.
    pub sq_poll: bool,
}

impl Default for UringConfig {
    fn default() -> Self {
        Self {
            queue_depth: 256,
            direct_io: true,
            io_poll: false,
            sq_poll: false,
        }
    }
}

/// Errors specific to io_uring operations.
#[derive(Debug, Error)]
pub enum UringError {
    #[error("io_uring setup failed: {0}")]
    SetupError(String),
    #[error("io_uring submission failed: {0}")]
    SubmitError(String),
    #[error("io_uring completion error: {0}")]
    CompletionError(String),
    #[error("Device not found: device_idx={0}")]
    DeviceNotFound(u16),
    #[error("Alignment error: {0}")]
    AlignmentError(String),
    #[error("Buffer allocation error: {0}")]
    BufferError(String),
}

/// Atomic statistics for the io_uring engine.
#[derive(Debug, Default)]
pub struct UringStats {
    reads_completed: AtomicU64,
    writes_completed: AtomicU64,
    flushes_completed: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    errors: AtomicU64,
}

impl UringStats {
    fn add_read(&self, bytes: u64) {
        self.reads_completed.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_write(&self, bytes: u64) {
        self.writes_completed.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_flush(&self) {
        self.flushes_completed.fetch_add(1, Ordering::Relaxed);
    }

    fn add_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

impl From<&UringStats> for IoStats {
    fn from(stats: &UringStats) -> Self {
        IoStats {
            reads_completed: stats.reads_completed.load(Ordering::Relaxed),
            writes_completed: stats.writes_completed.load(Ordering::Relaxed),
            flushes_completed: stats.flushes_completed.load(Ordering::Relaxed),
            bytes_read: stats.bytes_read.load(Ordering::Relaxed),
            bytes_written: stats.bytes_written.load(Ordering::Relaxed),
            errors: stats.errors.load(Ordering::Relaxed),
            queue_depth: 0,
        }
    }
}

/// Real io_uring-based I/O engine for NVMe block operations.
///
/// Uses Linux io_uring for efficient asynchronous I/O with support for:
/// - O_DIRECT to bypass page cache for NVMe passthrough
/// - Configurable queue depth for throughput tuning
/// - Optional IORING_SETUP_IOPOLL for lowest latency
/// - Fsync for durability, fallocate for TRIM/discard
pub struct UringIoEngine {
    config: UringConfig,
    ring: Mutex<IoUring>,
    device_fds: RwLock<HashMap<u16, RawFd>>,
    stats: UringStats,
}

unsafe impl Send for UringIoEngine {}
unsafe impl Sync for UringIoEngine {}

impl UringIoEngine {
    /// Creates a new UringIoEngine with the given configuration.
    pub fn new(config: UringConfig) -> StorageResult<Self> {
        let mut builder = IoUring::builder();

        builder
            .max_entries(config.queue_depth as u32)
            .flags(if config.io_poll {
                io_uring::flags::IOPOLL
            } else {
                io_uring::flags::None
            })
            .flags(if config.sq_poll {
                io_uring::flags::SQPOLL
            } else {
                io_uring::flags::None
            });

        let ring = builder
            .build()
            .map_err(|e| UringError::SetupError(e.to_string()))?;

        info!(
            "UringIoEngine initialized: queue_depth={}, direct_io={}, io_poll={}, sq_poll={}",
            config.queue_depth, config.direct_io, config.io_poll, config.sq_poll
        );

        Ok(Self {
            config,
            ring: Mutex::new(ring),
            device_fds: RwLock::new(HashMap::new()),
            stats: UringStats::default(),
        })
    }

    /// Registers a device with the engine.
    ///
    /// # Arguments
    /// * `device_idx` - The logical device index to associate with this file
    /// * `path` - Path to the block device or file (e.g., /dev/nvme0n1 or /data/file)
    ///
    /// # Returns
    /// `Ok(())` on success, error otherwise
    pub fn register_device(&self, device_idx: u16, path: &str) -> StorageResult<()> {
        let flags = if self.config.direct_io {
            libc::O_RDWR | libc::O_DIRECT
        } else {
            libc::O_RDWR
        };

        let fd = unsafe {
            libc::open(path.as_ptr() as *const libc::c_char, flags, 0o644)
        };

        if fd < 0 {
            let err = std::io::Error::last_os_error();
            error!("Failed to open device {}: {}", path, err);
            return Err(StorageError::DeviceError {
                device: path.to_string(),
                reason: err.to_string(),
            });
        }

        let mut fds = self.device_fds.write().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire device_fds write lock".to_string())
        })?;

        fds.insert(device_idx, fd);
        info!("Registered device {} at {}", device_idx, path);

        Ok(())
    }

    /// Registers a file descriptor directly with the engine.
    pub fn register_fd(&self, device_idx: u16, fd: RawFd) -> StorageResult<()> {
        let mut fds = self.device_fds.write().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire device_fds write lock".to_string())
        })?;

        fds.insert(device_idx, fd);
        Ok(())
    }

    /// Allocates an aligned buffer for I/O operations.
    fn allocate_buffer(size: usize) -> StorageResult<*mut u8> {
        let alignment = 4096usize;
        let mut ptr: *mut libc::c_void = std::ptr::null_mut();

        let ret = unsafe { libc::posix_memalign(&mut ptr, alignment, size) };

        if ret != 0 {
            return Err(UringError::BufferError(format!(
                "posix_memalign failed with code {}",
                ret
            ))
            .into());
        }

        Ok(ptr as *mut u8)
    }

    /// Gets the file descriptor for a device index.
    fn get_fd(&self, device_idx: u16) -> StorageResult<RawFd> {
        let fds = self.device_fds.read().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire device_fds read lock".to_string())
        })?;

        fds.get(&device_idx)
            .copied()
            .ok_or_else(|| UringError::DeviceNotFound(device_idx).into())
    }

    /// Performs a synchronous read operation through io_uring.
    fn read_at(&self, fd: RawFd, offset: u64, buf: &mut [u8]) -> StorageResult<usize> {
        let ring = self.ring.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire ring lock".to_string())
        })?;

        let mut sq = ring.submission_shared();
        let mut cq = ring.completion_shared();

        let read_op = Read::new(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32)
            .offset(offset as u64)
            .build()
            .user_data(0x1);

        unsafe {
            sq.push(&read_op).map_err(|e| UringError::SubmitError(e.to_string()))?;
        }

        ring.submit_and_wait(1).map_err(|e| UringError::SubmitError(e.to_string()))?;

        for cqe in &cq {
            if cqe.result() < 0 {
                let err = std::io::Error::from_raw_os_error(-cqe.result());
                return Err(StorageError::IoError(err));
            }
            return Ok(cqe.result() as usize);
        }

        Err(StorageError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "No completion received",
        )))
    }

    /// Performs a synchronous write operation through io_uring.
    fn write_at(&self, fd: RawFd, offset: u64, buf: &[u8]) -> StorageResult<usize> {
        let ring = self.ring.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire ring lock".to_string())
        })?;

        let mut sq = ring.submission_shared();
        let mut cq = ring.completion_shared();

        let write_op = Write::new(fd, buf.as_ptr() as *const libc::c_void, buf.len() as u32)
            .offset(offset as u64)
            .build()
            .user_data(0x2);

        unsafe {
            sq.push(&write_op).map_err(|e| UringError::SubmitError(e.to_string()))?;
        }

        ring.submit_and_wait(1).map_err(|e| UringError::SubmitError(e.to_string()))?;

        for cqe in &cq {
            if cqe.result() < 0 {
                let err = std::io::Error::from_raw_os_error(-cqe.result());
                return Err(StorageError::IoError(err));
            }
            return Ok(cqe.result() as usize);
        }

        Err(StorageError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "No completion received",
        )))
    }

    /// Performs fsync on a file descriptor.
    fn fsync(&self, fd: RawFd) -> StorageResult<()> {
        let ring = self.ring.lock().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire ring lock".to_string())
        })?;

        let mut sq = ring.submission_shared();
        let mut cq = ring.completion_shared();

        let fsync_op = Fsync::new(fd).build().user_data(0x3);

        unsafe {
            sq.push(&fsync_op).map_err(|e| UringError::SubmitError(e.to_string()))?;
        }

        ring.submit_and_wait(1).map_err(|e| UringError::SubmitError(e.to_string()))?;

        for cqe in &cq {
            if cqe.result() < 0 {
                let err = std::io::Error::from_raw_os_error(-cqe.result());
                return Err(StorageError::IoError(err));
            }
            return Ok(());
        }

        Ok(())
    }

    /// Performs discard (TRIM) operation using fallocate.
    fn discard_at(&self, fd: RawFd, offset: u64, length: u64) -> StorageResult<()> {
        let ret = unsafe {
            libc::fallocate(
                fd,
                libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                offset as libc::off_t,
                length as libc::off_t,
            )
        };

        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(StorageError::IoError(err));
        }

        Ok(())
    }

    fn block_ref_to_byte_offset(block_ref: BlockRef) -> u64 {
        block_ref.id.offset * block_ref.size.as_bytes()
    }
}

impl IoEngine for UringIoEngine {
    async fn read_block(&self, block_ref: BlockRef) -> StorageResult<Vec<u8>> {
        let fd = self.get_fd(block_ref.id.device_idx)?;
        let byte_offset = Self::block_ref_to_byte_offset(block_ref);
        let size = block_ref.size.as_bytes() as usize;

        let ptr = Self::allocate_buffer(size)?;
        let buf = unsafe { std::slice::from_raw_parts_mut(ptr, size) };

        let result = task::spawn_blocking(move || {
            let ring = match IoUring::builder().max_entries(256).build() {
                Ok(r) => r,
                Err(e) => return Err(StorageError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))),
            };

            let mut sq = ring.submission_shared();
            let mut cq = ring.completion_shared();

            let read_op = Read::new(fd, buf.as_mut_ptr() as *mut libc::c_void, size as u32)
                .offset(byte_offset)
                .build()
                .user_data(0x1);

            unsafe {
                if let Err(e) = sq.push(&read_op) {
                    return Err(StorageError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )));
                }
            }

            if let Err(e) = ring.submit_and_wait(1) {
                return Err(StorageError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }

            for cqe in &cq {
                if cqe.result() < 0 {
                    return Err(StorageError::IoError(std::io::Error::from_raw_os_error(
                        -cqe.result(),
                    )));
                }
                return Ok(cqe.result() as usize);
            }

            Err(StorageError::IoError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "No completion received",
            )))
        })
        .await
        .map_err(|e| StorageError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))??;

        let bytes_read = result?;
        let data = unsafe { Vec::from_raw_parts(ptr, bytes_read, size) };

        debug!(
            "Read {} bytes from device {} offset {}",
            bytes_read, block_ref.id.device_idx, byte_offset
        );

        self.stats.add_read(bytes_read as u64);

        Ok(data)
    }

    async fn write_block(
        &self,
        block_ref: BlockRef,
        data: Vec<u8>,
        _hint: Option<PlacementHint>,
    ) -> StorageResult<()> {
        let fd = self.get_fd(block_ref.id.device_idx)?;
        let byte_offset = Self::block_ref_to_byte_offset(block_ref);
        let expected_size = block_ref.size.as_bytes() as usize;

        if data.len() != expected_size {
            return Err(StorageError::InvalidBlockSize {
                requested: data.len() as u64,
                valid_sizes: vec![4096, 65536, 1048576, 67108864],
            });
        }

        let data_len = data.len();
        let ptr = data.as_ptr();
        let data = unsafe { std::slice::from_raw_parts(ptr, data_len) };

        task::spawn_blocking(move || {
            let ring = match IoUring::builder().max_entries(256).build() {
                Ok(r) => r,
                Err(e) => return Err(StorageError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))),
            };

            let mut sq = ring.submission_shared();
            let mut cq = ring.completion_shared();

            let write_op = Write::new(fd, data.as_ptr() as *const libc::c_void, data_len as u32)
                .offset(byte_offset)
                .build()
                .user_data(0x2);

            unsafe {
                if let Err(e) = sq.push(&write_op) {
                    return Err(StorageError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )));
                }
            }

            if let Err(e) = ring.submit_and_wait(1) {
                return Err(StorageError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }

            for cqe in &cq {
                if cqe.result() < 0 {
                    return Err(StorageError::IoError(std::io::Error::from_raw_os_error(
                        -cqe.result(),
                    )));
                }
                return Ok(());
            }

            Ok(())
        })
        .await
        .map_err(|e| StorageError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))??;

        debug!(
            "Wrote {} bytes to device {} offset {}",
            data_len, block_ref.id.device_idx, byte_offset
        );

        self.stats.add_write(data_len as u64);

        Ok(())
    }

    async fn flush(&self) -> StorageResult<()> {
        let fds = {
            let fds = self.device_fds.read().map_err(|_| {
                StorageError::AllocatorError("Failed to acquire device_fds read lock".to_string())
            })?;
            fds.clone()
        };

        for (&device_idx, &fd) in &fds {
            debug!("Flushing device {}", device_idx);
            self.fsync(fd)?;
            self.stats.add_flush();
        }

        Ok(())
    }

    async fn discard_block(&self, block_ref: BlockRef) -> StorageResult<()> {
        let fd = self.get_fd(block_ref.id.device_idx)?;
        let byte_offset = Self::block_ref_to_byte_offset(block_ref);
        let length = block_ref.size.as_bytes();

        task::spawn_blocking(move || {
            let ret = unsafe {
                libc::fallocate(
                    fd,
                    libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                    byte_offset as libc::off_t,
                    length as libc::off_t,
                )
            };

            if ret != 0 {
                return Err(StorageError::IoError(std::io::Error::last_os_error()));
            }
            Ok(())
        })
        .await
        .map_err(|e| StorageError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))??;

        debug!(
            "Discarded device {} offset {} ({} bytes)",
            block_ref.id.device_idx, byte_offset, length
        );

        Ok(())
    }

    fn stats(&self) -> IoStats {
        IoStats::from(&self.stats)
    }
}

impl Drop for UringIoEngine {
    fn drop(&mut self) {
        if let Ok(fds) = self.device_fds.read() {
            for (&_device_idx, &fd) in fds.iter() {
                unsafe { libc::close(fd) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn check_uring_available() -> bool {
        std::path::Path::new("/sys/kernel/debug/io_uring").exists()
    }

    #[tokio::test]
    async fn test_uring_engine_creation() {
        if !check_uring_available() {
            return;
        }

        let config = UringConfig::default();
        let engine = UringIoEngine::new(config).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.reads_completed, 0);
        assert_eq!(stats.writes_completed, 0);
    }

    #[tokio::test]
    async fn test_uring_engine_with_tempfile() {
        if !check_uring_available() {
            return;
        }

        let tmpdir = tempdir().unwrap();
        let file_path = tmpdir.path().join("test_device");
        let file = fs::File::create(&file_path).unwrap();

        // Set to non-direct IO for temp files (O_DIRECT may fail on tmpfs)
        let config = UringConfig {
            direct_io: false,
            ..Default::default()
        };

        let engine = UringIoEngine::new(config).unwrap();
        engine
            .register_device(0, file_path.to_str().unwrap())
            .unwrap();

        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };

        let test_data = vec![0xAB; 4096];
        engine
            .write_block(block_ref, test_data.clone(), None)
            .await
            .unwrap();

        let read_data = engine.read_block(block_ref).await.unwrap();
        assert_eq!(read_data, test_data);

        let stats = engine.stats();
        assert_eq!(stats.writes_completed, 1);
        assert_eq!(stats.reads_completed, 1);
    }

    #[tokio::test]
    async fn test_uring_engine_multiple_block_sizes() {
        if !check_uring_available() {
            return;
        }

        let tmpdir = tempdir().unwrap();
        let file_path = tmpdir.path().join("test_device");

        // Create a larger file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path)
            .unwrap();
        file.set_len(10 * 1024 * 1024).unwrap();

        let config = UringConfig {
            direct_io: false,
            ..Default::default()
        };

        let engine = UringIoEngine::new(config).unwrap();
        engine
            .register_device(0, file_path.to_str().unwrap())
            .unwrap();

        // Test 4K block
        let block_4k = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };
        let data_4k = vec![0x11; 4096];
        engine
            .write_block(block_4k, data_4k.clone(), None)
            .await
            .unwrap();

        // Test 64K block
        let block_64k = BlockRef {
            id: BlockId::new(0, 1),
            size: BlockSize::B64K,
        };
        let data_64k = vec![0x22; 65536];
        engine
            .write_block(block_64k, data_64k.clone(), None)
            .await
            .unwrap();

        // Test 1M block
        let block_1m = BlockRef {
            id: BlockId::new(0, 2),
            size: BlockSize::B1M,
        };
        let data_1m = vec![0x33; 1048576];
        engine
            .write_block(block_1m, data_1m.clone(), None)
            .await
            .unwrap();

        // Read back all blocks
        let read_4k = engine.read_block(block_4k).await.unwrap();
        let read_64k = engine.read_block(block_64k).await.unwrap();
        let read_1m = engine.read_block(block_1m).await.unwrap();

        assert_eq!(read_4k, data_4k);
        assert_eq!(read_64k, data_64k);
        assert_eq!(read_1m, data_1m);
    }

    #[tokio::test]
    async fn test_uring_engine_flush() {
        if !check_uring_available() {
            return;
        }

        let tmpdir = tempdir().unwrap();
        let file_path = tmpdir.path().join("test_flush");
        let _file = fs::File::create(&file_path).unwrap();

        let config = UringConfig {
            direct_io: false,
            ..Default::default()
        };

        let engine = UringIoEngine::new(config).unwrap();
        engine
            .register_device(0, file_path.to_str().unwrap())
            .unwrap();

        engine.flush().await.unwrap();

        let stats = engine.stats();
        assert!(stats.flushes_completed > 0);
    }

    #[tokio::test]
    async fn test_uring_engine_discard() {
        if !check_uring_available() {
            return;
        }

        let tmpdir = tempdir().unwrap();
        let file_path = tmpdir.path().join("test_discard");
        let _file = fs::File::create(&file_path).unwrap();

        let config = UringConfig {
            direct_io: false,
            ..Default::default()
        };

        let engine = UringIoEngine::new(config).unwrap();
        engine
            .register_device(0, file_path.to_str().unwrap())
            .unwrap();

        let block_ref = BlockRef {
            id: BlockId::new(0, 0),
            size: BlockSize::B4K,
        };

        let test_data = vec![0xCD; 4096];
        engine
            .write_block(block_ref, test_data, None)
            .await
            .unwrap();

        // Discard the block
        engine.discard_block(block_ref).await.unwrap();
    }

    #[test]
    fn test_uring_config_defaults() {
        let config = UringConfig::default();
        assert_eq!(config.queue_depth, 256);
        assert!(config.direct_io);
        assert!(!config.io_poll);
        assert!(!config.sq_poll);
    }

    #[test]
    fn test_uring_stats() {
        let stats = UringStats::default();

        stats.add_read(4096);
        stats.add_write(8192);
        stats.add_flush();
        stats.add_error();

        assert_eq!(stats.reads_completed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.writes_completed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.flushes_completed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.bytes_read.load(Ordering::Relaxed), 4096);
        assert_eq!(stats.bytes_written.load(Ordering::Relaxed), 8192);
        assert_eq!(stats.errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_block_ref_byte_offset_calculation() {
        let block_ref = BlockRef {
            id: BlockId::new(0, 100),
            size: BlockSize::B4K,
        };
        let offset = UringIoEngine::block_ref_to_byte_offset(block_ref);
        assert_eq!(offset, 100 * 4096);

        let block_ref_64k = BlockRef {
            id: BlockId::new(0, 10),
            size: BlockSize::B64K,
        };
        let offset_64k = UringIoEngine::block_ref_to_byte_offset(block_ref_64k);
        assert_eq!(offset_64k, 10 * 65536);

        let block_ref_1m = BlockRef {
            id: BlockId::new(0, 5),
            size: BlockSize::B1M,
        };
        let offset_1m = UringIoEngine::block_ref_to_byte_offset(block_ref_1m);
        assert_eq!(offset_1m, 5 * 1048576);
    }
}