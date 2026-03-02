//! NFSv4.2 Server-Side Copy

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Write stability level for NFS operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteStable {
    /// Unstable write (may be buffered)
    Unstable,
    /// Data sync write (synchronous to stable storage)
    DataSync,
    /// File sync write (synchronous metadata and data)
    FileSync,
}

/// State of an asynchronous copy operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyState {
    /// Copy is in progress
    InProgress,
    /// Copy completed successfully
    Completed,
    /// Copy failed
    Failed,
    /// Copy was cancelled
    Cancelled,
}

/// A copy segment: offset + length in source and destination
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopySegment {
    /// Source file offset in bytes
    pub src_offset: u64,
    /// Destination file offset in bytes
    pub dst_offset: u64,
    /// Bytes to copy; 0 = to end of file
    pub count: u64,
}

impl CopySegment {
    /// Creates a new copy segment
    pub fn new(src_offset: u64, dst_offset: u64, count: u64) -> Self {
        Self {
            src_offset,
            dst_offset,
            count,
        }
    }

    /// Validates the segment
    pub fn is_valid(&self) -> bool {
        self.count > 0 || self.count == 0 // 0 means to end of file
    }
}

/// Result from a synchronous copy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyResult {
    /// Number of bytes written
    pub bytes_written: u64,
    /// Whether the copy was performed consecutively (NFS4_CP_CONSECUTIVE)
    pub consecutive: bool,
    /// Write stability level used
    pub stable: WriteStable,
}

impl CopyResult {
    /// Creates a new copy result
    pub fn new(bytes_written: u64, consecutive: bool, stable: WriteStable) -> Self {
        Self {
            bytes_written,
            consecutive,
            stable,
        }
    }
}

/// Represents an in-flight async copy operation (CB_OFFLOAD callback)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncCopyHandle {
    /// Unique copy operation identifier
    pub copy_id: u64,
    /// Source file path
    pub src_file: String,
    /// Destination file path
    pub dst_file: String,
    /// Copy segments (for sparse file support)
    pub segments: Vec<CopySegment>,
    /// Current state of the copy
    pub state: CopyState,
    /// Bytes copied so far
    pub bytes_copied: u64,
    /// Total bytes to copy
    pub total_bytes: u64,
    /// When the copy was started
    pub created_at: std::time::SystemTime,
}

impl AsyncCopyHandle {
    /// Creates a new async copy handle
    pub fn new(
        copy_id: u64,
        src_file: String,
        dst_file: String,
        segments: Vec<CopySegment>,
        total_bytes: u64,
    ) -> Self {
        Self {
            copy_id,
            src_file,
            dst_file,
            segments,
            state: CopyState::InProgress,
            bytes_copied: 0,
            total_bytes,
            created_at: std::time::SystemTime::now(),
        }
    }

    /// Returns progress as a percentage
    pub fn progress_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            100.0
        } else {
            (self.bytes_copied as f64 / self.total_bytes as f64) * 100.0
        }
    }
}

/// Manager for server-side copy operations
#[derive(Debug)]
pub struct CopyOffloadManager {
    handles: HashMap<u64, AsyncCopyHandle>,
    next_id: u64,
    max_concurrent: usize,
}

