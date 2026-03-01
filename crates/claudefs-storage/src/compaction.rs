//! Background segment compaction and garbage collection.
//!
//! This module performs background compaction of storage segments, reclaiming
//! space from deleted/overwritten blocks. It works alongside defrag.rs (block-level
//! defrag) and segment.rs (segment packing).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{StorageError, StorageResult};

/// Newtype wrapper around u64 representing a segment identifier.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SegmentId(u64);

impl SegmentId {
    /// Creates a new SegmentId from a u64 value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the inner u64 value.
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for SegmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "seg:{}", self.0)
    }
}

impl From<u64> for SegmentId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

/// Information about a stored segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
    /// Unique segment identifier.
    pub id: SegmentId,
    /// Total bytes in the segment.
    pub total_bytes: u64,
    /// Live (used) bytes in the segment.
    pub live_bytes: u64,
    /// Dead (garbage) bytes in the segment.
    pub dead_bytes: u64,
    /// Total block count in the segment.
    pub block_count: u32,
    /// Live block count in the segment.
    pub live_block_count: u32,
    /// Unix timestamp when segment was created.
    pub created_at_secs: u64,
    /// Unix timestamp when segment was last modified.
    pub last_modified_secs: u64,
}

impl SegmentInfo {
    /// Creates a new SegmentInfo.
    pub fn new(
        id: SegmentId,
        total_bytes: u64,
        live_bytes: u64,
        block_count: u32,
        live_block_count: u32,
        created_at_secs: u64,
    ) -> Self {
        let dead_bytes = total_bytes.saturating_sub(live_bytes);
        Self {
            id,
            total_bytes,
            live_bytes,
            dead_bytes,
            block_count,
            live_block_count,
            created_at_secs,
            last_modified_secs: created_at_secs,
        }
    }

    /// Returns the percentage of dead bytes (0-100).
    pub fn dead_pct(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.dead_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}

/// Configuration for the compaction engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Minimum dead percentage required to compact a segment (default: 30.0).
    pub min_dead_pct: f64,
    /// Maximum number of concurrent compaction tasks (default: 2).
    pub max_concurrent: u32,
    /// Target fill percentage for new segments (default: 90.0).
    pub target_segment_fill_pct: f64,
    /// Garbage collection interval in seconds (default: 300).
    pub gc_interval_secs: u64,
    /// Minimum age in seconds before a segment can be compacted (default: 60).
    pub min_segment_age_secs: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            min_dead_pct: 30.0,
            max_concurrent: 2,
            target_segment_fill_pct: 90.0,
            gc_interval_secs: 300,
            min_segment_age_secs: 60,
        }
    }
}

/// State of a compaction task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompactionState {
    /// Task created, awaiting execution.
    Pending,
    /// Selecting source segments for compaction.
    Selecting,
    /// Reading live blocks from source segments.
    Reading,
    /// Writing live blocks to new segments.
    Writing,
    /// Verifying compacted data integrity.
    Verifying,
    /// Compaction completed successfully.
    Completed,
    /// Compaction failed with error message.
    Failed(String),
}

/// A compaction task for garbage collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionTask {
    /// Source segments to compact.
    pub source_segments: Vec<SegmentId>,
    /// Target segment for compacted data (if known).
    pub target_segment: Option<SegmentId>,
    /// Current state of the compaction.
    pub state: CompactionState,
    /// Estimated bytes to reclaim from this compaction.
    pub bytes_to_reclaim: u64,
    /// Actual bytes reclaimed after completion.
    pub bytes_reclaimed: u64,
    /// Timestamp when task started (None if not started).
    pub started_at: Option<u64>,
    /// Timestamp when task completed (None if not completed).
    pub completed_at: Option<u64>,
}

impl CompactionTask {
    /// Creates a new compaction task.
    pub fn new(source_segments: Vec<SegmentId>, bytes_to_reclaim: u64) -> Self {
        Self {
            source_segments,
            target_segment: None,
            state: CompactionState::Pending,
            bytes_to_reclaim,
            bytes_reclaimed: 0,
            started_at: None,
            completed_at: None,
        }
    }
}

/// A candidate segment for garbage collection.
#[derive(Debug, Clone)]
pub struct GcCandidate {
    /// The segment information.
    pub segment: SegmentInfo,
    /// Percentage of dead bytes (0-100).
    pub dead_pct: f64,
    /// Priority score (higher = more urgent).
    pub priority: f64,
}

