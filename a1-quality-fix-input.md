# Fix Code Quality Issues and Add Documentation in claudefs-storage

## Your Task

Fix all non-documentation clippy warnings in the `claudefs-storage` crate, AND add comprehensive documentation (doc comments) to ALL public items in the following files:

1. `crates/claudefs-storage/src/tiering_policy.rs` — ~40+ missing_docs warnings
2. `crates/claudefs-storage/src/recovery.rs` — ~60+ missing_docs warnings
3. `crates/claudefs-storage/src/hot_swap.rs` — ~8 missing_docs warnings

## Part 1: Code Quality Fixes (Non-Documentation Warnings)

### Fix 1: hot_swap.rs — Unused imports
File: `crates/claudefs-storage/src/hot_swap.rs`, line 15:
```rust
// BEFORE:
use crate::block::{BlockId, BlockRef, BlockSize};
// AFTER:
use crate::block::BlockRef;
```
Remove `BlockId` and `BlockSize` from the import since they're not used.

### Fix 2: scrub.rs — Unused variable
File: `crates/claudefs-storage/src/scrub.rs`, line 274:
```rust
// BEFORE:
let (blocks_checked, errors_found, errors_repaired) = match &self.state {
// AFTER:
let (blocks_checked, errors_found, _errors_repaired) = match &self.state {
```
Prefix with underscore to suppress warning.

### Fix 3: tiering_policy.rs — Unused variable `record` at line 271
File: `crates/claudefs-storage/src/tiering_policy.rs`:
In the `get_eviction_candidates` method, around line 271, there is:
```rust
let record = &self.access_records[&segment_id];
```
This variable `record` is never used in the closure body. Remove this line entirely.

### Fix 4: tiering_policy.rs — Nested `if` can be collapsed
File: `crates/claudefs-storage/src/tiering_policy.rs`, around lines 221-225:
```rust
// BEFORE:
if record.bytes_written > 0 && record.bytes_read > 0 {
    if record.bytes_read > record.bytes_written * 5 {
        return AccessPattern::WriteOnceReadMany;
    }
}

// AFTER:
if record.bytes_written > 0 && record.bytes_read > 0
    && record.bytes_read > record.bytes_written * 5 {
    return AccessPattern::WriteOnceReadMany;
}
```

### Fix 5: tiering_policy.rs — `contains_key` + `insert` on HashMap
File: `crates/claudefs-storage/src/tiering_policy.rs`, around lines 393-395 in `register_segment`:
```rust
// BEFORE:
if !self.current_tiers.contains_key(&segment_id) {
    self.current_tiers.insert(segment_id, TierClass::Cold);
}
// AFTER:
self.current_tiers.entry(segment_id).or_insert(TierClass::Cold);
```

### Fix 6: Find and fix `std::io::Error::new(std::io::ErrorKind::Other, msg)`
Search for this pattern in the codebase and replace with `std::io::Error::other(msg)`.

### Fix 7: s3_tier.rs — Derive `Default` for `TieringMode`
File: `crates/claudefs-storage/src/s3_tier.rs`:
Find the `TieringMode` enum and its `impl Default` block. Replace the manual impl with a derive:
```rust
// BEFORE: manual impl Default
impl Default for TieringMode {
    fn default() -> Self {
        TieringMode::Cache
    }
}

// AFTER: mark the default variant and derive
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]  // add Default
pub enum TieringMode {
    #[default]  // add this
    Cache,
    Tiered,
}
// Remove the manual impl block entirely
```

### Fix 8: quota.rs — `drop` of reference
File: `crates/claudefs-storage/src/quota.rs`, around line 324:
```rust
// BEFORE:
drop(tenant);
// AFTER:
let _ = tenant;
```

## Part 2: Add Documentation to tiering_policy.rs

Add `///` doc comments to ALL public items in `crates/claudefs-storage/src/tiering_policy.rs`.

Also add a module-level doc comment at the top of the file:
```rust
//! Tiering policy engine for intelligent flash/S3 data placement.
//!
//! This module classifies storage segments by access pattern and heat,
//! making automated decisions about whether data should reside on flash
//! or be evicted to S3-compatible object storage.
```

Items that need doc comments (add appropriate `///` before each):

**Enums and variants:**
- `TierClass` enum: "Storage tier classification for a segment."
  - `Hot`: "Frequently accessed data kept in the fastest flash tier."
  - `Warm`: "Moderately accessed data in standard flash."
  - `Cold`: "Infrequently accessed data, candidate for S3 tiering."
  - `Frozen`: "Data not accessed within the frozen threshold, moved to S3."
- `TierOverridePolicy` enum: "Manual override policy for segment tier placement."
  - `Auto`: "Automatic tier management based on access patterns."
  - `PinFlash`: "Force segment to remain on flash regardless of access."
  - `ForceS3`: "Force segment to S3 regardless of access frequency."
