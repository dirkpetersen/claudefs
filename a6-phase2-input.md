# OpenCode Task: claudefs-repl Phase 2 — Journal Source Trait, Sliding Window, Catchup Protocol

## Context

You are implementing Phase 2 features for `claudefs-repl` — the cross-site journal replication crate of ClaudeFS, a distributed POSIX filesystem.

The crate already has a complete Phase 1 implementation (35 modules, 742 tests), including:
- `journal.rs` — JournalEntry + OpKind types with CRC32
- `wal.rs` — write-ahead log with cursor tracking
- `conduit.rs` — in-memory conduit abstraction (Channel/Mock)
- `sync.rs` — ConflictDetector, BatchCompactor, ReplicationSync
- `pipeline.rs` — ReplicationPipeline that ties journal → compact → throttle → fanout
- `engine.rs` — ReplicationEngine orchestrator
- `fanout.rs` — multi-site parallel dispatch
- `backpressure.rs`, `throttle.rs`, `compression.rs`, etc.

## Phase 2 Task: Add 3 new modules

Add the following three new Rust source files to `crates/claudefs-repl/src/`. DO NOT modify any existing files except `lib.rs` (to add the new module declarations).

### 1. `src/journal_source.rs`

A trait-based interface for plugging different journal sources (A2 metadata journal, test mock, file replay) into the replication pipeline.

Requirements:
- `JournalSource` trait:
  ```rust
  #[async_trait::async_trait]
  pub trait JournalSource: Send + Sync {
      async fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError>;
      async fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError>;
      fn cursor(&self) -> SourceCursor;
  }
  ```
  **NOTE: Do NOT add async_trait as a dependency. Implement the trait using regular async fn in a trait (Rust 2021 edition RPITIT / return-position impl Trait) if possible, OR if async_trait is truly needed, use a Box<dyn Future> pattern manually. Actually, the SIMPLEST approach: make poll_batch and acknowledge synchronous methods that return futures using explicit return types. But the EASIEST approach for Rust stable is: avoid async in the trait entirely. Instead use `fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError>` for a synchronous poll-style interface. Use this approach.**

  Revised trait (synchronous, no async_trait needed):
  ```rust
  pub trait JournalSource: Send + Sync {
      /// Poll for next batch. Returns None if no entries available yet.
      fn poll_batch(&mut self, max_entries: usize) -> Result<Option<SourceBatch>, ReplError>;
      /// Acknowledge successful replication up to last_seq.
      fn acknowledge(&mut self, last_seq: u64) -> Result<(), ReplError>;
      /// Get the current cursor position.
      fn cursor(&self) -> SourceCursor;
  }
  ```

- `SourceBatch` struct:
  - `entries: Vec<JournalEntry>` — entries from the journal
  - `first_seq: u64` — sequence of the first entry
  - `last_seq: u64` — sequence of the last entry
  - `source_site_id: u64` — which site these came from

- `SourceCursor` struct:
  - `last_polled: u64` — last sequence polled from the journal
  - `last_acknowledged: u64` — last sequence ACK'd by remote
  - `source_id: String` — consumer identifier

- `MockJournalSource` struct (for testing):
  - Fields: `entries: VecDeque<JournalEntry>`, `cursor: SourceCursor`, `max_batch: usize`
  - Constructor: `new(source_id: impl Into<String>) -> Self`
  - Method: `push_entry(entry: JournalEntry)` — inject a test entry
  - Method: `push_entries(entries: Vec<JournalEntry>)` — inject multiple
  - Method: `entries_remaining(&self) -> usize`
  - Implements `JournalSource`

- `VecJournalSource` struct (replay from a Vec):
  - Fields: `entries: Vec<JournalEntry>`, `cursor_pos: usize`, `cursor: SourceCursor`
  - Constructor: `new(source_id: impl Into<String>, entries: Vec<JournalEntry>) -> Self`
  - Implements `JournalSource`
  - When all entries have been returned once, subsequent `poll_batch` returns `Ok(None)`

