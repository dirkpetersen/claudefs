# A6: Replication — Phase 4 Implementation Prompt

## Context

You are generating Rust code for the **claudefs-repl** crate (cross-site journal replication). This is Phase 4 of the replication subsystem, focusing on **Active-Active Failover and High-Availability** improvements.

### Current State
- **Crate:** `crates/claudefs-repl/src/`
- **Existing modules:** 45 modules, 878 tests passing (Phase 3 complete)
- **Key existing traits & types:**
  - `error::ReplError` — standard error type
  - `wal::WriteAheadLog`, `wal::Cursor` — WAL abstraction
  - `journal::JournalEntry`, `journal::JournalRecord` — journal primitives
  - `conflict_resolver::ConflictResolver`, `conflict_resolver::ResolutionPolicy::LastWriteWins`
  - `split_brain::SplitBrainDetector`, `split_brain::SplitBrainStatus`
  - `failover::FailoverManager`, `site_failover::SiteFailoverCoordinator`
  - `active_active::ActiveActiveReplica` — existing active-active stub
  - `conduit::ConduitClient`, `conduit::RemoteSite` — gRPC communication
  - `tracing::*` — distributed tracing

### Phase 4 Goal
Implement **4 core modules** for active-active failover and HA:
1. `write_aware_quorum.rs` — Quorum-based write coordination
2. `read_repair_coordinator.rs` — Anti-entropy read-repair
3. `vector_clock_replication.rs` — Causal consistency tracking
4. `dual_site_orchestrator.rs` — High-level HA orchestration

**Target:** 280-320 new tests across 4 modules (total → ~1158 tests)

## Constraints & Conventions

### Error Handling
- Use `thiserror` crate; define error types as enums inside each module
- Pattern: `type Result<T> = std::result::Result<T, <YourError>>`
- Implement `From<ReplError>` for conversion

### Async Runtime
- All async functions use `tokio` runtime with `.await`
- Timing-sensitive tests: use explicit `tokio::time::sleep` or test helpers
- No blocking operations in async code

### Testing
- Unit tests in `#[cfg(test)] mod tests { ... }` at bottom of each module
- Property-based tests using `proptest` for data transforms
- Test names: `test_<feature>_<scenario>` (e.g., `test_quorum_formation_majority_consensus`)

### Code Quality
- All public functions have doc comments (`/// ...`)
- No `unsafe` code (unless absolutely necessary, then document thoroughly)
- No clippy warnings allowed
- Follow Rust naming conventions (snake_case for functions/vars, CamelCase for types)

### Dependencies
- `serde` for serialization (if needed)
- `tokio` for async
- `tracing` for logging
- `thiserror` for error types
- `uuid` for unique IDs
- No new external crates without explicit approval

## Module Specifications

### Module 1: `write_aware_quorum.rs`

**Purpose:** Enforce write consensus across sites before acknowledging to client. Implements quorum-based voting.

**Key Types:**

```rust
pub enum QuorumType {
    Majority,           // Need >50% of sites
    All,                // Need 100% of sites
    Custom(usize),      // Need exactly N sites
}

pub struct WriteQuorumConfig {
    pub quorum_type: QuorumType,
    pub timeout_ms: u64,
    pub site_count: usize,
}

pub struct WriteRequest {
    pub site_id: String,
    pub shard_id: u32,
    pub seq: u64,
    pub data: Vec<u8>,
    pub client_id: String,
    pub timestamp: u64,
}

pub struct WriteResponse {
    pub quorum_acks: Vec<String>,  // Site IDs that acked
    pub write_ts: u64,              // Timestamp assigned by quorum
    pub committing_site: String,    // Which site is primary
}

pub enum WriteVoteResult {
    Accepted { commit_ts: u64 },
    Rejected { reason: String },
    Timeout,
}

pub struct QuorumMatcher {
    votes: std::collections::HashMap<String, WriteVoteResult>,
    config: WriteQuorumConfig,
}

impl QuorumMatcher {
    pub fn new(config: WriteQuorumConfig) -> Self { ... }
    pub fn add_vote(&mut self, site_id: String, vote: WriteVoteResult) { ... }
    pub fn is_quorum_satisfied(&self) -> bool { ... }
    pub fn detect_split_brain(&self) -> Option<SplitBrainVote> { ... }
}

pub struct SplitBrainVote {
    pub conflicting_sites: Vec<(String, WriteVoteResult)>,
    pub severity: SplitBrainSeverity,
}

pub enum SplitBrainSeverity {
    Low,      // Minority disagreement
    High,     // Majority split
    Critical, // Quorum impossible
}
```

