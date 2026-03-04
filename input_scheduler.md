# Task: Implement background_scheduler.rs module for claudefs-storage

## Location
Create file: `/home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs`

## Purpose
Unified background task scheduler that coordinates scrubbing, defragmentation, and compaction. Prevents multiple background tasks from running simultaneously and causing excessive I/O pressure. Enforces a budget (max IOPS and bandwidth) for background tasks.

## Requirements

### Data Structures

```rust
/// Unique identifier for a background task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BackgroundTaskId(pub u64);

/// Types of background tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskType {
    Scrub,
    Defrag,
    Compaction,
    TierEviction,
    JournalFlush,
}

/// A background task to be scheduled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: BackgroundTaskId,
    pub task_type: BackgroundTaskType,
    pub priority: u8,  // 0 = highest, 255 = lowest
    pub estimated_bytes_io: u64,
    pub created_at: u64,  // unix timestamp secs
    pub description: String,
}

/// Statistics about scheduler operation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulerStats {
    pub tasks_scheduled: u64,
    pub tasks_completed: u64,
    pub tasks_cancelled: u64,
    pub total_bytes_io: u64,
    pub pending_count: usize,
    pub budget_exhausted_count: u64,
}
```

### BackgroundScheduler

```rust
/// The background task scheduler with I/O budget enforcement
pub struct BackgroundScheduler {
    // Internal state:
    // - Priority queue (use BinaryHeap or sorted Vec)
    // - I/O budget: iops_limit, bandwidth_bytes_per_sec
    // - Bytes consumed in current window
    // - Set of running task IDs (to prevent cancellation)
    // - Counter for next task ID
}

impl BackgroundScheduler {
    pub fn new() -> Self;
    
    /// Add a task to the queue, returns its ID
    pub fn schedule(&mut self, mut task: BackgroundTask) -> BackgroundTaskId;
    
    /// Return highest-priority runnable task if budget allows
    /// Returns None if budget exhausted or queue empty
    pub fn next_runnable(&mut self) -> Option<BackgroundTask>;
    
    /// Mark a task as complete, deduct its bytes from budget tracking
    pub fn complete_task(&mut self, id: BackgroundTaskId, bytes_io: u64);
    
    /// Cancel a pending task (not running), returns true if cancelled
    pub fn cancel_task(&mut self, id: BackgroundTaskId) -> bool;
    
    /// Set the I/O budget limits
    pub fn set_io_budget(&mut self, iops_limit: u32, bandwidth_bytes_per_sec: u64);
    
    /// Advance the time window, resetting budget proportionally
    pub fn advance_window(&mut self, elapsed_secs: u64);
    
    /// Get statistics
    pub fn stats(&self) -> SchedulerStats;
}

impl Default for BackgroundScheduler {
    fn default() -> Self {
        Self::new()
    }
}
```

### Priority Queue Implementation

- Use `std::collections::BinaryHeap` with a custom `Ord` implementation for `BackgroundTask`
- Priority ordering: lower `priority` value = higher priority (runs first)
- Within same priority, FIFO: use `created_at` timestamp as tiebreaker (earlier = higher priority)
- For `BackgroundTaskType`: when all else equal, prefer `JournalFlush` over others (for data safety)

Implement `Ord` for `BackgroundTask`:

```rust
impl Ord for BackgroundTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower priority value = higher priority (reverse ordering)
        // Earlier created_at = higher priority (FIFO)
        // JournalFlush type gets priority boost when equal
        self.priority.cmp(&other.priority).reverse()
            .then_with(|| self.created_at.cmp(&other.created_at))
            .then_with(|| {
                let self_type_priority = self.task_type.type_priority();
                let other_type_priority = other.task_type.type_priority();
                self_type_priority.cmp(&other_type_priority)
            })
    }
}

impl PartialOrd for BackgroundTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl BackgroundTaskType {
    fn type_priority(&self) -> u8 {
        match self {
            BackgroundTaskType::JournalFlush => 0,  // highest
            BackgroundTaskType::TierEviction => 1,
            BackgroundTaskType::Scrub => 2,
            BackgroundTaskType::Defrag => 3,
            BackgroundTaskType::Compaction => 4,
        }
    }
}
```

### I/O Budget Enforcement

- Track `bytes_consumed_this_window: u64`
- Track `bandwidth_limit: u64` (bytes per second)
- When `next_runnable()` is called:
  - If `bytes_consumed_this_window >= bandwidth_limit`, return None and increment `budget_exhausted_count`
  - Otherwise, pop from queue, mark as "running" (add to running set), return task
- When `complete_task(id, bytes_io)` is called:
  - Remove from running set
  - Add `bytes_io` to `bytes_consumed_this_window`
  - Increment `total_bytes_io` in stats
- When `advance_window(elapsed_secs)` is called:
  - Reduce `bytes_consumed_this_window` proportionally, or reset to 0 if elapsed >= 1

### Test Coverage (at least 20 unit tests)

1. Default scheduler state
2. Schedule task and retrieve with `next_runnable()`
3. Priority ordering: higher priority (lower value) task returned first
4. Task not returned when budget exhausted
5. Budget resets after `advance_window(1)`
6. Cancel pending task → not returned by `next_runnable()`
7. Cancel already-running task → returns false
8. Complete task reduces pending count
9. Stats track completed/cancelled/scheduled counts correctly
10. Multiple tasks: FIFO within same priority level (earlier created_at wins)
11. JournalFlush priority is higher than Compaction by default (same priority value, different types)
12. Zero bandwidth budget: no tasks run
13. Unlimited budget (u64::MAX): all tasks run
14. Tasks with same priority, different types ordered correctly
15. `BackgroundTaskType` display/debug formatting works
16. `estimated_bytes_io` tracked in stats.total_bytes_io
17. `budget_exhausted_count` increments when budget prevents task
18. Empty scheduler: `next_runnable()` returns None
19. Schedule then immediately cancel same task
20. Multiple schedule and complete operations

## Style Rules
- All public structs/enums/fns MUST have `///` doc comments
- Use `thiserror` for any errors
- Use `serde` + `bincode` derives: `#[derive(Debug, Clone, Serialize, Deserialize)]`
- Use `tracing` crate: `use tracing::{debug, info, warn, error};`
- Use `std::collections::{BinaryHeap, HashSet}` for queue and running set
- No `unwrap()` in production code
- Tests use `#[test]` (sync), not async
- Idiomatic Rust: iterators, no manual index loops

## Output
Return the complete Rust code for background_scheduler.rs with all structs, impls, and tests.