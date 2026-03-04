//! Zero-copy splice queue for disk-to-network data transfer.
//!
//! A FIFO queue tracking zero-copy splice operations for disk-to-network data transfer
//! (D3 write path: read from NVMe → splice to network socket). Each entry represents
//! a pending or in-progress splice operation with source (NVMe block) and destination
//! (network connection) metadata.
//!
//! This is a pure state-tracking module — actual splice(2) calls happen in A1/A5
//! which use io_uring. This module tracks what's queued and provides backpressure.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Identifies the source of splice data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceSource {
    /// Device/file descriptor identifier (opaque u64, maps to NVMe block device fd).
    pub fd_id: u64,
    /// Byte offset in the source.
    pub offset: u64,
    /// Byte length to splice.
    pub length: u32,
}

/// Identifies the destination of splice data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceDestination {
    /// Connection identifier (opaque u64).
    pub conn_id: u64,
    /// Optional offset (for RDMA writes; 0 for TCP streaming).
    pub offset: u64,
}

/// State of a splice operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpliceState {
    /// Queued, not yet submitted to io_uring.
    Pending = 0,
    /// Submitted to io_uring, waiting for completion.
    InFlight = 1,
    /// Completed successfully.
    Done = 2,
    /// Failed.
    Failed = 3,
}

/// A single splice queue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceEntry {
    /// Unique identifier for this entry.
    pub id: u64,
    /// Source of splice data.
    pub source: SpliceSource,
    /// Destination of splice data.
    pub dest: SpliceDestination,
    /// Current state of the splice operation.
    pub state: SpliceState,
    /// Timestamp when the entry was queued (ms since epoch).
    pub queued_at_ms: u64,
    /// Timestamp when the entry was submitted to io_uring.
    pub submitted_at_ms: Option<u64>,
    /// Timestamp when the entry completed.
    pub completed_at_ms: Option<u64>,
}

/// Configuration for the splice queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceQueueConfig {
    /// Maximum entries in the queue (pending + in-flight).
    pub max_entries: usize,
    /// Maximum concurrent in-flight splice operations.
    pub max_inflight: usize,
    /// Timeout for an in-flight splice in milliseconds.
    pub inflight_timeout_ms: u64,
}

impl Default for SpliceQueueConfig {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            max_inflight: 64,
            inflight_timeout_ms: 5000,
        }
    }
}

/// Error type for splice queue operations.
#[derive(Debug, Error)]
pub enum SpliceQueueError {
    /// Queue is full.
    #[error("splice queue full (max {0})")]
    QueueFull(usize),
    /// Maximum in-flight limit reached.
    #[error("max inflight reached (max {0})")]
    MaxInflight(usize),
    /// Entry not found.
    #[error("splice entry {0} not found")]
    NotFound(u64),
}

/// Zero-copy splice operation queue.
pub struct SpliceQueue {
    config: SpliceQueueConfig,
    next_id: AtomicU64,
    pending: Mutex<VecDeque<SpliceEntry>>,
    inflight: Mutex<HashMap<u64, SpliceEntry>>,
    stats: Arc<SpliceQueueStats>,
}

impl SpliceQueue {
    /// Creates a new splice queue with the given configuration.
    pub fn new(config: SpliceQueueConfig) -> Self {
        Self {
            config,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(VecDeque::new()),
            inflight: Mutex::new(HashMap::new()),
            stats: Arc::new(SpliceQueueStats::new()),
        }
    }