- `AccessPattern` enum: "Detected access pattern for a segment."
  - `Sequential`: "Predominantly sequential read access."
  - `Random`: "Predominantly random read access."
  - `WriteOnceReadMany`: "Written once then read many times (WORM-like)."
  - `WriteHeavy`: "Write-dominated with few or no reads."
  - `ReadOnce`: "Accessed only once for reading."
  - `Unknown`: "Insufficient data to classify the pattern."

**Structs and fields:**
- `AccessRecord` struct: "Access history record for a single storage segment."
  - `segment_id`: "Unique identifier for the segment."
  - `access_count`: "Total number of accesses (reads + writes)."
  - `last_access_time`: "Timestamp of the most recent access."
  - `first_access_time`: "Timestamp of the first recorded access."
  - `bytes_read`: "Total bytes read from this segment."
  - `bytes_written`: "Total bytes written to this segment."
  - `sequential_read_count`: "Number of sequential read operations."
  - `random_read_count`: "Number of random read operations."
  - `size_bytes`: "Physical size of the segment in bytes."
- `TieringDecision` struct: "A tiering recommendation for a segment."
  - `segment_id`: "Segment this decision applies to."
  - `current_tier`: "Current tier where the segment resides."
  - `recommended_tier`: "Recommended destination tier."
  - `score`: "Eviction score (higher = stronger eviction candidate)."
  - `pattern`: "Detected access pattern that influenced this decision."
  - `override_policy`: "Any manual override policy in effect."
  - `reason`: "Human-readable reason for the recommendation."
- `TieringPolicyConfig` struct: "Configuration for the tiering policy engine."
  - `analysis_window_secs`: "Time window in seconds for access pattern analysis."
  - `hot_threshold`: "Minimum access count to classify a segment as Hot."
  - `warm_threshold`: "Minimum access count to classify a segment as Warm."
  - `frozen_after_secs`: "Seconds without access before classifying as Frozen."
  - `recency_weight`: "Weight applied to recency in eviction scoring."
  - `size_weight`: "Weight applied to segment size in eviction scoring."
  - `frequency_weight`: "Weight applied to access frequency in eviction scoring."
  - `high_watermark`: "Flash utilization fraction triggering eviction (e.g., 0.8 = 80%)."
  - `low_watermark`: "Flash utilization fraction stopping eviction (e.g., 0.6 = 60%)."
- `TieringPolicyStats` struct: "Operational statistics for the tiering policy engine."
  - `decisions_made`: "Total tiering decisions made."
  - `promotions_to_hot`: "Number of segments promoted to Hot tier."
  - `demotions_to_cold`: "Number of segments demoted to Cold tier."
  - `demotions_to_frozen`: "Number of segments demoted to Frozen tier."
  - `overrides_applied`: "Number of times a manual override was applied."
  - `patterns_detected`: "Number of access patterns successfully classified."
  - `eviction_candidates`: "Total segments identified as eviction candidates."
- `TieringPolicyEngine` struct: "Classifies segments by heat and access pattern for automated tiering."

**Methods on TieringPolicyEngine:**
- `new`: "Creates a new tiering policy engine with the given configuration."
- `record_access`: "Records an I/O access event for a segment."
- `set_override`: "Sets a manual tier override for a segment."
- `get_override`: "Returns the current tier override policy for a segment."
- `classify_segment`: "Classifies a segment's current tier based on access history."
- `detect_pattern`: "Detects the dominant access pattern for a segment."
- `compute_eviction_score`: "Computes the eviction score for a segment (higher = evict sooner)."
- `get_eviction_candidates`: "Returns the top eviction candidates ranked by score."
- `make_decision`: "Makes a full tiering decision for a segment."
- `register_segment`: "Registers a new segment with the policy engine."
- `remove_segment`: "Removes a segment from tracking."
- `segment_count`: "Returns the number of tracked segments."
- `stats`: "Returns current policy engine statistics."
- `get_tier`: "Returns the current tier for a segment, if tracked."

## Part 3: Add Documentation to recovery.rs

Add `///` doc comments to ALL public items in `crates/claudefs-storage/src/recovery.rs`.

Module-level doc comment at the top:
```rust
//! Crash recovery and journal replay for the storage engine.
//!
//! This module provides crash-consistent recovery by replaying the write
//! journal after an unclean shutdown, validating the superblock, and
//! reconstructing the allocator bitmap from persisted state.
```

Items needing documentation:

- `JOURNAL_CHECKPOINT_MAGIC`: "Magic number identifying a valid journal checkpoint record."
- `RecoveryConfig` struct: "Configuration for the recovery manager."
  - `cluster_uuid`: "UUID of the cluster, used to validate superblock ownership."
  - `max_journal_replay_entries`: "Maximum number of journal entries to replay per recovery."
  - `verify_checksums`: "Whether to verify checksums during recovery."
  - `allow_partial_recovery`: "Whether to allow partial recovery when some entries are corrupt."
