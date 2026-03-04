# A3 Phase 14: Chunk Rebalancer, Write Coalescer, EC Repair

You are implementing Phase 14 of the A3 (Data Reduction) agent for ClaudeFS.

## Working directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Current state
1021 tests across 52 modules. Phase 14 goal: ~1110 tests.

## TASK: Write these files directly to disk

### NEW FILE 1: `/home/cfs/claudefs/crates/claudefs-reduce/src/chunk_rebalancer.rs`

Implement chunk rebalancing when nodes join or leave the cluster.

When a node leaves, its chunks must be redistributed to remaining nodes.
When a node joins, some chunks from existing nodes can be moved to it.

Requirements:
- `RebalanceAction` enum: `Move { chunk_hash: [u8; 32], from_node: u64, to_node: u64, size_bytes: u64 }`, `Replicate { chunk_hash: [u8; 32], source_node: u64, dest_node: u64, size_bytes: u64 }`
  - Derive Debug, Clone
- `NodeLoad` struct: `node_id: u64`, `bytes_stored: u64`, `chunk_count: u64`
  - `fn load_fraction(&self, total_bytes: u64) -> f64` → bytes_stored / total_bytes, or 0.0
  - Derive Debug, Clone, Serialize, Deserialize
- `RebalancePlan` struct: `actions: Vec<RebalanceAction>`, `total_bytes_moved: u64`, `total_bytes_replicated: u64`
  - `fn action_count(&self) -> usize` → actions.len()
  - Derive Debug
- `RebalancerConfig` struct: `max_imbalance_pct: f64` (default 10.0, tolerate up to 10% imbalance), `max_actions_per_plan: usize` (default 100)
  - Derive Debug, Clone, Serialize, Deserialize
- `ChunkRebalancer` struct:
  - `fn new(config: RebalancerConfig) -> Self`
  - `fn is_balanced(&self, loads: &[NodeLoad]) -> bool` — max load fraction - min load fraction <= max_imbalance_pct/100.0
  - `fn plan_rebalance(&self, loads: &[NodeLoad], chunks: &[([u8; 32], u64, u64)]) -> RebalancePlan`
    - `chunks`: list of (hash, current_node_id, size_bytes)
    - Move chunks from overloaded nodes (above average) to underloaded nodes (below average)
    - Limit to max_actions_per_plan actions
  - `fn plan_node_departure(&self, departed_node_id: u64, remaining_nodes: &[u64], chunks: &[([u8; 32], u64)]) -> RebalancePlan`
    - `chunks`: list of (hash, size_bytes) that were on the departed node
    - Distribute to remaining nodes round-robin

Write at least **14 tests**:
1. rebalancer_config_default
2. node_load_fraction_normal
3. node_load_fraction_zero_total
4. is_balanced_balanced — all nodes equal → true
5. is_balanced_imbalanced — one node 30% over average → false
6. is_balanced_empty — no nodes → true
7. plan_rebalance_empty — no chunks → empty plan
8. plan_rebalance_balanced_no_actions
9. plan_node_departure_distributes_to_remaining
10. plan_node_departure_round_robin
11. plan_node_departure_empty_chunks
12. rebalance_plan_action_count
13. rebalance_plan_total_bytes_moved
14. plan_respects_max_actions_per_plan

---

### NEW FILE 2: `/home/cfs/claudefs/crates/claudefs-reduce/src/write_coalescer.rs`

Implement write coalescing to merge adjacent or overlapping writes before the pipeline.

Small sequential writes to the same inode at adjacent offsets can be coalesced
into a single larger write, improving throughput and dedup effectiveness.

Requirements:
- `CoalesceConfig` struct: `max_gap_bytes: u64` (default 0, only adjacent writes coalesced), `max_coalesced_bytes: u64` (default 8MB = 8*1024*1024), `window_ms: u64` (default 50ms, time window for coalescing)
  - Derive Debug, Clone, Serialize, Deserialize
- `WriteOp` struct: `inode_id: u64`, `offset: u64`, `data: Vec<u8>`, `timestamp_ms: u64`
  - `fn end_offset(&self) -> u64` → offset + data.len() as u64
  - Derive Debug, Clone
- `CoalescedWrite` struct: `inode_id: u64`, `offset: u64`, `data: Vec<u8>`, `source_count: usize`
  - Derive Debug
