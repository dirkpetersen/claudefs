# Task: Write repl_phase2_security_tests.rs for claudefs-security crate

Write a comprehensive Rust test module `repl_phase2_security_tests.rs` for the claudefs-security crate that tests security properties of the claudefs-repl crate's journal_source, sliding_window, and catchup modules.

## File location
`crates/claudefs-security/src/repl_phase2_security_tests.rs`

## Structure
```rust
//! Security tests for replication Phase 2 modules: journal source, sliding window, catchup
//!
//! Part of A10 Phase 3: Repl subsystem security audit — sequence attacks, replay, DoS vectors

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available APIs (use these exact types)

### From claudefs_repl::journal_source
```rust
pub struct SourceBatch { pub entries: Vec<JournalEntry>, pub first_seq: u64, pub last_seq: u64, pub source_site_id: u64 }
pub struct SourceCursor { pub last_polled: u64, pub last_acknowledged: u64, pub source_id: String }
pub trait JournalSource: Send + Sync {
    fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError>;
    fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError>;
    fn cursor(&self) -> SourceCursor;
}
pub struct MockJournalSource { /* private */ }
// MockJournalSource has: new(source_id), push_entry(entry), push_entries(entries), entries_remaining()
pub struct VecJournalSource { /* private */ }
// VecJournalSource has: new(source_id, entries)
// Both implement JournalSource trait
// SourceCursor has: new(source_id)
```

### From claudefs_repl::journal (for creating JournalEntry)
```rust
pub struct JournalEntry { pub sequence: u64, pub site_id: u64, pub shard_id: u32, pub op: OpKind, pub key: Vec<u8>, pub value: Vec<u8>, pub timestamp_ms: u64, pub crc32: u32 }
pub enum OpKind { Put, Delete, Mkdir, Rmdir, Rename, SetAttr, Link, Unlink }
// JournalEntry has: new(sequence, site_id, shard_id, op, key, value, timestamp_ms), compute_crc(&self) -> u32, validate_crc(&self) -> bool
```

### From claudefs_repl::sliding_window
```rust
pub struct WindowConfig { pub window_size: usize, pub ack_timeout_ms: u64 }
pub struct InFlightBatch { pub batch_seq: u64, pub entry_count: usize, pub sent_at_ms: u64, pub retransmit_count: u32 }
pub struct WindowStats { pub total_sent: u64, pub total_acked: u64, pub total_timed_out: u64, pub total_retransmits: u64, pub current_in_flight: usize }
pub enum WindowState { Ready, Full, Drained }
pub enum WindowError { Full(usize), NotFound(u64) }
pub struct SlidingWindow { /* private */ }
// SlidingWindow has: new(config), send_batch(entry_count, now_ms) -> Result<u64, WindowError>, acknowledge(batch_seq) -> Result<usize, WindowError>, window_state(), timed_out_batches(now_ms), mark_retransmit(batch_seq), in_flight_count(), next_seq(), stats()
// WindowConfig implements Default (window_size=16, ack_timeout_ms=5000)
```

### From claudefs_repl::catchup
```rust
pub struct CatchupConfig { pub consumer_id: String, pub max_batch_size: usize, pub timeout_ms: u64 }
pub struct CatchupStats { pub sessions_started: u32, pub sessions_completed: u32, pub sessions_failed: u32, pub total_entries_received: u64, pub total_batches_received: u32 }
pub enum CatchupPhase { Idle, Requested { cursor_seq }, InProgress { cursor_seq, batches_received }, Complete { final_seq, total_entries }, Failed { reason } }
pub enum CatchupError { AlreadyRunning, UnexpectedBatch(String) }
pub struct CatchupState { /* private */ }
// CatchupState has: new(config), request(from_seq) -> Result<(), CatchupError>, receive_batch(entry_count, is_final, final_seq) -> Result<(), CatchupError>, fail(reason), reset(), phase(), stats(), is_running(), is_complete()
// CatchupConfig implements Default (consumer_id="default", max_batch_size=1000, timeout_ms=30000)
```

### From claudefs_repl::error
```rust
// ReplError is the error type
```

## Security findings to test (25 tests total)

### A. Journal Source Security (8 tests)
1. `test_mock_source_empty_poll_returns_none` — Poll on empty source returns Ok(None), not error
2. `test_mock_source_acknowledge_advances_cursor` — After ack(5), cursor.last_acknowledged == 5
3. `test_mock_source_poll_respects_max_entries` — Push 10 entries, poll with max=3, get exactly 3
4. `test_mock_source_batch_sequences_correct` — Batch first_seq and last_seq match entry sequences
5. `test_mock_source_acknowledge_arbitrary_seq` — ack(u64::MAX) accepted without error (no bounds check)
6. `test_vec_source_exhaustion` — Poll all entries from VecJournalSource, then poll returns None
7. `test_vec_source_acknowledge_updates_cursor` — Verify acknowledge updates cursor on VecJournalSource
8. `test_source_cursor_initial_state` — New cursor has last_polled=0, last_acknowledged=0

### B. Sliding Window Security (10 tests)
9. `test_window_send_increments_sequence` — Each send_batch returns incrementing seq numbers
10. `test_window_full_returns_error` — Fill window to capacity, next send returns WindowError::Full
11. `test_window_acknowledge_clears_slot` — Ack a batch, window goes from Full back to Ready
12. `test_window_ack_nonexistent_seq` — Ack seq that was never sent returns WindowError::NotFound
13. `test_window_timed_out_detection` — Send batch at t=0, check timed_out_batches at t=timeout+1
14. `test_window_no_timeout_before_deadline` — Batches not timed out before ack_timeout_ms elapsed
15. `test_window_retransmit_increments_count` — mark_retransmit increments retransmit counter in stats
16. `test_window_stats_track_operations` — send+ack+timeout all reflected in stats
17. `test_window_state_transitions` — Ready -> Full (fill up) -> Ready (ack one) -> Drained (ack all)
18. `test_window_cumulative_ack` — Ack seq=3 when seqs 1,2,3 in flight; all three cleared

### C. Catchup Security (7 tests)
19. `test_catchup_starts_idle` — New CatchupState phase is Idle
20. `test_catchup_request_transitions_to_requested` — After request(100), phase is Requested
21. `test_catchup_double_request_fails` — Calling request() twice returns AlreadyRunning
22. `test_catchup_receive_batch_transitions_to_in_progress` — After request+receive_batch, phase is InProgress
23. `test_catchup_final_batch_completes` — receive_batch with is_final=true transitions to Complete
24. `test_catchup_fail_transitions_to_failed` — fail("error") transitions to Failed
25. `test_catchup_reset_returns_to_idle` — After fail, reset() returns to Idle, stats preserved

## Important rules
- Use `#[cfg(test)]` on the outer module
- Put all tests in a `mod tests { }` block
- Import from claudefs_repl, not from internal paths
- Do NOT use unsafe code
- Every test should have a comment explaining the security finding it validates
- Use assert!, assert_eq!, assert_ne! for assertions
- Use `matches!()` macro for enum variant matching where needed
- Keep tests simple and focused — test one security property per test
- To create JournalEntry instances, use JournalEntry::new(sequence, site_id, shard_id, op, key, value, timestamp_ms)
- For OpKind, use OpKind::Put, OpKind::Delete, etc.
- Make sure the file compiles with `cargo test -p claudefs-security`

## Output format
Output ONLY the complete Rust source file contents. No markdown, no explanation, no code fences — just the raw .rs file content.