- `RecoveryPhase` enum: "Current phase of the recovery process."
  - `NotStarted`: "Recovery has not yet been initiated."
  - `SuperblockRead`: "Superblock has been read and validated."
  - `BitmapLoaded`: "Allocator bitmap has been loaded from disk."
  - `JournalScanned`: "Journal entries have been scanned."
  - `JournalReplayed`: "Journal entries have been replayed."
  - `Complete`: "Recovery completed successfully."
  - `Failed`: "Recovery failed due to unrecoverable errors."
- `RecoveryState` struct: "Runtime state of an in-progress or completed recovery."
  - `phase`: "Current recovery phase."
  - `devices_discovered`: "Number of devices discovered during recovery."
  - `devices_valid`: "Number of devices with valid superblocks."
  - `journal_entries_found`: "Number of journal entries discovered."
  - `journal_entries_replayed`: "Number of journal entries successfully replayed."
  - `errors`: "List of errors encountered during recovery."
- `AllocatorBitmap` struct: "Bit-packed allocator bitmap for tracking block allocation state."
  - `new`: "Creates a new all-free bitmap for the given number of blocks."
  - `from_bytes`: "Deserializes a bitmap from raw bytes with validation."
  - `to_bytes`: "Serializes the bitmap to a byte vector."
  - `set_allocated`: "Marks a range of blocks as allocated."
  - `set_free`: "Marks a range of blocks as free."
  - `is_allocated`: "Returns true if the block at the given offset is allocated."
  - `allocated_count`: "Returns the total number of allocated blocks."
  - `free_count`: "Returns the total number of free blocks."
  - `allocated_ranges`: "Returns all contiguous allocated ranges as (start_offset, count) pairs."
- `JournalCheckpoint` struct: "Persistent checkpoint record that marks a consistent journal state."
  - `magic`: "Magic number for validation (must equal JOURNAL_CHECKPOINT_MAGIC)."
  - `last_committed_sequence`: "Sequence number of the last committed journal entry."
  - `last_flushed_sequence`: "Sequence number of the last entry flushed to persistent storage."
  - `checkpoint_timestamp_secs`: "Unix timestamp when this checkpoint was written."
  - `checksum`: "CRC32 checksum covering all other fields."
  - `new`: "Creates a new checkpoint for the given committed and flushed sequence numbers."
  - `validate`: "Validates the checkpoint magic and checksum."
  - `to_bytes`: "Serializes the checkpoint to a byte vector."
  - `from_bytes`: "Deserializes a checkpoint from raw bytes."
  - `compute_checksum`: "Computes the CRC32 checksum of this checkpoint's content."
  - `update_checksum`: "Recomputes and updates the stored checksum."
- `RecoveryReport` struct: "Summary report produced at the end of a recovery operation."
  - `phase`: "Final phase reached during recovery."
  - `devices_discovered`: "Number of devices found."
  - `devices_valid`: "Number of devices with valid state."
  - `journal_entries_found`: "Total journal entries discovered."
  - `journal_entries_replayed`: "Journal entries that were replayed."
  - `errors`: "Any errors encountered."
  - `duration_ms`: "Total recovery duration in milliseconds."
- `RecoveryManager` struct: "Orchestrates crash recovery for the storage engine."
  - `new`: "Creates a new recovery manager with the given configuration."
  - `validate_superblock`: "Validates a serialized superblock and returns the parsed result."
  - `load_bitmap`: "Loads and validates an allocator bitmap from raw bytes."
  - `scan_journal_entries`: "Scans raw journal data and returns all valid entries."
  - `entries_needing_replay`: "Filters journal entries to those that need replay after a checkpoint."
  - `report`: "Returns the current recovery report."
  - `state`: "Returns the current recovery state."
  - `mark_complete`: "Marks recovery as successfully completed."
  - `mark_failed`: "Marks recovery as failed with an error message."
  - `add_error`: "Records a non-fatal error encountered during recovery."

## Part 4: Add Documentation to hot_swap.rs Error Variants

File: `crates/claudefs-storage/src/hot_swap.rs`

Add `///` doc comments to the `HotSwapError` enum variants:
- `DeviceNotFound(u16)`: "The specified device index was not found in the pool."
- `InvalidStateTransition { from: DeviceState, to: DeviceState }`: "The requested state transition is not allowed."
  - field `from`: "Current state of the device."
  - field `to`: "Requested target state."
- `NotDrainable(u16, DeviceState)`: "The device cannot be drained in its current state."
- `NotRemovable(u16, DeviceState)`: "The device cannot be removed in its current state."
- `AlreadyRegistered(u16)`: "A device with this index is already registered."
- `DeviceFailed(u16, String)`: "The device operation failed."

## Constraints

1. Do NOT change any test code
2. Do NOT modify function signatures, struct layouts, or public API
3. Do NOT remove any existing doc comments
4. After making all changes, verify with `cargo check -p claudefs-storage`
5. Output a summary of what was changed

Focus on correctness — make all the changes described above precisely.
