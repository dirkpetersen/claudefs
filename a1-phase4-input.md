# Task: Implement 3 new Phase 4 modules for claudefs-storage

## Working directory
/home/cfs/claudefs

## Context
ClaudeFS storage crate currently has 38 modules (~28k lines, 809 tests).
Implement THREE new production-readiness modules. Follow existing conventions.

## Shared Conventions
- Error handling: `thiserror`
- Serialization: `serde` with `Serialize, Deserialize` derives
- Logging: `tracing` crate
- All public items: `///` doc comments (avoid `#[warn(missing_docs)]`)
- 20+ unit tests per module inside `#[cfg(test)] mod tests { ... }`
- NO `#[allow(dead_code)]` or suppression attributes
- NO async — all synchronous

## Available imports from within the crate
```rust
use crate::block::{BlockId, BlockRef, BlockSize, PlacementHint};
use crate::error::{StorageError, StorageResult};
use crate::checksum::{Checksum, ChecksumAlgorithm, compute, verify};
use crate::segment::{PackedSegment, SegmentEntry, SEGMENT_SIZE};
use crate::erasure::{EcProfile, ErasureCodingEngine};
use crate::background_scheduler::{BackgroundScheduler, BackgroundTask, BackgroundTaskType};
use crate::compaction::{CompactionConfig, CompactionEngine, SegmentId};
use crate::scrub::{ScrubEngine, ScrubConfig};
use crate::defrag::{DefragEngine, DefragConfig};
```

---

## Module 1: `crates/claudefs-storage/src/io_accounting.rs`

### Purpose
Per-tenant I/O accounting with sliding-window statistics. Tracks bytes read/written,
IOPS, and latency histograms per tenant ID. Used for quota enforcement and observability.

### Data structures

```rust
/// Unique tenant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub u64);

/// I/O operation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoDirection { Read, Write }

/// Aggregate I/O counters for a tenant over a time window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantIoStats {
    pub tenant_id: TenantId,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub total_latency_us: u64,  // sum of all op latencies for avg calculation
    pub max_latency_us: u64,
    pub window_start_secs: u64,
}

/// Configuration for the I/O accounting module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoAccountingConfig {
    pub window_secs: u64,     // sliding window size in seconds (default: 60)
    pub max_tenants: usize,   // max tracked tenants (default: 1024)
}

impl Default for IoAccountingConfig { ... } // window_secs=60, max_tenants=1024
```

### `IoAccounting` struct and methods
```rust
pub struct IoAccounting { ... }

impl IoAccounting {
    pub fn new(config: IoAccountingConfig) -> Self
    pub fn record_op(&mut self, tenant: TenantId, dir: IoDirection, bytes: u64, latency_us: u64)
    pub fn get_stats(&self, tenant: TenantId) -> Option<TenantIoStats>
    pub fn all_stats(&self) -> Vec<TenantIoStats>
    pub fn rotate_window(&mut self, current_secs: u64)  // expire old data, reset window
    pub fn tenant_count(&self) -> usize
    pub fn total_bytes_read(&self) -> u64  // sum across all tenants
    pub fn total_bytes_written(&self) -> u64
    pub fn top_tenants_by_bytes(&self, n: usize) -> Vec<TenantIoStats>  // sorted desc by total bytes
}
```

### Tests (25 tests)
1. New accounting has no tenants
2. Record single read op
3. Record single write op
4. Multiple ops same tenant accumulate
5. Different tenants tracked independently
6. total_bytes_read returns sum across tenants
7. total_bytes_written returns sum
8. top_tenants_by_bytes returns sorted desc
9. top_tenants_by_bytes with n > total returns all
10. top_tenants_by_bytes empty returns empty
11. rotate_window clears old data
12. After rotate_window, new ops start fresh
13. get_stats for unknown tenant returns None
14. max_latency_us tracks maximum
15. tenant_count returns correct count
16. tenant_count reaches max_tenants limit
17. IoDirection distinguishes read from write
18. TenantId(0) is valid
19. Record op for TenantId(u64::MAX)
20. all_stats returns all tracked tenants
21. Window keeps data within window_secs
22. Multiple windows accumulate correctly
23. Bytes counted per op (not per record call)
24. Default config has window_secs=60
25. Rotate window does not lose in-window data

