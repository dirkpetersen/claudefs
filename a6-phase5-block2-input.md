# A6: Replication — Phase 5 Block 2: Operational Procedures & Documentation

**Date:** 2026-04-18
**Phase:** Phase 5 Block 2
**Target:** Operational Procedures, Failover Controller, Split-Brain Resolver, Ops Runbook
**Scope:** 3 new modules + 1 enhancement + comprehensive documentation
**Tests:** 20-24 tests, all in `#[cfg(test)]` sections within modules
**Success Criteria:** All tests pass, zero clippy warnings, failover <5s, zero data loss

---

## Architecture Context

### Replication Crate Structure

The `claudefs-repl` crate is well-established with 1,007 tests passing (Phase 5 Block 1 complete).

**Key existing modules:**
- `failover.rs` — Core failover logic with SiteMode enum (ActiveReadWrite, StandbyReadOnly, DegradedAcceptWrites, Offline)
- `split_brain.rs` — Split-brain detection with FencingToken and SplitBrainState enum
- `health.rs` — Health monitoring and status reporting
- `metrics.rs` — Prometheus metrics export (already integrated with Block 1)
- `pipeline.rs` — Replication pipeline with PipelineStats
- `lag_monitor.rs` — Replication lag tracking
- `conflict_resolver.rs` — Conflict resolution strategies (LWW)

**New modules to create:**
1. `failover_controller.rs` — Automated failover controller with health-check-based triggering
2. `ops_runbook.rs` — Operational state machine and procedures
3. Documentation files — `REPLICATION-OPERATIONS.md`, `REPLICATION-PROCEDURES.md`

**Module to enhance:**
- `split_brain.rs` — Add `SplitBrainResolver` struct with automated resolution

---

## Module 1: `failover_controller.rs` (NEW, ~250 lines)

### Purpose
Implement automated failover logic that triggers site failover based on health checks, maintains graceful degradation, and orchestrates recovery.

### Key Types & Traits

```rust
/// Failure tracking for a single site
#[derive(Debug, Clone)]
pub struct FailureTracker {
    /// Site ID
    pub site_id: u64,
    /// Consecutive failure count
    pub consecutive_failures: u32,
    /// Consecutive success count (for recovery)
    pub consecutive_successes: u32,
    /// Timestamp of last failure (nanoseconds)
    pub last_failure_ns: u64,
}

/// Failover controller state machine
#[derive(Debug, Clone, PartialEq)]
pub enum FailoverControllerState {
    /// Normal operation, all sites healthy
    Healthy,
    /// One or more sites degraded (failures detected but below threshold)
    Degraded,
    /// Primary site down, failover initiated
    FailoverInProgress,
    /// Active-active mode with both sites accepting writes
    ActiveActive,
    /// Single-site mode (replica down)
    SingleSite,
    /// Partial recovery: some sites recovering
    Recovering,
    /// Error state
    Error(String),
}

/// Main failover controller
#[derive(Debug)]
pub struct FailoverController {
    /// Configuration
    config: FailoverConfig,
    /// Per-site failure tracking
    trackers: HashMap<u64, FailureTracker>,
    /// Current controller state
    state: FailoverControllerState,
    /// Timestamp of last failover event (ns)
    last_failover_ts_ns: u64,
    /// Failover counter (for metrics)
    failover_count: u64,
}

impl FailoverController {
    /// Create a new controller with given config
    pub fn new(config: FailoverConfig) -> Self { /* ... */ }

    /// Record a health check success for a site
    pub fn record_success(&mut self, site_id: u64) -> Result<()> { /* ... */ }

    /// Record a health check failure for a site
    pub fn record_failure(&mut self, site_id: u64) -> Result<()> { /* ... */ }

    /// Check if failover should be triggered for a site
    pub fn should_failover(&self, site_id: u64) -> bool { /* ... */ }

    /// Check if recovery should be triggered for a site
    pub fn should_recover(&self, site_id: u64) -> bool { /* ... */ }

    /// Get current state
    pub fn state(&self) -> FailoverControllerState { /* ... */ }

    /// Get all active (non-failed) sites
    pub fn active_sites(&self) -> Vec<u64> { /* ... */ }

    /// Estimate failover time (milliseconds)
    pub fn estimated_failover_time_ms(&self) -> u64 {
        // Should be <5000ms (5 seconds)
        // Include: health check detection + quorum consensus + metadata switchover
    }

    /// Reset tracking for a site (used after recovery)
    pub fn reset_site(&mut self, site_id: u64) -> Result<()> { /* ... */ }
}
```

