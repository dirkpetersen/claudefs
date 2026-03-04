# Task: Write repl_deep_security_tests_v2.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-repl` crate focusing on sliding window protocol attacks, split-brain fencing, conduit integrity, active-active conflicts, and catchup state machine security.

## File location
`crates/claudefs-security/src/repl_deep_security_tests_v2.rs`

## Module structure
```rust
//! Deep security tests v2 for claudefs-repl: sliding window, split-brain, conduit, active-active, catchup.
//!
//! Part of A10 Phase 8: Replication deep security audit v2

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs and module exploration)

```rust
use claudefs_repl::sliding_window::{SlidingWindow, WindowConfig, WindowError, WindowState, WindowStats};
use claudefs_repl::split_brain::{FencingToken, SplitBrainDetector, SplitBrainEvidence, SplitBrainState, SplitBrainStats};
use claudefs_repl::active_active::{ActiveActiveController, ActiveActiveStats, ForwardedWrite, LinkStatus, SiteRole, WriteConflict};
use claudefs_repl::catchup::{CatchupConfig, CatchupError, CatchupPhase, CatchupState, CatchupStats};
use claudefs_repl::checkpoint::{CheckpointManager, ReplicationCheckpoint};
use claudefs_repl::conflict_resolver::{ConflictRecord, ConflictResolver, ConflictType, SiteId};
use claudefs_repl::journal::{JournalEntry, OpKind};
use claudefs_repl::journal_source::{MockJournalSource, JournalSource, SourceBatch, SourceCursor, VecJournalSource};
use claudefs_repl::tls_policy::{TlsMode, TlsPolicyBuilder, TlsPolicyError, TlsValidator, TlsConfigRef};
use claudefs_repl::wal::ReplicationCursor;
use claudefs_repl::error::ReplError;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating

The existing `repl_security_tests.rs` (26 tests) covers: journal CRC, replay protection, batch auth, journal tailer, site registry, TLS mode enforcement, fencing tokens basic, auth rate limiting.

The existing `repl_phase2_security_tests.rs` (25 tests) covers: mock/vec journal sources, sliding window basics (send/ack/timeout), catchup basic lifecycle.

DO NOT duplicate any of these. Focus on edge cases, attack vectors, and findings they miss.

## Test categories (25 tests total, 5 per category)

### Category 1: Sliding Window Protocol Attacks (5 tests)

1. **test_window_out_of_order_ack** — Create SlidingWindow with window_size=5. Send 3 batches (seq 1,2,3). Acknowledge seq=3 (cumulative ACK). Verify that seqs 1 and 2 are also acknowledged (cumulative semantics). Verify in_flight_count==0. (FINDING: cumulative ACK means acknowledging seq=3 removes 1 and 2 — out-of-order ACK vulnerability).

2. **test_window_ack_future_seq** — Create SlidingWindow with window_size=5. Send 1 batch (seq=1). Try to acknowledge seq=999 (far future). Document whether this succeeds or fails. (FINDING: if accepted, phantom ACKs could corrupt window state).

3. **test_window_retransmit_count_overflow** — Create SlidingWindow. Send batch. Call mark_retransmit on same batch 1000 times. Verify retransmit_count increments correctly without overflow (saturating expected).

4. **test_window_zero_entry_batch** — Create SlidingWindow. Send batch with entry_count=0. Verify it's tracked in the window. (FINDING: zero-entry batches waste window slots without replicating data).

5. **test_window_full_backpressure** — Create SlidingWindow with window_size=3. Send 3 batches. Try to send 4th. Verify WindowError::Full. Acknowledge one. Send again. Verify success.

### Category 2: Split-Brain Fencing Security (5 tests)

6. **test_fencing_token_monotonic** — Create SplitBrainDetector. Issue 3 fencing tokens. Verify each is strictly greater than the previous.

7. **test_fencing_validate_old_token_rejected** — Create SplitBrainDetector. Issue token T1. Issue token T2. Validate T1 against current. Verify T1 is no longer valid (token < current).

