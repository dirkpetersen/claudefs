# Fix 22 failing tests in 3 new claudefs-storage modules

## Working directory
/home/cfs/claudefs

## Files to fix
1. `crates/claudefs-storage/src/background_scheduler.rs`
2. `crates/claudefs-storage/src/device_health_monitor.rs`
3. `crates/claudefs-storage/src/prefetch_engine.rs`

Read each file fully before fixing. Fix ALL issues described below.

---

## FIX 1: background_scheduler.rs

### Problem A: Priority ordering is backwards (max-heap vs min-priority semantics)
`BinaryHeap` in Rust is a max-heap. Lower priority number = more urgent (e.g., JournalFlush=10 is more urgent than TierEviction=200). The `Ord::cmp` implementation must use REVERSED comparison so that lower-numbered tasks come out first.

**Fix in `impl Ord for ScheduledTask`:**
Change `self.task.priority.cmp(&other.task.priority)` to `other.task.priority.cmp(&self.task.priority)`.
Change `self.inserted_at.cmp(&other.inserted_at)` to `other.inserted_at.cmp(&self.inserted_at)` (for FIFO: earlier-inserted = higher priority).

### Problem B: Tests use hardcoded BackgroundTaskId(1) but static NEXT_ID is non-deterministic
`BackgroundTask::new` uses a static atomic NEXT_ID. In parallel test runs, the first task created may not have ID 1.

**Fix these tests** to use the actual task ID captured from `schedule()` or `next_runnable()`:

**test_stats_tracking** (around line 440):
```rust
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
```

**test_estimated_bytes_io_tracked** (around line 561):
```rust
fn test_estimated_bytes_io_tracked() {
    let mut scheduler = BackgroundScheduler::new();
    let task = BackgroundTask::new(BackgroundTaskType::Scrub, 50000, "test".to_string());
    let _id = scheduler.schedule(task);
    let started = scheduler.next_runnable().unwrap();
    scheduler.complete_task(started.id, 50000);
    let stats = scheduler.stats();
    assert_eq!(stats.total_bytes_io, 50000);
}
```

**test_budget_exhaustion** (around line 369): Tests that after exhausting budget, next task is blocked.
```rust
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
```

**test_budget_reset** (around line 387): Tests that advancing time window resets budget.
```rust
fn test_budget_reset() {
    let mut scheduler = BackgroundScheduler::new();
    scheduler.set_io_budget(1000, 100);
    let task1 = create_test_task(BackgroundTaskType::Scrub);
    let task2 = create_test_task(BackgroundTaskType::Scrub);
    scheduler.schedule(task1);
    scheduler.schedule(task2);
    let started = scheduler.next_runnable().unwrap();
    scheduler.complete_task(started.id, 100);
    // Budget exhausted: next task blocked
    assert!(scheduler.next_runnable().is_none());
    scheduler.advance_window(1);
    // Budget reset: task2 now runnable
    let next = scheduler.next_runnable();
    assert!(next.is_some());
}
```

**test_complete_task_reduces_pending** (around line 427): Also uses hardcoded ID. Fix similarly:
```rust
fn test_complete_task_reduces_pending() {
    let mut scheduler = BackgroundScheduler::new();
    let task = create_test_task(BackgroundTaskType::Scrub);
    scheduler.schedule(task);
    let started = scheduler.next_runnable().unwrap();
    scheduler.complete_task(started.id, 1000);
    let stats = scheduler.stats();
    assert_eq!(stats.pending_count, 0);
}
```

---

## FIX 2: device_health_monitor.rs

### Problem A: compute_health_score uses wrong defaults (1.0 for missing data)
When only capacity data is set, wear and smart default to 1.0, making health_score dominated by those defaults. Tests expect that when only one metric is set, the health_score equals that metric's score.

**Fix `compute_health_score` in `impl DeviceData`**: Use dynamic weighting — only include components for which data is available:

```rust
fn compute_health_score(&self) -> f64 {
    let mut total_weight = 0.0f64;
    let mut weighted_score = 0.0f64;

    if self.wear.is_some() {
        let w = self.compute_wear_score();
        weighted_score += w * 0.35;
        total_weight += 0.35;
    }

    if self.total_bytes > 0 {
        let c = self.compute_capacity_score();
        weighted_score += c * 0.35;
        total_weight += 0.35;
    }

    if self.smart.is_some() {
        let s = self.compute_smart_score();
        weighted_score += s * 0.30;
        total_weight += 0.30;
    }

    if total_weight == 0.0 {
        1.0 // no data = assume healthy
    } else {
        weighted_score / total_weight
    }
}
```