### FailoverConfig Enhancement
```rust
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Number of consecutive failures before demotion
    pub failure_threshold: u32,
    /// Number of consecutive successes before promotion
    pub recovery_threshold: u32,
    /// Health check interval in milliseconds
    pub check_interval_ms: u64,
    /// Enable active-active mode
    pub active_active: bool,
    /// Failover timeout in milliseconds (max time to complete failover)
    pub failover_timeout_ms: u64,  // NEW: default 5000
    /// Enable graceful degradation (continue single-site writes)
    pub graceful_degradation: bool,  // NEW: default true
    /// Minimum sites required for write quorum (default 1 for graceful)
    pub write_quorum_size: usize,  // NEW: default 1
}
```

### Test Cases (5-7 tests)

1. **test_failure_tracking_increments** — Verify consecutive_failures increments
2. **test_success_resets_counter** — Verify consecutive_successes increments, failures reset
3. **test_failover_trigger_on_threshold** — Verify should_failover returns true after N failures
4. **test_recovery_trigger_on_successes** — Verify should_recover returns true after M successes
5. **test_graceful_degradation_mode** — Verify controller stays healthy with one site down (graceful=true)
6. **test_active_sites_list** — Verify active_sites() returns correct non-failed sites
7. **test_failover_timing_estimate** — Verify estimated_failover_time_ms() < 5000

---

## Module 2: Enhanced `split_brain.rs` — Add `SplitBrainResolver`

### Purpose
Implement automated split-brain detection, resolution strategies (LWW, quorum-based, manual), and audit trail.

### Key New Types

```rust
/// Resolution strategies for split-brain
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Last-write-wins: accept the site with the highest journal sequence
    LastWriteWins,
    /// Quorum-based: accept writes from the majority partition
    QuorumBased,
    /// Manual: operator chooses which site to trust
    Manual { chosen_site_id: u64 },
}

/// Split-brain resolution event (for audit trail)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionEvent {
    /// Timestamp when resolution occurred (ns)
    pub resolved_at_ns: u64,
    /// Site A ID
    pub site_a: u64,
    /// Site B ID
    pub site_b: u64,
    /// Journal sequences where they diverged
    pub diverged_at_seq: u64,
    /// Strategy used
    pub strategy: ResolutionStrategy,
    /// Site chosen as source of truth
    pub chosen_site: u64,
    /// Entries reconciled (count)
    pub entries_reconciled: u64,
}

/// Split-brain resolver
#[derive(Debug)]
pub struct SplitBrainResolver {
    /// List of resolved events (audit trail)
    resolution_history: Vec<ResolutionEvent>,
    /// Current state
    state: SplitBrainState,
}

impl SplitBrainResolver {
    /// Create a new resolver
    pub fn new() -> Self { /* ... */ }

    /// Detect split-brain given two divergent journal sequences
    pub fn detect(
        &mut self,
        site_a: u64,
        site_b: u64,
        seq_a: u64,
        seq_b: u64,
    ) -> bool { /* ... */ }

    /// Resolve split-brain using the given strategy
    pub fn resolve(
        &mut self,
        strategy: ResolutionStrategy,
    ) -> Result<ResolutionEvent> { /* ... */ }

    /// Get resolution history
    pub fn history(&self) -> &[ResolutionEvent] { /* ... */ }

    /// Clear history (after backup)
    pub fn clear_history(&mut self) { /* ... */ }

    /// Get current state
    pub fn state(&self) -> &SplitBrainState { /* ... */ }
}
```

### Test Cases (5-6 tests)

1. **test_split_brain_detection_divergent_sequences** — Verify detect() returns true for divergent journals
2. **test_lww_resolution_chooses_higher_seq** — Verify LWW strategy chooses site with higher sequence
3. **test_quorum_resolution_requires_majority** — Verify quorum strategy rejects minority
4. **test_manual_resolution_accepts_chosen_site** — Verify manual strategy uses chosen_site_id
5. **test_resolution_event_audit_trail** — Verify resolution_history records all events
6. **test_resolver_state_transitions** — Verify state transitions: Normal → PartitionSuspected → Confirmed → Resolved

---

## Module 3: `ops_runbook.rs` (NEW, ~200 lines)

### Purpose
Implement operational state machine, common scenarios, and procedures.

### Key Types