8. **test_split_brain_confirm_without_partition** — Create SplitBrainDetector in Normal state. Try to confirm split-brain directly (without report_partition first). Document behavior. (FINDING: if allowed, false split-brain could be triggered).

9. **test_split_brain_heal_from_normal** — Create SplitBrainDetector in Normal state. Call mark_healed. Document whether it succeeds or is a no-op. Verify state remains Normal.

10. **test_split_brain_stats_tracking** — Create SplitBrainDetector. Report partition. Confirm split-brain. Issue fence. Mark healed. Verify stats: partitions_detected=1, split_brains_confirmed=1, fencing_tokens_issued=1, resolutions_completed=1.

### Category 3: Active-Active Conflict Resolution (5 tests)

11. **test_active_active_logical_time_increment** — Create ActiveActiveController. Perform 5 local_write operations. Verify logical_time increments by 1 each time (1,2,3,4,5).

12. **test_active_active_remote_conflict_lww** — Create 2 controllers (site 1 Primary, site 2 Secondary). Both write same key. Apply site2's write to site1. Verify conflict detected and winner is the one with higher logical_time (or site_id tiebreak).

13. **test_active_active_link_flap_counting** — Create controller. Set link Up, Down, Up, Down, Up. Verify link_flaps stat incremented for each Up transition (flap count = number of Up transitions after a Down).

14. **test_active_active_drain_pending_idempotent** — Create controller. Perform 3 local_writes. Call drain_pending() — verify returns 3 writes. Call drain_pending() again — verify returns empty vec.

15. **test_active_active_remote_write_from_past** — Create controller with logical_time=10. Apply remote write with logical_time=5. Document whether this is accepted or rejected. (FINDING: stale writes may overwrite current data).

### Category 4: Catchup State Machine Security (5 tests)

16. **test_catchup_request_while_running** — Create CatchupState. Call request(from_seq=0). Verify phase is Requested. Call request again. Verify CatchupError::AlreadyRunning.

17. **test_catchup_receive_batch_in_idle** — Create CatchupState (starts Idle). Call receive_batch without requesting first. Verify error (not in correct phase).

18. **test_catchup_zero_entry_batch** — Create CatchupState. Request. Receive batch with entry_count=0, is_final=false. Verify state transitions correctly but total_entries_received stays 0.

19. **test_catchup_fail_and_reset** — Create CatchupState. Request. Fail with reason. Verify phase is Failed. Reset. Verify phase is Idle. Request again. Verify success.

20. **test_catchup_stats_accumulation** — Create CatchupState. Complete 3 catchup sessions (request → receive_batch → complete each). Verify stats: sessions_started=3, sessions_completed=3, total entries and batches accumulated.

### Category 5: Checkpoint & Conflict Resolution Edge Cases (5 tests)

21. **test_checkpoint_fingerprint_deterministic** — Create 2 checkpoints with same cursors. Verify fingerprints are identical. Create 1 with different cursors. Verify fingerprint differs.

22. **test_checkpoint_max_zero** — Create CheckpointManager with max_checkpoints=0. Try to create a checkpoint. Verify it returns None (no storage capacity).

23. **test_checkpoint_serialization_roundtrip** — Create ReplicationCheckpoint. Call to_bytes(). Call from_bytes() on result. Verify all fields match original.

24. **test_conflict_resolver_identical_timestamps** — Create ConflictResolver. Resolve conflict where ts_a == ts_b and seq_a == seq_b. Verify deterministic winner (site_a wins as tiebreak).

25. **test_conflict_resolver_split_brain_count** — Create ConflictResolver. Resolve 3 normal conflicts and 2 that result in ManualResolutionRequired or SplitBrain. Verify split_brain_count() matches.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-REPL-DEEP2-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For JournalEntry, use JournalEntry { seq: N, site_id: 1, op: OpKind::Write, inode: 100, timestamp_ns: N * 1000 } or whatever constructor exists

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
