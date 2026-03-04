//! Write pipeline stage tracker for ClaudeFS distributed filesystem.
//!
//! Tracks a write request as it moves through the ClaudeFS write pipeline stages:
//! 1. Client receive → journal write (local NVMe)
//! 2. Journal replicate (2x sync to peers)
//! 3. Segment pack (accumulate to 2MB)
//! 4. EC distribute (4+2 stripes to 6 nodes)
//! 5. S3 async upload (cache mode, D5)
//!
//! Used by A1 (storage) to track write state, by A5 (FUSE) to know when to ack
//! to the client, and by A8 (monitoring) for write latency breakdown.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Unique ID for a write pipeline operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WriteId(pub u64);

/// Stage in the write pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WriteStage {
    /// Write request received, pending journal write.
    Received = 0,
    /// Written to local NVMe journal.
    JournalWritten = 1,
    /// Replicated to 2 peers (D3 — client ack point).
    JournalReplicated = 2,
    /// Packed into a 2MB segment.
    SegmentPacked = 3,
    /// EC stripes distributed to 6 nodes.
    EcDistributed = 4,
    /// Uploaded to S3 (cache mode, D5).
    S3Uploaded = 5,
    /// Write fully complete.
    Complete = 6,
}

impl WriteStage {
    /// The stage at which the client receives the ack (after journal replicated, per D3).
    pub fn client_ack_stage() -> Self {
        WriteStage::JournalReplicated
    }

    /// Whether the client has been acked at this stage.
    pub fn is_client_acked(&self) -> bool {
        self >= &WriteStage::JournalReplicated
    }
}

/// A write pipeline operation tracking one client write.
#[derive(Debug)]
pub struct WritePipelineOp {
    /// Unique identifier for this operation.
    pub id: WriteId,
    /// Current stage in the pipeline.
    pub stage: WriteStage,
    /// Size of the write in bytes.
    pub size_bytes: u32,
    /// Timestamp when the write was created (ms since epoch).
    pub created_at_ms: u64,
    /// Timestamp when each stage was reached (None if not yet reached).
    pub stage_timestamps: [Option<u64>; 7],
}

impl WritePipelineOp {
    pub fn new(id: WriteId, size_bytes: u32, now_ms: u64) -> Self {
        let mut timestamps = [None; 7];
        timestamps[WriteStage::Received as usize] = Some(now_ms);
        Self {
            id,
            stage: WriteStage::Received,
            size_bytes,
            created_at_ms: now_ms,
            stage_timestamps: timestamps,
        }
    }

    pub fn advance(&mut self, stage: WriteStage, now_ms: u64) -> Result<(), WriteError> {
        if self.stage == WriteStage::Complete {
            return Err(WriteError::AlreadyComplete(self.id));
        }

        if stage as usize <= self.stage as usize {
            return Err(WriteError::InvalidTransition {
                from: self.stage,
                to: stage,
            });
        }

        self.stage = stage;
        self.stage_timestamps[stage as usize] = Some(now_ms);
        Ok(())
    }

    pub fn latency_to_stage_ms(&self, stage: WriteStage) -> Option<u64> {
        self.stage_timestamps[stage as usize].map(|ts| ts.saturating_sub(self.created_at_ms))
    }

    pub fn is_client_acked(&self) -> bool {
        self.stage.is_client_acked()
    }
}

/// Error for write pipeline operations.
#[derive(Debug, Error)]
pub enum WriteError {
    /// Write not found in the manager.
    #[error("write {0:?} not found")]
    NotFound(WriteId),
    /// Invalid stage transition attempted.
    #[error("invalid stage transition from {from:?} to {to:?}")]
    InvalidTransition { from: WriteStage, to: WriteStage },
    /// Write already completed.
    #[error("write {0:?} already complete")]
    AlreadyComplete(WriteId),
}

/// Manager for concurrent write pipeline operations.
pub struct WritePipelineManager {
    next_id: AtomicU64,
    ops: Mutex<HashMap<WriteId, WritePipelineOp>>,
    stats: Arc<WritePipelineStats>,
}