impl GcCandidate {
    /// Creates a new GC candidate.
    pub fn new(segment: SegmentInfo, dead_pct: f64) -> Self {
        let priority = dead_pct * (segment.total_bytes as f64 / (1024.0 * 1024.0));
        Self {
            segment,
            dead_pct,
            priority,
        }
    }
}

/// Statistics about compaction operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    /// Total number of compactions performed.
    pub total_compactions: u64,
    /// Number of currently active compactions.
    pub active_compactions: u64,
    /// Number of pending compactions.
    pub pending_compactions: u64,
    /// Total bytes reclaimed through compaction.
    pub total_bytes_reclaimed: u64,
    /// Total bytes processed by compaction.
    pub total_bytes_processed: u64,
    /// Average reclaim percentage.
    pub avg_reclaim_pct: f64,
    /// Total number of tracked segments.
    pub segments_tracked: usize,
    /// Number of segments needing compaction.
    pub segments_needing_compaction: usize,
}

/// Compaction engine for background segment garbage collection.
#[derive(Debug)]
pub struct CompactionEngine {
    /// Configuration for compaction.
    config: CompactionConfig,
    /// Active and pending compaction tasks.
    tasks: Vec<CompactionTask>,
    /// Tracked segment information.
    segments: HashMap<SegmentId, SegmentInfo>,
    /// Total bytes reclaimed over lifetime.
    total_reclaimed_bytes: u64,
    /// Total number of completed compactions.
    total_compactions: u64,
}

impl CompactionEngine {
    /// Creates a new compaction engine with the given configuration.
    pub fn new(config: CompactionConfig) -> Self {
        Self {
            config,
            tasks: Vec::new(),
            segments: HashMap::new(),
            total_reclaimed_bytes: 0,
            total_compactions: 0,
        }
    }

    /// Registers a segment for tracking.
    pub fn register_segment(&mut self, info: SegmentInfo) {
        debug!("Registering segment {:?}", info.id);
        self.segments.insert(info.id, info);
    }

    /// Updates the live bytes and block count for a segment.
    pub fn update_segment(
        &mut self,
        id: SegmentId,
        live_bytes: u64,
        live_blocks: u32,
    ) -> StorageResult<()> {
        let segment = self
            .segments
            .get_mut(&id)
            .ok_or_else(|| StorageError::DeviceError {
                device: format!("{}", id),
                reason: "Segment not found".to_string(),
            })?;

        segment.live_bytes = live_bytes;
        segment.dead_bytes = segment.total_bytes.saturating_sub(live_bytes);
        segment.live_block_count = live_blocks;
        segment.block_count = segment.block_count.max(live_blocks);
        segment.last_modified_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug!(
            "Updated segment {:?}: {} live bytes, {} dead bytes",
            id, live_bytes, segment.dead_bytes
        );
        Ok(())
    }

    /// Removes a segment from tracking.
    pub fn remove_segment(&mut self, id: SegmentId) {
        debug!("Removing segment {:?}", id);
        self.segments.remove(&id);
    }

    /// Returns the dead percentage for a segment, or None if not found.
    pub fn segment_dead_pct(&self, id: SegmentId) -> Option<f64> {
        self.segments.get(&id).map(|s| s.dead_pct())
    }

    /// Finds segments exceeding the dead_pct threshold, sorted by priority descending.
    pub fn find_candidates(&self) -> Vec<GcCandidate> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut candidates: Vec<GcCandidate> = self
            .segments
            .values()
            .filter(|s| {
                let age = now.saturating_sub(s.created_at_secs);
                let old_enough = age >= self.config.min_segment_age_secs;
                let above_threshold = s.dead_pct() >= self.config.min_dead_pct;
                old_enough && above_threshold
            })
            .map(|s| GcCandidate::new(s.clone(), s.dead_pct()))
            .collect();

