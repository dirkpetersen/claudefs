# A3 Data Reduction — Phase 3: Production Readiness

You are implementing Phase 3 production-readiness improvements for the `claudefs-reduce` crate in the ClaudeFS distributed filesystem project.

## Context

The crate already has 20 modules with 193 passing tests. Phase 3 adds:
1. A new `tiering.rs` module for hot/cold chunk access-pattern tracking
2. A new `audit_log.rs` module for WORM compliance audit trail
3. A fix to `key_rotation_scheduler.rs` to support sequential rotations (reset after Complete)
4. ~30 new tests across the new modules and existing ones

## Workspace location

All files live in `/home/cfs/claudefs/crates/claudefs-reduce/src/`.

## Existing modules (DO NOT MODIFY unless fixing the bug below)

- pipeline.rs, dedupe.rs, compression.rs, encryption.rs, error.rs, fingerprint.rs, gc.rs
- key_manager.rs, key_rotation_scheduler.rs, metrics.rs, recompressor.rs, segment.rs
- similarity.rs, snapshot.rs, worm_reducer.rs, meta_bridge.rs, async_meta_bridge.rs
- write_path.rs, checksum.rs, background.rs

## Existing lib.rs public API

```rust
pub mod async_meta_bridge;
pub mod background;
pub mod checksum;
pub mod compression;
pub mod dedupe;
pub mod encryption;
pub mod error;
pub mod fingerprint;
pub mod gc;
pub mod key_manager;
pub mod key_rotation_scheduler;
pub mod meta_bridge;
pub mod metrics;
pub mod pipeline;
pub mod recompressor;
pub mod segment;
pub mod similarity;
pub mod snapshot;
pub mod write_path;
pub mod worm_reducer;

// ... pub use statements for all the above
```

## Existing Cargo.toml dependencies (use only these)

```toml
[dependencies]
aes-gcm = { version = "0.10", features = ["aes"] }
blake3 = "1"
bincode = "1"
chacha20poly1305 = "0.10"
fastcdc = "3"
lz4_flex = "0.11"
rand = "0.8"
serde = { version = "1", features = ["derive"] }
thiserror = "2"
tracing = "0.1"
zstd = "0.13"
zeroize = { version = "1", features = ["derive"] }
proptest = { version = "1", optional = true }

[dev-dependencies]
proptest = "1"
tokio = { version = "1", features = ["full"] }
```

---

## Task 1: Create `tiering.rs`

Create `/home/cfs/claudefs/crates/claudefs-reduce/src/tiering.rs` with the following design.

### Purpose
Track access frequency for chunks to support intelligent hot/cold tiering decisions.
A `TierTracker` keeps per-chunk access counts and timestamps, and classifies chunks
as Hot, Warm, or Cold based on configurable thresholds.

### Required types and impl

```rust
/// Tier classification for a data chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierClass {
    /// Frequently accessed — keep on NVMe flash layer
    Hot,
    /// Moderately accessed — candidate for background recompression to Zstd
    Warm,
    /// Rarely accessed — tier to S3 object store
    Cold,
}

/// Per-chunk access record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    /// Number of accesses since last reset
    pub access_count: u64,
    /// Unix timestamp of last access
    pub last_access_ts: u64,
    /// Unix timestamp of first access (creation)
    pub first_access_ts: u64,
}

/// Configuration for tiering thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    /// Access count >= this → Hot
    pub hot_threshold: u64,
    /// Access count >= this and < hot_threshold → Warm
    pub warm_threshold: u64,
    /// Age (seconds) after which an unaccessed chunk becomes Cold
    pub cold_age_secs: u64,
}

impl Default for TierConfig { ... } // hot=10, warm=3, cold_age=86400 (1 day)

/// Tracks access patterns for chunks to drive tiering decisions.
pub struct TierTracker {
    config: TierConfig,
    records: HashMap<u64, AccessRecord>,  // chunk_id → AccessRecord
}

impl TierTracker {
    pub fn new(config: TierConfig) -> Self;

    /// Record an access to a chunk at the given Unix timestamp.
    pub fn record_access(&mut self, chunk_id: u64, now_ts: u64);

    /// Classify a chunk based on its access record. Returns Cold if not tracked.
    pub fn classify(&self, chunk_id: u64, now_ts: u64) -> TierClass;

    /// Returns all chunk IDs classified as the given tier at the given timestamp.
    pub fn chunks_in_tier(&self, tier: TierClass, now_ts: u64) -> Vec<u64>;

    /// Returns the access record for a chunk.
    pub fn get_record(&self, chunk_id: u64) -> Option<&AccessRecord>;

    /// Resets access counts for all chunks (for periodic decay, not timestamps).
    pub fn reset_counts(&mut self);

    /// Evict records for chunks not accessed since before cutoff_ts.
    pub fn evict_stale(&mut self, cutoff_ts: u64) -> usize;

    /// Returns total number of tracked chunks.
    pub fn len(&self) -> usize;

    /// Returns true if no chunks are tracked.
    pub fn is_empty(&self) -> bool;
}
```

