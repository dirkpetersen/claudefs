# A10 Phase 35: Security Tests for Emerging Builder Modules

**Agent:** A10 (Security Audit) | **Phase:** 35 | **Date:** 2026-03-05
**Scope:** Create security/property tests for 6 newly-implemented builder modules from A1 Phase 9, A2 Phase 9, and A4 Phase 12.

---

## Context

A10 has completed 34 phases of comprehensive security testing across all 8 builder crates (claudefs-storage, claudefs-meta, claudefs-reduce, claudefs-transport, claudefs-fuse, claudefs-repl, claudefs-gateway, claudefs-mgmt). Current test count: **2383 tests**.

Recent builder phases have introduced new modules:
- **A1 Phase 9:** `io_depth_limiter.rs` (complete, 20KB), `command_queueing.rs` (complete, now in src/)
- **A2 Phase 9:** `client_session.rs` (complete, now in src/)
- **A4 Phase 12:** `trace_aggregator.rs`, `bandwidth_shaper.rs`, `adaptive_router.rs` (all in src/)

A10 Phase 35 must create security/property tests for these 6 modules to ensure:
1. **Concurrency safety** (multi-threaded Tokio task scenarios, no data races)
2. **Resource exhaustion resistance** (memory bounds, backpressure, counter overflow)
3. **State machine correctness** (valid transitions, timeout enforcement, gating logic)
4. **Data integrity** (FIFO ordering, field preservation, buffer lifecycle, stats accuracy)
5. **API boundary validation** (parameter clamping, error handling, configuration)

---

## New Test Modules to Create

You will implement **5 comprehensive test modules** with approximately **195 new tests** total. Each module focuses on security/property testing (not functional testing, which builder agents do).

### 1. **storage_io_depth_limiter_security_tests.rs** (~35 tests)

**Module under test:** `claudefs-storage/src/io_depth_limiter.rs`

**Key types to import:**
```rust
use claudefs_storage::io_depth_limiter::{
    HealthAdaptiveMode, IoDepthLimiter, IoDepthLimiterConfig, QueueDepthStats,
};
use claudefs_storage::device::DeviceHealth;
use claudefs_storage::nvme_passthrough::QueuePairId;
```

**Test categories (implement ~7 test per category):**

#### Concurrency & Race Conditions (8 tests)
- `test_storage_io_depth_sec_concurrent_acquire_no_data_race`: Spawn 20 concurrent tokio tasks attempting to acquire/release queue slots; verify no panics and pending_count remains valid
- `test_storage_io_depth_sec_mode_transition_healthy_degraded`: Monitor mode transitions from Healthy→Degraded under sustained latency; verify recovery_delay prevents flip-flop
- `test_storage_io_depth_sec_mode_transition_degraded_critical`: Force p99 latency > critical_latency_ms; verify transition to Critical and depth reduction
- `test_storage_io_depth_sec_mode_transition_critical_recovery`: In Critical mode, simulate device recovery; verify eventual transition back to Healthy
- `test_storage_io_depth_sec_pending_counter_concurrent_increments`: Arc<AtomicU32> pending counter with 100 concurrent increment/decrement operations; verify accuracy
- `test_storage_io_depth_sec_latency_history_concurrent_updates`: VecDeque latency history with RwLock; multiple tasks push latencies while stats reader reads; no panic
- `test_storage_io_depth_sec_dispatch_time_tracking_concurrent`: dispatch_times VecDeque updated concurrently; verify no duplicates, ordering preserved
- `test_storage_io_depth_sec_stats_snapshot_under_concurrent_load`: Call get_stats() while concurrent tasks update latencies; verify snapshot contains consistent data

#### Latency Calculation & Percentile Logic (7 tests)
- `test_storage_io_depth_sec_p99_calculation_empty_history`: VecDeque of latencies is empty; p99_latency should be 0, not panic
- `test_storage_io_depth_sec_p99_calculation_single_latency`: Single latency value in history; p99 should equal that value
- `test_storage_io_depth_sec_p99_calculation_sorted_correctly`: Insert 100 latencies in random order; compute p99; verify it's at 99th percentile
- `test_storage_io_depth_sec_avg_latency_computation`: Sum 50 known latencies; verify avg_latency matches expected value
- `test_storage_io_depth_sec_avg_latency_overflow_resistant`: Push u64::MAX-100 values and more; verify no overflow panic, saturates gracefully
- `test_storage_io_depth_sec_history_window_rolling`: history_size=10; push 100 latencies; verify VecDeque never exceeds size 10
- `test_storage_io_depth_sec_percentile_with_duplicates`: Push 1000 identical latencies; p99 should equal that value

