//! Compaction orchestration for the storage subsystem.
//!
//! Manages the full compaction pipeline: identifies fragmented segments, schedules
//! background compaction, tracks in-flight jobs, and reports results.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// State of a compaction job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionJobState {
    /// Waiting for background scheduler slot.
    Queued,
    /// Compaction in progress.
    Running,
    /// Completed successfully.
    Done,
    /// Failed with error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

/// Unique ID for a compaction job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompactionJobId(pub u64);

impl std::fmt::Display for CompactionJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompJob-{}", self.0)
    }
}

/// A single compaction job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionJob {
    /// Unique identifier for this job.
    pub id: CompactionJobId,
    /// Segments to compact.
    pub segment_ids: Vec<u64>,
    /// Estimated bytes to process.
    pub estimated_bytes: u64,
    /// Current state of the job.
    pub state: CompactionJobState,
    /// Unix timestamp when job was created.
    pub created_at: u64,
    /// Unix timestamp when job started (if running or done).
    pub started_at: Option<u64>,
    /// Unix timestamp when job finished (if done).
    pub finished_at: Option<u64>,
    /// Bytes freed by compaction (set when Done).
    pub bytes_freed: u64,
    /// Error message (set when Failed).
    pub error: Option<String>,
}

/// Statistics from the compaction manager.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CompactionManagerStats {
    /// Total jobs submitted.
    pub jobs_submitted: u64,
    /// Jobs completed successfully.
    pub jobs_completed: u64,
    /// Jobs that failed.
    pub jobs_failed: u64,
    /// Jobs cancelled.
    pub jobs_cancelled: u64,
    /// Total bytes freed by compaction.
    pub total_bytes_freed: u64,
    /// Number of currently running jobs.
    pub active_job_count: usize,
}

/// Configuration for the compaction manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionManagerConfig {
    /// Maximum concurrent compaction jobs.
    pub max_concurrent_jobs: usize,
    /// Minimum segments per job.
    pub min_segments_per_job: usize,
    /// Maximum segments per job.
    pub max_segments_per_job: usize,
    /// Fragmentation threshold percentage (0-100).
    pub fragmentation_threshold_pct: u8,
}

impl Default for CompactionManagerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 2,
            min_segments_per_job: 2,
            max_segments_per_job: 8,
            fragmentation_threshold_pct: 30,
        }
    }
}

/// Errors from the compaction manager.
#[derive(Debug, Error)]
pub enum CompactionError {
    /// Too few segments provided for compaction.
    #[error("Too few segments: need at least {min}, got {actual}")]
    TooFewSegments {
        /// Minimum required segments.
        min: usize,
        /// Actual segments provided.
        actual: usize,
    },
    /// Too many segments provided for compaction.
    #[error("Too many segments: max {max}, got {actual}")]
    TooManySegments {
        /// Maximum allowed segments.
        max: usize,
        /// Actual segments provided.
        actual: usize,
    },
    /// Too many concurrent jobs running.
    #[error("Too many concurrent jobs: limit {limit}")]
    TooManyConcurrent {
        /// The concurrent job limit.
        limit: usize,
    },
    /// Job not found.
    #[error("Job not found: {0}")]
    JobNotFound(CompactionJobId),
    /// Invalid state transition attempted.
    #[error("Invalid state transition: job {id} in state {state:?}")]
    InvalidStateTransition {
        /// Job ID.
        id: CompactionJobId,
        /// Current state.
        state: CompactionJobState,
    },
}

/// Compaction job manager.
#[derive(Debug, Clone)]
pub struct CompactionManager {
    config: CompactionManagerConfig,
    jobs: HashMap<CompactionJobId, CompactionJob>,
    next_job_id: u64,
    stats: CompactionManagerStats,
}

impl CompactionManager {
    /// Creates a new CompactionManager with the given configuration.
    pub fn new(config: CompactionManagerConfig) -> Self {
        info!(
            max_concurrent = config.max_concurrent_jobs,
            min_segments = config.min_segments_per_job,
            max_segments = config.max_segments_per_job,
            threshold_pct = config.fragmentation_threshold_pct,
            "creating compaction manager"
        );
        Self {
            config,
            jobs: HashMap::new(),
            next_job_id: 1,
            stats: CompactionManagerStats::default(),
        }
    }

