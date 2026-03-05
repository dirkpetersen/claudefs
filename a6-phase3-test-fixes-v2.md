# A6 Phase 3: Fix 13 Failing Tests — DETAILED ANALYSIS

## Executive Summary
Fix 13 failing tests in claudefs-repl (865/878 passing). Root causes identified:
1. **conduit_pool** (9 tests): Connections start in Failed state but require 3 reconnect attempts to reach Ready. Tests expect immediate or single-tick transition.
2. **entry_dedup** (3 tests): The `is_duplicate()` method updates stats (increments total_checked), but tests expect it to be read-only.
3. **repl_filter** (1 test): Stats tracking not working correctly (entries_passed not incremented).

## DETAILED FIXES

### FIX 1: conduit_pool.rs (9 failures)

**File**: `crates/claudefs-repl/src/conduit_pool.rs`

**Root Cause**:
```
register_site() creates connections in ConnectionState::Failed { reason: "initial", failed_at_ms: now_ms }
tick() transitions: Failed → Reconnecting (after initial_reconnect_delay_ms) → Ready (after 3 attempts)
Tests call register_site() + tick(now + 1000ms) once
But initial_reconnect_delay_ms = 500ms, so tick hits delay, creates Reconnecting with attempt=1
Then needs 2 more tick() calls to increment attempt to 3
```

**Solution**: Change the reconnection logic to differentiate between:
- **Initial connections** (just registered, Failed state with reason "initial"): transition directly to Ready after delay
- **Real failures** (reconnection attempts after a connection was healthy then failed): require 3 attempts

**Code Change in `tick()` method**:
```rust
pub fn tick(&mut self, now_ms: u64) {
    for connections in self.sites.values_mut() {
        for conn in connections.iter_mut() {
            match &conn.state {
                ConnectionState::Failed { failed_at_ms, reason } => {
                    let delay = self.config.initial_reconnect_delay_ms;
                    if *failed_at_ms + delay <= now_ms {
                        // Initial connections transition directly to Ready
                        // (reason == "initial" indicates first-time setup)
                        if reason == "initial" {
                            conn.state = ConnectionState::Ready;
                            tracing::info!(conn_id = conn.conn_id, "initial connection ready");
                        } else {
                            // Real failures: transition to Reconnecting with exponential backoff
                            conn.state = ConnectionState::Reconnecting {
                                attempt: 1,
                                next_retry_ms: now_ms + self.config.initial_reconnect_delay_ms,
                            };
                        }
                    }
                }
                ConnectionState::Reconnecting {
                    attempt,
                    next_retry_ms,
                } => {
                    if *next_retry_ms <= now_ms {
                        let delay = (self.config.initial_reconnect_delay_ms as f64
                            * self.config.backoff_multiplier.powi(*attempt as i32))
                        .min(self.config.max_reconnect_delay_ms as f64)
                            as u64;

                        if *attempt >= 3 {
                            conn.state = ConnectionState::Ready;
                            tracing::info!(conn_id = conn.conn_id, "connection reconnected after {} attempts", attempt);
                        } else {
                            conn.state = ConnectionState::Reconnecting {
                                attempt: attempt + 1,
                                next_retry_ms: now_ms + delay,
                            };
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
```

**Alternative Approach**: If the above is too invasive, add a test helper function:
```rust
#[cfg(test)]
fn tick_until_ready(pool: &mut ConduitPool, site_id: u64, now_ms: u64) {
    // Helper to rapidly transition initial connections to Ready for testing
    // Calls tick() multiple times or forces connections to Ready state
}
```

Then update all conduit_pool tests to call this helper instead of single tick().

**Recommendation**: Use the first approach (check reason == "initial") as it's the correct design semantics.

---

### FIX 2: entry_dedup.rs (3 failures)

**File**: `crates/claudefs-repl/src/entry_dedup.rs`

**Root Cause**:
The `is_duplicate()` method increments `total_checked` stats counter, but tests expect it to be read-only. Current implementation:
```rust
pub fn is_duplicate(&mut self, seq: u64, fingerprint: u64) -> bool {
    self.stats.total_checked += 1;  // <-- THIS is the problem
    // ... check logic ...
}
```

But test expects:
```rust
dedup.record(1, 1, now);
let is_dup = dedup.is_duplicate(1, 1);  // Should NOT increment stats.total_checked
assert_eq!(stats.total_checked, 0);  // <-- expects 0, not 1
```

**Solution**: Make `is_duplicate()` a read-only query method (no stats update). Only `record()` updates stats:

```rust
/// Check if an entry is a duplicate without updating stats.
pub fn is_duplicate(&self, seq: u64, fingerprint: u64) -> bool {
    // Read-only check, no stats update
    let ring_pos = (seq as usize) % self.config.ring_size;
    match &self.ring[ring_pos] {
        Some(entry) => entry.fingerprint == fingerprint,
        None => false,
    }
}

/// Record an entry as seen and update stats.
pub fn record_and_check(&mut self, seq: u64, fingerprint: u64, now_ms: u64) -> bool {
    // This method increments total_checked and total_duplicates
    self.stats.total_checked += 1;

    let ring_pos = (seq as usize) % self.config.ring_size;
    let is_dup = match &self.ring[ring_pos] {
        Some(entry) => entry.fingerprint == fingerprint,
        None => false,
    };

    if is_dup {
        self.stats.total_duplicates += 1;
    }

    self.ring[ring_pos] = Some(DedupEntry {
        seq,
        fingerprint,
        recorded_at_ms: now_ms,
    });

    is_dup
}
```

**Or simpler**: Keep both methods but have `is_duplicate()` not update stats and add comment:
```rust
/// Check if an entry is a duplicate. This is a read-only operation and does NOT update stats.
pub fn is_duplicate(&self, seq: u64, fingerprint: u64) -> bool {
    let ring_pos = (seq as usize) % self.config.ring_size;
    match &self.ring[ring_pos] {
        Some(entry) => entry.fingerprint == fingerprint,
        None => false,
    }
}

/// Record an entry and check for duplicates. Updates stats.
pub fn record(&mut self, seq: u64, fingerprint: u64, now_ms: u64) {
    self.stats.total_checked += 1;
    let is_dup = self.is_duplicate(seq, fingerprint);
    if is_dup {
        self.stats.total_duplicates += 1;
    }
    self.ring[(seq as usize) % self.config.ring_size] = Some(DedupEntry {
        seq,
        fingerprint,
        recorded_at_ms: now_ms,
    });
}
```

**Affected Tests**:
1. `test_is_duplicate_without_recording` — Now passes: is_duplicate() doesn't update stats
2. `test_evict_clears_old_entries_but_keeps_fresh_ones` — Fix: evict() should preserve fresh entries (test expects Entry 2 still present)
3. `test_different_seq_same_fingerprint_not_duplicate` — Fix: Different seq numbers should not be detected as duplicates even with same fingerprint. Check ring_pos calculation.

---

### FIX 3: repl_filter.rs (1 failure)

**File**: `crates/claudefs-repl/src/repl_filter.rs`

**Root Cause**:
Test `test_stats_track_correctly` expects `stats.entries_passed` to be 2, but it's 0. Likely:
1. `should_replicate()` returns wrong boolean values, or
2. Stats increment logic is not being reached, or
3. Test setup doesn't properly initialize filters

**Test Expectation** (line 535):
```rust
assert_eq!(stats.entries_passed, 2);  // left=0, right=2 — stats not being incremented
```

**Solution**:
1. Verify that `should_replicate()` is being called and returning true for expected entries
2. Verify that stats increment happens in the right place:
   ```rust
   if self.should_replicate(&entry) {
       self.stats.entries_passed += 1;  // Make sure this line executes
   }
   ```
3. Check test setup: ensure filter rules are initialized to allow entries through
4. Verify stats.entries_passed is incremented, not some other field

**Debug Steps**:
- Add `tracing::info!()` calls before and after `should_replicate()` check
- Verify filter config allows entries (e.g., default rule is "allow all" not "deny all")
- Check if stats are being reset unexpectedly before assertion

---

## Implementation Order

1. **Fix conduit_pool first** (9 tests) — all other conduit_pool tests likely pass once this is fixed
2. **Fix entry_dedup next** (3 tests) — independent from others
3. **Fix repl_filter** (1 test) — independent, should be quick debug

## Testing Strategy

After each fix:
```bash
cargo test -p claudefs-repl --lib [module_name]::tests
```

Then full test:
```bash
cargo test -p claudefs-repl --lib
```

Expected final result: **878 passing, 0 failing**

## Expected Code Modifications

- **conduit_pool.rs**: ~15 lines (tick() method change)
- **entry_dedup.rs**: ~20 lines (is_duplicate() made read-only, record() updated)
- **repl_filter.rs**: ~5-10 lines (debug stats tracking)

**Total impact**: ~40-50 lines modified across 3 files. No new files. No public API changes beyond entry_dedup separation.
