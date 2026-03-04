# Rewrite: claudefs-tests/src/crash.rs — extend to support enum-based crash injection API

## Problem

The new `crash_consistency_tests.rs` module was added to `lib.rs` but uses an
extended API for `CrashSimulator`, `CrashPoint`, `CrashReport`, and
`CrashConsistencyTest` that doesn't exist yet in `crash.rs`.

The existing `crash.rs` has `CrashPoint` as a struct with `offset` and
`description` fields (for file-offset based crash injection). The new tests
expect `CrashPoint` to be an enum with write-path semantics.

## Required Changes

Rewrite `crash.rs` to support both:
1. The NEW enum-based API (for `crash_consistency_tests.rs`)
2. The EXISTING API (for backward compatibility with tests already in `crash.rs`)

## New API Requirements (from crash_consistency_tests.rs)

### 1. `CrashPoint` must become an enum:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum CrashPoint {
    BeforeWrite,
    AfterWrite,
    DuringFlush,
    AfterFlush,
    DuringReplication,
    AfterReplication,
    // Keep a Custom variant for backward compatibility with old struct-based usage
    Custom { offset: u64, description: String },
}
```

### 2. `CrashError` must add `SimulatedCrash` variant:

```rust
pub enum CrashError {
    IoError(String),
    SimulationFailed(String),
    VerificationFailed(String),
    SimulatedCrash { at: CrashPoint },  // NEW
}
```

### 3. `CrashSimulator` must add these methods:

```rust
impl CrashSimulator {
    // Existing methods: new(), with_crash_point(), simulate_crash_at(), run_test(), with_crash_points()

    // NEW methods:
    pub fn set_crash_point(&mut self, point: CrashPoint)
    pub fn clear_crash_point(&mut self)
    pub fn should_crash(&self, point: CrashPoint) -> bool
    pub fn simulate_write_path(&self, data: &[u8]) -> Result<CrashReport, CrashError>
}
```

`set_crash_point` sets the current configured crash point (one at a time).
`clear_crash_point` removes the configured crash point.
`should_crash(point)` returns true if the configured crash point matches `point`.
`simulate_write_path(data)` simulates a write and returns a CrashReport. If a
crash point is set, returns `Err(CrashError::SimulatedCrash { at: crash_point })`.
When no crash point is set, returns `Ok(CrashReport { crash_point: None (encode as dummy), recovery_success: true, data_consistent: true, repaired_entries: 0 })`.

### 4. `CrashReport` must change to support per-write reports:

```rust
#[derive(Debug, Clone)]
pub struct CrashReport {
    pub crash_point: CrashPoint,
    pub recovery_success: bool,
    pub data_consistent: bool,
    pub repaired_entries: usize,
}
```

Note: `CrashReport` needs to keep the existing `CrashReport` (test_name/crash_points_tested/recoveries_succeeded/recoveries_failed) for the `run_test` method. Name the old one `CrashTestReport` to avoid conflict.

### 5. `CrashConsistencyTest` must change:

The new `CrashConsistencyTest` takes a `CrashSimulator` and has:
- `CrashConsistencyTest::new(simulator: CrashSimulator) -> Self`
- `fn run(&mut self) -> Result<(), CrashError>` — runs one test scenario, appends to results
- `fn results(&self) -> &[CrashReport]` — returns accumulated results

## Backward Compatibility

Keep `CrashPoint::new(offset: u64, description: &str) -> Self` as a convenience:
```rust
impl CrashPoint {
    pub fn new(offset: u64, description: &str) -> Self {
        CrashPoint::Custom { offset, description: description.to_string() }
    }
}
```

Keep `CrashSimulator::simulate_crash_at()` taking `&CrashPoint` (where CrashPoint::Custom carries the offset).

For `run_test()`, return a `CrashTestReport` (renamed from the old `CrashReport`).

## Public API from lib.rs that must be kept

The `lib.rs` exports:
```rust
pub use crash::{CrashConsistencyTest, CrashError, CrashPoint, CrashReport, CrashSimulator};
```

All five names must still be exported. `CrashReport` should be the new per-write report struct (crash_point, recovery_success, data_consistent, repaired_entries). The old `CrashTestReport` (with test_name etc.) is internal.

## File to modify

`crates/claudefs-tests/src/crash.rs`

## Constraints

1. All tests in `crash_consistency_tests.rs` must compile and pass
2. All existing tests inside `crash.rs` (in the `#[cfg(test)]` block) must still pass — update them to use the new `CrashPoint::Custom { offset, description }` variant or `CrashPoint::new(offset, description)` where they use the old struct API
3. The `should_crash` method compares using PartialEq on CrashPoint enum variants
4. `simulate_write_path` with no crash point set returns `Ok(CrashReport { crash_point: CrashPoint::BeforeWrite, recovery_success: true, data_consistent: true, repaired_entries: 0 })` — use BeforeWrite as default sentinel
5. The `CrashConsistencyTest::run()` appends one `CrashReport` to internal results each call

Please output the complete rewritten `crash.rs` file.
