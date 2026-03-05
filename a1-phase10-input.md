# A1: Storage Engine — Phase 10 Implementation Brief

## Current Status
- **Phase 9 Complete:** 1220 tests passing
- **Last commit:** a27820c [A1] Phase 9: Fix io_depth_limiter.rs async runtime issues
- **Crate location:** `crates/claudefs-storage/src/`
- **Test count target:** 1300+ tests (+80 from Phase 9)

## Phase 10: Command Batching & Timeout Management (80-100 new tests)

### Module 1: command_queueing.rs (~35 tests)

**Purpose:** Batch NVMe commands before submission to reduce syscalls and improve throughput.

**Design:**
- Per-core command queue (thread-local using tokio LocalSet)
- Collect write/read commands, batch them before io_uring submission
- Batch thresholds: trigger on count (16 cmds) or time (100µs)
- Reduce syscalls by ~10-15x under high concurrency
- Ring buffer + index tracking for zero-copy batch construction

**API:**
```rust
pub struct CommandQueue {
    qp_id: QueuePairId,
    commands: VecDeque<NvmeCommand>,
    batch_size: usize,
    batch_timeout_us: u64,
    last_flush: Instant,
}

impl CommandQueue {
    pub async fn queue(&mut self, cmd: NvmeCommand) -> Result<(), Error>
    pub async fn flush(&mut self) -> Result<Vec<CommandResult>, Error>
    pub async fn try_auto_flush(&mut self) -> Result<(), Error>
    pub fn pending_count(&self) -> usize
}
```

**Behaviors:**
1. Queue command → check batch size → auto-flush if >= 16
2. Flush periodically (100µs timer) even if < 16 commands
3. Failed submissions trigger backpressure (queue full error)
4. Preserve submission order (FIFO)
5. Thread-safe (Arc<Mutex<>>)

**Tests:**
- test_queue_creation
- test_single_command_flush
- test_batch_trigger_on_count (flush at 16 cmds)
- test_batch_trigger_on_timeout (flush after 100µs)
- test_fifo_ordering
- test_queue_capacity
- test_auto_flush_with_timer
- test_error_handling_on_full
- test_concurrent_queues
- test_results_ordering (10+ tests)
- test_mixed_read_write (5+ tests)

### Module 2: device_timeout_handler.rs (~30 tests)

**Purpose:** Track in-flight I/O operations and detect/recover from stuck commands.

**Design:**
- Track all pending commands with submission timestamp
- Compare elapsed time vs configured timeout (default 5s per D1 storage policy)
- Auto-retry with exponential backoff (50ms, 100ms, 200ms, 500ms)
- Mark device as degraded if timeout threshold exceeded (e.g., 3 timeouts in 60s window)
- Export latency histogram for alerts (P99 > 10s)

**API:**
```rust
pub struct TimeoutHandler {
    qp_id: QueuePairId,
    pending_ops: Arc<DashMap<OperationId, OpMetadata>>,
    timeout_ms: u64,
    retry_backoff: Vec<u64>,
    degradation_threshold: usize,
    config: TimeoutConfig,
}

#[derive(Clone, Debug)]
pub struct OpMetadata {
    cmd_id: u64,
    submitted_at: Instant,
    retry_count: u32,
    op_type: CommandType,
}

impl TimeoutHandler {
    pub fn track(&self, op_id: OperationId, metadata: OpMetadata) -> Result<(), Error>
    pub fn complete(&self, op_id: OperationId) -> Option<OpMetadata>
    pub async fn check_and_recover(&self) -> Result<Vec<TimedOutOp>, Error>
    pub fn degraded_ops_count(&self) -> usize
    pub fn timeout_histogram(&self) -> LatencyHistogram
}
```

**Behaviors:**
1. On command submission: store (cmd_id, timestamp, retry=0)
2. On completion: remove from pending
3. Periodic check (every 100ms):
   - Find ops with elapsed_time > timeout_ms
   - Increment retry_count
   - If retry_count > max_retries: fail operation
   - If retry_count < max_retries: resubmit with backoff
4. Track consecutive timeouts for degradation state
5. Export Prometheus metrics