**Responsibilities:**
- Accept write votes from remote sites
- Track quorum formation (majority/all/custom)
- Detect split-brain scenarios (conflicting votes)
- Timeout handling for slow sites
- Provide consistency guarantees

**Test Coverage** (22-26 tests):
- `test_quorum_formation_majority` — 2 sites vote, majority is 2/2
- `test_quorum_satisfied_at_majority` — 3 sites, 2 votes → satisfied
- `test_quorum_not_satisfied_minority` — 3 sites, 1 vote → not satisfied
- `test_quorum_all_type` — All sites required, one missing → not satisfied
- `test_quorum_custom_type` — Custom(2) required, 2 votes → satisfied
- `test_timeout_handling` — Write vote expires after timeout_ms
- `test_split_brain_detection_two_way` — 2 sites with conflicting votes
- `test_split_brain_detection_three_way` — 3 sites with conflicting decisions
- `test_split_brain_severity_classification` — Severity escalation
- `test_add_vote_idempotent` — Adding same vote twice
- `test_remove_site_updates_quorum` — Dynamic site removal
- `test_write_response_serialization` — Serde round-trip
- `test_consistency_timestamp_ordering` — Later votes have higher ts
- `test_conflicting_votes_same_site` — Site changes vote (newest wins)
- And 8-12 more edge cases

**Integration Points:**
- `split_brain::SplitBrainDetector` — cross-check detected splits
- `conflict_resolver::ConflictResolver` — resolve conflicting writes
- `conduit::ConduitClient` — send/receive votes
- `journal::JournalEntry` — log write votes

---

### Module 2: `read_repair_coordinator.rs`

**Purpose:** On read, repair diverged state across sites (read-repair anti-entropy). Implements Dynamo-style read-repair.

**Key Types:**

```rust
pub struct ReadRepairRequest {
    pub key: String,
    pub shard_id: u32,
    pub required_sites: Vec<String>,  // Which sites to check
}

pub struct VersionedValue {
    pub value: Vec<u8>,
    pub version: u64,
    pub site_id: String,
    pub timestamp: u64,
}

pub struct RepairDiverged {
    pub current_version: u64,
    pub diverged_sites: std::collections::HashMap<String, u64>,  // site → their version
    pub latest_value: Vec<u8>,
}

pub enum RepairDecision {
    QuickRepair,   // Accept current value, async update others
    SlowRepair,    // Verify before accepting
}

pub enum RepairAction {
    UpdateAndPropagate { value: Vec<u8>, new_version: u64 },
    ConflictResolve { resolution: String },
    Verify { sites_to_check: Vec<String> },
}

pub struct ReadRepairCoordinator {
    versions: std::collections::HashMap<String, VersionedValue>,
    config: ReadRepairConfig,
}

pub struct ReadRepairConfig {
    pub repair_threshold_ms: u64,  // Max time to wait for slow sites
    pub aggressive: bool,          // If true, always repair; else only on divergence
}

impl ReadRepairCoordinator {
    pub fn new(config: ReadRepairConfig) -> Self { ... }
    pub fn add_read_response(&mut self, site_id: String, value: VersionedValue) { ... }
    pub fn detect_divergence(&self) -> Option<RepairDiverged> { ... }
    pub fn decide_repair_strategy(&self) -> RepairDecision { ... }
    pub fn generate_repair_actions(&self) -> Vec<RepairAction> { ... }
}
```