---

## Module 2: `crates/claudefs-storage/src/block_verifier.rs`

### Purpose
End-to-end block integrity verification. Given a block's data and its stored checksum,
verifies integrity and reports corruption. Used by the scrub engine and recovery path.

### Data structures

```rust
/// Result of verifying a single block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub block_ref: BlockRef,
    pub passed: bool,
    pub computed_checksum: u32,  // CRC32c of the data
    pub stored_checksum: u32,    // checksum from the block header
    pub data_len: usize,
}

/// A block's data + stored checksum for verification.
#[derive(Debug, Clone)]
pub struct BlockToVerify {
    pub block_ref: BlockRef,
    pub data: Vec<u8>,
    pub stored_checksum: u32,
}

/// Aggregate statistics from a verification run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VerifierStats {
    pub blocks_checked: u64,
    pub blocks_passed: u64,
    pub blocks_failed: u64,
    pub total_bytes_verified: u64,
    pub corruptions_found: u64,
}

/// Configuration for the block verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    pub algorithm: VerifierAlgorithm,
    pub fail_fast: bool,  // stop on first corruption (default: false)
}

/// Checksum algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifierAlgorithm { Crc32c, Blake3 }
```

### `BlockVerifier` struct and methods
```rust
pub struct BlockVerifier { config: VerifierConfig, stats: VerifierStats }

impl BlockVerifier {
    pub fn new(config: VerifierConfig) -> Self
    pub fn verify_block(&mut self, block: &BlockToVerify) -> VerificationResult
    pub fn verify_batch(&mut self, blocks: &[BlockToVerify]) -> Vec<VerificationResult>
    pub fn stats(&self) -> VerifierStats
    pub fn reset_stats(&mut self)
    pub fn compute_checksum(&self, data: &[u8]) -> u32
}
```

**CRC32c implementation**: Use a simple CRC32c via polynomial `0x82F63B78`.
You may implement a simple table-based CRC32c or use Adler32 as a substitute.
Just be consistent: compute_checksum and verify_block use the same algorithm.

For BLAKE3: compute 32-byte hash, take first 4 bytes as u32 (little-endian).

### Tests (22 tests)
1. Verify block with correct CRC32c passes
2. Verify block with wrong checksum fails
3. Empty data verifies if checksum matches
4. 1-byte data
5. 64KB data verifies correctly
6. stats.blocks_checked increments
7. stats.blocks_failed increments on failure
8. stats.total_bytes_verified accumulates
9. reset_stats clears all counters
10. verify_batch with all passing returns all passed
11. verify_batch with one failing marks it
12. verify_batch with fail_fast=true stops early
13. verify_batch empty input returns empty
14. compute_checksum is deterministic
15. Different data gives different checksums
16. All-zeros data has valid checksum
17. All-0xFF data has valid checksum
18. VerifierAlgorithm::Crc32c and Blake3 differ for same data
19. corruptions_found increments on failed verification
20. blocks_passed + blocks_failed == blocks_checked
21. VerifierConfig::fail_fast=false processes all blocks
22. BlockToVerify with max-size block reference

---

## Module 3: `crates/claudefs-storage/src/compaction_manager.rs`

### Purpose
Orchestrates the full compaction pipeline: identify fragmented segments → schedule
background compaction → track in-flight compaction jobs → report results.
Coordinates compaction with scrub and defrag via BackgroundScheduler.

### Data structures

```rust
/// State of a compaction job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionJobState {
    Queued,      // waiting for background scheduler slot
    Running,     // compaction in progress
    Done,        // completed successfully
    Failed,      // failed with error
    Cancelled,   // cancelled before completion
}

/// Unique ID for a compaction job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompactionJobId(pub u64);

impl std::fmt::Display for CompactionJobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompJob-{}", self.0)
    }
}

/// A single compaction job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionJob {
    pub id: CompactionJobId,
    pub segment_ids: Vec<u64>,    // segments to compact
    pub estimated_bytes: u64,
    pub state: CompactionJobState,
    pub created_at: u64,          // unix secs
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub bytes_freed: u64,         // set when Done
    pub error: Option<String>,    // set when Failed
}

/// Statistics from the compaction manager.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CompactionManagerStats {
    pub jobs_submitted: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub jobs_cancelled: u64,
    pub total_bytes_freed: u64,
    pub active_job_count: usize,
}

/// Configuration for the compaction manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionManagerConfig {
    pub max_concurrent_jobs: usize,   // default: 2
    pub min_segments_per_job: usize,  // default: 2 (don't compact just 1 segment)
    pub max_segments_per_job: usize,  // default: 8
    pub fragmentation_threshold_pct: u8,  // default: 30 (compact if >30% frag)
}

impl Default for CompactionManagerConfig { ... }
```

