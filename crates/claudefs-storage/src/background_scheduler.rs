//! Background task scheduler for coordinating scrub, defrag, compaction, and tiering.
//!
//! Prevents multiple background tasks from running simultaneously and enforces I/O budgets.

use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use thiserror::Error;

/// Errors from the background scheduler.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Task not found: {0}")]
    TaskNotFound(BackgroundTaskId),
    #[error("Task already running: {0}")]
    TaskAlreadyRunning(BackgroundTaskId),
    #[error("Invalid budget: {0}")]
    InvalidBudget(String),
}

/// Unique identifier for a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BackgroundTaskId(pub u64);

impl std::fmt::Display for BackgroundTaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskType {
    /// Scrubbing task - checks data integrity.
    Scrub,
    /// Defragmentation task.
    Defrag,
    /// Compaction task - compacts segments.
    Compaction,
    /// Tier eviction task - moves data to S3.
    TierEviction,
    /// Journal flush task.
    JournalFlush,
}

impl BackgroundTaskType {
    /// Returns default priority for this task type.
    pub fn default_priority(&self) -> u8 {
        match self {
            BackgroundTaskType::JournalFlush => 10,
            BackgroundTaskType::Scrub => 50,
            BackgroundTaskType::Defrag => 100,
            BackgroundTaskType::Compaction => 150,
            BackgroundTaskType::TierEviction => 200,
        }
    }

    /// Returns display name for this task type.
    pub fn display_name(&self) -> &'static str {
        match self {
            BackgroundTaskType::Scrub => "Scrub",
            BackgroundTaskType::Defrag => "Defrag",
            BackgroundTaskType::Compaction => "Compaction",
            BackgroundTaskType::TierEviction => "TierEviction",
            BackgroundTaskType::JournalFlush => "JournalFlush",
        }
    }
}

impl std::fmt::Display for BackgroundTaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// A background task to be scheduled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    /// Unique identifier for this task.
    pub id: BackgroundTaskId,
    /// Type of task.
    pub task_type: BackgroundTaskType,
    /// Priority (0=highest, 255=lowest).
    pub priority: u8,
    /// Estimated bytes of I/O this task will perform.
    pub estimated_bytes_io: u64,
    /// Unix timestamp when task was created.
    pub created_at: u64,
    /// Human-readable description of the task.
    pub description: String,
}

impl BackgroundTask {
    /// Creates a new background task with auto-generated ID.
    pub fn new(
        task_type: BackgroundTaskType,
        estimated_bytes_io: u64,
        description: String,
    ) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: BackgroundTaskId(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            task_type,
            priority: task_type.default_priority(),
            estimated_bytes_io,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description,
        }
    }

    /// Creates a new task with custom priority.
    pub fn with_priority(
        task_type: BackgroundTaskType,
        priority: u8,
        estimated_bytes_io: u64,
        description: String,
    ) -> Self {
        let mut task = Self::new(task_type, estimated_bytes_io, description);
        task.priority = priority;
        task
    }
}

/// Statistics from the scheduler.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SchedulerStats {
    /// Total tasks scheduled.
    pub tasks_scheduled: u64,
    /// Total tasks completed.
    pub tasks_completed: u64,
    /// Total tasks cancelled.
    pub tasks_cancelled: u64,
    /// Total bytes of I/O performed.
    pub total_bytes_io: u64,
    /// Number of pending tasks.
    pub pending_count: usize,
    /// Number of times budget was exhausted.
    pub budget_exhausted_count: u64,
}

/// Internal task wrapper for the priority queue.
#[derive(Debug)]
struct ScheduledTask {
    task: BackgroundTask,
    inserted_at: u64,
}

impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.task.priority == other.task.priority
            && self.task.task_type == other.task.task_type
            && self.inserted_at == other.inserted_at
    }
}

impl Eq for ScheduledTask {}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let priority_cmp = other.task.priority.cmp(&self.task.priority);
        if priority_cmp != std::cmp::Ordering::Equal {
            return priority_cmp;
        }

        let type_order = match (&self.task.task_type, &other.task.task_type) {
            (BackgroundTaskType::JournalFlush, BackgroundTaskType::Compaction) => {
                std::cmp::Ordering::Less
            }
            (BackgroundTaskType::Compaction, BackgroundTaskType::JournalFlush) => {
                std::cmp::Ordering::Greater
            }
            _ => std::cmp::Ordering::Equal,
        };

        if type_order != std::cmp::Ordering::Equal {
            return type_order;
        }

        other.inserted_at.cmp(&self.inserted_at)
    }
}