impl WritePipelineManager {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            ops: Mutex::new(HashMap::new()),
            stats: Arc::new(WritePipelineStats::new()),
        }
    }

    pub fn start(&self, size_bytes: u32, now_ms: u64) -> WriteId {
        let id = WriteId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let op = WritePipelineOp::new(id, size_bytes, now_ms);
        self.ops.lock().unwrap().insert(id, op);
        self.stats.writes_started.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_bytes_written
            .fetch_add(size_bytes as u64, Ordering::Relaxed);
        id
    }

    pub fn advance(
        &self,
        id: WriteId,
        stage: WriteStage,
        now_ms: u64,
    ) -> Result<WriteStage, WriteError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(WriteError::NotFound(id))?;

        let old_stage = op.stage;
        op.advance(stage, now_ms)?;

        if old_stage < WriteStage::JournalReplicated && stage >= WriteStage::JournalReplicated {
            self.stats
                .client_acks_issued
                .fetch_add(1, Ordering::Relaxed);
        }
        if stage == WriteStage::EcDistributed {
            self.stats
                .ec_distributions_completed
                .fetch_add(1, Ordering::Relaxed);
        }
        if stage == WriteStage::S3Uploaded {
            self.stats
                .s3_uploads_completed
                .fetch_add(1, Ordering::Relaxed);
        }

        Ok(stage)
    }

    pub fn complete(&self, id: WriteId, now_ms: u64) -> Result<(), WriteError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(WriteError::NotFound(id))?;
        op.advance(WriteStage::Complete, now_ms)?;
        ops.remove(&id);
        self.stats.writes_completed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn stage(&self, id: WriteId) -> Option<WriteStage> {
        self.ops.lock().unwrap().get(&id).map(|op| op.stage)
    }

    pub fn active_count(&self) -> usize {
        self.ops.lock().unwrap().len()
    }

    pub fn pending_background_count(&self) -> usize {
        self.ops
            .lock()
            .unwrap()
            .values()
            .filter(|op| {
                op.stage >= WriteStage::JournalReplicated && op.stage < WriteStage::EcDistributed
            })
            .count()
    }

    pub fn stats(&self) -> Arc<WritePipelineStats> {
        Arc::clone(&self.stats)
    }
}

impl Default for WritePipelineManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WritePipelineStats {
    pub writes_started: AtomicU64,
    pub writes_completed: AtomicU64,
    pub client_acks_issued: AtomicU64,
    pub ec_distributions_completed: AtomicU64,
    pub s3_uploads_completed: AtomicU64,
    pub total_bytes_written: AtomicU64,
}

impl WritePipelineStats {
    pub fn new() -> Self {
        Self {
            writes_started: AtomicU64::new(0),
            writes_completed: AtomicU64::new(0),
            client_acks_issued: AtomicU64::new(0),
            ec_distributions_completed: AtomicU64::new(0),
            s3_uploads_completed: AtomicU64::new(0),
            total_bytes_written: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self, active: usize, pending_bg: usize) -> WritePipelineStatsSnapshot {
        WritePipelineStatsSnapshot {
            writes_started: self.writes_started.load(Ordering::Relaxed),
            writes_completed: self.writes_completed.load(Ordering::Relaxed),
            client_acks_issued: self.client_acks_issued.load(Ordering::Relaxed),
            ec_distributions_completed: self.ec_distributions_completed.load(Ordering::Relaxed),
            s3_uploads_completed: self.s3_uploads_completed.load(Ordering::Relaxed),
            total_bytes_written: self.total_bytes_written.load(Ordering::Relaxed),
            active_writes: active,
            pending_background: pending_bg,
        }
    }
}

impl Default for WritePipelineStats {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritePipelineStatsSnapshot {
    pub writes_started: u64,
    pub writes_completed: u64,
    pub client_acks_issued: u64,
    pub ec_distributions_completed: u64,
    pub s3_uploads_completed: u64,
    pub total_bytes_written: u64,
    pub active_writes: usize,
    pub pending_background: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_op_starts_at_received() {
        let id = WriteId(1);
        let op = WritePipelineOp::new(id, 1024, 1000);
        assert_eq!(op.stage, WriteStage::Received);
        assert_eq!(op.size_bytes, 1024);
        assert_eq!(op.created_at_ms, 1000);
        assert!(op.stage_timestamps[WriteStage::Received as usize].is_some());
    }

    #[test]
    fn test_advance_single_stage() {
        let id = WriteId(1);
        let mut op = WritePipelineOp::new(id, 1024, 1000);
        op.advance(WriteStage::JournalWritten, 1100).unwrap();
        assert_eq!(op.stage, WriteStage::JournalWritten);
        assert_eq!(
            op.stage_timestamps[WriteStage::JournalWritten as usize],
            Some(1100)
        );
    }

