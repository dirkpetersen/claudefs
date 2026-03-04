# A9 Phase 2: Transport Module Tests

Write one new test module `transport_phase2_tests.rs` for the `claudefs-tests` crate, covering A4 (Transport) Phase 2 modules.

## Public APIs to Test

### 1. `claudefs_transport::multipath` — MultipathRouter

```rust
pub struct PathId(u64);
impl PathId {
    pub fn new(id: u64) -> Self;
    pub fn as_u64(self) -> u64;
}

pub enum PathState { Active, Degraded, Failed, Draining }

pub struct PathMetrics {
    pub latency_us: u64,
    pub min_latency_us: u64,
    pub jitter_us: u64,
    pub loss_rate: f64,
    pub bandwidth_bps: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors: u64,
    pub last_probe_us: u64,
}

pub struct PathInfo {
    pub id: PathId,
    pub name: String,
    pub state: PathState,
    pub metrics: PathMetrics,
    pub weight: u32,
    pub priority: u32,
}

pub enum PathSelectionPolicy { RoundRobin, LeastLoaded, WeightedRoundRobin, LowestLatency, Priority }

pub struct MultipathConfig {
    pub policy: PathSelectionPolicy,
    pub min_active_paths: usize,
    pub health_check_interval_ms: u64,
    pub max_consecutive_failures: u32,
    pub degraded_threshold_latency_us: u64,
}

pub struct MultipathStats {
    pub total_paths: usize,
    pub active_paths: usize,
    pub degraded_paths: usize,
    pub failed_paths: usize,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_errors: u64,
    pub failovers: u64,
}

pub enum MultipathError {
    NoPathsAvailable,
    PathNotFound(PathId),
    // ...
}

pub struct MultipathRouter { /* private */ }

impl MultipathRouter {
    pub fn new(config: MultipathConfig) -> Self;
    pub fn add_path(&mut self, name: String, weight: u32, priority: u32) -> PathId;
    pub fn remove_path(&mut self, id: PathId) -> bool;
    pub fn select_path(&mut self) -> Option<PathId>;
    pub fn record_success(&mut self, id: PathId, latency_us: u64, bytes: u64);
    pub fn record_failure(&mut self, id: PathId, bytes: u64);
    pub fn mark_failed(&mut self, id: PathId);
    pub fn mark_active(&mut self, id: PathId);
    pub fn active_paths(&self) -> Vec<PathId>;
    pub fn path_info(&self, id: PathId) -> Option<&PathInfo>;
    pub fn stats(&self) -> MultipathStats;
}
```

### 2. `claudefs_transport::flowcontrol` — FlowControl, SlidingWindow

```rust
pub struct FlowControlConfig {
    pub max_inflight_bytes: u64,
    pub max_inflight_requests: u32,
    pub burst_factor: f64,
}

pub struct FlowPermit {
    pub bytes: u64,
}

pub enum FlowControlState { Open, Throttled, Closed }

pub struct FlowControl { /* private atomic */ }
impl FlowControl {
    pub fn new(config: FlowControlConfig) -> Self;
    pub fn try_acquire(&self, bytes: u64) -> Option<FlowPermit>;
    pub fn state(&self) -> FlowControlState;
    pub fn inflight_requests(&self) -> u32;
    pub fn inflight_bytes(&self) -> u64;
    pub fn release(&self, bytes: u64);
    pub fn config(&self) -> &FlowControlConfig;
}
impl FlowPermit {
    pub fn bytes(&self) -> u64;
}

pub struct SlidingWindow { /* private */ }
impl SlidingWindow {
    pub fn new(window_size: u32) -> Self;
    pub fn advance(&self, sequence: u64) -> bool;
    pub fn can_send(&self) -> bool;
    pub fn ack(&self, sequence: u64);
    pub fn window_start(&self) -> u64;
    pub fn window_end(&self) -> u64;
    pub fn window_size(&self) -> u32;
    pub fn in_flight(&self) -> u32;
}
```

## Write `transport_phase2_tests.rs` with at least 55 tests