**Tests:**
- test_track_operation
- test_complete_operation
- test_timeout_detection
- test_retry_logic
- test_exponential_backoff
- test_max_retries_exceeded
- test_degradation_threshold
- test_concurrent_ops_tracking (5+ variants)
- test_histogram_accuracy
- test_latency_p99_calculation
- test_recovery_after_timeout
- test_multiple_devices_independent
- test_backpressure_on_high_timeout_rate

### Module 3: request_deduplication.rs (~25 tests)

**Purpose:** Deduplicate identical read requests to the same block to avoid redundant I/O.

**Design:**
- Track in-flight read requests by (lba, length)
- If duplicate arrives before original completes, return cached result
- Use DashMap for lock-free concurrent lookups
- Clean up cache entries on completion

**API:**
```rust
pub struct RequestDeduplicator {
    inflight: Arc<DashMap<ReadKey, Arc<OnceCell<Result<Vec<u8>>>>>>,
}

#[derive(Eq, PartialEq, Hash)]
pub struct ReadKey {
    qp_id: QueuePairId,
    lba: u64,
    length: u32,
}

impl RequestDeduplicator {
    pub async fn read_deduplicated(&self, key: ReadKey) -> Result<Vec<u8>, Error>
}
```

**Behaviors:**
1. Check if (lba, length) already in-flight
2. If yes: wait on existing result (via Arc<OnceCell>)
3. If no: start new I/O, store result, other waiters get same result
4. Cache hit rate metrics

**Tests:**
- test_no_dedup_when_unique
- test_dedup_identical_reads
- test_dedup_concurrent_requests
- test_cache_invalidation_on_write (negative test)
- test_different_lbas_no_dedup
- test_different_lengths_no_dedup
- test_result_sharing
- test_error_handling

### Module 4: io_scheduler_fairness.rs (~20 tests)

**Purpose:** Ensure fair I/O scheduling across multiple concurrent workloads.

**Design:**
- Token bucket per workload/tenant (if multi-tenancy enabled)
- Prioritize metadata I/O over data
- Implement weighted round-robin across submission queues

**Tests:**
- test_fairness_distribution
- test_metadata_priority
- test_token_bucket_refill
- test_backpressure_on_exhausted_tokens

## Integration Points

1. **io_depth_limiter.rs** (Phase 9):
   - command_queueing feeds into adaptive queue depth
   - latency from command_queueing informs depth adjustments

2. **uring_engine.rs**:
   - command_queueing.flush() → uring_engine.submit()
   - device_timeout_handler.check_and_recover() → uring_engine.retry()

3. **storage_health.rs**:
   - device_timeout_handler updates health stats
   - Degradation state affects allocation strategy

4. **metrics** (existing):
   - Command queue depth histogram
   - Batch size distribution
   - Timeout recovery rate
   - Dedup hit rate

## File Locations

Create all files in: `crates/claudefs-storage/src/`

Files to create:
1. `command_queueing.rs`
2. `device_timeout_handler.rs`
3. `request_deduplication.rs`
4. `io_scheduler_fairness.rs`

Update: `crates/claudefs-storage/src/lib.rs`
- Add `pub mod command_queueing;`
- Add `pub mod device_timeout_handler;`
- Add `pub mod request_deduplication;`
- Add `pub mod io_scheduler_fairness;`
- Register with `pub use` if public API

## Test Requirements

**Target:** 1300+ total tests after Phase 10
- Phase 9 baseline: 1220 tests
- Phase 10 new: 80-100 tests
- Breakdown: 35 + 30 + 25 + 20 = 110 tests

**All tests must:**
- Use `#[tokio::test]` for async contexts
- Avoid blocking_write() / blocking in async contexts
- Have sleep() before check_and_adjust() for timing-sensitive assertions
- Cover success paths, error paths, edge cases
- Include concurrent/stress tests where appropriate

## Success Criteria

1. All 110 new tests pass: `cargo test -p claudefs-storage --lib | tail -3` shows 1300+
2. No compiler errors or warnings (besides allowed `unused`)
3. All modules properly exported in lib.rs
4. Integration with existing modules works (no runtime panics)
5. Code follows existing style (error handling, naming, module structure)

## Notes

- All files are Rust (.rs) — do NOT generate separate test files
- Tests are inline with #[cfg(test)] modules
- Use existing patterns from io_depth_limiter.rs for consistency
- Rely on tokio, parking_lot, dashmap (already in Cargo.toml)
- No new external dependencies