#### Mode Transition Security (8 tests)
- `test_storage_io_depth_sec_transition_gating_recovery_delay`: After Healthy→Degraded, immediately attempt back to Healthy; verify recovery_delay prevents it
- `test_storage_io_depth_sec_degradation_latency_threshold`: Set degradation_latency_ms=5; simulate p99 latency at 4, 5, 6 ms; verify transition at ≥5
- `test_storage_io_depth_sec_critical_latency_threshold`: Set critical_latency_ms=10; simulate p99 at 9, 10, 11; verify transition at ≥10
- `test_storage_io_depth_sec_mode_transition_via_device_health`: Simulate device critical_warning=true; verify mode→Critical regardless of latency
- `test_storage_io_depth_sec_min_max_depth_bounds`: Set min_depth=8, max_depth=256; transition to lower mode; verify new_depth ≥ min_depth
- `test_storage_io_depth_sec_reduction_percent_applied`: Healthy depth=32, reduction_percent=50; degrade; verify new depth ≤ 32/2=16
- `test_storage_io_depth_sec_depth_adjustment_clamped_min`: Force depth below min_depth; verify clamped back to min_depth
- `test_storage_io_depth_sec_depth_adjustment_clamped_max`: Force depth above 256; verify clamped to 256

#### Resource Exhaustion Resistance (7 tests)
- `test_storage_io_depth_sec_pending_counter_overflow_safe`: Acquire 2^32-1 operations; verify pending_count doesn't panic, saturates
- `test_storage_io_depth_sec_latency_history_bounded_memory`: history_size=100; push 1000000 latencies; verify memory stays ~fixed (VecDeque bounded)
- `test_storage_io_depth_sec_dispatch_time_deque_bounded`: Push dispatch times in loop; verify VecDeque capacity never exceeds history_size
- `test_storage_io_depth_sec_stats_aggregation_large_sample`: Aggregate stats from 10000 ops; verify no OOM, all fields remain valid
- `test_storage_io_depth_sec_concurrent_acquire_no_unbounded_growth`: Acquire from 50 concurrent tasks in loop; verify pending_count stays <1000
- `test_storage_io_depth_sec_large_config_values_handled`: Set history_size=100000; create limiter; verify no panic
- `test_storage_io_depth_sec_negative_latency_impossible`: Ensure end_time < start_time is impossible (saturating_sub used)

#### API Boundary Validation (5 tests)
- `test_storage_io_depth_sec_try_acquire_respects_limit`: current_limit=10; call try_acquire() 11 times; 11th should fail
- `test_storage_io_depth_sec_release_with_zero_pending_noop`: pending_ops=0; release() should not panic, just no-op
- `test_storage_io_depth_sec_set_depth_out_of_range_clamped`: Call set_depth(1000) with max=256; verify clamped to 256
- `test_storage_io_depth_sec_pending_count_reflects_actual_ops`: Acquire 5 ops; verify pending_count()=5; release 3; verify=2
- `test_storage_io_depth_sec_config_validation_on_create`: Create with invalid config (recovery_delay=-1, etc.); verify handled gracefully

---

### 2. **storage_command_queueing_security_tests.rs** (~32 tests)

**Module under test:** `claudefs-storage/src/command_queueing.rs`

**Key types to import:**
```rust
use claudefs_storage::command_queueing::{
    CommandQueue, CommandQueueConfig, CommandQueueStats, CommandType, NvmeCommand,
};
use claudefs_storage::block::BlockId;
use claudefs_storage::io_scheduler::IoPriority;
use std::sync::Arc;
```

**Test categories:**

