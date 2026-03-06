# A6: Replication — Phase 4: Active-Active Failover & HA

## Overview

Phase 4 of the ClaudeFS replication subsystem focuses on **Active-Active Failover and High-Availability** improvements. This is a competitive differentiator that sets ClaudeFS apart from VAST Data and Weka, enabling true write-on-both-sites capability.

**Status:** Planning Phase (ready for implementation)
**Target:** 280-320 new tests across 4-5 modules (total ~1158 tests)
**Current baseline:** 878 tests passing (Phase 3 complete)

## Phase 4 Goals

### 1. **Competitive Differentiation**
- Implement **write-aware quorum-based coordination** across sites
- Enable **read-repair anti-entropy** for automatic divergence healing
- Provide **causal consistency** tracking with vector clocks
- Implement **high-level HA orchestration** for true active-active

### 2. **Technical Objectives**
- Add 4-5 new production-ready modules
- Achieve ~950-1158 total tests
- Zero failing tests, clean build
- Full integration with existing replication modules

### 3. **Use Cases Enabled**

1. **Instant Failover:** Detect site failure in <1s, automatically promote secondary
2. **Causal Consistency:** Guarantee that if read sees write X, it won't see write Y that happened before X
3. **Read-Repair:** On read, automatically detect and heal diverged state across sites
4. **Dual-Site Writes:** Both sites accept writes independently with quorum coordination
5. **Zero Data Loss:** Quorum-based commit ensures no data loss on failover

## Architecture Design

### Module 1: `write_aware_quorum.rs` (22-26 tests)

**Purpose:** Enforce write consensus across sites before acknowledging to client.

**Key Types:**
```rust
pub enum QuorumType {
    Majority,        // Need >50% of sites
    All,             // Need 100% of sites
    Custom(usize),   // Need exactly N sites
}

pub struct WriteQuorumConfig {
    pub quorum_type: QuorumType,
    pub timeout_ms: u64,
    pub site_count: usize,
}

pub struct QuorumMatcher {
    votes: HashMap<String, WriteVoteResult>,
    config: WriteQuorumConfig,
}
```

**Responsibilities:**
- Accept write votes from remote sites
- Track quorum formation
- Detect split-brain scenarios
- Provide consistency guarantees

**Key Tests:**
- Quorum formation and satisfaction logic
- Split-brain detection (2-way, 3-way)
- Timeout handling
- Idempotent vote addition
- Dynamic site removal

**Integration Points:**
- `split_brain::SplitBrainDetector`
- `conflict_resolver::ConflictResolver`
- `conduit::ConduitClient`
- `journal::JournalEntry`

---

### Module 2: `read_repair_coordinator.rs` (20-24 tests)

**Purpose:** On read, repair diverged state across sites (Dynamo-style read-repair).

**Key Types:**
```rust
pub struct ReadRepairRequest {
    pub key: String,
    pub shard_id: u32,
    pub required_sites: Vec<String>,
}

pub enum RepairDecision {
    QuickRepair,   // Accept current value, async update others
    SlowRepair,    // Verify before accepting
}

pub struct ReadRepairCoordinator {
    versions: HashMap<String, VersionedValue>,
    config: ReadRepairConfig,
}
```

**Responsibilities:**
- Collect read responses from multiple sites
- Detect version divergence
- Decide repair strategy
- Generate repair actions
- Track repair statistics

**Key Tests:**
- Divergence detection (2-way, 3-way)
- Quick vs Slow repair decisions
- Repair action generation
- Timeout handling
- Idempotent repairs

**Integration Points:**
- `conflict_resolver::ConflictResolver`
- `conduit::ConduitClient`
- `metrics::*`

---

### Module 3: `vector_clock_replication.rs` (18-22 tests)

**Purpose:** Track causal history with vector clocks for stronger consistency.

**Key Types:**
```rust
pub struct VectorClock {
    clocks: BTreeMap<String, u64>,  // site_id → version
}

pub enum ClockOrdering {
    Before,      // self < other
    After,       // self > other
    Concurrent,  // neither < nor >
    Equal,       // self == other
}

pub struct VectorTimestamp {
    pub vector_clock: VectorClock,
    pub lamport_ts: u64,  // For total ordering
}

pub struct OrderingBuffer {
    pending: Vec<OrderedOperation>,
    delivered: Vec<VectorTimestamp>,
    validator: CausalityValidator,
}
```

**Responsibilities:**
- Create and merge vector clocks
- Compare clocks (before/after/concurrent)
- Validate causal dependencies
- Buffer out-of-order operations
- Provide total ordering via Lamport clocks

**Key Tests:**
- Clock comparison logic
- Merge operations
- Causality validation
- Operation ordering
- Timeout handling

**Integration Points:**
- `journal::JournalEntry`
- `conduit::ConduitClient`
- `conflict_resolver::ConflictResolver`

---

### Module 4: `dual_site_orchestrator.rs` (24-28 tests)

**Purpose:** High-level orchestration for true active-active HA.

**Key Types:**
```rust
pub enum ConsistencyLevel {
    Strong,    // Quorum write before ack
    Causal,    // Vector clock causal consistency
    Eventual,  // Local write before ack, async replicate
}

pub struct DualSiteConfig {
    pub site_a: String,
    pub site_b: String,
    pub quorum_type: QuorumType,
    pub consistency_level: ConsistencyLevel,
    pub health_check_interval_ms: u64,
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
```

**Responsibilities:**
- Route writes with quorum coordination
- Route reads with optional read-repair
- Monitor site health
- Detect and execute failover
- Track HA metrics

