# A6 Phase 4: Active-Active HA — Implementation Prompt

**Target:** Implement 4 core modules for active-active replication with quorum writes, read-repair, causal ordering, and HA orchestration.

**Baseline:** Phase 3 has 878 tests and 45 modules covering journal replication, conflict resolution, and failover.

**Goal:** Add 300-320 new tests across 4 modules → ~1200 total tests

---

## Module 1: `write_aware_quorum.rs` (22-26 tests)

**Purpose:** Quorum-based write coordination for multi-site writes.

**Key Concepts:**
- Write votes from remote sites
- Quorum formation (majority, all, custom)
- Split-brain detection
- Timeout handling

**Types to implement:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumType {
    Majority,
    All,
    Custom(usize),
}

#[derive(Debug, Clone)]
pub struct WriteQuorumConfig {
    pub quorum_type: QuorumType,
    pub timeout_ms: u64,
    pub site_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    pub site_id: u32,
    pub shard_id: u32,
    pub seq: u64,
    pub data: Vec<u8>,
    pub client_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct WriteResponse {
    pub quorum_acks: Vec<u32>,
    pub write_ts: u64,
    pub committing_site: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteVoteResult {
    Accepted,
    Rejected,
    Timeout,
}

pub struct QuorumMatcher {
    votes: HashMap<u32, WriteVoteResult>,
    config: WriteQuorumConfig,
}
```

**Methods to implement:**

```rust
impl WriteQuorumConfig {
    pub fn new(quorum_type: QuorumType, timeout_ms: u64, site_count: usize) -> Self;
    pub fn is_valid(&self) -> bool;
}

impl QuorumMatcher {
    pub fn new(config: WriteQuorumConfig) -> Self;
    pub fn add_vote(&mut self, site_id: u32, result: WriteVoteResult) -> bool;
    pub fn is_satisfied(&self) -> bool;
    pub fn pending_sites(&self) -> Vec<u32>;
    pub fn detect_split_brain(&self) -> Option<String>; // Returns reason if detected
    pub fn timed_out(&self, elapsed_ms: u64) -> bool;
}
```

**Test Coverage (22-26 tests):**
1. Quorum formation (majority)
2. Quorum formation (all)
3. Quorum formation (custom)
4. Satisfaction checks
5. Timeout handling
6. Split-brain detection (2-site conflict)
7. Split-brain detection (3-site conflict)
8. Vote idempotency
9. Dynamic site removal
10. Serialization round-trip
11. Config validation
12. Empty votes
13. All rejected
14. Single site
15. Large site count
16. Concurrent vote updates
17. Partial satisfaction
18. Vote counting accuracy
19. Config clone
20. Edge cases

---

## Module 2: `read_repair_coordinator.rs` (20-24 tests)

**Purpose:** Anti-entropy repair across replicas when values diverge.

**Types to implement:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRepairPolicy {
    Immediate,
    Deferred,
    Adaptive,
}

#[derive(Debug, Clone)]
pub struct ReadContext {
    pub read_id: String,
    pub site_ids: Vec<u32>,
    pub timestamp: u64,
    pub consistency_level: ConsistencyLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsistencyLevel {
    Strong,
    Eventual,
    Causal,
}

#[derive(Debug, Clone)]
pub struct ReadValue {
    pub value: Vec<u8>,
    pub version: u64,
    pub site_id: u32,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairAction {
    NoRepair,
    PatchMinority,
    PatchMajority,
    FullSync,
}

pub struct ReadRepairCoordinator {
    policy: ReadRepairPolicy,
    max_sites: usize,
}
```

**Methods to implement:**

```rust
impl ReadRepairCoordinator {
    pub fn new(policy: ReadRepairPolicy, max_sites: usize) -> Self;

    pub fn detect_divergence(&self, values: &[ReadValue]) -> bool;
    pub fn compute_repair_action(
        &self,
        values: &[ReadValue],
        site_count: usize,
    ) -> RepairAction;

    pub fn find_consensus(&self, values: &[ReadValue]) -> Option<ReadValue>;
    pub fn select_repair_targets(
        &self,
        action: RepairAction,
        values: &[ReadValue],
    ) -> Vec<u32>;

    pub fn is_idempotent(&self, action: RepairAction) -> bool;
}
```

**Test Coverage (20-24 tests):**
1. Consensus (unanimous)
2. Consensus (majority)
3. Consensus (minority)
4. Divergence detection
5. No divergence
6. Repair action: no repair
7. Repair action: patch minority
8. Repair action: patch majority
9. Repair action: full sync
10. Immediate policy
11. Deferred policy
12. Adaptive policy
13. Strong consistency
14. Eventual consistency
15. Causal consistency
16. Empty values list
17. Single value
18. Timestamp ordering
19. Version precedence
20. Idempotency checks
21. Error handling
22. Large value sets

---

## Module 3: `vector_clock_replication.rs` (24-28 tests)

**Purpose:** Causal consistency tracking via vector clocks.

**Types to implement:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorClock {
    clock: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct CausalEntry {
    pub vector_clock: VectorClock,
    pub operation_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CausalQueue {
    entries: Vec<CausalEntry>,
    pending: HashMap<String, Vec<CausalEntry>>,
}
```

**Methods to implement:**

```rust
impl VectorClock {
    pub fn new() -> Self;
    pub fn from_map(clock: HashMap<String, u64>) -> Self;
    pub fn increment(&mut self, node_id: &str);
    pub fn merge(&mut self, other: &VectorClock);
    pub fn happens_before(&self, other: &VectorClock) -> bool;
    pub fn concurrent(&self, other: &VectorClock) -> bool;
    pub fn to_bytes(&self) -> Result<Vec<u8>>;
    pub fn from_bytes(data: &[u8]) -> Result<Self>;
}

impl CausalQueue {
    pub fn new() -> Self;
    pub fn enqueue(&mut self, entry: CausalEntry) -> Result<()>;
    pub fn dequeue(&mut self) -> Option<CausalEntry>;
    pub fn pending_count(&self) -> usize;
    pub fn detect_cycles(&self) -> Option<Vec<String>>;
    pub fn apply_timeout(&mut self, timeout_ms: u64) -> Vec<CausalEntry>;
}
```

**Test Coverage (24-28 tests):**
1. Vector clock creation
2. Increment operation
3. Merge operation
4. Happens-before (strictly)
5. Happens-before (not)
6. Concurrent detection
7. Concurrent not-concurrent
8. Clock equality
9. Clock ordering
10. Causal chain (3 ops)
11. Out-of-order buffering
12. Partial order detection
13. Dequeue respects order
14. Timeout on stuck
15. Multi-site VC
16. VC serialization
17. VC round-trip
18. Empty queue
19. Single entry
20. Cycle detection
21. No cycle
22. Large clock
23. Clock merge commutative
24. Causality consistency

---

## Module 4: `dual_site_orchestrator.rs` (26-32 tests)

**Purpose:** High-level HA orchestration combining all Phase 4 components.

**Types to implement:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone)]
pub struct SiteStatus {
    pub site_id: u32,
    pub health: HealthStatus,
    pub last_seen: u64,
    pub version: u64,
    pub reachable: bool,
}

pub struct OrchestratorConfig {
    pub quorum_type: QuorumType,
    pub read_repair_policy: ReadRepairPolicy,
    pub write_timeout_ms: u64,
    pub health_check_interval_ms: u64,
}

pub struct DualSiteOrchestrator {
    local_site_id: u32,
    remote_site_id: u32,
    config: OrchestratorConfig,
    sites: HashMap<u32, SiteStatus>,
    quorum_matcher: QuorumMatcher,
    read_repair: ReadRepairCoordinator,
    causal_queue: CausalQueue,
}
```

**Methods to implement:**

```rust
impl DualSiteOrchestrator {
    pub fn new(
        local_id: u32,
        remote_id: u32,
        config: OrchestratorConfig,
    ) -> Self;

    pub fn on_local_write(
        &mut self,
        shard_id: u32,
        seq: u64,
        data: Vec<u8>,
    ) -> Result<WriteResponse>;

    pub fn on_remote_write(
        &mut self,
        req: WriteRequest,
    ) -> Result<()>;

    pub fn on_local_read(
        &mut self,
        shard_id: u32,
        key: &str,
    ) -> Result<Vec<u8>>;

    pub fn periodic_health_check(&mut self) -> Vec<SiteStatus>;
    pub fn handle_remote_failure(&mut self, reason: &str) -> Result<()>;
    pub fn detect_and_resolve_split_brain(&mut self) -> Option<String>;
    pub fn get_replication_lag(&self) -> u64;
    pub fn get_site_status(&self, site_id: u32) -> Option<SiteStatus>;
}
```

**Test Coverage (26-32 tests):**
1. Initialization (both healthy)
2. Local write success
3. Local write with remote down
4. Remote write success
5. Read from primary
6. Read from replica
7. Read-repair trigger
8. Failover on remote failure
9. Recovery on remote restore
10. Split-brain detection
11. Split-brain resolution
12. Health check update
13. Lag calculation
14. Concurrent writes
15. Causality ordering
16. Write in degraded mode
17. Read in degraded mode
18. Config validation
19. State persistence
20. Remote site removal
21. Remote site addition
22. Quorum timeout handling
23. Write response generation
24. Metrics export (lag, quorum, health)
25. Error handling (bad config)
26. Error handling (timeout)
27. Integration: full write cycle
28. Integration: full read cycle

---

## Implementation Notes

**Crate dependencies:**
- Use `thiserror` for error types
- Use `serde` for serialization (bincode for wire format)
- Use `tokio` for async (but orchestrator methods can be sync)
- All modules should be `#[cfg(test)]`-friendly with unit tests

**Safety:**
- All code should be safe Rust (100% safe)
- No unsafe blocks needed
- Thread-safe collections where needed (HashMap is fine for tests)

**Module structure:**
- Each module in `crates/claudefs-repl/src/` as a separate `.rs` file
- Tests in `#[cfg(test)]` mod within each file
- Export all public types in `lib.rs`
- Ensure `cargo test --package claudefs-repl` passes

**Integration with existing modules:**
- Import existing types from `error.rs`, `types.rs` (if needed)
- Use `tracing` for debug logs
- Metrics should be added to `metrics.rs` (not required for tests but good practice)

---

## Success Criteria

✅ All 4 modules compile without warnings
✅ All 300+ new tests pass
✅ `cargo clippy` runs clean
✅ Total test count: 878 + 300+ = 1200+ tests
✅ `cargo test --package claudefs-repl` shows all pass

