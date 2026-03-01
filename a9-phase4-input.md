# A9: Test & Validation — Phase 4: Transport + Replication Tests

You are extending the `claudefs-tests` crate for the ClaudeFS distributed filesystem project. This is Agent A9 (Test & Validation), Phase 4.

## Working directory: /home/cfs/claudefs

## Context

Current test count: 468 in claudefs-tests, 1838 in workspace
Previous phases added modules for: proptest, POSIX, Jepsen, soak, regression, CI matrix, storage/meta/reduce integration tests.

## Task: Phase 4 — Transport + Replication Tests (3 NEW modules)

Create 3 new modules. Target: **~80 new tests** (total ~550).

### Module 1: `src/transport_tests.rs` — Transport Integration Tests (~30 tests)

Deep integration tests for the claudefs-transport crate, testing the real APIs.

First READ `crates/claudefs-transport/src/lib.rs` to understand what's public, then read a few key files:
- `crates/claudefs-transport/src/circuitbreaker.rs`
- `crates/claudefs-transport/src/ratelimit.rs`
- `crates/claudefs-transport/src/routing.rs`
- `crates/claudefs-transport/src/metrics.rs`
- `crates/claudefs-transport/src/protocol.rs`

Write tests (~30) that exercise these APIs:
- CircuitBreaker: new, record_success, record_failure, state transitions (Closed/Open/HalfOpen)
- RateLimiter: new with config, try_acquire (returns bool), tokens_remaining
- ConsistentHashRing (from routing.rs): add_node, get_node for a key, consistent mapping
- TransportMetrics: record operations, get counts
- Protocol types: FrameHeader encoding/decoding, Opcode variants, ProtocolVersion comparison
- Message: creation, serialization check
- Error types: verify proper Display formatting

All tests should use the public API only — no internal mocking needed.

```rust
use claudefs_transport::{
    // Import what's actually public based on lib.rs
};
```

### Module 2: `src/distributed_tests.rs` — Distributed System Tests (~25 tests)

Tests that simulate multi-node distributed scenarios using the in-memory test infrastructure from previous phases:

```rust
use crate::chaos::{FaultInjector, FaultType, NetworkTopology};
use crate::jepsen::{JepsenHistory, RegisterModel, JepsenChecker, Nemesis};
use crate::linearizability::{History, Operation};

/// Tests for distributed system behavior patterns

/// Test: two-phase commit simulation
/// - Prepare phase: all nodes respond Ok
/// - Commit phase: all nodes respond Ok
/// - Verify all operations recorded
pub struct TwoPhaseCommitSim {
    pub nodes: Vec<u32>,
}

impl TwoPhaseCommitSim {
    pub fn new(node_count: u32) -> Self
    pub fn prepare_all(&self) -> bool  // returns true if all nodes ready
    pub fn commit_all(&self) -> bool   // returns true if all committed
    pub fn abort_all(&self) -> bool    // returns true if all aborted
    pub fn prepare_with_failures(&self, failing_nodes: &[u32]) -> bool  // returns false
}

/// Test: quorum-based voting
pub struct QuorumVote {
    pub total_nodes: u32,
    pub votes_cast: u32,
    pub votes_yes: u32,
}

impl QuorumVote {
    pub fn new(total: u32) -> Self
    pub fn cast_yes(&mut self)
    pub fn cast_no(&mut self)
    pub fn has_quorum(&self) -> bool  // majority quorum
    pub fn has_strong_quorum(&self) -> bool  // 2/3 quorum
}

/// Test: Raft leader election simulation
pub struct RaftElectionSim {
    pub node_count: u32,
    pub term: u64,
    pub votes: std::collections::HashMap<u32, u32>,  // candidate -> votes
}

impl RaftElectionSim {
    pub fn new(node_count: u32) -> Self
    pub fn start_election(&mut self, candidate: u32)
    pub fn vote_for(&mut self, voter: u32, candidate: u32)
    pub fn has_winner(&self) -> Option<u32>  // returns candidate with quorum
    pub fn advance_term(&mut self)
}

/// Test: network partition scenarios
pub struct PartitionScenario {
    pub topology: NetworkTopology,
    pub fault_injector: FaultInjector,
}

impl PartitionScenario {
    pub fn new(node_count: u32) -> Self
    pub fn partition_network(&mut self, group_a: Vec<u32>, group_b: Vec<u32>)
    pub fn heal_partition(&mut self)
    pub fn can_reach(&self, from: u32, to: u32) -> bool
    pub fn nodes_in_majority_partition(&self, all_nodes: &[u32]) -> Vec<u32>
}
```

Unit tests (~25):
- test TwoPhaseCommitSim with all nodes succeeding
- test TwoPhaseCommitSim with failure causing abort
- test QuorumVote: majority quorum requires >50%
- test QuorumVote: 3/5 nodes is quorum, 2/5 is not
- test strong quorum requires 2/3
- test RaftElectionSim: candidate with majority wins
- test RaftElectionSim: split vote has no winner
- test RaftElectionSim advance_term increments
- test PartitionScenario: nodes in same partition can reach each other
- test PartitionScenario: nodes in different partitions cannot reach each other
- test PartitionScenario: heal restores connectivity
- test nodes_in_majority_partition returns correct group