### Tests for `tiering.rs` (in `#[cfg(test)] mod tests`)

Write at least 15 tests:
1. `test_new_tracker_is_empty` — new tracker has 0 records
2. `test_record_access_creates_record` — single access creates record
3. `test_classify_untracked_is_cold` — unknown chunk → Cold
4. `test_classify_below_warm_is_cold` — 1 access, recent → Cold
5. `test_classify_warm_threshold` — access_count == warm_threshold → Warm
6. `test_classify_hot_threshold` — access_count >= hot_threshold → Hot
7. `test_classify_old_chunk_is_cold` — old chunk with few accesses → Cold despite warm count
8. `test_record_access_increments_count` — multiple accesses increment count
9. `test_chunks_in_tier_hot` — list all hot chunks
10. `test_chunks_in_tier_cold` — list all cold chunks
11. `test_reset_counts` — after reset, all counts are 0
12. `test_reset_counts_classifies_cold` — after reset, previously hot chunk → Cold
13. `test_evict_stale_removes_old` — stale chunks evicted, fresh kept
14. `test_evict_stale_returns_count` — returns number of evicted chunks
15. `test_first_and_last_access_timestamps` — first_access_ts set on first access, last_access_ts updated on subsequent

---

## Task 2: Create `audit_log.rs`

Create `/home/cfs/claudefs/crates/claudefs-reduce/src/audit_log.rs` for WORM compliance audit trailing.

### Purpose
Log WORM policy events (policy set, hold placed, hold released, expiry checked, GC suppressed)
for compliance audit trails. Stores events in an in-memory ring buffer with configurable max capacity.
Each event has a monotonic sequence number, Unix timestamp, event type, chunk ID, and optional metadata.

### Required types and impl

```rust
use crate::worm_reducer::WormMode;

/// Type of WORM audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    /// A retention policy was set on a chunk
    PolicySet { mode: WormMode },
    /// A legal hold was placed
    HoldPlaced,
    /// A legal hold was released
    HoldReleased,
    /// An expiry check was performed
    ExpiryChecked { expired: bool },
    /// GC was suppressed due to active retention
    GcSuppressed,
    /// A policy was removed (retention period ended)
    PolicyRemoved,
}

/// A single WORM audit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Monotonically increasing sequence number
    pub seq: u64,
    /// Unix timestamp when the event occurred
    pub timestamp_ts: u64,
    /// The chunk this event pertains to
    pub chunk_id: u64,
    /// What happened
    pub kind: AuditEventKind,
}

/// Configuration for the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogConfig {
    /// Maximum number of events to retain (ring buffer)
    pub max_events: usize,
    /// Whether audit logging is enabled
    pub enabled: bool,
}

impl Default for AuditLogConfig { ... } // max_events=10000, enabled=true

/// Ring-buffer audit log for WORM compliance events.
pub struct AuditLog {
    config: AuditLogConfig,
    events: VecDeque<AuditEvent>,
    next_seq: u64,
}

impl AuditLog {
    pub fn new(config: AuditLogConfig) -> Self;

    /// Record an audit event. No-op if disabled.
    pub fn record(&mut self, chunk_id: u64, now_ts: u64, kind: AuditEventKind);

    /// Returns all events in order (oldest first).
    pub fn events(&self) -> impl Iterator<Item = &AuditEvent>;

    /// Returns events for a specific chunk (oldest first).
    pub fn events_for_chunk(&self, chunk_id: u64) -> Vec<&AuditEvent>;

    /// Returns the total number of events in the log.
    pub fn len(&self) -> usize;

    /// Returns true if the log is empty.
    pub fn is_empty(&self) -> bool;

    /// Clears all events.
    pub fn clear(&mut self);

    /// Returns events since a given sequence number (exclusive).
    pub fn events_since(&self, seq: u64) -> Vec<&AuditEvent>;

    /// Returns the sequence number of the most recent event, or None.
    pub fn last_seq(&self) -> Option<u64>;
}
```