- Unit tests (25+ tests in `mod tests`) covering:
  1. `test_mock_source_empty_returns_none`
  2. `test_mock_source_push_and_poll`
  3. `test_mock_source_poll_batch_respects_max`
  4. `test_mock_source_acknowledges_advances_cursor`
  5. `test_mock_source_cursor_initial_state`
  6. `test_mock_source_push_entries_batch`
  7. `test_mock_source_sequential_polls`
  8. `test_mock_source_entries_remaining_count`
  9. `test_vec_source_empty`
  10. `test_vec_source_poll_all`
  11. `test_vec_source_poll_respects_max`
  12. `test_vec_source_exhausted_returns_none`
  13. `test_vec_source_acknowledge_cursor`
  14. `test_vec_source_cursor_tracks_position`
  15. `test_source_batch_fields`
  16. `test_source_cursor_clone`
  17. `test_mock_then_vec_source_integration`
  18. `test_mock_source_multiple_polls_drain`
  19. `test_vec_source_single_entry`
  20. `test_vec_source_large_batch`
  21. `test_mock_source_acknowledge_unknown_seq` (should not panic)
  22. `test_source_cursor_last_polled_tracks_poll`
  23. `test_source_cursor_last_acked_tracks_ack`
  24. `test_mock_source_empty_after_drain`
  25. `prop_mock_source_roundtrip` (proptest: push N entries, poll max M, count correct)

---

### 2. `src/sliding_window.rs`

A sliding window acknowledgment protocol for reliable in-order batch delivery.