    /// Enqueue a new splice operation. Returns id, or error if queue full.
    pub fn enqueue(
        &self,
        source: SpliceSource,
        dest: SpliceDestination,
        now_ms: u64,
    ) -> Result<u64, SpliceQueueError> {
        let mut pending = self.pending.lock().unwrap();
        let inflight = self.inflight.lock().unwrap();
        let total = pending.len() + inflight.len();

        if total >= self.config.max_entries {
            return Err(SpliceQueueError::QueueFull(self.config.max_entries));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = SpliceEntry {
            id,
            source,
            dest,
            state: SpliceState::Pending,
            queued_at_ms: now_ms,
            submitted_at_ms: None,
            completed_at_ms: None,
        };

        pending.push_back(entry);
        self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(id)
    }

    /// Dequeue the next pending entry for submission to io_uring.
    /// Returns None if no pending entries, or if max_inflight is reached.
    pub fn dequeue_for_submit(&self, now_ms: u64) -> Result<Option<SpliceEntry>, SpliceQueueError> {
        let mut pending = self.pending.lock().unwrap();
        let mut inflight = self.inflight.lock().unwrap();

        if inflight.len() >= self.config.max_inflight {
            return Ok(None);
        }

        if let Some(mut entry) = pending.pop_front() {
            entry.state = SpliceState::InFlight;
            entry.submitted_at_ms = Some(now_ms);
            inflight.insert(entry.id, entry.clone());
            self.stats.submitted.fetch_add(1, Ordering::Relaxed);
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    /// Record completion of an in-flight splice.
    /// Returns the completed entry, or error if not found.
    pub fn complete(
        &self,
        id: u64,
        success: bool,
        now_ms: u64,
    ) -> Result<SpliceEntry, SpliceQueueError> {
        let mut inflight = self.inflight.lock().unwrap();

        let mut entry = inflight.remove(&id).ok_or(SpliceQueueError::NotFound(id))?;
        entry.state = if success {
            SpliceState::Done
        } else {
            SpliceState::Failed
        };
        entry.completed_at_ms = Some(now_ms);

        if success {
            self.stats.completed.fetch_add(1, Ordering::Relaxed);
            self.stats
                .total_bytes_spliced
                .fetch_add(entry.source.length as u64, Ordering::Relaxed);
        } else {
            self.stats.failed.fetch_add(1, Ordering::Relaxed);
        }

        Ok(entry)
    }

    /// Check for timed-out in-flight splices. Returns their IDs.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64> {
        let inflight = self.inflight.lock().unwrap();
        let mut timed_out = Vec::new();

        for (id, entry) in inflight.iter() {
            if let Some(submitted_at) = entry.submitted_at_ms {
                if now_ms.saturating_sub(submitted_at) > self.config.inflight_timeout_ms {
                    timed_out.push(*id);
                }
            }
        }

        drop(inflight);

        for _id in &timed_out {
            self.stats.timed_out.fetch_add(1, Ordering::Relaxed);
        }

        timed_out
    }

    /// Number of pending entries.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Number of in-flight entries.
    pub fn inflight_count(&self) -> usize {
        self.inflight.lock().unwrap().len()
    }

    /// Total entries (pending + in-flight).
    pub fn total_count(&self) -> usize {
        let pending = self.pending.lock().unwrap();
        let inflight = self.inflight.lock().unwrap();
        pending.len() + inflight.len()
    }

    /// Returns the stats for this queue.
    pub fn stats(&self) -> Arc<SpliceQueueStats> {
        Arc::clone(&self.stats)
    }
}

impl Default for SpliceQueue {
    fn default() -> Self {
        Self::new(SpliceQueueConfig::default())
    }
}

/// Statistics for the splice queue.
pub struct SpliceQueueStats {
    /// Number of entries enqueued.
    pub enqueued: AtomicU64,
    /// Number of entries submitted.
    pub submitted: AtomicU64,
    /// Number of entries completed successfully.
    pub completed: AtomicU64,
    /// Number of entries that failed.
    pub failed: AtomicU64,
    /// Number of entries that timed out.
    pub timed_out: AtomicU64,
    /// Total bytes spliced.
    pub total_bytes_spliced: AtomicU64,
}

impl SpliceQueueStats {
    /// Creates new empty statistics.
    pub fn new() -> Self {
        Self {
            enqueued: AtomicU64::new(0),
            submitted: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            timed_out: AtomicU64::new(0),
            total_bytes_spliced: AtomicU64::new(0),
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn snapshot(&self, pending: usize, inflight: usize) -> SpliceQueueStatsSnapshot {
        SpliceQueueStatsSnapshot {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            submitted: self.submitted.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            timed_out: self.timed_out.load(Ordering::Relaxed),
            total_bytes_spliced: self.total_bytes_spliced.load(Ordering::Relaxed),
            pending_count: pending,
            inflight_count: inflight,
        }
    }
}

impl Default for SpliceQueueStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of splice queue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpliceQueueStatsSnapshot {
    /// Number of entries enqueued.
    pub enqueued: u64,
    /// Number of entries submitted.
    pub submitted: u64,
    /// Number of entries completed successfully.
    pub completed: u64,
    /// Number of entries that failed.
    pub failed: u64,
    /// Number of entries that timed out.
    pub timed_out: u64,
    /// Total bytes spliced.
    pub total_bytes_spliced: u64,
    /// Number of pending entries.
    pub pending_count: usize,
    /// Number of in-flight entries.
    pub inflight_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_success() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        let id = queue.enqueue(source, dest, 1000).unwrap();
        assert!(id > 0);
        assert_eq!(queue.pending_count(), 1);
    }

    #[test]
    fn test_enqueue_queue_full() {
        let config = SpliceQueueConfig {
            max_entries: 2,
            max_inflight: 1,
            inflight_timeout_ms: 5000,
        };
        let queue = SpliceQueue::new(config);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();

        let result = queue.enqueue(source, dest, 1000);
        assert!(matches!(result, Err(SpliceQueueError::QueueFull(2))));
    }

    #[test]
    fn test_dequeue_for_submit_basic() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source, dest, 1000).unwrap();
        assert_eq!(queue.pending_count(), 1);
        assert_eq!(queue.inflight_count(), 0);

        let entry = queue.dequeue_for_submit(1100).unwrap().unwrap();
        assert_eq!(entry.state, SpliceState::InFlight);
        assert_eq!(entry.submitted_at_ms, Some(1100));
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.inflight_count(), 1);
    }