#### Capacity & Backpressure Enforcement (7 tests)
- `test_storage_cmd_q_sec_enqueue_returns_queue_full`: capacity=10; enqueue 11 commands; 11th should return QueueFull error or Err
- `test_storage_cmd_q_sec_backpressure_prevents_unbounded_growth`: Repeatedly try to enqueue > capacity; verify queue size never exceeds capacity
- `test_storage_cmd_q_sec_capacity_enforced_in_constructor`: CommandQueueConfig with capacity=50; construct queue; verify internal capacity=50
- `test_storage_cmd_q_sec_full_event_counter_increments`: Enqueue to capacity; try 10 more; verify full_events counter=10
- `test_storage_cmd_q_sec_recovery_after_queue_full`: Fill queue; flush some; verify can enqueue again successfully
- `test_storage_cmd_q_sec_zero_capacity_rejected`: Try to create CommandQueueConfig with capacity=0; should be rejected or create with min capacity
- `test_storage_cmd_q_sec_large_capacity_handled`: capacity=10000; enqueue 9999; verify no panic, queue functions normally

#### Buffer Lifecycle Safety (8 tests)
- `test_storage_cmd_q_sec_arc_buffer_refcount_no_leak`: Create commands with Arc<Vec<u8>> buffers; enqueue/dequeue many; verify no memory leak via Arc semantics
- `test_storage_cmd_q_sec_large_buffer_no_oom`: Create command with 16MB Arc<Vec<u8>>; enqueue/dequeue; verify no OOM panic
- `test_storage_cmd_q_sec_empty_buffer_none_option`: Create NvmeCommand with buffer=None; enqueue/dequeue; verify Option preserved as None
- `test_storage_cmd_q_sec_buffer_ownership_preserved`: Arc<Vec<u8>> with 1000 bytes; enqueue; dequeue; verify data unchanged, arc still valid
- `test_storage_cmd_q_sec_multiple_commands_share_buffer`: Create 10 commands pointing to same Arc<Vec<u8>>; enqueue all; dequeue; verify Arc refcount drops to 1
- `test_storage_cmd_q_sec_buffer_drop_on_flush`: Create command with Arc<Vec<u8>>; enqueue; verify drop not called until command leaves queue
- `test_storage_cmd_q_sec_small_buffer_allocation`: buffer=1 byte; enqueue/dequeue; verify works
- `test_storage_cmd_q_sec_concurrent_buffer_modification_safe`: Multiple commands with same Arc<Vec> backing; enqueue concurrently; dequeue; Arc prevents data races

#### Command Ordering & Integrity (6 tests)
- `test_storage_cmd_q_sec_fifo_ordering_preserved`: Enqueue commands with user_data=[1,2,3,4,5]; dequeue; verify order is 1,2,3,4,5
- `test_storage_cmd_q_sec_priority_field_preserved`: Enqueue commands with priority=High; dequeue; verify priority still High
- `test_storage_cmd_q_sec_block_id_offset_preserved`: Enqueue with block_id=100, offset=4096; dequeue; verify both unchanged
- `test_storage_cmd_q_sec_command_type_preserved`: Enqueue [Read, Write, Read, Flush]; dequeue; verify type sequence unchanged
- `test_storage_cmd_q_sec_metadata_timestamps_monotonic`: submitted_at should be monotonically increasing across enqueued commands
- `test_storage_cmd_q_sec_command_length_preserved`: Enqueue commands with length=[100, 200, 50]; dequeue; verify lengths match

#### Batch Threshold Enforcement (6 tests)
- `test_storage_cmd_q_sec_should_flush_at_threshold`: batch_threshold=10; enqueue 9, should_flush()=false; enqueue 1 more, should_flush()=true
- `test_storage_cmd_q_sec_should_flush_max_latency_timeout`: batch_threshold=100; enqueue 1 command; wait for max_queue_latency_us to elapse; should_flush()=true
- `test_storage_cmd_q_sec_should_flush_independent_conditions`: batch_threshold=10, max_queue_latency_us=100; enqueue 1, immediately check should_flush() from both; verify either condition works
- `test_storage_cmd_q_sec_flush_returns_command_count`: Enqueue 25 commands; flush(); verify returns 25
- `test_storage_cmd_q_sec_flush_empty_queue_returns_zero`: Enqueue 0 commands; flush(); verify returns 0
- `test_storage_cmd_q_sec_flush_idempotent`: Flush twice on same queue; first returns N, second returns 0