    #[test]
    fn test_advance_multiple_stages_in_order() {
        let id = WriteId(1);
        let mut op = WritePipelineOp::new(id, 1024, 1000);
        op.advance(WriteStage::JournalWritten, 1100).unwrap();
        op.advance(WriteStage::JournalReplicated, 1200).unwrap();
        op.advance(WriteStage::SegmentPacked, 1300).unwrap();
        op.advance(WriteStage::EcDistributed, 1400).unwrap();
        op.advance(WriteStage::S3Uploaded, 1500).unwrap();
        op.advance(WriteStage::Complete, 1600).unwrap();
        assert_eq!(op.stage, WriteStage::Complete);
    }

    #[test]
    fn test_client_ack_stage_is_journal_replicated() {
        assert_eq!(
            WriteStage::client_ack_stage(),
            WriteStage::JournalReplicated
        );
    }

    #[test]
    fn test_is_client_acked_false_before_replication() {
        let stage = WriteStage::Received;
        assert!(!stage.is_client_acked());
        let stage = WriteStage::JournalWritten;
        assert!(!stage.is_client_acked());
    }

    #[test]
    fn test_is_client_acked_true_after_replication() {
        let stage = WriteStage::JournalReplicated;
        assert!(stage.is_client_acked());
        let stage = WriteStage::SegmentPacked;
        assert!(stage.is_client_acked());
        let stage = WriteStage::EcDistributed;
        assert!(stage.is_client_acked());
        let stage = WriteStage::S3Uploaded;
        assert!(stage.is_client_acked());
        let stage = WriteStage::Complete;
        assert!(stage.is_client_acked());
    }

    #[test]
    fn test_latency_to_stage() {
        let id = WriteId(1);
        let mut op = WritePipelineOp::new(id, 1024, 1000);
        op.advance(WriteStage::JournalWritten, 1100).unwrap();
        op.advance(WriteStage::JournalReplicated, 1250).unwrap();

        let latency = op
            .latency_to_stage_ms(WriteStage::JournalReplicated)
            .unwrap();
        assert_eq!(latency, 250);
    }

    #[test]
    fn test_latency_to_stage_unreached() {
        let id = WriteId(1);
        let op = WritePipelineOp::new(id, 1024, 1000);
        assert!(op.latency_to_stage_ms(WriteStage::JournalWritten).is_none());
    }

    #[test]
    fn test_advance_invalid_transition() {
        let id = WriteId(1);
        let mut op = WritePipelineOp::new(id, 1024, 1000);
        op.advance(WriteStage::JournalWritten, 1100).unwrap();
        let result = op.advance(WriteStage::Received, 1200);
        assert!(matches!(result, Err(WriteError::InvalidTransition { .. })));
    }

    #[test]
    fn test_advance_already_complete() {
        let id = WriteId(1);
        let mut op = WritePipelineOp::new(id, 1024, 1000);
        op.advance(WriteStage::JournalWritten, 1100).unwrap();
        op.advance(WriteStage::JournalReplicated, 1200).unwrap();
        op.advance(WriteStage::SegmentPacked, 1300).unwrap();
        op.advance(WriteStage::EcDistributed, 1400).unwrap();
        op.advance(WriteStage::S3Uploaded, 1500).unwrap();
        op.advance(WriteStage::Complete, 1600).unwrap();
        let result = op.advance(WriteStage::Complete, 1700);
        assert!(matches!(result, Err(WriteError::AlreadyComplete(_))));
    }

    #[test]
    fn test_manager_start_and_advance() {
        let manager = WritePipelineManager::new();
        let id = manager.start(1024, 1000);
        assert_eq!(manager.stage(id), Some(WriteStage::Received));

        manager
            .advance(id, WriteStage::JournalWritten, 1100)
            .unwrap();
        assert_eq!(manager.stage(id), Some(WriteStage::JournalWritten));
    }

    #[test]
    fn test_manager_complete() {
        let manager = WritePipelineManager::new();
        let id = manager.start(1024, 1000);
        manager
            .advance(id, WriteStage::JournalWritten, 1100)
            .unwrap();
        manager
            .advance(id, WriteStage::JournalReplicated, 1200)
            .unwrap();
        manager
            .advance(id, WriteStage::SegmentPacked, 1300)
            .unwrap();
        manager
            .advance(id, WriteStage::EcDistributed, 1400)
            .unwrap();
        manager.advance(id, WriteStage::S3Uploaded, 1500).unwrap();
        manager.complete(id, 1600).unwrap();
        assert_eq!(manager.stage(id), None);
    }