    /// Submits a new compaction job.
    ///
    /// Returns an error if:
    /// - segment_ids.len() < min_segments_per_job
    /// - segment_ids.len() > max_segments_per_job
    /// - active_job_count >= max_concurrent_jobs
    pub fn submit_job(
        &mut self,
        segment_ids: Vec<u64>,
    ) -> Result<CompactionJobId, CompactionError> {
        let count = segment_ids.len();
        if count < self.config.min_segments_per_job {
            return Err(CompactionError::TooFewSegments {
                min: self.config.min_segments_per_job,
                actual: count,
            });
        }
        if count > self.config.max_segments_per_job {
            return Err(CompactionError::TooManySegments {
                max: self.config.max_segments_per_job,
                actual: count,
            });
        }

        let active_count = self.active_jobs().len();
        if active_count >= self.config.max_concurrent_jobs {
            return Err(CompactionError::TooManyConcurrent {
                limit: self.config.max_concurrent_jobs,
            });
        }

        let id = CompactionJobId(self.next_job_id);
        self.next_job_id += 1;

        let estimated_bytes = segment_ids.len() as u64 * 2_097_152;
        let job = CompactionJob {
            id,
            segment_ids,
            estimated_bytes,
            state: CompactionJobState::Queued,
            created_at: now_secs(),
            started_at: None,
            finished_at: None,
            bytes_freed: 0,
            error: None,
        };

        debug!(job_id = %id, segments = count, "submitted compaction job");
        self.jobs.insert(id, job);
        self.stats.jobs_submitted += 1;

        Ok(id)
    }

    /// Starts a queued job.
    ///
    /// Transitions job state from Queued -> Running.
    pub fn start_job(&mut self, id: CompactionJobId) -> Result<(), CompactionError> {
        let job = self
            .jobs
            .get_mut(&id)
            .ok_or(CompactionError::JobNotFound(id))?;

        if job.state != CompactionJobState::Queued {
            return Err(CompactionError::InvalidStateTransition {
                id,
                state: job.state,
            });
        }

        job.state = CompactionJobState::Running;
        job.started_at = Some(now_secs());

        debug!(job_id = %id, "started compaction job");
        Ok(())
    }

    /// Completes a running job.
    ///
    /// Transitions job state from Running -> Done.
    /// Records bytes_freed and sets finished_at.
    pub fn complete_job(
        &mut self,
        id: CompactionJobId,
        bytes_freed: u64,
    ) -> Result<(), CompactionError> {
        let job = self
            .jobs
            .get_mut(&id)
            .ok_or(CompactionError::JobNotFound(id))?;

        if job.state != CompactionJobState::Running {
            return Err(CompactionError::InvalidStateTransition {
                id,
                state: job.state,
            });
        }

        job.state = CompactionJobState::Done;
        job.bytes_freed = bytes_freed;
        job.finished_at = Some(now_secs());

        self.stats.jobs_completed += 1;
        self.stats.total_bytes_freed += bytes_freed;

        debug!(job_id = %id, bytes_freed = bytes_freed, "completed compaction job");
        Ok(())
    }

    /// Fails a running job.
    ///
    /// Transitions job state from Running -> Failed.
    /// Records the error message.
    pub fn fail_job(&mut self, id: CompactionJobId, error: String) -> Result<(), CompactionError> {
        let job = self
            .jobs
            .get_mut(&id)
            .ok_or(CompactionError::JobNotFound(id))?;

        if job.state != CompactionJobState::Running {
            return Err(CompactionError::InvalidStateTransition {
                id,
                state: job.state,
            });
        }

        job.state = CompactionJobState::Failed;
        job.error = Some(error);
        job.finished_at = Some(now_secs());

        self.stats.jobs_failed += 1;

        debug!(job_id = %id, "failed compaction job");
        Ok(())
    }

    /// Cancels a queued job.
    ///
    /// Transitions job state from Queued -> Cancelled.
    /// Cannot cancel a Running job.
    /// Returns true if the job was cancelled.
    pub fn cancel_job(&mut self, id: CompactionJobId) -> bool {
        if let Some(job) = self.jobs.get_mut(&id) {
            if job.state == CompactionJobState::Queued {
                job.state = CompactionJobState::Cancelled;
                self.stats.jobs_cancelled += 1;
                debug!(job_id = %id, "cancelled compaction job");
                return true;
            }
        }
        false
    }

    /// Returns the job with the given ID, or None if not found.
    pub fn get_job(&self, id: CompactionJobId) -> Option<&CompactionJob> {
        self.jobs.get(&id)
    }

    /// Returns all currently running jobs.
    pub fn active_jobs(&self) -> Vec<&CompactionJob> {
        self.jobs
            .values()
            .filter(|j| j.state == CompactionJobState::Running)
            .collect()
    }

    /// Returns all queued jobs waiting to run.
    pub fn pending_jobs(&self) -> Vec<&CompactionJob> {
        self.jobs
            .values()
            .filter(|j| j.state == CompactionJobState::Queued)
            .collect()
    }