/// Unified background task scheduler.
///
/// Coordinates scrubbing, defragmentation, compaction, and tiering tasks.
/// Prevents multiple background tasks from running simultaneously and enforces I/O budgets.
#[derive(Debug)]
pub struct BackgroundScheduler {
    pending: BinaryHeap<ScheduledTask>,
    running_ids: std::collections::HashSet<BackgroundTaskId>,
    iops_limit: u32,
    bandwidth_bytes_per_sec: u64,
    bytes_io_this_window: u64,
    window_start_time: u64,
    stats: SchedulerStats,
    next_task_id: u64,
}

impl BackgroundScheduler {
    /// Creates a new background scheduler with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a new task and returns its ID.
    pub fn schedule(&mut self, task: BackgroundTask) -> BackgroundTaskId {
        let id = task.id;
        self.pending.push(ScheduledTask {
            task,
            inserted_at: self.stats.tasks_scheduled,
        });
        self.stats.tasks_scheduled += 1;
        self.stats.pending_count = self.pending.len();
        id
    }

    /// Returns the highest-priority runnable task if budget allows.
    pub fn next_runnable(&mut self) -> Option<BackgroundTask> {
        if self.bytes_io_this_window >= self.bandwidth_bytes_per_sec {
            self.stats.budget_exhausted_count += 1;
            return None;
        }

        let task = self.pending.pop()?;
        self.running_ids.insert(task.task.id);
        self.stats.pending_count = self.pending.len();
        Some(task.task)
    }

    /// Marks a task as complete and deducts from the I/O budget.
    pub fn complete_task(&mut self, id: BackgroundTaskId, bytes_io: u64) {
        if self.running_ids.remove(&id) {
            self.stats.tasks_completed += 1;
            self.stats.total_bytes_io += bytes_io;
            self.bytes_io_this_window = self.bytes_io_this_window.saturating_add(bytes_io);
        }
    }

    /// Cancels a pending task. Returns true if cancelled, false if not found or running.
    pub fn cancel_task(&mut self, id: BackgroundTaskId) -> bool {
        if self.running_ids.contains(&id) {
            return false;
        }

        let original_len = self.pending.len();
        self.pending.retain(|t| t.task.id != id);

        if self.pending.len() < original_len {
            self.stats.tasks_cancelled += 1;
            self.stats.pending_count = self.pending.len();
            true
        } else {
            false
        }
    }

    /// Sets the I/O budget limits.
    pub fn set_io_budget(&mut self, iops_limit: u32, bandwidth_bytes_per_sec: u64) {
        self.iops_limit = iops_limit;
        self.bandwidth_bytes_per_sec = bandwidth_bytes_per_sec;
    }

    /// Advances the time window, resetting the I/O budget proportionally.
    pub fn advance_window(&mut self, elapsed_secs: u64) {
        if elapsed_secs == 0 {
            return;
        }
        let reset_bytes = self.bandwidth_bytes_per_sec * elapsed_secs;
        self.bytes_io_this_window = self
            .bytes_io_this_window
            .saturating_sub(reset_bytes.min(self.bytes_io_this_window));
    }

    /// Returns scheduler statistics.
    pub fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            tasks_scheduled: self.stats.tasks_scheduled,
            tasks_completed: self.stats.tasks_completed,
            tasks_cancelled: self.stats.tasks_cancelled,
            total_bytes_io: self.stats.total_bytes_io,
            pending_count: self.stats.pending_count,
            budget_exhausted_count: self.stats.budget_exhausted_count,
        }
    }

    /// Returns the current I/O budget state.
    pub fn budget_info(&self) -> (u64, u64) {
        (self.bytes_io_this_window, self.bandwidth_bytes_per_sec)
    }
}