impl CopyOffloadManager {
    /// Creates a new copy offload manager
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            handles: HashMap::new(),
            next_id: 1,
            max_concurrent,
        }
    }

    /// Register a new async copy; returns copy_id
    pub fn start_copy(
        &mut self,
        src: &str,
        dst: &str,
        segments: Vec<CopySegment>,
    ) -> Result<u64, CopyOffloadError> {
        if self.active_count() >= self.max_concurrent {
            return Err(CopyOffloadError::LimitExceeded(
                "max concurrent copies reached".into(),
            ));
        }

        // Validate segments
        for seg in &segments {
            if !seg.is_valid() {
                return Err(CopyOffloadError::InvalidSegment(
                    "segment count must be > 0".into(),
                ));
            }
        }

        let copy_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let total_bytes: u64 = segments
            .iter()
            .map(|s| if s.count == 0 { 0 } else { s.count })
            .sum();

        let handle = AsyncCopyHandle::new(
            copy_id,
            src.to_string(),
            dst.to_string(),
            segments,
            total_bytes,
        );
        debug!("Started async copy {}: {} -> {}", copy_id, src, dst);

        self.handles.insert(copy_id, handle);
        Ok(copy_id)
    }

    /// Poll state of a copy operation
    pub fn poll_copy(&self, copy_id: u64) -> Option<&AsyncCopyHandle> {
        self.handles.get(&copy_id)
    }

    /// Cancel an in-progress copy
    pub fn cancel_copy(&mut self, copy_id: u64) -> bool {
        if let Some(handle) = self.handles.get_mut(&copy_id) {
            if handle.state == CopyState::InProgress {
                handle.state = CopyState::Cancelled;
                debug!("Cancelled copy {}", copy_id);
                return true;
            }
        }
        false
    }

    /// Mark a copy as completed (called by the actual copy executor)
    pub fn complete_copy(
        &mut self,
        copy_id: u64,
        bytes_copied: u64,
    ) -> Result<(), CopyOffloadError> {
        let handle = self
            .handles
            .get_mut(&copy_id)
            .ok_or(CopyOffloadError::NotFound)?;

        if handle.state != CopyState::InProgress {
            return Err(CopyOffloadError::AlreadyComplete(
                "copy is not in progress".into(),
            ));
        }

        handle.bytes_copied = bytes_copied;
        handle.state = CopyState::Completed;
        info!("Copy {} completed: {} bytes", copy_id, bytes_copied);
        Ok(())
    }

    /// Mark a copy as failed
    pub fn fail_copy(&mut self, copy_id: u64) -> Result<(), CopyOffloadError> {
        let handle = self
            .handles
            .get_mut(&copy_id)
            .ok_or(CopyOffloadError::NotFound)?;

        if handle.state != CopyState::InProgress {
            return Err(CopyOffloadError::AlreadyComplete(
                "copy is not in progress".into(),
            ));
        }

        handle.state = CopyState::Failed;
        warn!("Copy {} failed", copy_id);
        Ok(())
    }

    /// Active copy count
    pub fn active_count(&self) -> usize {
        self.handles
            .values()
            .filter(|h| h.state == CopyState::InProgress)
            .count()
    }

    /// Remove completed/failed/cancelled handles (cleanup)
    pub fn purge_finished(&mut self) -> usize {
        let before = self.handles.len();
        self.handles
            .retain(|_, h| matches!(h.state, CopyState::InProgress));
        let removed = before - self.handles.len();
        if removed > 0 {
            debug!("Purged {} finished copy handles", removed);
        }
        removed
    }

    /// Total number of handles
    pub fn total_handles(&self) -> usize {
        self.handles.len()
    }
}

/// CLONE operation: block-level CoW clone (like ioctl FICLONE)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    /// Source file path
    pub src_file: String,
    /// Destination file path
    pub dst_file: String,
    /// Source offset for partial clone
    pub src_offset: u64,
    /// Destination offset for partial clone
    pub dst_offset: u64,
    /// Length to clone; 0 = entire file
    pub length: u64,
}

impl CloneRequest {
    /// Creates a new clone request
    pub fn new(src_file: String, dst_file: String) -> Self {
        Self {
            src_file,
            dst_file,
            src_offset: 0,
            dst_offset: 0,
            length: 0,
        }
    }

    /// Sets the source offset
    pub fn with_src_offset(mut self, offset: u64) -> Self {
        self.src_offset = offset;
        self
    }

    /// Sets the destination offset
    pub fn with_dst_offset(mut self, offset: u64) -> Self {
        self.dst_offset = offset;
        self
    }

    /// Sets the length
    pub fn with_length(mut self, length: u64) -> Self {
        self.length = length;
        self
    }
}

/// Result from a CLONE operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneResult {
    /// Source file path
    pub src_file: String,
    /// Destination file path
    pub dst_file: String,
    /// Number of bytes cloned
    pub cloned_bytes: u64,
}

impl CloneResult {
    /// Creates a new clone result
    pub fn new(src_file: String, dst_file: String, cloned_bytes: u64) -> Self {
        Self {
            src_file,
            dst_file,
            cloned_bytes,
        }
    }
}