#### Statistics Accuracy (5 tests)
- `test_storage_cmd_q_sec_total_commands_tracks_all_flushed`: Enqueue 100 commands, flush; total_commands stat should be 100 (cumulative)
- `test_storage_cmd_q_sec_total_syscalls_incremented_per_flush`: Flush 5 separate batches; total_syscalls should be 5
- `test_storage_cmd_q_sec_avg_commands_per_syscall_computed`: total_commands=100, total_syscalls=5; avg_commands_per_syscall should be 20.0
- `test_storage_cmd_q_sec_total_bytes_sum_of_lengths`: Enqueue commands with length=[100, 200, 150]; flush; total_bytes=450
- `test_storage_cmd_q_sec_queue_size_reset_after_flush`: Enqueue 10; queue_size=10; flush; queue_size should be 0

---

### 3. **meta_client_session_security_tests.rs** (~38 tests)

**Module under test:** `claudefs-meta/src/client_session.rs`

**Key types to import:**
```rust
use claudefs_meta::client_session::{
    ClientId, OperationId, OpResult, PendingOperation, SessionId, SessionLeaseRenewal,
    SessionManager, SessionManagerConfig, SessionState,
};
use claudefs_meta::types::{InodeId, MetaError, NodeId, Timestamp};
use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Duration;
```

**Test categories:**

#### Session Lifecycle Management (8 tests)
- `test_meta_session_sec_new_session_generates_unique_id`: Create 100 sessions; verify all SessionIds are unique (UUID-based)
- `test_meta_session_sec_initial_state_is_active`: New session should have state=Active
- `test_meta_session_sec_state_transition_active_to_idle`: Simulate client inactivity; verify state→Idle with idle_since timestamp
- `test_meta_session_sec_state_transition_idle_to_expired`: From Idle, wait for max_session_age_secs; verify state→Expired with expired_at
- `test_meta_session_sec_state_transition_to_revoked`: Call revoke() with reason="user_logout"; verify state=Revoked with reason preserved
- `test_meta_session_sec_revoked_timestamp_monotonic`: Revoke at time T; revoked_at ≥ T
- `test_meta_session_sec_revoked_reason_nonempty`: Revoke with reason="..." (any string); verify reason preserved
- `test_meta_session_sec_heartbeat_renewal_resets_idle`: In Idle state, call heartbeat(); verify state→Active

#### Lease Renewal & Expiry (7 tests)
- `test_meta_session_sec_lease_expiry_updated_on_renewal`: Lease expires at T; renew; new expiry > T
- `test_meta_session_sec_operations_completed_tracked`: Renew lease with operations_completed=10; verify tracker increments
- `test_meta_session_sec_bytes_transferred_aggregated`: Multiple renewals with bytes_transferred=[100, 200, 50]; verify aggregation (cumulative or tracked)
- `test_meta_session_sec_expired_sessions_reject_new_ops`: Session expired at T; current_time > T; attempt new operation; should fail
- `test_meta_session_sec_lease_duration_enforced`: config.lease_duration_secs=10; renew; next expiry ≥ current_time + 10
- `test_meta_session_sec_cleanup_removes_expired_sessions`: 10 expired sessions; run cleanup; verify count drops (depends on impl)
- `test_meta_session_sec_lease_expiry_timestamp_clamped`: Ensure expiry never goes backward (monotonic)

#### Pending Operations Tracking (8 tests)
- `test_meta_session_sec_pending_ops_limited_to_max`: max_pending_ops=5; add 6th operation; should reject
- `test_meta_session_sec_operation_timeout_enforced`: Add operation with timeout_secs=1; wait 2 seconds; verify operation marked timeout/expired
- `test_meta_session_sec_op_result_success_variant`: Set OpResult::Success{value: vec![1,2,3]}; retrieve; verify value preserved
- `test_meta_session_sec_op_result_failure_variant`: Set OpResult::Failure{error: "file_not_found"}; retrieve; verify error string preserved
- `test_meta_session_sec_pending_op_result_retrieval`: Get pending operation result; verify returns Option with correct variant
- `test_meta_session_sec_operation_completion_removes_from_pending`: Add operation; mark complete; verify removed from pending list
- `test_meta_session_sec_concurrent_pending_ops_don_exceed_limit`: 10 concurrent tasks each adding operations; verify total never exceeds max_pending_ops
- `test_meta_session_sec_operation_expiry_detected_on_timeout`: Operation timeout_secs=1; wait; check is_expired(); should return true

