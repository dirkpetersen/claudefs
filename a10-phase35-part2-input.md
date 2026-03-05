# A10 Phase 35 Part 2: Metadata + Transport Security Tests

**Scope:** Create 3 security test modules for A2 Phase 9 metadata and A4 Phase 12 transport modules.

---

## Task: Create 3 Security Test Modules (~96 tests total)

### Module 1: meta_client_session_security_tests.rs (~38 tests)

**Located in:** `/home/cfs/claudefs/crates/claudefs-security/src/meta_client_session_security_tests.rs`

**Module under test:** `claudefs-meta/src/client_session.rs`

**Imports:**
```rust
use claudefs_meta::client_session::*;
use claudefs_meta::types::{InodeId, MetaError, Timestamp};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use dashmap::DashMap;
```

**Test structure: 5 categories with 8, 7, 8, 7, 8 tests respectively**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod session_lifecycle_management {
        use super::*;

        #[tokio::test]
        async fn test_meta_session_sec_new_session_generates_unique_id() {
            // Create 100 sessions; verify all SessionIds are unique
        }

        #[tokio::test]
        async fn test_meta_session_sec_initial_state_is_active() {
            // New session should have state=Active
        }

        #[tokio::test]
        async fn test_meta_session_sec_state_transition_active_to_idle() {
            // Simulate client inactivity; verify state→Idle
        }

        #[tokio::test]
        async fn test_meta_session_sec_state_transition_idle_to_expired() {
            // From Idle, wait for max_session_age_secs; verify state→Expired
        }

        #[tokio::test]
        async fn test_meta_session_sec_state_transition_to_revoked() {
            // Call revoke() with reason; verify state=Revoked with reason
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoked_timestamp_monotonic() {
            // Revoke at time T; revoked_at ≥ T
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoked_reason_nonempty() {
            // Revoke with reason; verify reason preserved
        }

        #[tokio::test]
        async fn test_meta_session_sec_heartbeat_renewal_resets_idle() {
            // In Idle state, call heartbeat(); verify state→Active
        }
    }

    mod lease_renewal_and_expiry {
        use super::*;

        #[tokio::test]
        async fn test_meta_session_sec_lease_expiry_updated_on_renewal() {
            // Lease expires at T; renew; new expiry > T
        }

        #[tokio::test]
        async fn test_meta_session_sec_operations_completed_tracked() {
            // Renew lease with operations_completed=10; verify tracker
        }

        #[tokio::test]
        async fn test_meta_session_sec_bytes_transferred_aggregated() {
            // Multiple renewals with bytes_transferred; verify aggregation
        }

        #[tokio::test]
        async fn test_meta_session_sec_expired_sessions_reject_new_ops() {
            // Session expired; attempt new operation; should fail
        }

        #[tokio::test]
        async fn test_meta_session_sec_lease_duration_enforced() {
            // config.lease_duration_secs=10; renew; next expiry ≥ current_time + 10
        }

        #[tokio::test]
        async fn test_meta_session_sec_cleanup_removes_expired_sessions() {
            // 10 expired sessions; run cleanup; verify count drops
        }

        #[tokio::test]
        async fn test_meta_session_sec_lease_expiry_timestamp_clamped() {
            // Ensure expiry never goes backward (monotonic)
        }
    }

    mod pending_operations_tracking {
        use super::*;

        #[tokio::test]
        async fn test_meta_session_sec_pending_ops_limited_to_max() {
            // max_pending_ops=5; add 6th; should reject
        }

        #[tokio::test]
        async fn test_meta_session_sec_operation_timeout_enforced() {
            // Add operation with timeout_secs=1; wait 2 seconds
            // Verify operation marked timeout/expired
        }

        #[tokio::test]
        async fn test_meta_session_sec_op_result_success_variant() {
            // Set OpResult::Success{value}; retrieve; verify value preserved
        }

        #[tokio::test]
        async fn test_meta_session_sec_op_result_failure_variant() {
            // Set OpResult::Failure{error}; retrieve; verify error preserved
        }

        #[tokio::test]
        async fn test_meta_session_sec_pending_op_result_retrieval() {
            // Get pending operation result; verify returns Option with correct variant
        }

        #[tokio::test]
        async fn test_meta_session_sec_operation_completion_removes_from_pending() {
            // Add operation; mark complete; verify removed from pending
        }

        #[tokio::test]
        async fn test_meta_session_sec_concurrent_pending_ops_exceed_limit() {
            // 10 concurrent tasks each adding operations
            // Verify total never exceeds max_pending_ops
        }

        #[tokio::test]
        async fn test_meta_session_sec_operation_expiry_detected_on_timeout() {
            // Operation timeout_secs=1; wait; check is_expired()
        }
    }

    mod dashmap_concurrency {
        use super::*;

        #[tokio::test]
        async fn test_meta_session_sec_multiple_clients_concurrent_sessions() {
            // 20 concurrent clients creating sessions
            // Verify each gets unique SessionId
        }

        #[tokio::test]
        async fn test_meta_session_sec_session_lookup_thread_safe() {
            // Client A adds session; Client B immediately looks up
            // No race condition panic
        }

        #[tokio::test]
        async fn test_meta_session_sec_concurrent_session_cleanup_nopanic() {
            // Multiple tasks calling cleanup()
            // Should not panic, idempotent
        }

        #[tokio::test]
        async fn test_meta_session_sec_session_removal_idempotent() {
            // Remove session S; try remove again; should succeed (no-op)
        }

        #[tokio::test]
        async fn test_meta_session_sec_dashmap_iterator_consistent_snapshot() {
            // Iterate over all sessions while concurrent tasks add/remove
            // Should see consistent snapshot
        }

        #[tokio::test]
        async fn test_meta_session_sec_session_get_after_insert() {
            // Insert session; immediately get it; retrieves correct value
        }

        #[tokio::test]
        async fn test_meta_session_sec_dashmap_values_not_corrupted() {
            // Insert 100 sessions; iterate; all unique and non-corrupted
        }
    }

    mod authorization_and_revocation {
        use super::*;

        #[tokio::test]
        async fn test_meta_session_sec_revoked_session_has_timestamp() {
            // Revoke session; revoked_at field is set
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoked_sessions_cannot_renew() {
            // Revoke session S; attempt renew; should fail
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoke_idempotent() {
            // Revoke session; revoke again; should be OK
        }

        #[tokio::test]
        async fn test_meta_session_sec_revocation_reason_preserved() {
            // Revoke with reason; retrieve; reason matches
        }

        #[tokio::test]
        async fn test_meta_session_sec_admin_can_revoke_any_session() {
            // Admin calls revoke(S); should succeed
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoked_operations_rejected() {
            // Session revoked; try to add pending operation; should reject
        }

        #[tokio::test]
        async fn test_meta_session_sec_revocation_cascades_to_operations() {
            // Revoke session; all pending operations in it marked failed
        }

        #[tokio::test]
        async fn test_meta_session_sec_revoked_state_not_transition_back() {
            // Once Revoked state, cannot go back to Active
        }
    }
}
```

---

### Module 2: transport_trace_aggregator_security_tests.rs (~28 tests)

**Located in:** `/home/cfs/claudefs/crates/claudefs-security/src/transport_trace_aggregator_security_tests.rs`

**Module under test:** `claudefs-transport/src/trace_aggregator.rs`

**Imports:**
```rust
use claudefs_transport::trace_aggregator::{TraceId, SpanRecord, TraceData};
use claudefs_transport::otel::{OtlpStatusCode, OtlpAttribute};
use std::time::SystemTime;
use std::collections::HashMap;
```

**Test structure: 5 categories**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod trace_id_generation_and_uniqueness {
        use super::*;

        #[test]
        fn test_transport_trace_sec_random_traceid_distinct() {
            // Generate 1000 random TraceIds; verify all unique
        }

        #[test]
        fn test_transport_trace_sec_traceid_from_bytes_deterministic() {
            // Create TraceId from same [u8;16] twice; verify equal
        }

        #[test]
        fn test_transport_trace_sec_traceid_default_is_zero() {
            // TraceId::default() should equal [0u8; 16]
        }

        #[test]
        fn test_transport_trace_sec_traceid_hashable() {
            // Use TraceIds as HashMap keys; verify no panic
        }

        #[test]
        fn test_transport_trace_sec_traceid_equality_reflexive() {
            // trace_id == trace_id should be true
        }

        #[test]
        fn test_transport_trace_sec_traceid_clone_identical() {
            // TraceId::clone() should be equal to original
        }
    }

    mod span_record_integrity {
        use super::*;

        #[test]
        fn test_transport_trace_sec_span_fields_preserved() {
            // Create SpanRecord with all fields; read back; verify each field unchanged
        }

        #[test]
        fn test_transport_trace_sec_duration_ns_saturating_subtract() {
            // end_time < start_time; duration_ns should not panic, return 0
        }

        #[test]
        fn test_transport_trace_sec_duration_ns_calculation_correct() {
            // start=100, end=500; duration_ns=400
        }

        #[test]
        fn test_transport_trace_sec_status_code_unset_default() {
            // New SpanRecord should have status=Unset
        }

        #[test]
        fn test_transport_trace_sec_with_status_builder_pattern() {
            // Create span; call with_status(Ok); verify status changed
        }

        #[test]
        fn test_transport_trace_sec_attributes_list_preserved() {
            // Create span with attributes; add 100; read back; count=100
        }

        #[test]
        fn test_transport_trace_sec_span_name_into_conversion() {
            // Create span with name="test_op"; verify name field="test_op"
        }
    }

    mod trace_data_aggregation {
        use super::*;

        #[test]
        fn test_transport_trace_sec_multiple_spans_aggregated() {
            // Create TraceData with 10 SpanRecords; verify all stored
        }

        #[test]
        fn test_transport_trace_sec_root_span_identification() {
            // First span in trace should be root_span_id
        }

        #[test]
        fn test_transport_trace_sec_parent_child_relationships() {
            // Span A parent=None, Span B parent=A.span_id
            // Relationships maintained
        }

        #[test]
        fn test_transport_trace_sec_timeline_ordering_by_start_time() {
            // Create spans with start_times=[100, 50, 200]
            // Verify sortable by start_time
        }

        #[test]
        fn test_transport_trace_sec_received_at_ns_recorded() {
            // Create TraceData; received_at_ns should be valid timestamp (> 0 typically)
        }

        #[test]
        fn test_transport_trace_sec_trace_id_in_data_preserved() {
            // TraceData with trace_id=X; retrieve; verify X
        }
    }

    mod critical_path_analysis {
        use super::*;

        #[test]
        fn test_transport_trace_sec_critical_path_longest_dependency_chain() {
            // 3 spans: A→B→C (parent-child)
            // Critical path should include all 3
        }

        #[test]
        fn test_transport_trace_sec_critical_path_excludes_parallel_branches() {
            // Spans A, B (both parent=root), should not double-count
        }

        #[test]
        fn test_transport_trace_sec_latency_attribution_per_stage() {
            // Client span, metadata span, storage span
            // Compute latencies per stage
        }

        #[test]
        fn test_transport_trace_sec_p50_p95_p99_percentiles_computed() {
            // 100 spans with durations
            // Compute percentiles; verify p50 < p95 < p99
        }

        #[test]
        fn test_transport_trace_sec_outlier_detection_anomalous_spans() {
            // Most spans ~1000ns, one span ~100000ns
            // Outlier detection identifies it
        }
    }

    mod memory_and_performance {
        use super::*;

        #[test]
        fn test_transport_trace_sec_large_trace_no_oom() {
            // Create TraceData with 10000 spans; verify no OOM
        }

        #[test]
        fn test_transport_trace_sec_span_storage_not_copied() {
            // Span added to TraceData; original not deep-copied multiple times
        }

        #[test]
        fn test_transport_trace_sec_hash_based_lookup_efficient() {
            // 10000 TraceIds; lookup by ID should be O(1)
        }

        #[tokio::test]
        async fn test_transport_trace_sec_concurrent_span_insertion_thread_safe() {
            // Multiple tokio tasks inserting spans into same trace
            // No panic
        }
    }
}
```

---

### Module 3: transport_bandwidth_shaper_security_tests.rs (~30 tests)

**Located in:** `/home/cfs/claudefs/crates/claudefs-security/src/transport_bandwidth_shaper_security_tests.rs`

**Module under test:** `claudefs-transport/src/bandwidth_shaper.rs`

**Imports:**
```rust
use claudefs_transport::bandwidth_shaper::*;
use std::time::Duration;
use std::thread;
```

**Test structure: 5 categories**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod token_bucket_correctness {
        use super::*;

        #[test]
        fn test_transport_bw_sec_initial_tokens_equals_capacity() {
            // Create bucket with capacity=1000
            // Initial tokens should be 1000
        }

        #[test]
        fn test_transport_bw_sec_refill_respects_rate() {
            // Create bucket; refill_rate_per_sec=100
            // Wait 1 sec; verify tokens +100 (clamped at capacity)
        }

        #[test]
        fn test_transport_bw_sec_refill_stops_at_capacity() {
            // tokens=1000, capacity=1000; refill
            // tokens should stay 1000 (no overfill)
        }

        #[test]
        fn test_transport_bw_sec_try_consume_fails_insufficient_tokens() {
            // tokens=50, try_consume(100); should fail
        }

        #[test]
        fn test_transport_bw_sec_try_consume_succeeds_if_sufficient() {
            // tokens=100, try_consume(50); should succeed, tokens→50
        }

        #[tokio::test]
        async fn test_transport_bw_sec_try_consume_atomic_check_and_subtract() {
            // Concurrent try_consume() calls
            // Verify no double-booking
        }

        #[test]
        fn test_transport_bw_sec_last_refill_timestamp_updated() {
            // Create bucket; call refill()
            // last_refill_ns should be updated
        }

        #[test]
        fn test_transport_bw_sec_refill_calculation_correct() {
            // refill_rate=1000 tokens/sec; elapsed=0.5s
            // Should add 500 tokens
        }
    }

    mod enforcement_modes {
        use super::*;

        #[test]
        fn test_transport_bw_sec_hard_mode_rejects_over_limit() {
            // Hard mode, allocation=100 bytes/sec
            // Try to consume 200; should reject
        }

        #[test]
        fn test_transport_bw_sec_soft_mode_allows_over_limit() {
            // Soft mode, allocation=100 bytes/sec
            // Try to consume 200; should allow (warn/backpressure only)
        }

        #[test]
        fn test_transport_bw_sec_hard_soft_mode_switch() {
            // Allocation in Hard mode; switch to Soft
            // Behavior should change
        }

        #[test]
        fn test_transport_bw_sec_mode_change_does_not_reset_tokens() {
            // Hard mode, tokens=50; switch to Soft
            // tokens should still be 50
        }

        #[tokio::test]
        async fn test_transport_bw_sec_per_tenant_enforcement_independent() {
            // Tenant A in Hard, Tenant B in Soft
            // Enforcement independent
        }

        #[test]
        fn test_transport_bw_sec_enforcement_mode_validation() {
            // Try to set invalid enforcement_mode
            // Should reject or default to Soft
        }

        #[test]
        fn test_transport_bw_sec_hard_mode_with_burst_capacity() {
            // Hard mode; burst_bytes=500
            // Can exceed per_sec temporarily up to burst
        }

        #[test]
        fn test_transport_bw_sec_soft_mode_logs_warning() {
            // Soft mode, exceed limit
            // Should log warning (verify via test output or tracing)
        }
    }

    mod per_tenant_isolation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_separate_bucket_per_tenant() {
            // 10 different BandwidthIds; each gets separate token bucket
        }

        #[test]
        fn test_transport_bw_sec_tenant_a_usage_independent_tenant_b() {
            // Tenant A consumes 100 tokens
            // Tenant B's bucket unaffected
        }

        #[tokio::test]
        async fn test_transport_bw_sec_concurrent_allocations_different_tenants() {
            // Spawn 20 tasks for Tenant A and 20 for Tenant B
            // No cross-tenant interference
        }

        #[test]
        fn test_transport_bw_sec_dashmap_no_cross_tenant_leak() {
            // DashMap stores buckets
            // lookup(Tenant A) doesn't see Tenant B's data
        }

        #[test]
        fn test_transport_bw_sec_tenant_removal_idempotent() {
            // Remove allocation for Tenant A; try remove again
            // Idempotent (OK)
        }

        #[test]
        fn test_transport_bw_sec_tenant_allocation_add_independently() {
            // Add allocation for A; add for B
            // Both present and independent
        }

        #[test]
        fn test_transport_bw_sec_same_tenant_multiple_allocations_share_bucket() {
            // Multiple threads for same Tenant
            // All share same token bucket
        }
    }

    mod burst_capacity_handling {
        use super::*;

        #[test]
        fn test_transport_bw_sec_burst_allows_temporary_exceed() {
            // per_sec=100, burst=500
            // consume 300 (exceeds per_sec but < burst); should succeed
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_depletes() {
            // Use burst; burst depletes
            // Subsequent try_consume waits for normal rate
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_replenishes() {
            // Use burst; wait 1 sec
            // burst should partially replenish
        }

        #[test]
        fn test_transport_bw_sec_over_burst_rejected() {
            // burst=500; try_consume(600)
            // should reject as exceeds burst_bytes
        }
    }

    mod configuration_validation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_valid_config_bytes_per_sec_gt_zero() {
            // BandwidthAllocation::is_valid() with bytes_per_sec > 0
            // Should return true
        }

        #[test]
        fn test_transport_bw_sec_valid_config_burst_bytes_gt_zero() {
            // BandwidthAllocation::is_valid() with burst_bytes > 0
            // Should return true
        }

        #[test]
        fn test_transport_bw_sec_invalid_config_zero_values() {
            // bytes_per_sec=0 or burst_bytes=0
            // is_valid() should return false
        }
    }
}
```

---

## Code Style

- **File headers:** Same as Part 1
- **Test organization:** mod tests { ... } with nested mod <category> { ... }
- **Assertions:** Use descriptive messages
- **Concurrency:** Use `#[tokio::test]` for async, `#[test]` for sync
- **No stubs:** All test functions must have full implementation

---

## Expected Output

- `meta_client_session_security_tests.rs` (38 tests)
- `transport_trace_aggregator_security_tests.rs` (28 tests)
- `transport_bandwidth_shaper_security_tests.rs` (30 tests)
- **Total: 96 tests**, all fully functional and compile-ready

---

**Ready for implementation. Generate all 3 test modules with ~96 tests total.**