### Module 3: `src/fuzz_helpers.rs` — Fuzzing Infrastructure (~25 tests)

Utilities for structure-aware fuzzing of ClaudeFS components:

```rust
use rand::Rng;
use rand::SeedableRng;

/// A deterministic fuzzer that generates structured inputs
pub struct StructuredFuzzer {
    rng: rand::rngs::SmallRng,
    pub seed: u64,
}

impl StructuredFuzzer {
    pub fn new(seed: u64) -> Self
    pub fn random_bytes(&mut self, len: usize) -> Vec<u8>
    pub fn random_string(&mut self, max_len: usize) -> String  // valid UTF-8
    pub fn random_path(&mut self, max_depth: usize) -> std::path::PathBuf
    pub fn random_filename(&mut self) -> String  // valid POSIX filename (no / or \0)
    pub fn random_u64(&mut self) -> u64
    pub fn random_f64_0_1(&mut self) -> f64
    pub fn random_bytes_range(&mut self, min: usize, max: usize) -> Vec<u8>
}

/// Generates malformed/edge-case RPC frames for fuzzing the transport layer
pub struct RpcFuzzer {
    fuzzer: StructuredFuzzer,
}

impl RpcFuzzer {
    pub fn new(seed: u64) -> Self
    pub fn empty_frame(&self) -> Vec<u8>
    pub fn truncated_frame(&mut self) -> Vec<u8>  // random bytes, too short
    pub fn oversized_frame(&mut self, max_size: usize) -> Vec<u8>  // larger than max
    pub fn random_frame(&mut self) -> Vec<u8>  // completely random bytes
    pub fn malformed_header(&mut self) -> Vec<u8>  // valid size but invalid header fields
}

/// Generates invalid/edge-case filesystem paths for POSIX testing
pub struct PathFuzzer {
    fuzzer: StructuredFuzzer,
}

impl PathFuzzer {
    pub fn new(seed: u64) -> Self
    pub fn absolute_path(&mut self) -> String
    pub fn path_with_dots(&mut self) -> String  // includes . and ..
    pub fn path_with_spaces(&mut self) -> String
    pub fn very_long_path(&mut self, components: usize) -> String
    pub fn path_with_unicode(&mut self) -> String
    pub fn null_byte_path(&mut self) -> Vec<u8>  // contains \0 — invalid for POSIX
}

/// Test corpus management for regression testing
pub struct FuzzCorpus {
    entries: Vec<FuzzEntry>,
}

#[derive(Debug, Clone)]
pub struct FuzzEntry {
    pub id: String,
    pub data: Vec<u8>,
    pub description: String,
    pub triggers_bug: bool,
}

impl FuzzCorpus {
    pub fn new() -> Self
    pub fn add(&mut self, entry: FuzzEntry)
    pub fn seed_corpus() -> Self  // returns a corpus of known interesting inputs
    pub fn len(&self) -> usize
    pub fn interesting_entries(&self) -> Vec<&FuzzEntry>  // only triggers_bug=false ones
    pub fn bug_entries(&self) -> Vec<&FuzzEntry>  // only triggers_bug=true ones
    pub fn get_by_id(&self, id: &str) -> Option<&FuzzEntry>
}
```

Unit tests (~25):
- test StructuredFuzzer with seed produces deterministic output
- test random_bytes length matches requested length
- test random_string is valid UTF-8
- test random_filename contains no / or \0
- test random_path depth matches max_depth
- test RpcFuzzer empty_frame returns zero-length
- test RpcFuzzer truncated_frame is shorter than minimum valid frame
- test RpcFuzzer oversized_frame exceeds max_size
- test PathFuzzer absolute_path starts with /
- test PathFuzzer null_byte_path contains \0
- test FuzzCorpus add and len
- test seed_corpus has entries
- test interesting_entries filters out bug entries
- test get_by_id finds entry by id
- test two fuzzers with same seed produce same output

## Requirements

1. All 3 new modules must compile with zero errors
2. All ~80 new tests must pass
3. Update `src/lib.rs` to add `pub mod` for all 3 new modules
4. No unsafe code
5. For transport_tests.rs: READ crates/claudefs-transport/src/lib.rs FIRST before importing anything

## Files to create/modify

1. `crates/claudefs-tests/src/transport_tests.rs` — NEW
2. `crates/claudefs-tests/src/distributed_tests.rs` — NEW
3. `crates/claudefs-tests/src/fuzz_helpers.rs` — NEW
4. `crates/claudefs-tests/src/lib.rs` — MODIFY

Output each file with clear delimiters:
```
=== FILE: path/to/file ===
<content>
=== END FILE ===
```