### Tests for `audit_log.rs` (in `#[cfg(test)] mod tests`)

Write at least 15 tests:
1. `test_new_log_is_empty` — new log has 0 events
2. `test_record_disabled_noop` — when disabled, record does nothing
3. `test_record_single_event` — record one event, len == 1
4. `test_record_policy_set` — PolicySet event recorded correctly
5. `test_record_hold_placed_and_released` — two events, correct kinds
6. `test_events_for_chunk_filters` — only returns events for requested chunk
7. `test_seq_monotonically_increasing` — sequence numbers increment
8. `test_ring_buffer_eviction` — at max_events, oldest is evicted
9. `test_events_since` — events_since(N) returns only events with seq > N
10. `test_events_since_all` — events_since(0) returns all
11. `test_clear_resets_log` — clear empties events but NOT next_seq
12. `test_last_seq_none_when_empty` — empty log returns None
13. `test_last_seq_after_events` — last_seq returns seq of last event
14. `test_gc_suppressed_event` — GcSuppressed event round-trips
15. `test_expiry_checked_event` — ExpiryChecked event carries expired flag

---

## Task 3: Fix `key_rotation_scheduler.rs`

Fix the bug in `KeyRotationScheduler::schedule_rotation`: currently it returns an error
when called in `Complete` state, but it should allow re-scheduling (reset to `Scheduled`)
after a rotation completes. This enables multiple sequential key rotations.

**Current broken behavior:**
```rust
pub fn schedule_rotation(&mut self, target_version: KeyVersion) -> Result<(), ReduceError> {
    if !matches!(self.status, RotationStatus::Idle) {  // BUG: Complete is also not Idle
        return Err(ReduceError::EncryptionFailed("rotation already scheduled".to_string()));
    }
    ...
}
```

**Fix:** Change the condition to allow both `Idle` and `Complete` states:
```rust
if !matches!(self.status, RotationStatus::Idle | RotationStatus::Complete { .. }) {
    return Err(ReduceError::EncryptionFailed("rotation already scheduled".to_string()));
}
```

Also update the existing test `test_schedule_rotation_from_complete_fails` to instead verify
that scheduling from Complete state SUCCEEDS (rename it to `test_schedule_rotation_from_complete_succeeds`).

---

## Task 4: Update `lib.rs`

Add the two new modules to `lib.rs`:

```rust
pub mod audit_log;
pub mod tiering;
```

And add pub use statements:
```rust
pub use audit_log::{AuditEvent, AuditEventKind, AuditLog, AuditLogConfig};
pub use tiering::{AccessRecord, TierClass, TierConfig, TierTracker};
```

---

## Implementation requirements

1. Use `std::collections::{HashMap, VecDeque}` as needed
2. Use `serde::{Serialize, Deserialize}` on all public types
3. Use `tracing::debug!` for any significant state changes
4. All types must derive `Debug` where possible
5. Write all tests in `#[cfg(test)] mod tests` at the bottom of each file
6. Tests must be self-contained (no external files, no I/O)
7. No `unwrap()` in production code — use `?` operator with proper error types
8. No `unsafe` code — this crate is safe Rust only

## Output format

For each file, output:
```
=== FILE: crates/claudefs-reduce/src/tiering.rs ===
<complete file contents>

=== FILE: crates/claudefs-reduce/src/audit_log.rs ===
<complete file contents>

=== FILE: crates/claudefs-reduce/src/key_rotation_scheduler.rs ===
<complete file contents with the bug fix and updated test>

=== FILE: crates/claudefs-reduce/src/lib.rs ===
<complete file contents with two new modules added>
```

Output the complete files. Do not truncate. Do not add extra commentary outside the file blocks.