**Responsibilities:**
- Collect read responses from multiple sites
- Detect version divergence
- Decide whether to repair (QuickRepair vs SlowRepair)
- Generate repair actions (update, resolve, verify)
- Track repair statistics

**Test Coverage** (20-24 tests):
- `test_no_divergence_detected` — All sites same version
- `test_divergence_two_versions` — 2 sites, 2 different versions
- `test_divergence_three_versions` — 3 sites, 3 different versions
- `test_quick_repair_decision` — Minority diverged → QuickRepair
- `test_slow_repair_decision` — Majority diverged → SlowRepair
- `test_repair_action_update_propagate` — Generate update + propagate action
- `test_repair_action_conflict_resolve` — Conflicting values
- `test_repair_action_verify` — Slow site needs verification
- `test_timeout_repair_incomplete` — Some sites don't respond in time
- `test_read_repair_idempotent` — Repairing same divergence twice
- `test_version_comparison_chronological` — Later timestamp wins
- `test_repair_statistics_tracking` — Count repairs, successes
- `test_aggressive_mode_always_repairs` — Config flag
- `test_passive_mode_only_divergence` — Config flag
- `test_concurrent_repairs_same_key` — Multiple repairs in flight
- And 5-9 more edge cases

**Integration Points:**
- `conflict_resolver::ConflictResolver` — resolve conflicting versions
- `conduit::ConduitClient` — fetch reads, propagate repairs
- `metrics::*` — export repair statistics

---

### Module 3: `vector_clock_replication.rs`

**Purpose:** Track causal history with vector clocks for stronger consistency. Enables causal consistency guarantee.

**Key Types:**

```rust
pub type VectorClockMap = std::collections::BTreeMap<String, u64>;  // site_id → version

pub struct VectorClock {
    clocks: VectorClockMap,
}

pub enum ClockOrdering {
    Before,      // self < other
    After,       // self > other
    Concurrent,  // neither < nor >
    Equal,       // self == other
}

pub struct VectorTimestamp {
    pub vector_clock: VectorClock,
    pub lamport_ts: u64,  // For total ordering when vector clocks concurrent
}

pub struct CausalDependency {
    pub operation_id: String,
    pub site_id: String,
    pub dependency_ts: VectorTimestamp,
}

pub struct OrderedOperation {
    pub op_id: String,
    pub data: Vec<u8>,
    pub timestamp: VectorTimestamp,
    pub dependencies: Vec<CausalDependency>,
}

pub struct CausalityValidator {
    delivered: Vec<VectorTimestamp>,  // Already-delivered operations
    config: CausalityConfig,
}

pub struct CausalityConfig {
    pub max_buffer_size: usize,
    pub timeout_ms: u64,
}

pub struct OrderingBuffer {
    pending: Vec<OrderedOperation>,
    delivered: Vec<VectorTimestamp>,
    validator: CausalityValidator,
}

impl VectorClock {
    pub fn new() -> Self { ... }
    pub fn increment(&mut self, site_id: &str) { ... }
    pub fn merge(&self, other: &VectorClock) -> VectorClock { ... }
    pub fn compare(&self, other: &VectorClock) -> ClockOrdering { ... }
}

impl CausalityValidator {
    pub fn new(config: CausalityConfig) -> Self { ... }
    pub fn is_causally_ready(&self, op: &OrderedOperation) -> bool { ... }
    pub fn mark_delivered(&mut self, ts: VectorTimestamp) { ... }
}

impl OrderingBuffer {
    pub fn new(config: CausalityConfig) -> Self { ... }
    pub fn add_operation(&mut self, op: OrderedOperation) { ... }
    pub fn try_deliver(&mut self) -> Vec<OrderedOperation> { ... }
}
```

