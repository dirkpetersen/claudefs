# Task: Add 3 new modules to claudefs-storage/src/lib.rs

## Working directory
/home/cfs/claudefs

## What needs to be done
The following 3 new .rs files exist in `crates/claudefs-storage/src/` but are NOT yet declared in `lib.rs`:
- `background_scheduler.rs`
- `device_health_monitor.rs`
- `prefetch_engine.rs`

Add these modules to `crates/claudefs-storage/src/lib.rs`.

## Current lib.rs state
Read `crates/claudefs-storage/src/lib.rs` first to see the current content.

The file currently has module declarations (pub mod) and re-exports (pub use) for other modules.

## Exact changes required

### 1. Add `pub mod` declarations
After `pub mod tracing_storage;` (line 44), add:
```rust
pub mod background_scheduler;
pub mod device_health_monitor;
pub mod prefetch_engine;
```

### 2. Add `pub use` exports
After `pub use tracing_storage::{...};` at the end of the file, add:
```rust
pub use background_scheduler::{
    BackgroundScheduler, BackgroundTask, BackgroundTaskId, BackgroundTaskType,
    SchedulerStats,
};
pub use device_health_monitor::{
    AlertSeverity as HealthAlertSeverity, DeviceHealthMonitor, DeviceHealthSummary,
    HealthAlert, HealthAlertType, SmartSnapshot, WearSnapshot,
};
pub use prefetch_engine::{
    PrefetchConfig, PrefetchEngine, PrefetchHint, PrefetchStats,
};
```

**IMPORTANT**: The file already exports `AlertSeverity` from `smart` module. The new `AlertSeverity` from `device_health_monitor` must be aliased as `HealthAlertSeverity` to avoid conflict.

## Verification
After editing, run:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | grep "^test result"
```

Expected: all tests pass (at least 716 unit tests + 28 proptest).

## Output format
Show the diff of changes made. Then show test results.
