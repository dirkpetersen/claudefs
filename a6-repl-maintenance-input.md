# Task: Implement repl_maintenance.rs for claudefs-repl crate

Create `/home/cfs/claudefs/crates/claudefs-repl/src/repl_maintenance.rs`

## Context
ClaudeFS cross-site replication maintenance windows. Operators need to
pause replication for maintenance (e.g., network upgrades, site maintenance)
and then resume it, allowing the site to catch up afterward.

## Requirements

Implement `repl_maintenance.rs` with:

### Types

```rust
/// Whether replication is currently paused for maintenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaintenanceState {
    Active,    // Normal replication
    /// Replication paused for maintenance.
    Paused {
        reason: String,
        since_ns: u64,
        paused_by: String,  // operator name/id
        resume_at_ns: Option<u64>,  // planned resume time
    },
    /// Replication catching up after maintenance.
    CatchingUp {
        lag_entries: u64,
        resumed_at_ns: u64,
    },
}

/// A maintenance window definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: u64,
    pub start_ns: u64,
    pub end_ns: u64,
    pub reason: String,
    pub affected_sites: Vec<u64>,
}

impl MaintenanceWindow {
    pub fn new(id: u64, start_ns: u64, duration_ms: u64, reason: String, sites: Vec<u64>) -> Self
    pub fn is_active(&self, now_ns: u64) -> bool
    pub fn duration_ms(&self) -> u64
}

/// Statistics for maintenance operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaintenanceStats {
    pub pauses_initiated: u64,
    pub resumes_completed: u64,
    pub total_pause_duration_ms: u64,
    pub max_pause_duration_ms: u64,
    pub catchup_entries_replayed: u64,
}
```

### Main struct: `MaintenanceCoordinator`

```rust
pub struct MaintenanceCoordinator {
    state: MaintenanceState,
    scheduled_windows: Vec<MaintenanceWindow>,
    stats: MaintenanceStats,
    pause_started_ns: Option<u64>,
}

impl MaintenanceCoordinator {
    pub fn new() -> Self

    /// Pause replication for maintenance.
    pub fn pause(&mut self, reason: String, operator: String, now_ns: u64, planned_resume_ns: Option<u64>)

    /// Resume replication after maintenance. lag_entries = how many journal entries to catch up.
    pub fn resume(&mut self, now_ns: u64, lag_entries: u64)

    /// Update catchup progress (entries remaining).
    pub fn update_catchup(&mut self, remaining_entries: u64)

    /// Mark catchup complete; transition back to Active.
    pub fn catchup_complete(&mut self)

    /// Schedule a future maintenance window.
    pub fn schedule_window(&mut self, window: MaintenanceWindow)

    /// Get any scheduled windows active at now_ns.
    pub fn active_window(&self, now_ns: u64) -> Option<&MaintenanceWindow>

    /// Get all scheduled windows.
    pub fn windows(&self) -> &[MaintenanceWindow]

    /// Get current maintenance state.
    pub fn state(&self) -> &MaintenanceState

    /// Returns true if replication is paused.
    pub fn is_paused(&self) -> bool

    /// Get stats.
    pub fn stats(&self) -> &MaintenanceStats
}

impl Default for MaintenanceCoordinator {
    fn default() -> Self { Self::new() }
}
```

## Conventions
- `use serde::{Deserialize, Serialize};`
- `use tracing::{info, warn};`
- Pure synchronous state machine, no async
- At least 20 unit tests: pause/resume cycle, stats tracking, window scheduling, active_window detection, catchup tracking

Output ONLY the complete Rust source file for `repl_maintenance.rs`. No explanations.