**Responsibilities:**
- Create and merge vector clocks
- Compare vector clocks (before/after/concurrent/equal)
- Validate causal dependencies
- Buffer out-of-order operations until causal deps satisfied
- Track lamport timestamps for total ordering

**Test Coverage** (18-22 tests):
- `test_vector_clock_new_empty` — New clock all zeros
- `test_vector_clock_increment_single_site` — site_a increments
- `test_vector_clock_increment_multiple_sites` — Multiple sites, independent
- `test_vector_clock_merge_combines` — Merge takes max per site
- `test_clock_ordering_before` — VC1 strictly before VC2
- `test_clock_ordering_after` — VC1 strictly after VC2
- `test_clock_ordering_concurrent` — Neither before nor after
- `test_clock_ordering_equal` — Identical clocks
- `test_causality_validator_ready` — Operation with satisfied deps
- `test_causality_validator_not_ready` — Operation with unsatisfied deps
- `test_ordering_buffer_in_order` — Operations delivered in order
- `test_ordering_buffer_out_of_order` — Operations reordered
- `test_ordering_buffer_timeout` — Pending operations timeout
- `test_lamport_timestamp_total_ordering` — Lamport breaks VC ties
- `test_concurrent_operations_lamport_order` — Concurrent ops, lamport order
- `test_causality_chain_delivery` — A → B → C delivered in order
- And 1-5 more edge cases

**Integration Points:**
- `journal::JournalEntry` — embed vector timestamp
- `conduit::ConduitClient` — exchange clocks with remote sites
- `conflict_resolver::ConflictResolver` — resolve concurrent writes

---

### Module 4: `dual_site_orchestrator.rs`

**Purpose:** High-level orchestration for true active-active (both sites accepting writes). Main HA interface.

**Key Types:**

```rust
pub enum ConsistencyLevel {
    Strong,   // Quorum write before ack
    Causal,   // Vector clock causal consistency
    Eventual, // Local write before ack, async replicate
}

pub struct DualSiteConfig {
    pub site_a: String,
    pub site_b: String,
    pub quorum_type: QuorumType,
    pub consistency_level: ConsistencyLevel,
    pub health_check_interval_ms: u64,
}

pub enum WritePathOrchestrator {
    Primary {
        preferred_site: String,
        consistency_level: ConsistencyLevel,
    },
    Dual {
        primary: String,
        secondary: String,
    },
}

pub enum ReadPathOrchestrator {
    Local,           // Read from local site only
    WithRepair,      // Read from any site, repair divergence
    StrongConsistent, // Read from quorum
}

pub enum FailoverPolicy {
    Automatic {
        detection_timeout_ms: u64,
        promotion_delay_ms: u64,
    },
    Manual,
}

pub struct HealthProbe {
    site_id: String,
    last_success: std::time::Instant,
    consecutive_failures: u32,
    status: SiteHealth,
}

pub enum SiteHealth {
    Healthy,
    Degraded { latency_ms: u64 },
    Unhealthy,
    Down,
}

pub struct DualSiteOrchestrator {
    config: DualSiteConfig,
    site_a_health: HealthProbe,
    site_b_health: HealthProbe,
    active_site: String,
    failover_policy: FailoverPolicy,
}

impl DualSiteOrchestrator {
    pub fn new(config: DualSiteConfig, failover_policy: FailoverPolicy) -> Self { ... }
    pub async fn write_orchestrated(&mut self, req: &WriteRequest) -> Result<WriteResponse, ReplError> { ... }
    pub async fn read_orchestrated(&mut self, key: &str) -> Result<Vec<u8>, ReplError> { ... }
    pub async fn probe_health(&mut self) -> Result<(), ReplError> { ... }
    pub async fn detect_failover_needed(&self) -> Option<String> { ... }  // Returns site to promote
    pub async fn execute_failover(&mut self, promote_site: String) -> Result<(), ReplError> { ... }
    pub fn get_active_site(&self) -> &str { ... }
}
```