        candidates.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap());
        candidates
    }

    /// Creates a new compaction task and returns its index.
    pub fn create_compaction_task(&mut self, segment_ids: Vec<SegmentId>) -> StorageResult<usize> {
        for id in &segment_ids {
            if !self.segments.contains_key(id) {
                return Err(StorageError::DeviceError {
                    device: format!("{}", id),
                    reason: "Segment not found".to_string(),
                });
            }
        }

        let bytes_to_reclaim: u64 = segment_ids
            .iter()
            .filter_map(|id| self.segments.get(id))
            .map(|s| s.dead_bytes)
            .sum();

        let task = CompactionTask::new(segment_ids, bytes_to_reclaim);
        let idx = self.tasks.len();
        self.tasks.push(task);

        info!(
            "Created compaction task {} with {} segments, {} bytes to reclaim",
            idx,
            self.tasks[idx].source_segments.len(),
            bytes_to_reclaim
        );

        Ok(idx)
    }

    /// Advances a task through its state machine.
    pub fn advance_task(&mut self, task_idx: usize) -> StorageResult<CompactionState> {
        let task = self
            .tasks
            .get_mut(task_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: "compaction".to_string(),
                reason: format!("Task {} not found", task_idx),
            })?;

        if matches!(task.state, CompactionState::Completed) {
            return Err(StorageError::DeviceError {
                device: "compaction".to_string(),
                reason: format!("Task {} already completed", task_idx),
            });
        }

        if let CompactionState::Failed(_) = task.state {
            return Err(StorageError::DeviceError {
                device: "compaction".to_string(),
                reason: format!("Task {} already failed", task_idx),
            });
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if task.started_at.is_none() {
            task.started_at = Some(now);
        }

        let new_state = match &task.state {
            CompactionState::Pending => {
                debug!("Task {} advancing: Pending -> Selecting", task_idx);
                CompactionState::Selecting
            }
            CompactionState::Selecting => {
                debug!("Task {} advancing: Selecting -> Reading", task_idx);
                CompactionState::Reading
            }
            CompactionState::Reading => {
                debug!("Task {} advancing: Reading -> Writing", task_idx);
                CompactionState::Writing
            }
            CompactionState::Writing => {
                debug!("Task {} advancing: Writing -> Verifying", task_idx);
                CompactionState::Verifying
            }
            CompactionState::Verifying => {
                debug!("Task {} advancing: Verifying -> Completed", task_idx);
                task.completed_at = Some(now);
                CompactionState::Completed
            }
            CompactionState::Completed => unreachable!(),
            CompactionState::Failed(_) => unreachable!(),
        };

        task.state = new_state.clone();
        Ok(new_state)
    }

    /// Marks a task as failed with the given reason.
    pub fn fail_task(&mut self, task_idx: usize, reason: String) -> StorageResult<()> {
        let task = self
            .tasks
            .get_mut(task_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: "compaction".to_string(),
                reason: format!("Task {} not found", task_idx),
            })?;

        warn!("Task {} failed: {}", task_idx, reason);
        task.state = CompactionState::Failed(reason);
        Ok(())
    }

    /// Marks a task as completed with the actual bytes reclaimed.
    pub fn complete_task(&mut self, task_idx: usize, bytes_reclaimed: u64) -> StorageResult<()> {
        let task = self
            .tasks
            .get_mut(task_idx)
            .ok_or_else(|| StorageError::DeviceError {
                device: "compaction".to_string(),
                reason: format!("Task {} not found", task_idx),
            })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        task.state = CompactionState::Completed;
        task.completed_at = Some(now);
        task.bytes_reclaimed = bytes_reclaimed;

        self.total_reclaimed_bytes += bytes_reclaimed;
        self.total_compactions += 1;

        info!(
            "Task {} completed: {} bytes reclaimed",
            task_idx, bytes_reclaimed
        );
        Ok(())
    }

    /// Returns all active (non-Completed, non-Failed) tasks.
    pub fn active_tasks(&self) -> Vec<&CompactionTask> {
        self.tasks
            .iter()
            .filter(|t| {
                !matches!(
                    t.state,
                    CompactionState::Completed | CompactionState::Failed(_)
                )
            })
            .collect()
    }

    /// Returns true if a new compaction can be started.
    pub fn can_start_compaction(&self) -> bool {
        (self.active_tasks().len() as u32) < self.config.max_concurrent
    }

    /// Returns current compaction statistics.
    pub fn stats(&self) -> CompactionStats {
        let active = self.active_tasks();
        let active_count = active.len() as u64;
        let pending_count = active
            .iter()
            .filter(|t| matches!(t.state, CompactionState::Pending))
            .count() as u64;

        let candidates = self.find_candidates();
        let total_processed: u64 = self
            .tasks
            .iter()
            .filter(|t| matches!(t.state, CompactionState::Completed))
            .map(|t| t.bytes_to_reclaim)
            .sum();

        let avg_reclaim_pct = if self.total_compactions > 0 {
            let total_reclaimed = self
                .tasks
                .iter()
                .filter(|t| matches!(t.state, CompactionState::Completed))
                .map(|t| t.bytes_reclaimed as f64)
                .sum::<f64>();
            let total_processed_f = total_processed as f64;
            if total_processed_f > 0.0 {
                (total_reclaimed / total_processed_f) * 100.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        CompactionStats {
            total_compactions: self.total_compactions,
            active_compactions: active_count,
            pending_compactions: pending_count,
            total_bytes_reclaimed: self.total_reclaimed_bytes,
            total_bytes_processed: total_processed,
            avg_reclaim_pct,
            segments_tracked: self.segments.len(),
            segments_needing_compaction: candidates.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_segment(id: u64, total: u64, live: u64, created_at: u64) -> SegmentInfo {
        SegmentInfo::new(
            SegmentId::new(id),
            total,
            live,
            (total / 4096) as u32,
            (live / 4096) as u32,
            created_at,
        )
    }

    fn far_past_time() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(300)
    }

    #[test]
    fn test_segment_id_display() {
        let id = SegmentId::new(42);
        assert_eq!(format!("{}", id), "seg:42");
    }

    #[test]
    fn test_segment_info_dead_bytes() {
        let info = create_segment(1, 2_000_000, 1_200_000, far_past_time());
        assert_eq!(info.dead_bytes, 800_000);
        assert!((info.dead_pct() - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_compaction_config_defaults() {
        let config = CompactionConfig::default();
        assert_eq!(config.min_dead_pct, 30.0);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.target_segment_fill_pct, 90.0);
        assert_eq!(config.gc_interval_secs, 300);
        assert_eq!(config.min_segment_age_secs, 60);
    }

    #[test]
    fn test_register_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let info = create_segment(1, 2_000_000, 1_500_000, far_past_time());
        engine.register_segment(info.clone());
        assert!(engine.segments.contains_key(&SegmentId::new(1)));
    }

    #[test]
    fn test_register_multiple() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        for i in 1..=5 {
            engine.register_segment(create_segment(i, 2_000_000, 1_500_000, far_past_time()));
        }
        assert_eq!(engine.segments.len(), 5);
    }

    #[test]
    fn test_update_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let info = create_segment(1, 2_000_000, 1_500_000, far_past_time());
        engine.register_segment(info);
        engine
            .update_segment(SegmentId::new(1), 1_000_000, 244)
            .unwrap();
        let seg = engine.segments.get(&SegmentId::new(1)).unwrap();
        assert_eq!(seg.live_bytes, 1_000_000);
        assert_eq!(seg.dead_bytes, 1_000_000);
    }

    #[test]
    fn test_update_unknown_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let result = engine.update_segment(SegmentId::new(999), 1_000_000, 244);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let info = create_segment(1, 2_000_000, 1_500_000, far_past_time());
        engine.register_segment(info);
        engine.remove_segment(SegmentId::new(1));
        assert!(!engine.segments.contains_key(&SegmentId::new(1)));
    }

    #[test]
    fn test_find_candidates_none() {
        let mut config = CompactionConfig::default();
        config.min_dead_pct = 50.0;
        let mut engine = CompactionEngine::new(config);
        engine.register_segment(create_segment(1, 2_000_000, 1_500_000, far_past_time()));
        let candidates = engine.find_candidates();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_find_candidates_some() {
        let config = CompactionConfig::default();
        let mut engine = CompactionEngine::new(config);
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 1_500_000, far_past_time()));
        let candidates = engine.find_candidates();
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_find_candidates_sorted() {
        let config = CompactionConfig::default();
        let mut engine = CompactionEngine::new(config);
        engine.register_segment(create_segment(1, 4_000_000, 2_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 1_000_000, far_past_time()));
        let candidates = engine.find_candidates();
        assert!(candidates.len() >= 2);
        for i in 1..candidates.len() {
            assert!(candidates[i - 1].priority >= candidates[i].priority);
        }
    }

    #[test]
    fn test_create_task() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 1_200_000, far_past_time()));
        let idx = engine
            .create_compaction_task(vec![SegmentId::new(1), SegmentId::new(2)])
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(engine.tasks[0].source_segments.len(), 2);
    }

    #[test]
    fn test_create_task_unknown_segment() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let result = engine.create_compaction_task(vec![SegmentId::new(999)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_advance_task_full_cycle() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();

        let state = engine.advance_task(0).unwrap();
        assert!(matches!(state, CompactionState::Selecting));

        let state = engine.advance_task(0).unwrap();
        assert!(matches!(state, CompactionState::Reading));

        let state = engine.advance_task(0).unwrap();
        assert!(matches!(state, CompactionState::Writing));

        let state = engine.advance_task(0).unwrap();
        assert!(matches!(state, CompactionState::Verifying));
    }

    #[test]
    fn test_advance_completed_task() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();

        while !matches!(engine.tasks[0].state, CompactionState::Verifying) {
            engine.advance_task(0).unwrap();
        }
        engine.advance_task(0).unwrap();

        let result = engine.advance_task(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_fail_task() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();
        engine.fail_task(0, "test failure".to_string()).unwrap();
        if let CompactionState::Failed(msg) = &engine.tasks[0].state {
            assert_eq!(msg, "test failure");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn test_complete_task() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();
        engine.complete_task(0, 500_000).unwrap();
        assert!(matches!(engine.tasks[0].state, CompactionState::Completed));
        assert_eq!(engine.total_reclaimed_bytes, 500_000);
    }

    #[test]
    fn test_active_tasks() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 1_000_000, far_past_time()));
        engine.register_segment(create_segment(3, 2_000_000, 1_000_000, far_past_time()));

        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();
        engine
            .create_compaction_task(vec![SegmentId::new(2)])
            .unwrap();
        engine
            .create_compaction_task(vec![SegmentId::new(3)])
            .unwrap();

        engine.complete_task(2, 100_000).unwrap();

        let active = engine.active_tasks();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_can_start_compaction() {
        let mut config = CompactionConfig::default();
        config.max_concurrent = 2;
        let mut engine = CompactionEngine::new(config);

        for i in 1..=3 {
            engine.register_segment(create_segment(i, 2_000_000, 1_000_000, far_past_time()));
        }

        assert!(engine.can_start_compaction());

        engine
            .create_compaction_task(vec![SegmentId::new(1)])
            .unwrap();
        engine
            .create_compaction_task(vec![SegmentId::new(2)])
            .unwrap();

        assert!(!engine.can_start_compaction());
    }

    #[test]
    fn test_segment_dead_pct() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        let info = create_segment(1, 2_000_000, 500_000, far_past_time());
        engine.register_segment(info);

        let pct = engine.segment_dead_pct(SegmentId::new(1)).unwrap();
        assert!((pct - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_segment_dead_pct_unknown() {
        let engine = CompactionEngine::new(CompactionConfig::default());
        assert!(engine.segment_dead_pct(SegmentId::new(999)).is_none());
    }

    #[test]
    fn test_stats() {
        let mut engine = CompactionEngine::new(CompactionConfig::default());
        engine.register_segment(create_segment(1, 2_000_000, 1_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 500_000, far_past_time()));

        let stats = engine.stats();
        assert_eq!(stats.segments_tracked, 2);
    }

    #[test]
    fn test_min_segment_age() {
        let mut config = CompactionConfig::default();
        config.min_segment_age_secs = 1000;
        let mut engine = CompactionEngine::new(config);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        engine.register_segment(SegmentInfo::new(
            SegmentId::new(1),
            2_000_000,
            1_000_000,
            488,
            244,
            now,
        ));

        let candidates = engine.find_candidates();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_compaction_priority() {
        let config = CompactionConfig::default();
        let mut engine = CompactionEngine::new(config);

        engine.register_segment(create_segment(1, 8_000_000, 4_000_000, far_past_time()));
        engine.register_segment(create_segment(2, 2_000_000, 1_000_000, far_past_time()));

        let candidates = engine.find_candidates();
        assert!(!candidates.is_empty());

        let c1 = candidates
            .iter()
            .find(|c| c.segment.id == SegmentId::new(1))
            .unwrap();
        let c2 = candidates
            .iter()
            .find(|c| c.segment.id == SegmentId::new(2))
            .unwrap();
        assert!(c1.priority > c2.priority);
    }
}