- `WriteCoalescer` struct:
  - `fn new(config: CoalesceConfig) -> Self`
  - `fn add(&mut self, op: WriteOp)` — buffer the write
  - `fn flush_ready(&mut self, now_ms: u64) -> Vec<CoalescedWrite>` — return coalesced writes where window has expired; coalesce adjacent writes (end_offset matches next write offset)
  - `fn flush_inode(&mut self, inode_id: u64) -> Option<CoalescedWrite>` — force flush all pending for inode, coalescing what's possible
  - `fn flush_all(&mut self) -> Vec<CoalescedWrite>` — flush everything
  - `fn pending_count(&self) -> usize`

Write at least **15 tests**:
1. coalesce_config_default
2. add_single_write
3. flush_all_single_write — returns one CoalescedWrite
4. flush_inode_not_found — returns None
5. coalesce_adjacent_writes — two adjacent writes coalesced into one
6. coalesce_nonadjacent — gap between writes, not coalesced
7. flush_all_multiple_inodes — separate CoalescedWrite per inode
8. coalesced_write_source_count_1
9. coalesced_write_source_count_2
10. flush_ready_no_ready_writes
11. flush_ready_expired_window
12. pending_count_after_add
13. pending_count_after_flush
14. max_coalesced_size_respected — stop coalescing when max reached
15. coalesce_three_adjacent_writes

---

### NEW FILE 3: `/home/cfs/claudefs/crates/claudefs-reduce/src/ec_repair.rs`

Implement EC repair planning when shards are lost or corrupted.

D1: 4+2 EC can tolerate up to 2 shard losses. When a node fails, repair reads
surviving shards and reconstructs lost ones on healthy nodes.

Requirements:
- `ShardState` enum: `Available`, `Lost`, `Corrupted`
  - Derive Debug, Clone, Copy, PartialEq, Eq
- `RepairAssessment` struct: `segment_id: u64`, `total_shards: u8`, `available: u8`, `lost: u8`, `corrupted: u8`
  - `fn is_degraded(&self) -> bool` → (lost + corrupted) > 0
  - `fn can_recover(&self, data_shards: u8) -> bool` → available >= data_shards
  - `fn needs_immediate_repair(&self, parity_shards: u8) -> bool` → (lost + corrupted) >= parity_shards
  - Derive Debug, Clone
- `RepairPlan` struct: `segment_id: u64`, `source_shards: Vec<u8>`, `target_shards: Vec<u8>`, `target_nodes: Vec<u64>`
  - `fn repair_count(&self) -> usize` → target_shards.len()
  - Derive Debug, Clone
- `EcRepair` struct with `data_shards: u8`, `parity_shards: u8`:
  - `fn new(data_shards: u8, parity_shards: u8) -> Self`
  - `fn assess(&self, segment_id: u64, shard_states: &[(u8, ShardState)]) -> RepairAssessment`
    - `shard_states`: list of (shard_index, state)
  - `fn plan_repair(&self, assessment: &RepairAssessment, available_nodes: &[u64]) -> Option<RepairPlan>`
    - Returns None if cannot recover (available < data_shards)
    - Otherwise: pick data_shards from available shards as source, fill in missing shards on available_nodes
  - `fn plan_preventive_repair(&self, assessments: &[RepairAssessment]) -> Vec<RepairPlan>`
    - Return repair plans for all degraded but recoverable segments, prioritizing most degraded first

Write at least **15 tests**:
1. ec_repair_new_4_2
2. assess_all_available — no degradation
3. assess_some_lost
4. assess_some_corrupted
5. is_degraded_false
6. is_degraded_true
7. can_recover_true — enough shards available
8. can_recover_false — too many lost
9. needs_immediate_repair_false
10. needs_immediate_repair_true — lost == parity_shards
11. plan_repair_returns_none_if_unrecoverable
12. plan_repair_returns_plan_if_recoverable
13. plan_repair_source_has_data_shards
14. plan_preventive_repair_prioritizes_most_degraded
15. plan_preventive_repair_empty
16. repair_plan_repair_count

---

## EXPAND TESTS in existing modules

### Expand write_amplification.rs (17 tests → +8)
Read the file. Add 8 more tests covering edge cases in WriteAmplificationTracker.

### Expand pipeline_monitor.rs (17 tests → +8)
Read the file. Add 8 more tests covering PipelineMonitor, StageMetrics edge cases.

### Expand chunk_verifier.rs (15 tests → +7)
Read the file. Add 7 more tests for ChunkVerifier, VerificationSchedule.

---

## Also update lib.rs

Add:
- `pub mod chunk_rebalancer;`
- `pub mod write_coalescer;`
- `pub mod ec_repair;`
- Re-exports for key types

## Goal
- Build: 0 errors, 0 warnings
- Tests: ~1110+ passing
- Clippy: 0 warnings
