//! Compaction scheduling that throttles compaction to avoid impacting foreground I/O.
//!
//! Compaction rewrites sparse segments, reclaiming space freed by GC. It must be throttled
//! to avoid saturating disk bandwidth during peak I/O periods.

use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;

/// Priority level for compaction jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CompactionPriority {
    /// Lowest priority, runs in background when idle.
    Background,
    /// Normal priority.
    Normal,
    /// Urgent priority, segment is getting sparse.
    Urgent,
    /// Emergency priority, space critically low.
    Emergency,
}

/// A compaction job to be scheduled.
#[derive(Debug, Clone)]
pub struct CompactionJob {
    /// Unique job identifier.
    pub job_id: u64,
    /// Segment IDs to compact.
    pub segment_ids: Vec<u64>,
    /// Priority of this job.
    pub priority: CompactionPriority,
    /// Estimated bytes to process.
    pub estimated_bytes: u64,
}

impl PartialEq for CompactionJob {
    fn eq(&self, other: &Self) -> bool {
        self.job_id == other.job_id
    }
}

impl Eq for CompactionJob {}

impl PartialOrd for CompactionJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CompactionJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

/// Configuration for the compaction scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionSchedulerConfig {
    /// Maximum concurrent compaction jobs.
    pub max_concurrent_jobs: usize,
    /// Bandwidth limit in MB/s.
    pub bandwidth_limit_mb_per_sec: u32,
    /// Urgency threshold percentage (e.g., 30.0 = 30% waste triggers urgent).
    pub urgency_threshold_pct: f64,
}

impl Default for CompactionSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 2,
            bandwidth_limit_mb_per_sec: 100,
            urgency_threshold_pct: 30.0,
        }
    }
}

/// Statistics for the compaction scheduler.
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    /// Jobs currently queued.
    pub jobs_queued: u64,
    /// Jobs completed.
    pub jobs_completed: u64,
    /// Jobs cancelled.
    pub jobs_cancelled: u64,
    /// Bytes compacted.
    pub bytes_compacted: u64,
}

/// Scheduler for compaction jobs.
pub struct CompactionScheduler {
    config: CompactionSchedulerConfig,
    queue: BinaryHeap<CompactionJob>,
    running: std::collections::HashMap<u64, CompactionJob>,
    next_job_id: u64,
    stats: SchedulerStats,
}

impl CompactionScheduler {
    /// Create a new compaction scheduler.
    pub fn new(config: CompactionSchedulerConfig) -> Self {
        Self {
            config,
            queue: BinaryHeap::new(),
            running: std::collections::HashMap::new(),
            next_job_id: 1,
            stats: SchedulerStats::default(),
        }
    }

    /// Submit a compaction job to the scheduler.
    ///
    /// Returns the assigned job ID.
    pub fn submit(&mut self, mut job: CompactionJob) -> u64 {
        let job_id = self.next_job_id;
        self.next_job_id += 1;
        job.job_id = job_id;
        self.queue.push(job);
        self.stats.jobs_queued += 1;
        job_id
    }

    /// Cancel a queued job.
    ///
    /// Returns `true` if the job was found and cancelled.
    pub fn cancel(&mut self, job_id: u64) -> bool {
        let initial_len = self.queue.len();
        self.queue.retain(|j| j.job_id != job_id);
        if self.queue.len() < initial_len {
            self.stats.jobs_cancelled += 1;
            true
        } else {
            false
        }
    }

    /// Get the next job to run.
    ///
    /// Returns `None` if max concurrent jobs reached or queue is empty.
    pub fn next_job(&mut self) -> Option<CompactionJob> {
        if self.running.len() >= self.config.max_concurrent_jobs {
            return None;
        }
        let job = self.queue.pop()?;
        self.running.insert(job.job_id, job.clone());
        Some(job)
    }

    /// Mark a job as completed.
    ///
    /// Updates statistics with bytes compacted.
    pub fn complete_job(&mut self, job_id: u64, bytes_compacted: u64) {
        if self.running.remove(&job_id).is_some() {
            self.stats.jobs_completed += 1;
            self.stats.bytes_compacted += bytes_compacted;
        }
    }