**Key Tests:**
- Write orchestration (all consistency levels)
- Read orchestration (all strategies)
- Health probing and status transitions
- Automatic and manual failover
- Asymmetric failures
- Graceful switchover
- Recovery after partition

**Integration Points:**
- `write_aware_quorum::QuorumMatcher`
- `read_repair_coordinator::ReadRepairCoordinator`
- `vector_clock_replication::VectorClock`
- `failover::FailoverManager`
- `health::*`
- `metrics::*`

---

### Module 5: `dual_site_metrics.rs` (14-18 tests) — Optional

**Purpose:** Monitor active-active replication health and export metrics.

**Key Types:**
```rust
pub struct DualSiteStats {
    pub write_count: u64,
    pub read_count: u64,
    pub repair_count: u64,
    pub conflict_count: u64,
    pub partition_detections: u64,
}

pub struct ConsistencyMetrics {
    pub divergence_samples: u64,
    pub max_repair_time_ms: u64,
    pub causal_violations: u64,  // Should be 0
}

pub struct HAMetrics {
    pub failover_time_ms: u64,
    pub site_availability_percent: f64,
    pub quorum_ack_latency: u64,
}
```

## Design Decisions

### 1. **Quorum-Based Writes** (Not Consensus)
- Faster than full Raft across sites
- Configurable quorum: Majority, All, or Custom
- Allows asymmetric failure scenarios
- Explicit split-brain detection

### 2. **Read-Repair Pattern** (Eventual Consistency with Anti-Entropy)
- Dynamo-style approach proven in production
- Automatic divergence healing
- Two strategies: QuickRepair (fast) vs SlowRepair (verify)
- Minimal overhead on reads

### 3. **Vector Clocks + Lamport Timestamps**
- Enables causal consistency without full consensus
- Handles concurrent operations gracefully
- Lamport timestamp breaks ties for total ordering
- Proven approach in distributed systems

### 4. **Three Consistency Levels**
- **Strong:** Quorum write (most consistent, slowest)
- **Causal:** Vector clock consistency (balance)
- **Eventual:** Local write, async replicate (fastest)
- Tunable per operation or globally

### 5. **Health-Aware Orchestration**
- Continuous health probes
- Status transitions: Healthy → Degraded → Unhealthy → Down
- Automatic failover with detection timeout
- Manual failover support for admin override

## Implementation Strategy

### Phase 4a: Core Modules (OpenCode/minimax-m2p5)
1. Generate 4 core modules in parallel
2. Each module: 22-28 tests, full documentation
3. Target: ~310 total tests

### Phase 4b: Metrics Module (OpenCode or Claude)
1. If time permits: Generate dual_site_metrics.rs
2. Else: Simple monitoring wrapper written by Claude
3. Target: +14-18 tests

### Phase 4c: Integration & Testing
1. Update lib.rs with module declarations
2. Verify all tests pass
3. Verify no clippy warnings
4. Commit with descriptive message

## Success Criteria

- ✅ All 878 baseline tests still passing
- ✅ 280+ new tests from Phase 4 modules
- ✅ Zero failing tests
- ✅ Zero clippy warnings on new code
- ✅ Clean `cargo build` and `cargo test`
- ✅ Integration with existing A6 modules verified
- ✅ Commit pushed to GitHub
- ✅ CHANGELOG updated with Phase 4 completion

## Phase 4 Implementation Prompt

A comprehensive OpenCode prompt (`a6-phase4-input.md`) has been created with:
- Full context of existing codebase
- Detailed type specifications for each module
- Test case requirements (22-28 tests per module)
- Integration point documentation
- Error handling patterns
- Code quality requirements

## Timeline Estimate

- write_aware_quorum.rs: 25 min (quorum logic + 26 tests)
- read_repair_coordinator.rs: 25 min (read-repair + 24 tests)
- vector_clock_replication.rs: 22 min (VC logic + 22 tests)
- dual_site_orchestrator.rs: 30 min (orchestration + 28 tests)
- dual_site_metrics.rs: 15 min (monitoring, optional)

**Total:** ~120 minutes, ~320 tests, ~2500-3000 lines of Rust code

## Related Issues & PRs

- **Competitive Landscape:** ClaudeFS vs VAST Data, Weka (see docs/market.md)
- **Existing Failover:** failover.rs, site_failover.rs (Phase 3)
- **Existing Health:** health.rs (Phase 3)
- **Existing Active-Active:** active_active.rs (Phase 3, stub)

## Phase 5 Preview

After Phase 4 completes:
- **Phase 5a:** Performance optimization (latency reduction in quorum path)
- **Phase 5b:** Advanced features (intelligent quorum selection, adaptive timeout)
- **Phase 5c:** Operational tooling (CLI commands for failover, health checks)

## References

### Research & Theory
- Dynamo: Amazon's Highly Available Key-value Store (Read-repair)
- Google Chubby (Lock service with quorum)
- Lamport Clocks & Vector Clocks (Distributed systems)
- Quorum-based Replication (State machine replication)

### Existing ClaudeFS Modules
- failover.rs, site_failover.rs: Basic failover coordination
- active_active.rs: Existing active-active stub
- split_brain.rs: Split-brain detection
- conflict_resolver.rs: Last-write-wins resolution
- health.rs: Site health monitoring
- conduit.rs, conduit_pool.rs: gRPC communication

---

**Document Version:** 1.0
**Date:** 2026-03-06
**Status:** Ready for Implementation
**Next Action:** Run OpenCode to generate Phase 4 modules
