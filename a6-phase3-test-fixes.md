# A6 Phase 3: Fix 13 Failing Tests

## Context
The `claudefs-repl` crate currently has 13 failing tests out of 878 total (865 passing). These are recent regressions in three modules:
- **conduit_pool.rs** — 9 failures (connection pool management for gRPC conduit)
- **entry_dedup.rs** — 3 failures (duplicate entry detection for cross-site replication)
- **repl_filter.rs** — 1 failure (selective replication filtering)

This is Phase 3 Production Readiness work: fix test failures from validation findings.

## Current Status
- A6 is at Phase 2 complete: 817 tests (+75 new modules)
- Recent test count: 878 tests with 13 failures
- This indicates incomplete implementations or test/implementation mismatches
- Goal: 100% passing tests before moving to new features

## Failing Tests Analysis

### 1. conduit_pool.rs (9 failures)

**Module**: `crates/claudefs-repl/src/conduit_pool.rs`

**Overview**: Connection pool for gRPC conduit connections to remote replication sites. Manages pre-established connections with round-robin load balancing, health checking, and graceful drain support.

**Key Issue**: The `tick()` method handles state transitions: Failed → Reconnecting → Ready.
- `register_site()` creates connections in Failed state
- Tests call `tick()` once expecting connections to become Ready
- But the implementation requires 3 reconnection attempts before Ready state

**Failing Tests**:
1. `test_acquire_returns_conn_and_marks_in_use` — Panics at `acquire()` with NoHealthyConnections error. Expects 3 ready + 1 in_use after register/tick. Fix: `tick()` needs to transition Failed → Ready in one call for testing, or tests need multiple tick() calls, OR add a test helper to force connections ready.

2. `test_release_returns_to_ready` — Same issue as above. Calls acquire/release expecting 4 ready connections after one tick(). Fix: Same as above.

3. `test_is_site_healthy_returns_true_with_ready_connections` — Expects healthy site after tick(), but no ready connections. Fix: Same root cause.

4. `test_round_robin_across_connections` — Can't acquire first connection, so round-robin test fails. Fix: Same root cause.

5. `test_mark_failed_transitions_to_failed` — Likely expects to acquire a connection first. Fix: Same root cause.

6. `test_shutdown_marks_all_connections_draining` — Can't get baseline connection state. Fix: Same root cause.

7. `test_site_stats_correct_per_site` — Can't acquire connections. Fix: Same root cause.

8. `test_tick_advances_reconnect_after_delay` — Specific test for tick() transitions. Panics suggest tick() not working as expected. May need to verify the reconnection logic.

9. `test_global_stats_aggregates_all_sites` — Can't acquire connections. Fix: Same root cause.

**Root Cause**: The tick() logic requires 3 reconnect attempts to transition from Failed to Ready:
```rust
if *attempt >= 3 {
    conn.state = ConnectionState::Ready;
} else {
    // increment attempt and set next retry
}
```

But tests only call tick() once. The delay is 500ms initially, but tests call tick(now + 1000) expecting immediate transition.

**Solution Options**:
A) Change tick() to transition Failed → Ready immediately when delay has elapsed (simpler, matches test expectations)
B) Add test helper function that calls tick() multiple times
C) Make PoolConfig::max_reconnect_attempts configurable and set it to 0 in tests
D) Add a test-only method to force connections to Ready state

**Recommendation**: Option A is best — Failed state should transition to Ready after initial_reconnect_delay_ms, not require 3 attempts. The 3-attempt logic seems intended for actual reconnection failures, not initial connections. For initial connections (just registered), they should go directly to Ready after delay.

Alternatively, check if the design intent is that connections should start in Ready state, not Failed state, when first registered via register_site().

### 2. entry_dedup.rs (3 failures)

**Module**: `crates/claudefs-repl/src/entry_dedup.rs`