    #[test]
    fn test_manager_active_count() {
        let manager = WritePipelineManager::new();
        assert_eq!(manager.active_count(), 0);

        let id1 = manager.start(1024, 1000);
        let id2 = manager.start(2048, 1100);
        assert_eq!(manager.active_count(), 2);

        manager
            .advance(id1, WriteStage::JournalWritten, 1200)
            .unwrap();
        manager
            .advance(id1, WriteStage::JournalReplicated, 1300)
            .unwrap();
        manager
            .advance(id1, WriteStage::SegmentPacked, 1400)
            .unwrap();
        manager
            .advance(id1, WriteStage::EcDistributed, 1500)
            .unwrap();
        manager.advance(id1, WriteStage::S3Uploaded, 1600).unwrap();
        manager.complete(id1, 1700).unwrap();
        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn test_manager_pending_background_count() {
        let manager = WritePipelineManager::new();
        let id1 = manager.start(1024, 1000);
        let id2 = manager.start(2048, 1100);

        assert_eq!(manager.pending_background_count(), 0);

        manager
            .advance(id1, WriteStage::JournalWritten, 1200)
            .unwrap();
        manager
            .advance(id1, WriteStage::JournalReplicated, 1300)
            .unwrap();
        assert_eq!(manager.pending_background_count(), 1);

        manager
            .advance(id2, WriteStage::JournalWritten, 1250)
            .unwrap();
        manager
            .advance(id2, WriteStage::JournalReplicated, 1350)
            .unwrap();
        assert_eq!(manager.pending_background_count(), 2);

        manager
            .advance(id1, WriteStage::SegmentPacked, 1400)
            .unwrap();
        manager
            .advance(id1, WriteStage::EcDistributed, 1500)
            .unwrap();
        assert_eq!(manager.pending_background_count(), 1);
    }

    #[test]
    fn test_stats_counts() {
        let manager = WritePipelineManager::new();
        let stats = manager.stats();

        let id = manager.start(1024, 1000);
        assert_eq!(stats.writes_started.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_bytes_written.load(Ordering::Relaxed), 1024);

        manager
            .advance(id, WriteStage::JournalWritten, 1100)
            .unwrap();
        manager
            .advance(id, WriteStage::JournalReplicated, 1200)
            .unwrap();
        assert_eq!(stats.client_acks_issued.load(Ordering::Relaxed), 1);

        manager
            .advance(id, WriteStage::SegmentPacked, 1300)
            .unwrap();
        manager
            .advance(id, WriteStage::EcDistributed, 1400)
            .unwrap();
        assert_eq!(stats.ec_distributions_completed.load(Ordering::Relaxed), 1);

        manager.advance(id, WriteStage::S3Uploaded, 1500).unwrap();
        assert_eq!(stats.s3_uploads_completed.load(Ordering::Relaxed), 1);

        manager.complete(id, 1600).unwrap();
        assert_eq!(stats.writes_completed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_write_error_not_found() {
        let manager = WritePipelineManager::new();
        let result = manager.advance(WriteId(999), WriteStage::JournalWritten, 1100);
        assert!(matches!(result, Err(WriteError::NotFound(_))));
    }

    #[test]
    fn test_write_error_display() {
        let err = WriteError::NotFound(WriteId(42));
        assert!(err.to_string().contains("not found"));

        let err = WriteError::InvalidTransition {
            from: WriteStage::Received,
            to: WriteStage::JournalReplicated,
        };
        assert!(err.to_string().contains("invalid stage transition"));

        let err = WriteError::AlreadyComplete(WriteId(42));
        assert!(err.to_string().contains("already complete"));
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = WritePipelineStats::new();
        stats.writes_started.store(10, Ordering::Relaxed);
        stats.writes_completed.store(5, Ordering::Relaxed);

        let snapshot = stats.snapshot(5, 2);
        assert_eq!(snapshot.writes_started, 10);
        assert_eq!(snapshot.writes_completed, 5);
        assert_eq!(snapshot.active_writes, 5);
        assert_eq!(snapshot.pending_background, 2);
    }
}