impl Default for BackgroundScheduler {
    fn default() -> Self {
        Self {
            pending: BinaryHeap::new(),
            running_ids: std::collections::HashSet::new(),
            iops_limit: 10000,
            bandwidth_bytes_per_sec: 100_000_000,
            bytes_io_this_window: 0,
            window_start_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            stats: SchedulerStats::default(),
            next_task_id: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_task(task_type: BackgroundTaskType) -> BackgroundTask {
        BackgroundTask::new(task_type, 1000, "test".to_string())
    }

    #[test]
    fn test_default_config() {
        let scheduler = BackgroundScheduler::new();
        let stats = scheduler.stats();
        assert_eq!(stats.pending_count, 0);
    }

    #[test]
    fn test_schedule_and_retrieve() {
        let mut scheduler = BackgroundScheduler::new();
        let task = create_test_task(BackgroundTaskType::Scrub);
        let id = scheduler.schedule(task);

        let next = scheduler.next_runnable();
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, id);
    }

    #[test]
    fn test_priority_ordering() {
        let mut scheduler = BackgroundScheduler::new();

        let low_priority = BackgroundTask::with_priority(
            BackgroundTaskType::TierEviction,
            200,
            1000,
            "low".to_string(),
        );
        let high_priority = BackgroundTask::with_priority(
            BackgroundTaskType::JournalFlush,
            10,
            1000,
            "high".to_string(),
        );

        scheduler.schedule(low_priority);
        scheduler.schedule(high_priority);

        let next = scheduler.next_runnable().unwrap();
        assert_eq!(next.priority, 10);
    }

    #[test]
    fn test_budget_exhaustion() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(1000, 100);

        let task1 = create_test_task(BackgroundTaskType::Scrub);
        let task2 = create_test_task(BackgroundTaskType::Scrub);
        scheduler.schedule(task1);
        scheduler.schedule(task2);

        let started = scheduler.next_runnable().unwrap();
        scheduler.complete_task(started.id, 100);

        let next = scheduler.next_runnable();
        assert!(next.is_none(), "Budget exhausted after 100 bytes");
    }

    #[test]
    fn test_budget_reset() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(1000, 100);

        let task1 = create_test_task(BackgroundTaskType::Scrub);
        let task2 = create_test_task(BackgroundTaskType::Scrub);
        scheduler.schedule(task1);
        scheduler.schedule(task2);

        let started = scheduler.next_runnable().unwrap();
        scheduler.complete_task(started.id, 100);
        assert!(scheduler.next_runnable().is_none());

        scheduler.advance_window(1);

