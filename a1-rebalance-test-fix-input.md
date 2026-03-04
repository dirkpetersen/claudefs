# Fix: Failing test in node_rebalance.rs

## File to fix
`/home/cfs/claudefs/crates/claudefs-storage/src/node_rebalance.rs`

## Problem
The test `test_progress_pct_all_done` (around line 710) calls `complete_rebalance()` without
first advancing the migration to the `Completed` state. The `complete_rebalance()` method
requires all migrations to be in a terminal state (Completed or Failed) before it succeeds.

The test currently does:
```rust
engine.start_rebalance().unwrap();
engine.complete_rebalance().unwrap();  // fails — migrations still Queued
```

The working test `test_complete_rebalance_all_done` (around line 599) shows the correct pattern:
```rust
engine.start_rebalance().unwrap();
engine.advance_migration(RebalanceSegmentId(1)).unwrap();  // Queued -> Transferring
engine.advance_migration(RebalanceSegmentId(1)).unwrap();  // Transferring -> Verifying
engine.advance_migration(RebalanceSegmentId(1)).unwrap();  // Verifying -> Completed
engine.complete_rebalance().unwrap();  // succeeds
```

## Fix required
In `test_progress_pct_all_done`, add the three `advance_migration` calls between
`start_rebalance()` and `complete_rebalance()`.

The test should look like this after the fix:
```rust
fn test_progress_pct_all_done() {
    let config = RebalanceConfig::default();
    let mut engine = RebalanceEngine::new(config, new_node("node1"));

    let mut shard_map = HashMap::new();
    shard_map.insert(ShardId(0), new_node("node2"));
    engine.update_shard_map(shard_map);
    engine.register_segment(RebalanceSegmentId(1), ShardId(0));

    engine.start_rebalance().unwrap();
    engine.advance_migration(RebalanceSegmentId(1)).unwrap();
    engine.advance_migration(RebalanceSegmentId(1)).unwrap();
    engine.advance_migration(RebalanceSegmentId(1)).unwrap();
    engine.complete_rebalance().unwrap();

    assert_eq!(engine.progress_pct(), 100.0);
}
```

## Instructions
1. Read `/home/cfs/claudefs/crates/claudefs-storage/src/node_rebalance.rs`
2. Find the `test_progress_pct_all_done` function
3. Add three `engine.advance_migration(RebalanceSegmentId(1)).unwrap();` calls between
   `engine.start_rebalance().unwrap();` and `engine.complete_rebalance().unwrap();`
4. Write the complete updated file
5. Do NOT change anything else in the file