    #[test]
    fn test_dequeue_for_submit_max_inflight() {
        let config = SpliceQueueConfig {
            max_entries: 10,
            max_inflight: 2,
            inflight_timeout_ms: 5000,
        };
        let queue = SpliceQueue::new(config);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        for _ in 0..5 {
            queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        }

        queue.dequeue_for_submit(1100).unwrap();
        queue.dequeue_for_submit(1100).unwrap();
        assert_eq!(queue.inflight_count(), 2);

        let result = queue.dequeue_for_submit(1100).unwrap();
        assert!(result.is_none());
        assert_eq!(queue.inflight_count(), 2);
    }

    #[test]
    fn test_dequeue_empty_queue() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let result = queue.dequeue_for_submit(1000).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_complete_success() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source, dest, 1000).unwrap();
        let entry = queue.dequeue_for_submit(1100).unwrap().unwrap();
        assert_eq!(queue.inflight_count(), 1);

        let completed = queue.complete(entry.id, true, 1200).unwrap();
        assert_eq!(completed.state, SpliceState::Done);
        assert_eq!(completed.completed_at_ms, Some(1200));
        assert_eq!(queue.inflight_count(), 0);
    }

    #[test]
    fn test_complete_not_found() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let result = queue.complete(999, true, 1000);
        assert!(matches!(result, Err(SpliceQueueError::NotFound(999))));
    }

    #[test]
    fn test_complete_failure() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source, dest, 1000).unwrap();
        let entry = queue.dequeue_for_submit(1100).unwrap().unwrap();

        let completed = queue.complete(entry.id, false, 1200).unwrap();
        assert_eq!(completed.state, SpliceState::Failed);
    }

    #[test]
    fn test_check_timeouts() {
        let config = SpliceQueueConfig {
            max_entries: 10,
            max_inflight: 10,
            inflight_timeout_ms: 1000,
        };
        let queue = SpliceQueue::new(config);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();

        let e1 = queue.dequeue_for_submit(1000).unwrap().unwrap();
        queue.dequeue_for_submit(2500).unwrap();

        let timed_out = queue.check_timeouts(3000);
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], e1.id);
    }

    #[test]
    fn test_check_timeouts_fresh() {
        let config = SpliceQueueConfig {
            max_entries: 10,
            max_inflight: 10,
            inflight_timeout_ms: 5000,
        };
        let queue = SpliceQueue::new(config);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source, dest, 1000).unwrap();
        queue.dequeue_for_submit(1000).unwrap();

        let timed_out = queue.check_timeouts(2000);
        assert!(timed_out.is_empty());
    }

    #[test]
    fn test_pending_count() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        assert_eq!(queue.pending_count(), 0);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.enqueue(source, dest, 1000).unwrap();
        assert_eq!(queue.pending_count(), 2);
    }

    #[test]
    fn test_inflight_count() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        assert_eq!(queue.inflight_count(), 0);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.dequeue_for_submit(1100).unwrap();

        assert_eq!(queue.inflight_count(), 1);
    }

    #[test]
    fn test_total_count() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        assert_eq!(queue.total_count(), 0);

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 1024,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.enqueue(source.clone(), dest.clone(), 1000).unwrap();
        queue.dequeue_for_submit(1100).unwrap();

        assert_eq!(queue.total_count(), 2);
    }

    #[test]
    fn test_stats_bytes_spliced() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());
        let stats = queue.stats();

        let source = SpliceSource {
            fd_id: 1,
            offset: 0,
            length: 2048,
        };
        let dest = SpliceDestination {
            conn_id: 1,
            offset: 0,
        };

        queue.enqueue(source, dest, 1000).unwrap();
        let entry = queue.dequeue_for_submit(1100).unwrap().unwrap();
        queue.complete(entry.id, true, 1200).unwrap();

        assert_eq!(stats.total_bytes_spliced.load(Ordering::Relaxed), 2048);
    }

    #[test]
    fn test_multiple_enqueue_dequeue() {
        let queue = SpliceQueue::new(SpliceQueueConfig::default());

        for i in 0..5 {
            let source = SpliceSource {
                fd_id: i,
                offset: i * 1024,
                length: 1024,
            };
            let dest = SpliceDestination {
                conn_id: i,
                offset: 0,
            };
            queue.enqueue(source, dest, 1000 + i).unwrap();
        }

        assert_eq!(queue.pending_count(), 5);

        for _ in 0..3 {
            queue.dequeue_for_submit(2000).unwrap();
        }

        assert_eq!(queue.pending_count(), 2);
        assert_eq!(queue.inflight_count(), 3);
    }

    #[test]
    fn test_config_default() {
        let config = SpliceQueueConfig::default();
        assert_eq!(config.max_entries, 1024);
        assert_eq!(config.max_inflight, 64);
        assert_eq!(config.inflight_timeout_ms, 5000);
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = SpliceQueueStats::new();
        stats.enqueued.store(10, Ordering::Relaxed);
        stats.completed.store(5, Ordering::Relaxed);

        let snapshot = stats.snapshot(3, 2);
        assert_eq!(snapshot.enqueued, 10);
        assert_eq!(snapshot.completed, 5);
        assert_eq!(snapshot.pending_count, 3);
        assert_eq!(snapshot.inflight_count, 2);
    }

    #[test]
    fn test_error_display() {
        let err = SpliceQueueError::QueueFull(100);
        assert!(err.to_string().contains("queue full"));

        let err = SpliceQueueError::MaxInflight(64);
        assert!(err.to_string().contains("max inflight"));

        let err = SpliceQueueError::NotFound(42);
        assert!(err.to_string().contains("not found"));
    }
}