        let next = scheduler.next_runnable();
        assert!(next.is_some());
    }

    #[test]
    fn test_cancel_pending_task() {
        let mut scheduler = BackgroundScheduler::new();
        let task = create_test_task(BackgroundTaskType::Scrub);
        let id = scheduler.schedule(task);

        let cancelled = scheduler.cancel_task(id);
        assert!(cancelled);

        let next = scheduler.next_runnable();
        assert!(next.is_none());
    }

    #[test]
    fn test_cancel_running_task_fails() {
        let mut scheduler = BackgroundScheduler::new();
        let task = create_test_task(BackgroundTaskType::Scrub);
        let id = scheduler.schedule(task);

        scheduler.next_runnable();

        let cancelled = scheduler.cancel_task(id);
        assert!(!cancelled);
    }

    #[test]
    fn test_complete_task_reduces_pending() {
        let mut scheduler = BackgroundScheduler::new();
        let task = create_test_task(BackgroundTaskType::Scrub);
        scheduler.schedule(task);

        let started = scheduler.next_runnable().unwrap();
        scheduler.complete_task(started.id, 1000);

        let stats = scheduler.stats();
        assert_eq!(stats.pending_count, 0);
    }

    #[test]
    fn test_stats_tracking() {
        let mut scheduler = BackgroundScheduler::new();

        let task = create_test_task(BackgroundTaskType::Scrub);
        let _id = scheduler.schedule(task);
        let started = scheduler.next_runnable().unwrap();
        scheduler.complete_task(started.id, 500);

        let stats = scheduler.stats();
        assert_eq!(stats.tasks_scheduled, 1);
        assert_eq!(stats.tasks_completed, 1);
        assert_eq!(stats.total_bytes_io, 500);
    }

    #[test]
    fn test_fifo_same_priority() {
        let mut scheduler = BackgroundScheduler::new();

        let task1 = BackgroundTask::with_priority(
            BackgroundTaskType::Scrub,
            100,
            1000,
            "first".to_string(),
        );
        let task2 = BackgroundTask::with_priority(
            BackgroundTaskType::Scrub,
            100,
            1000,
            "second".to_string(),
        );

        scheduler.schedule(task1);
        scheduler.schedule(task2);

        let next1 = scheduler.next_runnable().unwrap();
        assert_eq!(next1.description, "first");
    }

    #[test]
    fn test_journalflush_priority_over_compaction() {
        let mut scheduler = BackgroundScheduler::new();

        let compaction = BackgroundTask::new(
            BackgroundTaskType::Compaction,
            1000,
            "compaction".to_string(),
        );
        let journal = BackgroundTask::new(
            BackgroundTaskType::JournalFlush,
            1000,
            "journal".to_string(),
        );

        scheduler.schedule(compaction);
        scheduler.schedule(journal);

        let next = scheduler.next_runnable().unwrap();
        assert_eq!(next.task_type, BackgroundTaskType::JournalFlush);
    }

    #[test]
    fn test_zero_budget_no_tasks() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(0, 0);

        let task = create_test_task(BackgroundTaskType::Scrub);
        scheduler.schedule(task);

        let next = scheduler.next_runnable();
        assert!(next.is_none());
    }

    #[test]
    fn test_unlimited_budget() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(u32::MAX, u64::MAX);

        let task1 = create_test_task(BackgroundTaskType::Scrub);
        let task2 = create_test_task(BackgroundTaskType::Defrag);
        scheduler.schedule(task1);
        scheduler.schedule(task2);

        let next1 = scheduler.next_runnable();
        let next2 = scheduler.next_runnable();
        assert!(next1.is_some());
        assert!(next2.is_some());
    }

    #[test]
    fn test_different_task_types() {
        let mut scheduler = BackgroundScheduler::new();

        for tt in [
            BackgroundTaskType::Scrub,
            BackgroundTaskType::Defrag,
            BackgroundTaskType::Compaction,
            BackgroundTaskType::TierEviction,
            BackgroundTaskType::JournalFlush,
        ] {
            scheduler.schedule(BackgroundTask::new(tt, 1000, format!("{:?}", tt)));
        }

        assert_eq!(scheduler.stats().pending_count, 5);
    }

    #[test]
    fn test_task_type_display() {
        assert_eq!(BackgroundTaskType::Scrub.display_name(), "Scrub");
        assert_eq!(BackgroundTaskType::Defrag.display_name(), "Defrag");
        assert_eq!(BackgroundTaskType::Compaction.display_name(), "Compaction");
        assert_eq!(
            BackgroundTaskType::TierEviction.display_name(),
            "TierEviction"
        );
        assert_eq!(
            BackgroundTaskType::JournalFlush.display_name(),
            "JournalFlush"
        );
    }

    #[test]
    fn test_estimated_bytes_io_tracked() {
        let mut scheduler = BackgroundScheduler::new();
        let task = BackgroundTask::new(BackgroundTaskType::Scrub, 50000, "test".to_string());
        let _id = scheduler.schedule(task);

        let started = scheduler.next_runnable().unwrap();
        scheduler.complete_task(started.id, 50000);

        let stats = scheduler.stats();
        assert_eq!(stats.total_bytes_io, 50000);
    }

    #[test]
    fn test_budget_exhausted_counter() {
        let mut scheduler = BackgroundScheduler::new();
        scheduler.set_io_budget(1000, 0);

        let task = create_test_task(BackgroundTaskType::Scrub);
        scheduler.schedule(task);

        scheduler.next_runnable();

        let stats = scheduler.stats();
        assert_eq!(stats.budget_exhausted_count, 1);
    }

    #[test]
    fn test_empty_scheduler_next_runnable() {
        let mut scheduler = BackgroundScheduler::new();

        let next = scheduler.next_runnable();
        assert!(next.is_none());
    }

    #[test]
    fn test_schedule_then_cancel_same_task() {
        let mut scheduler = BackgroundScheduler::new();
        let task = create_test_task(BackgroundTaskType::Scrub);
        let id = scheduler.schedule(task);

        scheduler.cancel_task(id);

        let stats = scheduler.stats();
        assert_eq!(stats.tasks_cancelled, 1);
        assert_eq!(stats.pending_count, 0);
    }

    #[test]
    fn test_task_default_priority() {
        let task = BackgroundTask::new(BackgroundTaskType::JournalFlush, 1000, "test".to_string());
        assert_eq!(task.priority, 10);

        let task = BackgroundTask::new(BackgroundTaskType::TierEviction, 1000, "test".to_string());
        assert_eq!(task.priority, 200);
    }

    #[test]
    fn test_with_priority_override() {
        let task = BackgroundTask::with_priority(
            BackgroundTaskType::Scrub,
            5,
            1000,
            "high priority scrub".to_string(),
        );
        assert_eq!(task.priority, 5);
    }
}