```rust
/// Operational scenario types
#[derive(Debug, Clone, PartialEq)]
pub enum OperationalScenario {
    /// Primary site health check passes
    PrimarySiteHealthy,
    /// Primary site unresponsive
    PrimarySiteDown,
    /// Replica site unresponsive
    ReplicaSiteDown,
    /// Both sites responsive but lagging
    BothSitesLagging,
    /// Network partition (minority quorum)
    NetworkPartitionMinority,
    /// Network partition (majority quorum)
    NetworkPartitionMajority,
    /// Both sites recovered after partition
    BothSitesRecovered,
}

/// Operational procedure step
#[derive(Debug, Clone)]
pub struct ProcedureStep {
    /// Step number
    pub step: usize,
    /// Description
    pub description: String,
    /// Estimated duration (milliseconds)
    pub estimated_duration_ms: u64,
    /// Whether this step can be automated
    pub automatable: bool,
}

/// Operational runbook
#[derive(Debug)]
pub struct OperationalRunbook {
    /// Current scenario
    current_scenario: OperationalScenario,
    /// Procedure steps for current scenario
    steps: Vec<ProcedureStep>,
    /// Step index (which step we're on)
    current_step: usize,
}

impl OperationalRunbook {
    /// Create a new runbook
    pub fn new() -> Self { /* ... */ }

    /// Handle a scenario transition
    pub fn handle_scenario(&mut self, scenario: OperationalScenario) -> Vec<ProcedureStep> { /* ... */ }

    /// Get current procedure steps
    pub fn current_steps(&self) -> &[ProcedureStep] { /* ... */ }

    /// Advance to next step
    pub fn advance_step(&mut self) -> bool { /* ... */ }

    /// Get total estimated time for procedure (ms)
    pub fn total_estimated_time_ms(&self) -> u64 { /* ... */ }

    /// Get all recoverable from current scenario
    pub fn recovery_procedures(&self) -> Vec<ProcedureStep> { /* ... */ }
}
```

### Scenario Implementations

For each scenario, define:
- Detection conditions
- Procedure steps (with duration + automatable flag)
- Recovery actions
- Success criteria

**Example: PrimarySiteDown**
- Detection: 3+ consecutive health check failures on primary
- Steps:
  1. Verify replica is healthy (100ms, auto)
  2. Promote replica to primary (500ms, auto)
  3. Update DNS/routing (1000ms, manual for now)
  4. Verify client reconnection (2000ms, auto)
  5. Initiate primary recovery (on-demand, manual)
- Recovery: Wait for primary to recover, reconcile journals
- Success: Client writes confirmed on new primary, <5s total

### Test Cases (5-6 tests)

1. **test_scenario_primary_site_down** — Verify correct procedure steps generated
2. **test_procedure_step_ordering** — Verify steps are in correct order
3. **test_estimated_time_accuracy** — Verify total_estimated_time_ms() sums all steps
4. **test_advance_step_progression** — Verify advance_step() iterates through all steps
5. **test_recovery_procedures_exist** — Verify recovery_procedures() returns non-empty for all scenarios
6. **test_scenario_transitions** — Verify handle_scenario() transitions correctly between scenarios

---

## Documentation Files

### File 1: `docs/REPLICATION-OPERATIONS.md` (300+ lines)

**Sections:**
1. Quick Start
   - 30-second cluster overview
   - Health status check commands
   - Common alerts and quick fixes

2. Monitoring & Metrics
   - Replication lag tracking (from Block 1 metrics)
   - Quorum write latency percentiles
   - Split-brain event counters
   - Alert thresholds (warn >60s lag, critical >300s)

3. Failover Procedures
   - Automatic failover (what happens)
   - Manual failover (when to trigger)
   - Failover timing expectations (<5s)
   - Validation steps post-failover

4. Split-Brain Troubleshooting
   - Symptoms and detection
   - Automatic resolution (LWW, quorum)
   - Manual resolution (choosing a site)
   - Verification after resolution

5. Performance Tuning
   - Replication lag targets (per SLA)
   - Checkpoint frequency tuning
   - Compression settings
   - Network bandwidth optimization

6. SLA Definitions
   - RPO (recovery point objective): <1 minute
   - RTO (recovery time objective): <5 seconds
   - Consistency guarantees: Strong consistency post-failover
   - Availability: 99.95% (planned downtime excluded)

7. Common Scenarios & Remediation
   - "Replication is lagging behind" → actions
   - "Split-brain detected" → resolution steps
   - "One site is slow" → diagnosis + tuning
   - "Cluster lost quorum" → recovery from S3

### File 2: `docs/REPLICATION-PROCEDURES.md` (200+ lines)

**Sections:**
1. Step-by-Step Failover
   - Prerequisites checklist
   - Commands to execute (with expected output)
   - Verification steps
   - Rollback procedures

2. Manual Recovery Procedures
   - Bringing a failed site back online
   - Catching up a lagging site
   - Force-syncing journals
   - Handling persistent failures