### Section 1: MultipathRouter (25 tests)
1. `test_multipath_new` — creates without panic
2. `test_add_path_returns_id` — add_path returns a PathId
3. `test_add_multiple_paths` — add 3 paths, stats.total_paths=3
4. `test_select_path_empty` — select_path on empty router returns None
5. `test_select_path_single` — select_path with one active path returns it
6. `test_active_paths_empty_initially` — active_paths() is empty on new router
7. `test_active_paths_after_add` — after adding a path, active_paths contains it
8. `test_path_info_returns_entry` — path_info returns correct name and weight
9. `test_path_info_missing` — path_info for unknown PathId returns None
10. `test_remove_path` — remove_path returns true for known path
11. `test_remove_path_missing` — remove_path returns false for unknown path
12. `test_remove_path_clears_active` — after remove, active_paths does not contain it
13. `test_record_success_updates_metrics` — after record_success, path bytes_sent increases
14. `test_record_failure_marks_failed` — record max failures, path becomes failed
15. `test_mark_failed_removes_from_active` — mark_failed removes path from active_paths
16. `test_mark_active_restores_to_active` — mark_active then mark_failed then mark_active restores
17. `test_stats_total_paths` — stats.total_paths matches added path count
18. `test_stats_active_paths` — stats.active_paths matches active count
19. `test_stats_failed_paths` — stats.failed_paths after mark_failed
20. `test_round_robin_selection` — 3 paths, select 6 times, each selected 2 times (approximately)
21. `test_path_id_as_u64` — PathId::new(42).as_u64() == 42
22. `test_path_state_default_active` — newly added path has Active state
23. `test_path_metrics_initial_zero` — new path has zero metrics
24. `test_config_defaults` — MultipathConfig has sensible defaults
25. `prop_add_remove_paths` — proptest: add N paths, remove them, active_paths is empty

### Section 2: FlowControl (20 tests)
1. `test_flow_control_new` — creates without panic
2. `test_try_acquire_below_limit` — try_acquire(100) when limit=1000 succeeds
3. `test_try_acquire_returns_permit_bytes` — permit.bytes() matches requested
4. `test_try_acquire_exceeds_limit` — try_acquire when at limit returns None
5. `test_inflight_bytes_after_acquire` — inflight_bytes increases after acquire
6. `test_inflight_requests_after_acquire` — inflight_requests increases after acquire
7. `test_release_decrements_inflight` — after release, inflight_bytes decreases
8. `test_state_open_initially` — initial state is Open
9. `test_state_closed_when_full` — state is Closed/Throttled when at capacity
10. `test_config_accessor` — config() returns the original config
11. `test_flow_permit_bytes` — FlowPermit.bytes() returns the correct value

### Section 3: SlidingWindow (20 tests)
1. `test_sliding_window_new` — creates with correct window_size
2. `test_window_size_accessor` — window_size() returns configured value
3. `test_initial_can_send` — can_send() is true initially (nothing in flight)
4. `test_window_start_initial` — window_start() is 0 initially
5. `test_window_end_initial` — window_end() == window_size initially
6. `test_advance_first_seq` — advance(0) returns true
7. `test_advance_within_window` — advance(window_size-1) returns true
8. `test_advance_beyond_window` — advance(window_size+1) returns false (window exceeded)
9. `test_in_flight_after_advance` — in_flight() increases after advance
10. `test_ack_decrements_in_flight` — ack(seq) decrements in_flight
11. `test_can_send_false_when_full` — advance window_size times, can_send returns false
12. `test_ack_slides_window` — ack allows advancing further
13. `test_window_boundaries_after_advance` — window_start and window_end track correctly
14. `test_multiple_advances` — multiple advances within window all succeed
15. `test_ack_out_of_order` — acking a later seq still decrements in_flight
16. `test_zero_window_size` — window_size(0) means no sends allowed
17. `test_window_size_32` — standard 32-sequence window works correctly
18. `test_in_flight_initial_zero` — in_flight() starts at 0
19. `test_advance_duplicate_seq` — advancing same seq twice returns false second time
20. `prop_sliding_window_invariant` — proptest: in_flight never exceeds window_size

## Imports

```rust
use claudefs_transport::multipath::{
    MultipathConfig, MultipathError, MultipathRouter, MultipathStats,
    PathId, PathInfo, PathMetrics, PathSelectionPolicy, PathState,
};
use claudefs_transport::flowcontrol::{FlowControl, FlowControlConfig, FlowControlState, FlowPermit, SlidingWindow};
use proptest::prelude::*;
```

Check what is actually exported from `claudefs_transport` by looking at `crates/claudefs-transport/src/lib.rs` and `crates/claudefs-transport/src/flowcontrol.rs` before writing, to ensure imports are correct.

## Output

Complete `transport_phase2_tests.rs` file. Organize tests in `#[cfg(test)]` with `tests` and `proptest_tests` submodules. Use `#[test]` for sync tests, `#[tokio::test]` for async.