### Problem B: Critical capacity threshold is too high (< 10% should be Warning, < 5% should be Critical)
In `check_alerts`, the severity for LowCapacity is: `if capacity_pct < 10.0 { Critical } else { Warning }`.
But test_check_alerts_warning_vs_critical expects: capacity=5% → Warning (not Critical), capacity=0.5% → Critical.

**Fix**: Change `capacity_pct < 10.0` to `capacity_pct < 5.0`:
```rust
severity: if capacity_pct < 5.0 {
    AlertSeverity::Critical
} else {
    AlertSeverity::Warning
},
```

---

## FIX 3: prefetch_engine.rs

### Problem A: detect_pattern sets is_sequential=true too early (before threshold)
Current code sets `is_sequential = true` when len >= 2, but threshold is 3. On the 3rd access, `!self.is_sequential` is false (it was set on 2nd access), so confidence is never boosted to >= threshold.

**Fix `detect_pattern` in `impl StreamState`**: Only mark as sequential AND boost confidence when `history_len >= sequential_threshold`. Use a sliding window of the most recent `sequential_threshold` entries for the sequential check (enables re-detection after random breaks):

```rust
fn detect_pattern(&mut self, config: &PrefetchConfig) {
    let history_len = self.history.len();
    if history_len < 2 {
        self.is_sequential = false;
        return;
    }

    // Check only the most recent window (enables re-detection after random break)
    let check_window = config.sequential_threshold.min(history_len);
    let start = history_len - check_window;

    let mut is_seq = true;
    for i in (start + 1)..history_len {
        let expected = self.history[i - 1].offset + self.history[i - 1].size;
        if self.history[i].offset != expected {
            is_seq = false;
            break;
        }
    }

    if is_seq && history_len >= config.sequential_threshold {
        if !self.is_sequential {
            // Transition to sequential: boost confidence
            self.confidence = (self.confidence + 0.3).min(1.0);
        }
        self.is_sequential = true;
    } else if !is_seq {
        if self.is_sequential {
            // Transition to random: reduce confidence more aggressively
            self.confidence = (self.confidence - 0.3).max(0.0);
        }
        self.is_sequential = false;
        // If len < threshold, keep is_sequential as false (no change)
    }
    // If is_seq but len < threshold: do NOT set is_sequential=true yet
}
```

Key changes:
- Uses `check_window = min(sequential_threshold, history_len)` to check only recent entries
- Only updates `is_sequential = true` when `history_len >= sequential_threshold`
- Drops confidence by 0.3 (not 0.2) on random detection, ensuring it falls below threshold (0.6) after one random break

### Problem B: random_streams_detected counter not incremented on first random detection
The counter only increments when transitioning FROM sequential TO random (was_sequential=true). But `test_random_streams_detected_counter` expects a count after 2 non-sequential accesses with no prior sequential detection.

**Fix in `record_access` in `impl PrefetchEngine`**: Also increment when detecting random at len==2 (first detectable pattern):

```rust
// After stream.add_access(...):
let stream_after = self.streams.get(&stream_id).unwrap();
if stream_after.is_sequential && !was_sequential {
    self.stats.sequential_streams_detected += 1;
} else if !stream_after.is_sequential && was_sequential {
    self.stats.random_streams_detected += 1;
} else if !stream_after.is_sequential && !was_sequential && stream_after.history.len() == 2 {
    // First random detection: 2 accesses that aren't sequential
    self.stats.random_streams_detected += 1;
}
```

**IMPORTANT NOTE on borrowing**: `record_access` currently calls `stream.add_access(...)` with `stream` as a mutable borrow from the HashMap. After calling `add_access`, you cannot get a second borrow. Structure the code to avoid double-borrow: capture `is_sequential` and `history_len` from the stream BEFORE the `add_access` call if needed for the post-access check, or restructure the borrow.

Here is a safe approach:
```rust
pub fn record_access(&mut self, stream_id: u64, block_offset: u64, size: u64) {
    self.access_counter += 1;

    if self.streams.len() >= self.config.max_streams && !self.streams.contains_key(&stream_id) {
        self.evict_lru_stream();
    }

    let stream = self.streams.entry(stream_id).or_insert_with(StreamState::new);
    let was_sequential = stream.is_sequential;
    stream.add_access(block_offset, size, self.access_counter, &self.config);
    let is_now_sequential = stream.is_sequential;
    let history_len = stream.history.len();

    if is_now_sequential && !was_sequential {
        self.stats.sequential_streams_detected += 1;
    } else if !is_now_sequential && was_sequential {
        self.stats.random_streams_detected += 1;
    } else if !is_now_sequential && !was_sequential && history_len == 2 {
        self.stats.random_streams_detected += 1;
    }
}
```

---

## Verification
After ALL fixes, run:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | tail -5
```

Expected: ALL tests pass (0 failures). At least 781 tests (759 currently passing + 22 failing).

Show the final test result line.