    /// Number of jobs waiting in queue.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Number of currently running jobs.
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// Get current statistics.
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Check if compaction is urgently needed based on waste percentage.
    pub fn needs_urgent_compaction(&self, waste_pct: f64) -> bool {
        waste_pct > self.config.urgency_threshold_pct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_job(priority: CompactionPriority) -> CompactionJob {
        CompactionJob {
            job_id: 0,
            segment_ids: vec![1, 2, 3],
            priority,
            estimated_bytes: 1024 * 1024,
        }
    }

    #[test]
    fn scheduler_config_default() {
        let config = CompactionSchedulerConfig::default();
        assert_eq!(config.max_concurrent_jobs, 2);
        assert_eq!(config.bandwidth_limit_mb_per_sec, 100);
        assert_eq!(config.urgency_threshold_pct, 30.0);
    }

    #[test]
    fn scheduler_stats_default() {
        let stats = SchedulerStats::default();
        assert_eq!(stats.jobs_queued, 0);
        assert_eq!(stats.jobs_completed, 0);
        assert_eq!(stats.jobs_cancelled, 0);
        assert_eq!(stats.bytes_compacted, 0);
    }

    #[test]
    fn submit_returns_job_id() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let job = make_job(CompactionPriority::Normal);
        let id = scheduler.submit(job);
        assert!(id > 0);
    }

    #[test]
    fn submit_increments_queue() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        scheduler.submit(make_job(CompactionPriority::Normal));
        assert_eq!(scheduler.queue_len(), 1);
    }

    #[test]
    fn next_job_returns_highest_priority() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        scheduler.submit(make_job(CompactionPriority::Background));
        scheduler.submit(make_job(CompactionPriority::Emergency));
        scheduler.submit(make_job(CompactionPriority::Normal));

        let job = scheduler.next_job().unwrap();
        assert_eq!(job.priority, CompactionPriority::Emergency);
    }

    #[test]
    fn next_job_respects_max_concurrent() {
        let config = CompactionSchedulerConfig {
            max_concurrent_jobs: 2,
            ..Default::default()
        };
        let mut scheduler = CompactionScheduler::new(config);

        scheduler.submit(make_job(CompactionPriority::Normal));
        scheduler.submit(make_job(CompactionPriority::Normal));
        scheduler.submit(make_job(CompactionPriority::Normal));

        let j1 = scheduler.next_job();
        let j2 = scheduler.next_job();
        let j3 = scheduler.next_job();

        assert!(j1.is_some());
        assert!(j2.is_some());
        assert!(j3.is_none());
    }

    #[test]
    fn next_job_empty_queue_returns_none() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let result = scheduler.next_job();
        assert!(result.is_none());
    }

    #[test]
    fn cancel_removes_job() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let job_id = scheduler.submit(make_job(CompactionPriority::Normal));
        let cancelled = scheduler.cancel(job_id);
        assert!(cancelled);
        assert_eq!(scheduler.queue_len(), 0);
    }

    #[test]
    fn cancel_unknown_returns_false() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let cancelled = scheduler.cancel(999);
        assert!(!cancelled);
    }

    #[test]
    fn complete_job_updates_stats() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let job_id = scheduler.submit(make_job(CompactionPriority::Normal));
        scheduler.next_job();
        scheduler.complete_job(job_id, 1024);

        assert_eq!(scheduler.stats().jobs_completed, 1);
        assert_eq!(scheduler.stats().bytes_compacted, 1024);
    }

    #[test]
    fn complete_job_decrements_running_count() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        let job_id = scheduler.submit(make_job(CompactionPriority::Normal));
        scheduler.next_job();
        assert_eq!(scheduler.running_count(), 1);
        scheduler.complete_job(job_id, 1024);
        assert_eq!(scheduler.running_count(), 0);
    }

    #[test]
    fn queue_len_after_submit() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        assert_eq!(scheduler.queue_len(), 0);
        scheduler.submit(make_job(CompactionPriority::Normal));
        assert_eq!(scheduler.queue_len(), 1);
        scheduler.submit(make_job(CompactionPriority::Normal));
        assert_eq!(scheduler.queue_len(), 2);
    }

    #[test]
    fn running_count_after_next_job() {
        let mut scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        scheduler.submit(make_job(CompactionPriority::Normal));
        assert_eq!(scheduler.running_count(), 0);
        scheduler.next_job();
        assert_eq!(scheduler.running_count(), 1);
    }

    #[test]
    fn needs_urgent_compaction_true() {
        let scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        assert!(scheduler.needs_urgent_compaction(50.0));
    }

    #[test]
    fn needs_urgent_compaction_false() {
        let scheduler = CompactionScheduler::new(CompactionSchedulerConfig::default());
        assert!(!scheduler.needs_urgent_compaction(10.0));
    }

    #[test]
    fn priority_ordering() {
        assert!(CompactionPriority::Emergency > CompactionPriority::Urgent);
        assert!(CompactionPriority::Urgent > CompactionPriority::Normal);
        assert!(CompactionPriority::Normal > CompactionPriority::Background);
    }
}
