# Task: Implement compaction_manager.rs for claudefs-storage

## Location
Create file: `crates/claudefs-storage/src/compaction_manager.rs`

## Purpose
Orchestrates compaction pipeline: submit jobs, track state, report results.

## Conventions
- thiserror for errors, serde Serialize+Deserialize, tracing for logging
- Full doc comments (///), no #[allow(dead_code)]
- 25+ tests in #[cfg(test)] mod tests

## NO EXTERNAL CRATE DEPENDENCIES. Use only std + serde + thiserror + tracing.

## Types

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionJobState { Queued, Running, Done, Failed, Cancelled }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompactionJobId(pub u64);

impl std::fmt::Display for CompactionJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompJob-{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionJob {
    pub id: CompactionJobId,
    pub segment_ids: Vec<u64>,
    pub estimated_bytes: u64,
    pub state: CompactionJobState,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub bytes_freed: u64,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CompactionManagerStats {
    pub jobs_submitted: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_cancelled: u64,
    pub total_bytes_freed: u64,
    pub active_job_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionManagerConfig {
    pub max_concurrent_jobs: usize,    // default: 2
    pub min_segments_per_job: usize,   // default: 2
    pub max_segments_per_job: usize,   // default: 8
    pub fragmentation_threshold_pct: u8,  // default: 30
}

impl Default for CompactionManagerConfig {
    fn default() -> Self {
        Self { max_concurrent_jobs: 2, min_segments_per_job: 2, max_segments_per_job: 8, fragmentation_threshold_pct: 30 }
    }
}

#[derive(Debug, Error)]
pub enum CompactionError {
    #[error("Too few segments: need at least {min}, got {actual}")]
    TooFewSegments { min: usize, actual: usize },
    #[error("Too many segments: max {max}, got {actual}")]
    TooManySegments { max: usize, actual: usize },
    #[error("Too many concurrent jobs: limit {limit}")]
    TooManyConcurrent { limit: usize },
    #[error("Job not found: {0}")]
    JobNotFound(CompactionJobId),
    #[error("Invalid state transition for job {id}: currently in state {state:?}")]
    InvalidStateTransition { id: CompactionJobId, state: CompactionJobState },
}

pub struct CompactionManager {
    config: CompactionManagerConfig,
    jobs: HashMap<CompactionJobId, CompactionJob>,
    next_id: u64,
    stats: CompactionManagerStats,
}
```

## Methods

```rust
impl CompactionManager {
    pub fn new(config: CompactionManagerConfig) -> Self

    /// Submit a new compaction job. Returns error if:
    /// - segment_ids.len() < min_segments_per_job
    /// - segment_ids.len() > max_segments_per_job
    /// - running_job_count >= max_concurrent_jobs
    pub fn submit_job(&mut self, segment_ids: Vec<u64>) -> Result<CompactionJobId, CompactionError>
    // Note: "active_job_count" for the concurrent limit means RUNNING jobs (not queued).
    // You can have many queued; limit applies to running.

    /// Transition Queued -> Running
    pub fn start_job(&mut self, id: CompactionJobId) -> Result<(), CompactionError>
    // Error if not found, or if not in Queued state

    /// Transition Running -> Done
    pub fn complete_job(&mut self, id: CompactionJobId, bytes_freed: u64) -> Result<(), CompactionError>
    // Error if not found, or if not in Running state

    /// Transition Running -> Failed
    pub fn fail_job(&mut self, id: CompactionJobId, error: String) -> Result<(), CompactionError>
    // Error if not found, or if not in Running state

    /// Transition Queued -> Cancelled (cannot cancel Running)
    /// Returns true if successfully cancelled, false if not found or not in Queued state
    pub fn cancel_job(&mut self, id: CompactionJobId) -> bool

    pub fn get_job(&self, id: CompactionJobId) -> Option<&CompactionJob>
    pub fn active_jobs(&self) -> Vec<&CompactionJob>   // Running jobs
    pub fn pending_jobs(&self) -> Vec<&CompactionJob>  // Queued jobs
    pub fn stats(&self) -> CompactionManagerStats
}
```

For timestamps (created_at, started_at, finished_at), use:
```rust
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
```

estimated_bytes = segment_ids.len() as u64 * 2_097_152  // 2MB per segment estimate

## Tests (25 tests)

1. test_new_manager_no_active — starts empty
2. test_submit_valid_job — returns ID
3. test_submit_too_few_segments — error
4. test_submit_too_many_segments — error
5. test_submit_when_max_concurrent_reached — error (queue 2 running jobs, then submit 3rd to running)
   (Note: submit succeeds even when 2 jobs running, ONLY start_job is blocked by max_concurrent.
   Actually re-read: submit_job checks running count. But actually, to test this: start 2 jobs via submit+start, then submit a 3rd... Actually make max_concurrent=1, submit+start 1 job, then submit another, which should succeed (queued), then start the 2nd which should fail with TooManyConcurrent. OR: change the submit check to check running count. Go with the approach where submit itself returns TooManyConcurrent if too many RUNNING jobs exist.)
6. test_start_job_transitions_queued_to_running — state change
7. test_start_job_not_found — error
8. test_start_job_not_queued — error (already running)
9. test_complete_job_running_to_done — state change
10. test_complete_job_records_bytes_freed
11. test_complete_job_not_running — error
12. test_fail_job_running_to_failed
13. test_fail_job_records_error_message
14. test_fail_job_not_running — error
15. test_cancel_queued_job — returns true
16. test_cancel_running_job — returns false
17. test_cancel_nonexistent — returns false
18. test_stats_jobs_submitted
19. test_stats_jobs_completed
20. test_stats_jobs_failed
21. test_stats_jobs_cancelled
22. test_stats_total_bytes_freed
23. test_active_jobs_returns_running
24. test_pending_jobs_returns_queued
25. test_get_job_returns_correct

## Output
Write the complete Rust file. Then run:
```
cd /home/cfs/claudefs && cargo test -p claudefs-storage compaction_manager 2>&1 | tail -3
```

Show the test result.