    /// Returns current statistics.
    pub fn stats(&self) -> CompactionManagerStats {
        let mut stats = self.stats.clone();
        stats.active_job_count = self.active_jobs().len();
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_has_no_active_jobs() {
        let manager = CompactionManager::new(CompactionManagerConfig::default());
        assert!(manager.active_jobs().is_empty());
        assert!(manager.pending_jobs().is_empty());
    }

    #[test]
    fn submit_valid_job_returns_id() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let result = manager.submit_job(vec![1, 2, 3]);
        assert!(result.is_ok());
        let id = result.unwrap();
        assert_eq!(id.0, 1);
    }

    #[test]
    fn submit_with_too_few_segments_returns_error() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let result = manager.submit_job(vec![1]);
        assert!(matches!(
            result,
            Err(CompactionError::TooFewSegments { .. })
        ));
    }

    #[test]
    fn submit_with_too_many_segments_returns_error() {
        let config = CompactionManagerConfig {
            max_segments_per_job: 4,
            ..Default::default()
        };
        let mut manager = CompactionManager::new(config);
        let result = manager.submit_job(vec![1, 2, 3, 4, 5]);
        assert!(matches!(
            result,
            Err(CompactionError::TooManySegments { .. })
        ));
    }

    #[test]
    fn submit_when_max_concurrent_reached_returns_error() {
        let config = CompactionManagerConfig {
            max_concurrent_jobs: 1,
            ..Default::default()
        };
        let mut manager = CompactionManager::new(config);
        let _ = manager.submit_job(vec![1, 2]);
        manager.start_job(CompactionJobId(1)).unwrap();
        let result = manager.submit_job(vec![3, 4]);
        assert!(matches!(
            result,
            Err(CompactionError::TooManyConcurrent { .. })
        ));
    }

    #[test]
    fn start_job_transitions_queued_to_running() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        let result = manager.start_job(id);
        assert!(result.is_ok());
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.state, CompactionJobState::Running);
        assert!(job.started_at.is_some());
    }

    #[test]
    fn start_job_on_nonexistent_returns_error() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let result = manager.start_job(CompactionJobId(999));
        assert!(matches!(result, Err(CompactionError::JobNotFound(_))));
    }

    #[test]
    fn start_job_on_running_returns_error() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        let result = manager.start_job(id);
        assert!(matches!(
            result,
            Err(CompactionError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn complete_job_transitions_running_to_done() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        let result = manager.complete_job(id, 1000);
        assert!(result.is_ok());
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.state, CompactionJobState::Done);
    }

    #[test]
    fn complete_job_records_bytes_freed() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        manager.complete_job(id, 5000).unwrap();
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.bytes_freed, 5000);
    }

    #[test]
    fn complete_job_sets_finished_at() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        manager.complete_job(id, 100).unwrap();
        let job = manager.get_job(id).unwrap();
        assert!(job.finished_at.is_some());
    }

    #[test]
    fn fail_job_transitions_running_to_failed() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        let result = manager.fail_job(id, "disk error".to_string());
        assert!(result.is_ok());
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.state, CompactionJobState::Failed);
    }

    #[test]
    fn fail_job_records_error_message() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        manager.fail_job(id, "out of memory".to_string()).unwrap();
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.error, Some("out of memory".to_string()));
    }

    #[test]
    fn cancel_job_transitions_queued_to_cancelled() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        let result = manager.cancel_job(id);
        assert!(result);
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.state, CompactionJobState::Cancelled);
    }

    #[test]
    fn cancel_job_on_running_returns_false() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        let result = manager.cancel_job(id);
        assert!(!result);
        let job = manager.get_job(id).unwrap();
        assert_eq!(job.state, CompactionJobState::Running);
    }

    #[test]
    fn cancel_job_on_nonexistent_returns_false() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let result = manager.cancel_job(CompactionJobId(999));
        assert!(!result);
    }

    #[test]
    fn stats_jobs_submitted_increments_on_submit() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        assert_eq!(manager.stats().jobs_submitted, 0);
        manager.submit_job(vec![1, 2]).unwrap();
        assert_eq!(manager.stats().jobs_submitted, 1);
        manager.submit_job(vec![3, 4]).unwrap();
        assert_eq!(manager.stats().jobs_submitted, 2);
    }

    #[test]
    fn stats_jobs_completed_increments_on_complete() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        assert_eq!(manager.stats().jobs_completed, 0);
        manager.complete_job(id, 100).unwrap();
        assert_eq!(manager.stats().jobs_completed, 1);
    }

    #[test]
    fn stats_jobs_failed_increments_on_fail() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        assert_eq!(manager.stats().jobs_failed, 0);
        manager.fail_job(id, "error".to_string()).unwrap();
        assert_eq!(manager.stats().jobs_failed, 1);
    }

    #[test]
    fn stats_jobs_cancelled_increments_on_cancel() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        assert_eq!(manager.stats().jobs_cancelled, 0);
        manager.cancel_job(id);
        assert_eq!(manager.stats().jobs_cancelled, 1);
    }

    #[test]
    fn stats_total_bytes_freed_accumulates() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id1 = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id1).unwrap();
        manager.complete_job(id1, 1000).unwrap();
        let id2 = manager.submit_job(vec![3, 4]).unwrap();
        manager.start_job(id2).unwrap();
        manager.complete_job(id2, 2000).unwrap();
        assert_eq!(manager.stats().total_bytes_freed, 3000);
    }

    #[test]
    fn active_jobs_returns_only_running_jobs() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id1 = manager.submit_job(vec![1, 2]).unwrap();
        let id2 = manager.submit_job(vec![3, 4]).unwrap();
        manager.start_job(id1).unwrap();
        let active = manager.active_jobs();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id1);
    }

    #[test]
    fn pending_jobs_returns_only_queued_jobs() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id1 = manager.submit_job(vec![1, 2]).unwrap();
        let _id2 = manager.submit_job(vec![3, 4]).unwrap();
        manager.start_job(id1).unwrap();
        let pending = manager.pending_jobs();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id.0, 2);
    }

    #[test]
    fn get_job_returns_correct_job() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![10, 20]).unwrap();
        let job = manager.get_job(id);
        assert!(job.is_some());
        assert_eq!(job.unwrap().segment_ids, vec![10, 20]);
    }

    #[test]
    fn multiple_jobs_tracked_independently() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id1 = manager.submit_job(vec![1, 2]).unwrap();
        let id2 = manager.submit_job(vec![3, 4]).unwrap();
        manager.start_job(id1).unwrap();
        manager.complete_job(id1, 500).unwrap();
        manager.start_job(id2).unwrap();
        manager.fail_job(id2, "failed".to_string()).unwrap();

        let job1 = manager.get_job(id1).unwrap();
        let job2 = manager.get_job(id2).unwrap();

        assert_eq!(job1.state, CompactionJobState::Done);
        assert_eq!(job1.bytes_freed, 500);
        assert_eq!(job2.state, CompactionJobState::Failed);
        assert!(job2.error.is_some());
    }

    #[test]
    fn default_config_has_correct_values() {
        let config = CompactionManagerConfig::default();
        assert_eq!(config.max_concurrent_jobs, 2);
        assert_eq!(config.min_segments_per_job, 2);
        assert_eq!(config.max_segments_per_job, 8);
        assert_eq!(config.fragmentation_threshold_pct, 30);
    }

    #[test]
    fn job_id_display() {
        let id = CompactionJobId(42);
        assert_eq!(format!("{}", id), "CompJob-42");
    }

    #[test]
    fn compaction_job_serialization() {
        let job = CompactionJob {
            id: CompactionJobId(1),
            segment_ids: vec![1, 2, 3],
            estimated_bytes: 10000,
            state: CompactionJobState::Done,
            created_at: 1000,
            started_at: Some(1010),
            finished_at: Some(1020),
            bytes_freed: 5000,
            error: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        let decoded: CompactionJob = serde_json::from_str(&json).unwrap();
        assert_eq!(job.id, decoded.id);
        assert_eq!(job.segment_ids, decoded.segment_ids);
        assert_eq!(job.state, decoded.state);
    }

    #[test]
    fn stats_active_job_count() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id1 = manager.submit_job(vec![1, 2]).unwrap();
        let _id2 = manager.submit_job(vec![3, 4]).unwrap();
        manager.start_job(id1).unwrap();
        let stats = manager.stats();
        assert_eq!(stats.active_job_count, 1);
    }

    #[test]
    fn cannot_start_completed_job() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        manager.start_job(id).unwrap();
        manager.complete_job(id, 100).unwrap();
        let result = manager.start_job(id);
        assert!(matches!(
            result,
            Err(CompactionError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn cannot_complete_queued_job() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        let result = manager.complete_job(id, 100);
        assert!(matches!(
            result,
            Err(CompactionError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn cannot_fail_queued_job() {
        let mut manager = CompactionManager::new(CompactionManagerConfig::default());
        let id = manager.submit_job(vec![1, 2]).unwrap();
        let result = manager.fail_job(id, "error".to_string());
        assert!(matches!(
            result,
            Err(CompactionError::InvalidStateTransition { .. })
        ));
    }
}