**Responsibilities:**
- Route writes to primary site with quorum coordination
- Route reads with optional read-repair
- Monitor site health
- Detect failover conditions
- Execute automatic or manual failover
- Track failover metrics

**Test Coverage** (24-28 tests):
- `test_dual_site_config_creation` — Create orchestrator
- `test_write_orchestrated_strong_consistency` — Write with quorum
- `test_write_orchestrated_causal_consistency` — Write with vector clock
- `test_write_orchestrated_eventual_consistency` — Write local, async replicate
- `test_read_orchestrated_local` — Read from local only
- `test_read_orchestrated_with_repair` — Read with read-repair
- `test_read_orchestrated_strong_consistent` — Read from quorum
- `test_health_probe_success` — Health check succeeds
- `test_health_probe_timeout` — Health check timeout
- `test_health_status_degraded` — Status transitions to Degraded
- `test_health_status_unhealthy` — Status transitions to Unhealthy
- `test_failover_automatic_triggered` — Automatic failover on site down
- `test_failover_manual_triggered` — Manual failover
- `test_failover_recovery` — Failed site comes back, promotes back
- `test_asymmetric_failure_site_a_down` — Only site A fails
- `test_asymmetric_failure_site_b_down` — Only site B fails
- `test_switchover_graceful` — Graceful switchover to other site
- `test_recovery_after_partition_both_up` — Partition heals, both sites up
- `test_detect_failover_needed_site_a_down` — Detection logic
- `test_active_site_tracking` — Active site updated after failover
- `test_consecutive_failures_escalate_health` — Multiple failures → Unhealthy
- `test_single_failure_not_immediate_failover` — Single failure doesn't trigger failover
- `test_failover_with_pending_writes` — In-flight writes during failover
- And 1-4 more edge cases

**Integration Points:**
- `write_aware_quorum::QuorumMatcher` — coordinate writes
- `read_repair_coordinator::ReadRepairCoordinator` — repair on reads
- `vector_clock_replication::VectorClock` — causal consistency
- `failover::FailoverManager` — existing failover integration
- `health::*` — health monitoring
- `metrics::*` — export HA metrics

---

## Implementation Notes

1. **Error Types:** Each module defines its own error enum. No panics in production code.
2. **Async/Await:** All network I/O and coordination is async. Use `tokio::spawn` for background tasks.
3. **Tracing:** Use `#[instrument]` on public methods. Include `event!(...)` for state transitions.
4. **Testing:** Each test is independent. Use `tokio::test` for async tests. Mock remote sites with local state machines.
5. **Idempotency:** Design operations to be idempotent (can retry safely).
6. **No Panics:** Use `Result<T, Error>` for error handling. Never unwrap in production code.

## Deliverable Format

For each module, provide:
1. Complete `.rs` file with full implementation
2. Comprehensive test suite (22-28 tests per module)
3. Documentation comments for all public types and methods
4. No compiler warnings or clippy issues

Files to generate:
- `write_aware_quorum.rs`
- `read_repair_coordinator.rs`
- `vector_clock_replication.rs`
- `dual_site_orchestrator.rs`

After generation, these files will be placed in `crates/claudefs-repl/src/` and registered in `lib.rs`.

## Success Criteria

✅ All 4 modules compile without warnings
✅ 280-320 new tests pass (22-28 × 4)
✅ Integration with existing modules (split_brain, conflict_resolver, failover, etc.) verified
✅ Clean `cargo build` and `cargo test`
✅ No unsafe code
✅ All doc comments present
✅ Ready to commit as `[A6] Phase 4: Active-Active Failover & HA`

## Estimated Effort

- write_aware_quorum.rs: 25 min (core quorum logic + 26 tests)
- read_repair_coordinator.rs: 25 min (read-repair + 24 tests)
- vector_clock_replication.rs: 22 min (VC logic + 22 tests)
- dual_site_orchestrator.rs: 30 min (orchestration + 28 tests)

**Total:** ~100 min, ~320 tests, ~2500 lines of Rust code