/// Copy offload errors
#[derive(Debug, Error)]
pub enum CopyOffloadError {
    #[error("Copy not found: {0}")]
    NotFound(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Copy already complete: {0}")]
    AlreadyComplete(String),

    #[error("Invalid segment: {0}")]
    InvalidSegment(String),

    #[error("IO error: {0}")]
    IoError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let segments = vec![CopySegment::new(0, 0, 4096)];

        let id = manager
            .start_copy("/src/file", "/dst/file", segments.clone())
            .unwrap();
        assert_eq!(id, 1);

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.src_file, "/src/file");
        assert_eq!(handle.dst_file, "/dst/file");
        assert_eq!(handle.state, CopyState::InProgress);
    }

    #[test]
    fn test_poll_copy_not_found() {
        let manager = CopyOffloadManager::new(5);
        assert!(manager.poll_copy(999).is_none());
    }

    #[test]
    fn test_cancel_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        assert!(manager.cancel_copy(id));

        let handle = manager.poll_copy(id).unwrap();
        assert!(matches!(handle.state, CopyState::Cancelled));
    }

    #[test]
    fn test_complete_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        manager.complete_copy(id, 1000).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert!(matches!(handle.state, CopyState::Completed));
        assert_eq!(handle.bytes_copied, 1000);
    }

    #[test]
    fn test_fail_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        manager.fail_copy(id).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert!(matches!(handle.state, CopyState::Failed));
    }

    #[test]
    fn test_purge_finished() {
        let mut manager = CopyOffloadManager::new(5);

        let id1 = manager
            .start_copy("/src1", "/dst1", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        let id2 = manager
            .start_copy("/src2", "/dst2", vec![CopySegment::new(0, 0, 2000)])
            .unwrap();
        let id3 = manager
            .start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 3000)])
            .unwrap();

        manager.complete_copy(id1, 1000).unwrap();
        manager.fail_copy(id2).unwrap();
        manager.cancel_copy(id3).unwrap();

        assert_eq!(manager.active_count(), 1); // id3 still in progress

        let purged = manager.purge_finished();
        assert_eq!(purged, 3);

        assert!(manager.poll_copy(id1).is_none());
        assert!(manager.poll_copy(id2).is_none());
        assert!(manager.poll_copy(id3).is_none());
    }

    #[test]
    fn test_max_concurrent_limit() {
        let mut manager = CopyOffloadManager::new(2);

        let id1 = manager
            .start_copy("/src1", "/dst1", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        let id2 = manager
            .start_copy("/src2", "/dst2", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        let result = manager.start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 1000)]);
        assert!(result.is_err());

        manager.complete_copy(id1, 1000).unwrap();

        let id3 = manager
            .start_copy("/src3", "/dst3", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();
        assert!(manager.poll_copy(id3).is_some());
    }

    #[test]
    fn test_segment_validation() {
        let mut manager = CopyOffloadManager::new(5);

        // count = 0 is valid (means to end of file in some contexts)
        let result = manager.start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 0)]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_clone_request_builder() {
        let req = CloneRequest::new("/src/file".to_string(), "/dst/file".to_string())
            .with_src_offset(4096)
            .with_dst_offset(8192)
            .with_length(4096);

        assert_eq!(req.src_file, "/src/file");
        assert_eq!(req.dst_file, "/dst/file");
        assert_eq!(req.src_offset, 4096);
        assert_eq!(req.dst_offset, 8192);
        assert_eq!(req.length, 4096);
    }

    #[test]
    fn test_clone_request_default() {
        let req = CloneRequest::new("src".to_string(), "dst".to_string());

        assert_eq!(req.src_offset, 0);
        assert_eq!(req.dst_offset, 0);
        assert_eq!(req.length, 0);
    }

    #[test]
    fn test_copy_result_builder() {
        let result = CopyResult::new(4096, true, WriteStable::FileSync);

        assert_eq!(result.bytes_written, 4096);
        assert!(result.consecutive);
        assert!(matches!(result.stable, WriteStable::FileSync));
    }

    #[test]
    fn test_async_copy_handle_progress() {
        let handle = AsyncCopyHandle::new(1, "src".to_string(), "dst".to_string(), vec![], 1000);
        assert_eq!(handle.progress_percent(), 0.0);

        let mut handle = handle;
        handle.bytes_copied = 500;
        assert_eq!(handle.progress_percent(), 50.0);

        handle.bytes_copied = 1000;
        assert_eq!(handle.progress_percent(), 100.0);
    }

    #[test]
    fn test_zero_total_bytes_progress() {
        let handle = AsyncCopyHandle::new(1, "src".to_string(), "dst".to_string(), vec![], 0);
        assert_eq!(handle.progress_percent(), 100.0);
    }

    #[test]
    fn test_multiple_segments() {
        let segments = vec![
            CopySegment::new(0, 0, 4096),
            CopySegment::new(4096, 4096, 4096),
            CopySegment::new(8192, 8192, 4096),
        ];

        let mut manager = CopyOffloadManager::new(5);
        let id = manager.start_copy("/src", "/dst", segments).unwrap();

        let handle = manager.poll_copy(id).unwrap();
        assert_eq!(handle.segments.len(), 3);
    }

    #[test]
    fn test_complete_non_existent_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let result = manager.complete_copy(999, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_fail_non_existent_copy() {
        let mut manager = CopyOffloadManager::new(5);
        let result = manager.fail_copy(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_already_complete_cannot_fail() {
        let mut manager = CopyOffloadManager::new(5);
        let id = manager
            .start_copy("/src", "/dst", vec![CopySegment::new(0, 0, 1000)])
            .unwrap();

        manager.complete_copy(id, 1000).unwrap();

        let result = manager.fail_copy(id);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_stable_values() {
        assert!(matches!(WriteStable::Unstable, WriteStable::Unstable));
        assert!(matches!(WriteStable::DataSync, WriteStable::DataSync));
        assert!(matches!(WriteStable::FileSync, WriteStable::FileSync));
    }

    #[test]
    fn test_copy_state_values() {
        assert!(matches!(CopyState::InProgress, CopyState::InProgress));
        assert!(matches!(CopyState::Completed, CopyState::Completed));
        assert!(matches!(CopyState::Failed, CopyState::Failed));
        assert!(matches!(CopyState::Cancelled, CopyState::Cancelled));
    }
}
