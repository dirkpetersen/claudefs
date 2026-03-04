# Implement journal_replay.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Context
You are implementing Phase 18 of the A3 (Data Reduction) agent for ClaudeFS.
The reduction crate currently has 1303 tests across 64 modules. Goal: ~1390 tests.

## Task
Create NEW FILE: `/home/cfs/claudefs/crates/claudefs-reduce/src/journal_replay.rs`

Implement journal replay for crash recovery — reads journal entries and reconstructs write state.

## Requirements

### Types

1. **ReplayAction enum**:
```rust
pub enum ReplayAction {
    WriteChunk { inode_id: u64, offset: u64, hash: [u8; 32], size: u32 },
    DeleteInode { inode_id: u64 },
    TruncateInode { inode_id: u64, new_size: u64 },
}
```
- Derive: Debug, Clone

2. **ReplayConfig struct**:
```rust
pub struct ReplayConfig {
    pub max_entries_per_batch: usize,  // default 1000
    pub verify_hashes: bool,           // default true
}
```
- Derive: Debug, Clone, Serialize, Deserialize
- impl Default

3. **ReplayStats struct**:
```rust
pub struct ReplayStats {
    pub entries_replayed: u64,
    pub chunks_written: u64,
    pub inodes_deleted: u64,
    pub inodes_truncated: u64,
    pub errors: u64,
}
```
- Derive: Debug, Clone, Default

4. **InodeReplayState struct**:
```rust
pub struct InodeReplayState {
    pub inode_id: u64,
    pub chunks: Vec<(u64, [u8; 32])>,  // (offset, hash)
    pub deleted: bool,
    pub final_size: Option<u64>,
}
```
- Derive: Debug, Clone

5. **ReplayState struct**:
```rust
pub struct ReplayState {
    pub inode_states: HashMap<u64, InodeReplayState>,
}
```
- Derive: Debug
- impl Default (empty HashMap)

6. **JournalReplayer struct**:
- `fn new(config: ReplayConfig) -> Self`
- `fn apply(&mut self, state: &mut ReplayState, action: ReplayAction)` — apply one action to state:
  - WriteChunk: get or create inode state, add (offset, hash) to chunks (don't duplicate same offset)
  - DeleteInode: mark inode state as deleted=true
  - TruncateInode: set inode state's final_size
- `fn replay_batch(&mut self, state: &mut ReplayState, actions: &[ReplayAction]) -> ReplayStats` — apply batch of actions; track stats
- `fn finalize(&self, state: &ReplayState) -> Vec<InodeReplayState>` — return non-deleted inode states (cloned)

## Required Tests (at least 14)

1. `replayer_config_default` — verify defaults
2. `replay_stats_default` — all zeros
3. `apply_write_chunk` — creates inode state, adds chunk
4. `apply_delete_inode` — marks deleted
5. `apply_truncate_inode` — sets final_size
6. `replay_batch_empty` — empty batch, empty stats
7. `replay_batch_multiple_writes` — multiple chunks added
8. `replay_batch_stats_chunks_written` — correct count
9. `replay_batch_stats_inodes_deleted` — correct count
10. `replay_batch_stats_inodes_truncated` — correct count
11. `finalize_excludes_deleted` — deleted inodes not in output
12. `finalize_includes_alive` — non-deleted inodes in output
13. `inode_replay_state_chunks` — verify chunks field
14. `replay_idempotent_hash` — write same chunk twice, only one entry in state

## Style
- Follow existing crate patterns
- Use `use serde::{Deserialize, Serialize};`
- Use `use std::collections::HashMap;`
- NO COMMENTS in code
- Use `#[cfg(test)] mod tests { ... }` pattern

## Validation
After writing the file, verify it compiles:
```bash
cd /home/cfs/claudefs && cargo check -p claudefs-reduce
```