#### DashMap Concurrency (7 tests)
- `test_meta_session_sec_multiple_clients_concurrent_sessions`: 20 concurrent clients creating sessions; verify each gets unique SessionId
- `test_meta_session_sec_session_lookup_thread_safe`: Client A adds session; Client B immediately looks up; no race condition panic
- `test_meta_session_sec_concurrent_session_cleanup_nopanic`: Multiple tasks calling cleanup(); should not panic, idempotent
- `test_meta_session_sec_session_removal_idempotent`: Remove session S; try remove again; should succeed (no-op)
- `test_meta_session_sec_dashmap_iterator_consistent_snapshot`: Iterate over all sessions while concurrent tasks add/remove; should see consistent snapshot
- `test_meta_session_sec_session_get_after_insert`: Insert session; immediately get it; retrieves correct value
- `test_meta_session_sec_dashmap_values_not_corrupted`: Insert 100 sessions; iterate; all SessionIds unique and non-corrupted

#### Authorization & Revocation (8 tests)
- `test_meta_session_sec_revoked_session_has_timestamp`: Revoke session; revoked_at field is set
- `test_meta_session_sec_revoked_sessions_cannot_renew`: Revoke session S; attempt renew; should fail
- `test_meta_session_sec_revoke_idempotent`: Revoke session; revoke again; should be OK (idempotent)
- `test_meta_session_sec_revocation_reason_preserved`: Revoke with reason="admin_revoked"; retrieve; reason matches
- `test_meta_session_sec_admin_can_revoke_any_session`: (No auth check in unit test layer) Admin calls revoke(S); should succeed
- `test_meta_session_sec_revoked_operations_rejected`: Session revoked; try to add pending operation; should reject
- `test_meta_session_sec_revocation_cascades_to_operations`: Revoke session; all pending operations in it should be marked failed
- `test_meta_session_sec_revoked_state_not_transition_back`: Once Revoked state, cannot go back to Active

---

### 4. **transport_trace_aggregator_security_tests.rs** (~28 tests)

**Module under test:** `claudefs-transport/src/trace_aggregator.rs`

**Key types to import:**
```rust
use claudefs_transport::trace_aggregator::{TraceId, SpanRecord, TraceData};
use claudefs_transport::otel::{OtlpStatusCode, OtlpAttribute};
use std::time::SystemTime;
```

**Test categories:**

#### Trace ID Generation & Uniqueness (6 tests)
- `test_transport_trace_sec_random_traceid_distinct`: Generate 1000 random TraceIds; verify all unique (no collisions)
- `test_transport_trace_sec_traceid_from_bytes_deterministic`: Create TraceId from same [u8;16] twice; verify equal
- `test_transport_trace_sec_traceid_default_is_zero`: TraceId::default() should equal [0u8; 16]
- `test_transport_trace_sec_traceid_hashable`: Use TraceIds as HashMap keys; verify no panic
- `test_transport_trace_sec_traceid_equality_reflexive`: trace_id == trace_id should be true
- `test_transport_trace_sec_traceid_clone_identical`: TraceId::clone() should be equal to original

