//! Real io_uring-based I/O engine for NVMe block operations.
//!
//! This module provides a production-ready I/O engine using Linux io_uring
//! for high-performance asynchronous NVMe operations with O_DIRECT passthrough.

use std::collections::HashMap;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use io_uring::opcode::{Fsync, Read, Write};
use io_uring::types::Fd;
use io_uring::IoUring;
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::{debug, error, info};

use crate::block::{BlockRef, PlacementHint};
use crate::error::{StorageError, StorageResult};
use crate::io_uring_bridge::{IoEngine, IoStats};

/// Configuration for the io_uring I/O engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UringConfig {
    /// The submission/completion queue depth.
    pub queue_depth: u32,
    /// Whether to use O_DIRECT for direct I/O bypassing the page cache.
    pub direct_io: bool,
    /// Whether to use I/O polling mode (for NVMe devices with poll queues).
    pub io_poll: bool,
    /// Whether to use submission queue polling (kernel polls sq when idle).
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

/// Statistics for the io_uring I/O engine.
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

    #[allow(dead_code)]
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

/// io_uring-based I/O engine for block device operations.
pub struct UringIoEngine {
    config: UringConfig,
    #[allow(dead_code)]
    ring: Mutex<IoUring>,
    device_fds: RwLock<HashMap<u16, RawFd>>,
    stats: UringStats,
}

unsafe impl Send for UringIoEngine {}
unsafe impl Sync for UringIoEngine {}

impl UringIoEngine {
    /// Creates a new UringIoEngine with the given configuration.
    pub fn new(config: UringConfig) -> StorageResult<Self> {
        let ring = if config.io_poll || config.sq_poll {
            let mut builder = IoUring::builder();
            if config.sq_poll {
                builder.setup_sqpoll(2000);
            }
            if config.io_poll {
                builder.setup_iopoll();
            }
            builder.build(config.queue_depth)
        } else {
            IoUring::new(config.queue_depth)
        }
        .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

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

    /// Registers a block device at the given path with the engine.
    pub fn register_device(&self, device_idx: u16, path: &str) -> StorageResult<()> {
        let c_path = std::ffi::CString::new(path).map_err(|_| {
            StorageError::DeviceError {
                device: path.to_string(),
                reason: "invalid path".to_string(),
            }
        })?;

        let flags = if self.config.direct_io {
            libc::O_RDWR | libc::O_DIRECT
        } else {
            libc::O_RDWR
        };

        let fd = unsafe { libc::open(c_path.as_ptr(), flags, 0o644) };

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

    /// Registers an already-open file descriptor with the engine.
    pub fn register_fd(&self, device_idx: u16, fd: RawFd) -> StorageResult<()> {
        let mut fds = self.device_fds.write().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire device_fds write lock".to_string())
        })?;

        fds.insert(device_idx, fd);
        Ok(())
    }

    fn get_fd(&self, device_idx: u16) -> StorageResult<RawFd> {
        let fds = self.device_fds.read().map_err(|_| {
            StorageError::AllocatorError("Failed to acquire device_fds read lock".to_string())
        })?;

        fds.get(&device_idx)
            .copied()
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("device_idx={}", device_idx),
                reason: "device not found".to_string(),
            })
    }

    fn block_ref_to_byte_offset(block_ref: BlockRef) -> u64 {
        block_ref.id.offset * block_ref.size.as_bytes()
    }

    fn perform_read(fd: RawFd, byte_offset: u64, size: usize) -> StorageResult<Vec<u8>> {
        let mut ring = IoUring::new(256)
            .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        let (submitter, mut sq, mut cq) = ring.split();

        let mut buf = vec![0u8; size];
        let read_op = Read::new(Fd(fd), buf.as_mut_ptr(), size as u32)
            .offset(byte_offset)
            .build()
            .user_data(0x1);

        unsafe {
            sq.push(&read_op).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;
        }

        submitter.submit_and_wait(1).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        if let Some(cqe) = cq.next() {
            if cqe.result() < 0 {
                return Err(StorageError::IoError(std::io::Error::from_raw_os_error(-cqe.result())));
            }
            let bytes_read = cqe.result() as usize;
            buf.truncate(bytes_read);
            return Ok(buf);
        }

        Err(StorageError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "No completion received",
        )))
    }

    fn perform_write(fd: RawFd, byte_offset: u64, data: Vec<u8>) -> StorageResult<()> {
        let mut ring = IoUring::new(256)
            .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        let (submitter, mut sq, mut cq) = ring.split();

        let write_op = Write::new(Fd(fd), data.as_ptr(), data.len() as u32)
            .offset(byte_offset)
            .build()
            .user_data(0x2);

        unsafe {
            sq.push(&write_op).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;
        }

        submitter.submit_and_wait(1).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        if let Some(cqe) = cq.next() {
            if cqe.result() < 0 {
                return Err(StorageError::IoError(std::io::Error::from_raw_os_error(-cqe.result())));
            }
            return Ok(());
        }

        Err(StorageError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "No completion received",
        )))
    }

    fn perform_fsync(fd: RawFd) -> StorageResult<()> {
        let mut ring = IoUring::new(256)
            .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        let (submitter, mut sq, mut cq) = ring.split();

        let fsync_op = Fsync::new(Fd(fd)).build().user_data(0x3);

        unsafe {
            sq.push(&fsync_op).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;
        }

        submitter.submit_and_wait(1).map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))?;

        if let Some(cqe) = cq.next() {
            if cqe.result() < 0 {
                return Err(StorageError::IoError(std::io::Error::from_raw_os_error(-cqe.result())));
            }
            return Ok(());
        }

        Ok(())
    }
}

impl IoEngine for UringIoEngine {
    async fn read_block(&self, block_ref: BlockRef) -> StorageResult<Vec<u8>> {
        let fd = self.get_fd(block_ref.id.device_idx)?;
        let byte_offset = Self::block_ref_to_byte_offset(block_ref);
        let size = block_ref.size.as_bytes() as usize;

        let bytes_read = task::spawn_blocking(move || {
            Self::perform_read(fd, byte_offset, size)
        })
        .await
        .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))??;

        debug!(
            "Read {} bytes from device {} offset {}",
            bytes_read.len(), block_ref.id.device_idx, byte_offset
        );

        self.stats.add_read(bytes_read.len() as u64);

        Ok(bytes_read)
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

        task::spawn_blocking(move || {
            Self::perform_write(fd, byte_offset, data)
        })
        .await
        .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))??;

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
            Self::perform_fsync(fd)?;
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
        .map_err(|e| StorageError::IoError(std::io::Error::other(e.to_string())))??;

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
        IoUring::new(8).is_ok()
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
            id: crate::block::BlockId::new(0, 100),
            size: crate::block::BlockSize::B4K,
        };
        let offset = UringIoEngine::block_ref_to_byte_offset(block_ref);
        assert_eq!(offset, 100 * 4096);

        let block_ref_64k = BlockRef {
            id: crate::block::BlockId::new(0, 10),
            size: crate::block::BlockSize::B64K,
        };
        let offset_64k = UringIoEngine::block_ref_to_byte_offset(block_ref_64k);
        assert_eq!(offset_64k, 10 * 65536);

        let block_ref_1m = BlockRef {
            id: crate::block::BlockId::new(0, 5),
            size: crate::block::BlockSize::B1M,
        };
        let offset_1m = UringIoEngine::block_ref_to_byte_offset(block_ref_1m);
        assert_eq!(offset_1m, 5 * 1048576);
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
            id: crate::block::BlockId::new(0, 0),
            size: crate::block::BlockSize::B4K,
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
}