**Overview**: Duplicate journal entry detection for cross-site replication. Detects if a journal entry was already processed to prevent double-applies.

**Failing Tests**:
1. `test_is_duplicate_without_recording` — Panics at line 305: `assert_eq!(stats.total_checked, 0)` but got 1. The test calls `is_duplicate()` without calling `record()` first, expecting the method to NOT increment stats.total_checked. But implementation is incrementing it. Fix: The `is_duplicate()` method should be a read-only operation that doesn't update stats. Or separate `is_duplicate_and_record()` vs `is_duplicate_check_only()`.

2. `test_evict_clears_old_entries_but_keeps_fresh_ones` — Similar issue: stats tracking incorrectly when checking duplicates. Fix: Same as above.

3. `test_different_seq_same_fingerprint_not_duplicate` — Stats tracking issue. Fix: Same as above.

**Root Cause**: The `is_duplicate()` method increments the `total_checked` counter even when called as a check-only operation. The tests expect `is_duplicate()` to be read-only and `record()` to be the operation that updates stats.

**Solution**: Separate the concerns:
- `is_duplicate(seq, fingerprint)` → bool (read-only, doesn't update stats)
- `record(seq, fingerprint, now_ms)` → bool (write operation, updates stats)

Or if current design has is_duplicate() update stats, then all tests that call is_duplicate() must account for that increment.

Check the module's public API and fix to match the intended design (likely read-only is_duplicate).

### 3. repl_filter.rs (1 failure)

**Module**: `crates/claudefs-repl/src/repl_filter.rs`

**Overview**: Selective replication filtering. Allows per-site policies to include/exclude replication entries based on rules.

**Failing Test**:
1. `test_stats_track_correctly` — Panics at line 535: `assert_eq!(left, right)` where left=0 but right=2. Test expects 2 stats to be tracked (likely entries matching filter rules), but got 0. Fix: Check if `should_replicate()` is returning correct values, or if stats are not being incremented when they should be.

**Root Cause**: Stats tracking not working correctly. Likely:
- `should_replicate()` returns wrong boolean
- Stats increment logic is broken
- Test assumptions about filter behavior don't match implementation

**Solution**: Review the test expectations and the `should_replicate()` logic. Verify that stats.entries_passed (or similar) is incremented when entries match the filter rules.

## Implementation Plan

### Step 1: Fix conduit_pool.rs
Design decision needed: Should Failed → Ready transition happen after initial_reconnect_delay_ms, or should initial connections start in Ready state?

**Most likely fix**: In `register_site()`, create connections in Ready state instead of Failed state. OR in `tick()`, treat initial Failed state differently (no attempt counting) — just wait for delay then go Ready.

### Step 2: Fix entry_dedup.rs
Verify that `is_duplicate()` is intended to be read-only. If so, remove stats updates from is_duplicate(). If write-heavy operations need stats, add a separate `record()` call or rename method to `is_duplicate_and_check()`.

### Step 3: Fix repl_filter.rs
Debug the stats tracking. Verify `should_replicate()` logic and stats increment calls.

## Code Changes Needed

1. **conduit_pool.rs**: Modify `register_site()` to create connections in Ready state, OR modify `tick()` to treat initial Failed connections differently (transition to Ready after initial_reconnect_delay_ms without attempt counting).

2. **entry_dedup.rs**: Verify or change `is_duplicate()` semantics to be read-only (no stats updates). Make `record()` responsible for stats updates.

3. **repl_filter.rs**: Debug and fix stats tracking logic.

## Expected Outcome
- All 13 tests pass
- `cargo test -p claudefs-repl --lib` shows 878 passing, 0 failing
- No regression in other tests
- Ready to proceed with Phase 3 feature work (e.g., active-active failover)

## Testing Strategy
1. Fix one module at a time (conduit_pool first, as it blocks other tests)
2. Run full test suite after each fix
3. Verify no new failures introduced
4. Document design decisions in code comments