3. Disaster Recovery (Dual-Site Failure)
   - Recovery from backup/snapshot
   - Using S3-backed copy
   - Timeline expectations
   - Data verification

4. Validation Checklists
   - Post-failover validation
   - Post-recovery validation
   - Health check verification
   - Performance baseline validation

---

## Integration Points

1. **With existing `failover.rs`:**
   - `FailoverController` orchestrates state transitions
   - Uses existing `SiteMode` enum from `failover.rs`
   - Calls existing health check APIs

2. **With existing `split_brain.rs`:**
   - Enhance with `SplitBrainResolver` (new)
   - Keep existing `SplitBrainState` unchanged
   - Add `ResolutionEvent` for audit trail

3. **With metrics.rs (Block 1):**
   - Export failover event count
   - Export split-brain resolution count
   - Export failover latency histogram

4. **With health.rs:**
   - FailoverController uses health check results
   - Updates health status based on failover state

5. **With A8 Management (future):**
   - Procedures documented for Grafana dashboard
   - Alert integration (split-brain → SNS notification)
   - CLI commands (`cfs admin failover status`)

---

## Test Requirements

### All tests in `#[cfg(test)]` sections

1. **FailoverController tests (7 tests)**
   - Threshold crossing, state transitions, metrics

2. **SplitBrainResolver tests (6 tests)**
   - Detection, resolution strategies, audit trail

3. **OperationalRunbook tests (6 tests)**
   - Scenario handling, step progression, timing

4. **Integration tests (3-4 tests)**
   - FailoverController + SplitBrainResolver together
   - Recovery scenarios
   - Timing validation

### Test Framework
- Use `#[tokio::test]` for async tests
- Use `proptest` for property-based tests where applicable
- Mock external dependencies (health checks, network calls)
- All tests must pass with `cargo test --release`
- Zero clippy warnings

---

## Success Criteria

1. ✅ All 20-24 tests pass (`cargo test --release`)
2. ✅ Zero clippy warnings
3. ✅ Code compiles cleanly (`cargo build`)
4. ✅ Failover time <5 seconds (verified in tests)
5. ✅ Zero data loss during failover (design verified)
6. ✅ Documentation is comprehensive and procedural
7. ✅ Integration with existing modules seamless

---

## Implementation Notes

- Follow existing `claudefs-repl` patterns:
  - Error handling with `thiserror` and `anyhow`
  - Async with `tokio`
  - Serialization with `serde` + `bincode`
  - Testing with `proptest` + `#[tokio::test]`

- Naming conventions:
  - Structs: `FailoverController`, `SplitBrainResolver`, `OperationalRunbook`
  - Methods: `pub fn method_name(...)`
  - Tests: `test_specific_behavior_scenario`

- Code quality:
  - All public items documented with `///` comments
  - All public types and functions exported in `lib.rs`
  - Modules in `src/failover_controller.rs`, `src/ops_runbook.rs`, enhanced `src/split_brain.rs`

---

## Deliverables Summary

| Item | Type | Lines | Tests |
|------|------|-------|-------|
| `failover_controller.rs` | Module | ~250 | 7 |
| `ops_runbook.rs` | Module | ~200 | 6 |
| Enhanced `split_brain.rs` | Module | +150 | 6 |
| Integration tests | Module | ~100 | 3-4 |
| `REPLICATION-OPERATIONS.md` | Doc | 300+ | — |
| `REPLICATION-PROCEDURES.md` | Doc | 200+ | — |
| **TOTAL** | | ~1,200 | 22-24 |

---

## Files to Create/Modify

1. **Create:** `/home/cfs/claudefs/crates/claudefs-repl/src/failover_controller.rs`
2. **Create:** `/home/cfs/claudefs/crates/claudefs-repl/src/ops_runbook.rs`
3. **Modify:** `/home/cfs/claudefs/crates/claudefs-repl/src/split_brain.rs` (add SplitBrainResolver + tests)
4. **Modify:** `/home/cfs/claudefs/crates/claudefs-repl/src/lib.rs` (add module exports)
5. **Create:** `/home/cfs/claudefs/docs/REPLICATION-OPERATIONS.md`
6. **Create:** `/home/cfs/claudefs/docs/REPLICATION-PROCEDURES.md`

---

## Rust Compiler Expectations

- `cargo build -p claudefs-repl` — should succeed
- `cargo test --release -p claudefs-repl` — all 1,027+ tests pass (1,007 existing + 20-24 new)
- `cargo clippy -p claudefs-repl` — zero warnings
- All public APIs properly exported and documented

---

End of specification.