#### Span Record Integrity (7 tests)
- `test_transport_trace_sec_span_fields_preserved`: Create SpanRecord with all fields; read back; verify each field unchanged
- `test_transport_trace_sec_duration_ns_saturating_subtract`: end_time < start_time (shouldn't happen); duration_ns should not panic, return 0
- `test_transport_trace_sec_duration_ns_calculation_correct`: start=100, end=500; duration_ns=400
- `test_transport_trace_sec_status_code_unset_default`: New SpanRecord should have status=Unset
- `test_transport_trace_sec_with_status_builder_pattern`: Create span; call with_status(Ok); verify status changed
- `test_transport_trace_sec_attributes_list_preserved`: Create span with attributes; add 100 attributes; read back; count=100
- `test_transport_trace_sec_span_name_into_conversion`: Create span with name="test_op"; verify name field="test_op"

#### Trace Data Aggregation (6 tests)
- `test_transport_trace_sec_multiple_spans_aggregated`: Create TraceData with 10 SpanRecords; verify all stored in spans vec
- `test_transport_trace_sec_root_span_identification`: First span in trace should be root_span_id
- `test_transport_trace_sec_parent_child_relationships`: Span A parent=None, Span B parent=A.span_id; relationships maintained
- `test_transport_trace_sec_timeline_ordering_by_start_time`: Create spans with start_times=[100, 50, 200]; aggregate; verify sortable by start_time
- `test_transport_trace_sec_received_at_ns_recorded`: Create TraceData; received_at_ns should be valid timestamp (> 0 typically)
- `test_transport_trace_sec_trace_id_in_data_preserved`: TraceData with trace_id=X; retrieve; verify X

#### Critical Path Analysis (5 tests)
- `test_transport_trace_sec_critical_path_longest_dependency_chain`: 3 spans: A→B→C (parent-child); critical path should include all 3
- `test_transport_trace_sec_critical_path_excludes_parallel_branches`: Spans A, B (both parent=root), should not double-count in critical path
- `test_transport_trace_sec_latency_attribution_per_stage`: Client span, metadata span, storage span; compute latencies per stage
- `test_transport_trace_sec_p50_p95_p99_percentiles_computed`: 100 spans with durations; compute percentiles; verify p50 < p95 < p99
- `test_transport_trace_sec_outlier_detection_anomalous_spans`: Most spans ~1000ns, one span ~100000ns; outlier detection identifies it

#### Memory & Performance (4 tests)
- `test_transport_trace_sec_large_trace_no_oom`: Create TraceData with 10000 spans; verify no OOM
- `test_transport_trace_sec_span_storage_not_copied`: Span added to TraceData; original not deep-copied multiple times
- `test_transport_trace_sec_hash_based_lookup_efficient`: 10000 TraceIds; lookup by ID should be O(1) not O(n)
- `test_transport_trace_sec_concurrent_span_insertion_thread_safe`: Multiple tokio tasks inserting spans into same trace; no panic

---

### 5. **transport_bandwidth_shaper_security_tests.rs** (~30 tests)

**Module under test:** `claudefs-transport/src/bandwidth_shaper.rs`

**Key types to import:**
```rust
use claudefs_transport::bandwidth_shaper::{
    BandwidthAllocation, BandwidthId, BandwidthShaper, EnforcementMode, TokenBucket,
};
use std::time::Duration;
```

**Test categories:**

#### Token Bucket Correctness (8 tests)
- `test_transport_bw_sec_initial_tokens_equals_capacity`: Create bucket with capacity=1000; initial tokens should be 1000
- `test_transport_bw_sec_refill_respects_rate`: Create bucket; refill_rate_per_sec=100; wait 1 sec; verify tokens +100 (clamped at capacity)
- `test_transport_bw_sec_refill_stops_at_capacity`: Tokens=1000, capacity=1000; refill; tokens should stay 1000 (no overfill)
- `test_transport_bw_sec_try_consume_fails_insufficient_tokens`: tokens=50, try_consume(100); should fail
- `test_transport_bw_sec_try_consume_succeeds_if_sufficient`: tokens=100, try_consume(50); should succeed, tokens→50
- `test_transport_bw_sec_try_consume_atomic_check_and_subtract`: Concurrent try_consume() calls; verify no double-booking
- `test_transport_bw_sec_last_refill_timestamp_updated`: Create bucket; call refill(); last_refill_ns should be updated
- `test_transport_bw_sec_refill_calculation_correct`: refill_rate=1000 tokens/sec; elapsed=0.5s; should add 500 tokens

#### Enforcement Modes (8 tests)
- `test_transport_bw_sec_hard_mode_rejects_over_limit`: Hard mode, allocation=100 bytes/sec; try to consume 200; should reject
- `test_transport_bw_sec_soft_mode_allows_over_limit`: Soft mode, allocation=100 bytes/sec; try to consume 200; should allow (warn/backpressure only)
- `test_transport_bw_sec_hard_soft_mode_switch`: Allocation in Hard mode; switch to Soft; behavior should change
- `test_transport_bw_sec_mode_change_does_not_reset_tokens`: Hard mode, tokens=50; switch to Soft; tokens should still be 50
- `test_transport_bw_sec_per_tenant_enforcement_independent`: Tenant A in Hard, Tenant B in Soft; enforcement independent
- `test_transport_bw_sec_enforcement_mode_validation`: Try to set invalid enforcement_mode; should reject or default to Soft
- `test_transport_bw_sec_hard_mode_with_burst_capacity`: Hard mode; burst_bytes=500; can exceed per_sec temporarily up to burst
- `test_transport_bw_sec_soft_mode_logs_warning`: Soft mode, exceed limit; should log warning (verify via test output or tracing)

#### Per-Tenant Isolation (7 tests)
- `test_transport_bw_sec_separate_bucket_per_tenant`: 10 different BandwidthIds; each gets separate token bucket
- `test_transport_bw_sec_tenant_a_usage_independent_tenant_b`: Tenant A consumes 100 tokens; Tenant B's bucket unaffected
- `test_transport_bw_sec_concurrent_allocations_different_tenants`: Spawn 20 tasks for Tenant A and 20 for Tenant B; no cross-tenant interference
- `test_transport_bw_sec_dashmap_no_cross_tenant_leak`: DashMap stores buckets; lookup(Tenant A) doesn't see Tenant B's data
- `test_transport_bw_sec_tenant_removal_idempotent`: Remove allocation for Tenant A; try remove again; idempotent (OK)
- `test_transport_bw_sec_tenant_allocation_add_independently`: Add allocation for A; add for B; both present and independent
- `test_transport_bw_sec_same_tenant_multiple_allocations_share_bucket`: Multiple threads for same Tenant; all share same token bucket

#### Burst Capacity Handling (4 tests)
- `test_transport_bw_sec_burst_allows_temporary_exceed`: per_sec=100, burst=500; consume 300 (exceeds per_sec but < burst); should succeed
- `test_transport_bw_sec_burst_capacity_depletes`: Use burst; burst depletes; subsequent try_consume waits for normal rate
- `test_transport_bw_sec_burst_capacity_replenishes`: Use burst; wait 1 sec; burst should partially replenish
- `test_transport_bw_sec_over_burst_rejected`: burst=500; try_consume(600); should reject as exceeds burst_bytes

#### Configuration Validation (3 tests)
- `test_transport_bw_sec_valid_config_bytes_per_sec_gt_zero`: BandwidthAllocation::is_valid() with bytes_per_sec > 0 should return true
- `test_transport_bw_sec_valid_config_burst_bytes_gt_zero`: BandwidthAllocation::is_valid() with burst_bytes > 0 should return true
- `test_transport_bw_sec_invalid_config_zero_values`: bytes_per_sec=0 or burst_bytes=0; is_valid() should return false

---

## Implementation Notes

### File Placement
All files go in `/home/cfs/claudefs/crates/claudefs-security/src/`:
- `storage_io_depth_limiter_security_tests.rs`
- `storage_command_queueing_security_tests.rs`
- `meta_client_session_security_tests.rs`
- `transport_trace_aggregator_security_tests.rs`
- `transport_bandwidth_shaper_security_tests.rs`

### Code Style & Conventions
- Use `#[cfg(test)] mod tests { ... }` outer structure, then `mod <category> { ... }` inner modules
- Each test is `#[tokio::test] async fn test_<prefix>_sec_<description>()`
- Prefix format: `test_<crate>_<module_short>_sec_<scenario>`
  - Example: `test_storage_io_depth_sec_concurrent_acquire_release`
  - Example: `test_meta_session_sec_state_machine`
  - Example: `test_transport_bw_sec_token_bucket`
- Use descriptive assertion messages: `assert!(..., "description of what was tested")`
- Mock external dependencies where needed (e.g., DeviceHealth for io_depth_limiter tests)

### Tokio Runtime & Async
- All tests using async operations (Tokio RwLock, Mutex, task spawn) must be `#[tokio::test]`
- Create tasks with `tokio::spawn(async { ... })`; collect with `futures::join_all()` or similar
- Use `tokio::time::sleep()` for waiting in tests
- Use `tokio::sync::Barrier` for coordinating parallel test tasks if needed

### Error Handling in Tests
- Tests should not panic on legitimate errors; use `assert!(result.is_err(), "...")`
- Use `.unwrap()` only for operations that MUST succeed (e.g., creating test fixtures)
- Catch panics in concurrent tests via `catch_unwind()` or test framework's panic isolation

### Testing Patterns to Follow

Look at existing tests in `crates/claudefs-security/src/storage_tier_security_tests.rs` for style:
- Organize by domain (concurrency, resource exhaustion, API validation)
- Test both happy path and error path
- Use realistic values from actual config defaults
- Mock where possible; real integration tests belong in builder crates

---

## Key Files for Reference

**Existing test patterns:**
- `crates/claudefs-security/src/storage_tier_security_tests.rs` (storage test style)
- `crates/claudefs-security/src/transport_security_tests.rs` (transport test style)
- `crates/claudefs-security/src/dos_resilience.rs` (resource exhaustion tests)
- `crates/claudefs-security/src/transport_conn_security_tests.rs` (concurrency tests)

**Builder modules to understand:**
- `crates/claudefs-storage/src/io_depth_limiter.rs` (20KB, study full implementation)
- `crates/claudefs-storage/src/command_queueing.rs` (study enqueue/dequeue APIs)
- `crates/claudefs-meta/src/client_session.rs` (study SessionManager interface)
- `crates/claudefs-transport/src/trace_aggregator.rs` (study TraceData/SpanRecord)
- `crates/claudefs-transport/src/bandwidth_shaper.rs` (study TokenBucket, EnforcementMode)

---

## Synchronization with Builder Work

**Known constraints:**
1. **A1 io_depth_limiter:** Module is stable. Tests can use public API directly.
2. **A1 command_queueing:** Recently added. Tests should use public async APIs only.
3. **A2 client_session:** Recently added. DashMap for concurrent session storage.
4. **A4 modules:** trace_aggregator, bandwidth_shaper are in src/. Test public interfaces.

**Builder test count baseline (before A10 Phase 35):**
- A1: ~1220 tests (Phase 9 complete with just io_depth_limiter)
- A2: ~997 tests (Phase 9 planned)
- A4: ~1304 tests (Phase 12 starting)

---

## Deliverables

1. **5 new test modules** (~195 tests total)
2. **Update `lib.rs`** to register all 5 modules:
   ```rust
   #[cfg(test)]
   #[allow(missing_docs)]
   pub mod storage_io_depth_limiter_security_tests;

   #[cfg(test)]
   #[allow(missing_docs)]
   pub mod storage_command_queueing_security_tests;

   #[cfg(test)]
   #[allow(missing_docs)]
   pub mod meta_client_session_security_tests;

   #[cfg(test)]
   #[allow(missing_docs)]
   pub mod transport_trace_aggregator_security_tests;

   #[cfg(test)]
   #[allow(missing_docs)]
   pub mod transport_bandwidth_shaper_security_tests;
   ```

3. **All tests compile and pass** on first run: `cargo test -p claudefs-security --lib`

4. **Expected final test count:** 2383 (current A10) + ~195 = **2500+ tests**

---

## Quality Checklist

- [ ] All 5 modules compile without warnings
- [ ] All ~195 tests execute and pass
- [ ] No `#[ignore]` tests (all should run)
- [ ] Concurrency tests use `#[tokio::test]` and proper async/await
- [ ] Resource exhaustion tests verify bounds without panicking
- [ ] State machine tests validate all transitions
- [ ] API boundary tests cover error cases
- [ ] Test names follow `test_<crate>_<module>_sec_<scenario>` pattern
- [ ] Imports correct and complete (no missing types)
- [ ] No hardcoded timeouts that flake (use reasonable delays)
- [ ] Documentation/comments explain security intent of each test

---

**Ready for implementation. Generate all 5 test modules with ~195 tests total, compile-checked and ready to integrate.**