When sending batches, only `window_size` batches can be in-flight (sent but not yet ACK'd).
When the window is full, new sends are blocked until ACKs arrive.
On timeout, in-flight batches are eligible for retransmission.

Requirements:

- `WindowConfig` struct (with `Default`):
  - `window_size: usize` — max in-flight batches (default: 32)
  - `ack_timeout_ms: u64` — ms before an in-flight batch is considered timed out (default: 5000)

- `InFlightBatch` struct:
  - `batch_seq: u64` — batch sequence number
  - `entry_count: usize` — number of entries in this batch
  - `sent_at_ms: u64` — Unix milliseconds when batch was first sent (use `std::time::SystemTime` → duration → millis)
  - `retransmit_count: u32` — how many times this batch has been retransmitted

- `WindowState` enum: `Ready`, `Full`, `Drained`

- `SlidingWindow` struct:
  - Fields: `config: WindowConfig`, `next_batch_seq: u64`, `in_flight: VecDeque<InFlightBatch>`, stats fields
  - Constructor: `new(config: WindowConfig) -> Self`
  - `fn send_batch(&mut self, entry_count: usize, now_ms: u64) -> Result<u64, ReplError>`
    - Returns the assigned batch_seq, or `Err(ReplError::WindowFull)` if window is full
    - Adds to `in_flight`, increments `next_batch_seq`
  - `fn acknowledge(&mut self, batch_seq: u64) -> Result<usize, ReplError>`
    - Removes batch from `in_flight`, returns entry_count
    - Removes all in-flight batches with seq <= batch_seq (cumulative ACK)
  - `fn window_state(&self) -> WindowState`
    - `Drained` if `in_flight` is empty
    - `Full` if `in_flight.len() >= config.window_size`
    - `Ready` otherwise
  - `fn timed_out_batches(&self, now_ms: u64) -> Vec<u64>` — batch_seqs that have timed out
  - `fn mark_retransmit(&mut self, batch_seq: u64)` — increment retransmit_count for a batch
  - `fn in_flight_count(&self) -> usize`
  - `fn next_seq(&self) -> u64` — next batch sequence number to assign
  - `fn stats(&self) -> WindowStats`

- `WindowStats` struct:
  - `total_sent: u64`, `total_acked: u64`, `total_timed_out: u64`, `total_retransmits: u64`
  - `current_in_flight: usize`

- Add `WindowFull` to `ReplError` enum in `error.rs`. Wait — do not modify `error.rs` directly; instead define a local enum:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum WindowError {
      #[error("sliding window is full: {0} batches in flight")]
      Full(usize),
      #[error("batch seq {0} not found in flight")]
      NotFound(u64),
  }
  ```
  Make `send_batch` return `Result<u64, WindowError>` and `acknowledge` return `Result<usize, WindowError>`.

- Unit tests (25+ tests in `mod tests`) covering:
  1. `test_window_new_default_config`
  2. `test_window_send_batch_assigns_seq`
  3. `test_window_send_increments_next_seq`
  4. `test_window_acknowledge_removes_batch`
  5. `test_window_cumulative_ack`
  6. `test_window_full_rejects_send`
  7. `test_window_state_transitions`
  8. `test_window_state_ready_when_below_limit`
  9. `test_window_state_drained_when_empty`
  10. `test_window_timed_out_batches_empty_when_fresh`
  11. `test_window_timed_out_batches_after_timeout`
  12. `test_window_mark_retransmit`
  13. `test_window_in_flight_count`
  14. `test_window_stats_sent`
  15. `test_window_stats_acked`
  16. `test_window_ack_unknown_returns_error`
  17. `test_window_ack_cumulative_removes_older`
  18. `test_window_send_ack_cycle`
  19. `test_window_multiple_timeouts`
  20. `test_window_retransmit_count_increments`
  21. `test_window_stats_timed_out_count`
  22. `test_window_drain_all_via_ack`
  23. `test_window_next_seq_starts_at_one`
  24. `test_window_in_flight_entry_count_preserved`
  25. `prop_window_send_ack_order_preserved` (proptest: send N batches, ack in order, total acked == N)

---

### 3. `src/catchup.rs`

A state machine managing the catch-up protocol for a replica that has fallen behind.

When a replica connects to the primary (or after detecting a gap), it requests catch-up:
the primary replays journal entries from the replica's last cursor forward.

Requirements:

- `CatchupConfig` struct (with `Default`):
  - `consumer_id: String` — identifier for this catch-up session (default: "default-catchup")
  - `max_batch_size: usize` — max entries per catch-up batch (default: 500)
  - `timeout_ms: u64` — time to wait for catch-up completion before giving up (default: 30_000)

- `CatchupPhase` enum (with `Debug`, `Clone`, `PartialEq`):
  - `Idle` — not running
  - `Requested { cursor_seq: u64 }` — request sent, waiting for first batch
  - `InProgress { cursor_seq: u64, batches_received: u32 }` — receiving batches
  - `Complete { final_seq: u64, total_entries: u64 }` — all caught up
  - `Failed { reason: String }` — catch-up failed

- `CatchupStats` struct (with `Default`, `Clone`):
  - `sessions_started: u32`, `sessions_completed: u32`, `sessions_failed: u32`
  - `total_entries_received: u64`, `total_batches_received: u32`

- `CatchupState` struct:
  - Fields: `config: CatchupConfig`, `phase: CatchupPhase`, `stats: CatchupStats`
  - Constructor: `new(config: CatchupConfig) -> Self`
  - `fn request(&mut self, from_seq: u64) -> Result<(), CatchupError>`
    - Transitions `Idle` → `Requested { cursor_seq: from_seq }`
    - Returns `Err(CatchupError::AlreadyRunning)` if not Idle
  - `fn receive_batch(&mut self, entry_count: usize, is_final: bool, final_seq: u64) -> Result<(), CatchupError>`
    - Transitions `Requested` → `InProgress` (first batch) or `InProgress` → `InProgress` (more)
    - On `is_final`: transitions to `Complete { final_seq, total_entries }`
    - Returns `Err(CatchupError::UnexpectedBatch)` if phase is not Requested/InProgress
    - Accumulates `total_entries_received` stats
  - `fn fail(&mut self, reason: impl Into<String>)`
    - Transitions any active state → `Failed { reason }`
    - Increments `stats.sessions_failed`
  - `fn reset(&mut self)`
    - Transitions any state → `Idle`
  - `fn phase(&self) -> &CatchupPhase`
  - `fn stats(&self) -> &CatchupStats`
  - `fn is_running(&self) -> bool` — true if Requested or InProgress
  - `fn is_complete(&self) -> bool` — true if Complete

- `CatchupError` enum (thiserror):
  - `#[error("catch-up already running")]` AlreadyRunning
  - `#[error("unexpected batch in phase {0:?}")]` UnexpectedBatch(String)

- Unit tests (25+ tests in `mod tests`) covering:
  1. `test_catchup_new_default_config`
  2. `test_catchup_initial_phase_idle`
  3. `test_catchup_request_transitions_to_requested`
  4. `test_catchup_request_while_running_fails`
  5. `test_catchup_first_batch_transitions_to_in_progress`
  6. `test_catchup_multiple_batches_accumulate`
  7. `test_catchup_final_batch_completes`
  8. `test_catchup_unexpected_batch_when_idle_fails`
  9. `test_catchup_fail_transitions_to_failed`
  10. `test_catchup_reset_from_failed`
  11. `test_catchup_reset_from_complete`
  12. `test_catchup_reset_from_in_progress`
  13. `test_catchup_is_running_true_when_active`
  14. `test_catchup_is_running_false_when_idle`
  15. `test_catchup_is_complete_false_when_in_progress`
  16. `test_catchup_is_complete_true_after_final`
  17. `test_catchup_stats_sessions_started`
  18. `test_catchup_stats_sessions_completed`
  19. `test_catchup_stats_sessions_failed`
  20. `test_catchup_stats_total_entries`
  21. `test_catchup_stats_total_batches`
  22. `test_catchup_complete_final_seq_recorded`
  23. `test_catchup_complete_total_entries_correct`
  24. `test_catchup_fail_reason_preserved`
  25. `prop_catchup_session_lifecycle` (proptest: request → N batches → final → complete)

---

### 4. Modify `src/lib.rs`

Add the three new module declarations at the end of the existing declarations:
```rust
/// Trait-based interface for journal sources (A2 integration boundary).
pub mod journal_source;
/// Sliding window acknowledgment protocol for reliable in-order delivery.
pub mod sliding_window;
/// Catch-up state machine for replicas that fall behind.
pub mod catchup;
```

---

## Existing code you MUST use

From `crates/claudefs-repl/src/journal.rs`:
```rust
use claudefs_repl::journal::{JournalEntry, OpKind};
// JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload) -> Self
// JournalEntry fields: seq, shard_id, site_id, timestamp_us, inode, op, payload, crc32
```

From `crates/claudefs-repl/src/error.rs`:
```rust
use claudefs_repl::error::ReplError;
// ReplError is a thiserror enum with variants like NotConnected, ProtocolError, etc.
```

## Dependencies already in Cargo.toml (DO NOT modify Cargo.toml)

```toml
tokio = { version = "1.40", features = ["full"] }
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
rand = "0.8"
bytes = "1"
```

## Constraints

- NO new crate dependencies. Use only what's already in Cargo.toml.
- NO async_trait. Use synchronous methods or return types explicitly.
- NO unsafe code.
- All tests must be in `#[cfg(test)] mod tests { ... }` within each file.
- proptest is in dev-dependencies: `proptest = "1"`.
- Every test must be named exactly as listed above.
- Use `use proptest::prelude::*;` inside the proptest test modules.

## File paths

- `crates/claudefs-repl/src/journal_source.rs`
- `crates/claudefs-repl/src/sliding_window.rs`
- `crates/claudefs-repl/src/catchup.rs`
- Modify `crates/claudefs-repl/src/lib.rs` (add 3 module declarations at the end)

Write all 4 files completely. After writing, verify by showing the cargo build command output would be clean.