### `CompactionManager` struct and methods
```rust
pub struct CompactionManager { config: CompactionManagerConfig, ... }

impl CompactionManager {
    pub fn new(config: CompactionManagerConfig) -> Self
    pub fn submit_job(&mut self, segment_ids: Vec<u64>) -> Result<CompactionJobId, CompactionError>
        // Returns Err if: segment_ids.len() < min_segments_per_job,
        //                  segment_ids.len() > max_segments_per_job,
        //                  active_job_count >= max_concurrent_jobs
    pub fn start_job(&mut self, id: CompactionJobId) -> Result<(), CompactionError>
        // Transitions Queued -> Running
    pub fn complete_job(&mut self, id: CompactionJobId, bytes_freed: u64) -> Result<(), CompactionError>
        // Transitions Running -> Done, records bytes_freed, sets finished_at
    pub fn fail_job(&mut self, id: CompactionJobId, error: String) -> Result<(), CompactionError>
        // Transitions Running -> Failed
    pub fn cancel_job(&mut self, id: CompactionJobId) -> bool
        // Transitions Queued -> Cancelled (can't cancel Running). Returns true if cancelled.
    pub fn get_job(&self, id: CompactionJobId) -> Option<&CompactionJob>
    pub fn active_jobs(&self) -> Vec<&CompactionJob>  // Running jobs
    pub fn pending_jobs(&self) -> Vec<&CompactionJob>  // Queued jobs
    pub fn stats(&self) -> CompactionManagerStats
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("Too few segments: need at least {min}, got {actual}")]
    TooFewSegments { min: usize, actual: usize },
    #[error("Too many segments: max {max}, got {actual}")]
    TooManySegments { max: usize, actual: usize },
    #[error("Too many concurrent jobs: limit {limit}")]
    TooManyConcurrent { limit: usize },
    #[error("Job not found: {0}")]
    JobNotFound(CompactionJobId),
    #[error("Invalid state transition: job {id} in state {state:?}")]
    InvalidStateTransition { id: CompactionJobId, state: CompactionJobState },
}
```

### Tests (25 tests)
1. New manager has no active jobs
2. Submit valid job returns ID
3. Submit with too few segments returns error
4. Submit with too many segments returns error
5. Submit when max_concurrent reached returns error
6. start_job transitions Queued -> Running
7. start_job on non-existent job returns error
8. start_job on Running job returns error (already running)
9. complete_job transitions Running -> Done
10. complete_job records bytes_freed
11. complete_job sets finished_at
12. fail_job transitions Running -> Failed
13. fail_job records error message
14. cancel_job transitions Queued -> Cancelled
15. cancel_job on Running returns false
16. cancel_job on non-existent returns false
17. stats().jobs_submitted increments on submit
18. stats().jobs_completed increments on complete
19. stats().jobs_failed increments on fail
20. stats().jobs_cancelled increments on cancel
21. stats().total_bytes_freed accumulates
22. active_jobs returns only Running jobs
23. pending_jobs returns only Queued jobs
24. get_job returns correct job
25. Multiple jobs tracked independently

---

## Output

For EACH module:
1. Write the .rs file to its location
2. Run `cd /home/cfs/claudefs && cargo test -p claudefs-storage <module_name> 2>&1 | tail -5`
3. Fix any compilation errors
4. Show final test result

Then update `crates/claudefs-storage/src/lib.rs` to add all three modules:
- `pub mod io_accounting;`
- `pub mod block_verifier;`
- `pub mod compaction_manager;`

And add pub use re-exports for all public types.

Final verification:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | grep "^test result"
```

Expected: all tests pass (at least 809 + 65 = 874 new tests).
