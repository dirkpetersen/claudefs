# Task: Write transport_conn_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-transport` crate focusing on connection migration, multiplexing, keepalive, deadline management, and cancellation token propagation.

## File location
`crates/claudefs-security/src/transport_conn_security_tests.rs`

## Module structure
```rust
//! Connection security tests for claudefs-transport: migration, mux, keepalive, deadline, cancel.
//!
//! Part of A10 Phase 9: Transport connection security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs and module exploration)

```rust
use claudefs_transport::{
    // Connection migration
    ConnectionId, MigrationManager, MigrationConfig, MigrationState, MigrationReason, MigrationError, MigrationRecord, MigrationStats, MigrationStatsSnapshot,
    // Multiplexing
    Multiplexer, MuxConfig, StreamHandle, StreamState, StreamId,
    // Keep-alive
    KeepAliveConfig, KeepAliveManager, KeepAliveState, KeepAliveStats, KeepAliveTracker,
    // Deadline
    Deadline, DeadlineContext,
    // Cancel
    CancelToken, CancelHandle, CancelReason, CancelRegistry, CancelStats,
    // Hedge
    HedgeConfig, HedgePolicy, HedgeStats, HedgeTracker,
    // Congestion
    CongestionAlgorithm, CongestionConfig, CongestionState, CongestionStats, CongestionWindow,
    // Batch
    BatchConfig, BatchCollector, BatchEnvelope, BatchRequest, BatchResponse, BatchResult, BatchStats,
    // Compression
    CompressionAlgorithm, CompressionConfig, CompressedPayload, Compressor, CompressionStats,
    // Frame
    Frame, FrameHeader, Opcode, FrameFlags,
    // Error
    TransportError,
};
use claudefs_transport::deadline::{encode_deadline, decode_deadline};
use claudefs_transport::cancel::new_cancel_pair;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating

- `transport_security_tests.rs`: cert auth, zero-copy pool, flow control, backpressure
- `transport_deep_security_tests.rs`: auth time default, frame validation, request dedup, rate limiting, enrollment, multipath
- `transport_tests.rs`: basic frame encode/decode

DO NOT duplicate any of these. Focus on migration, mux, keepalive, deadline, cancel, hedge, congestion, batch, compression.

## Test categories (25 tests total, 5 per category)

### Category 1: Connection Migration Security (5 tests)

1. **test_migration_concurrent_limit** — Create MigrationManager with max_concurrent_migrations=2. Start 2 migrations. Try to start 3rd. Verify MigrationError::TooManyConcurrent.

2. **test_migration_already_migrating** — Create MigrationManager. Start migration for connection 1. Try to start another migration for connection 1. Verify MigrationError::AlreadyMigrating.

3. **test_migration_id_uniqueness** — Create MigrationManager. Start 10 migrations. Verify all migration IDs are unique.

4. **test_migration_state_machine** — Create MigrationManager. Start migration. Record request migrated. Complete migration. Verify final state is Completed. Start another, fail it. Verify state is Failed.

5. **test_migration_disabled** — Create MigrationManager with enabled=false. Try to start migration. Verify MigrationError::Disabled.

### Category 2: Multiplexing Security (5 tests)

6. **test_mux_max_concurrent_streams** — Create Multiplexer with max_concurrent_streams=3. Open 3 streams. Try to open 4th. Verify error returned.

7. **test_mux_stream_id_uniqueness** — Create Multiplexer. Open 100 streams. Verify all stream IDs are unique.

8. **test_mux_dispatch_unknown_stream** — Create Multiplexer. Call dispatch_response with non-existent stream ID. Verify returns false (idempotent, no panic).

9. **test_mux_cancel_stream** — Create Multiplexer. Open stream. Cancel it. Verify active_streams() decreased. Open another stream — verify succeeds (slot freed).

10. **test_mux_cancel_nonexistent** — Create Multiplexer. Call cancel_stream with non-existent ID. Verify returns false (no panic).

### Category 3: Keep-Alive State Machine (5 tests)

11. **test_keepalive_initial_state** — Create KeepAliveTracker with enabled config. Verify state is Active and missed_count is 0.

12. **test_keepalive_timeout_transitions** — Create KeepAliveTracker. Record timeout. Verify state is Warning. Record 2 more timeouts. Verify state is Dead (3 misses = dead).

13. **test_keepalive_reset_recovers** — Create KeepAliveTracker. Record 3 timeouts (state=Dead). Call reset(). Verify state returns to Active and missed_count is 0.

14. **test_keepalive_disabled_state** — Create KeepAliveTracker with enabled=false. Verify state is Disabled. Record timeout. Verify state remains Disabled.

15. **test_keepalive_is_alive_check** — Create KeepAliveTracker. Verify is_alive() true in Active state. Record 1 timeout (Warning). Verify is_alive() still true. Record 2 more (Dead). Verify is_alive() false.

### Category 4: Deadline & Hedge (5 tests)

16. **test_deadline_zero_duration_expired** — Create Deadline with Duration::ZERO. Verify is_expired() returns true immediately. Verify remaining() returns None.

17. **test_deadline_encode_decode_roundtrip** — Create DeadlineContext with a 30-second timeout. Encode. Decode. Verify the decoded deadline has approximately the same expiry.

18. **test_deadline_no_deadline_check_ok** — Create DeadlineContext::new() (no deadline). Call check(). Verify Ok(()) returned. Verify is_expired() is false.

19. **test_hedge_disabled_blocks_all** — Create HedgePolicy with enabled=false. Call should_hedge(1000, false). Verify returns false (hedging disabled).

20. **test_hedge_write_exclusion** — Create HedgePolicy with exclude_writes=true. Call should_hedge(1000, true). Verify returns false (writes excluded). Call should_hedge(1000, false). Verify returns true (reads allowed).

### Category 5: Cancellation & Batch (5 tests)

21. **test_cancel_token_propagation** — Create cancel pair. Cancel with reason ClientDisconnected. Verify token.is_cancelled() true. Verify cancelled_reason() is ClientDisconnected.

22. **test_cancel_registry_cancel_all** — Create CancelRegistry. Register 5 requests. Call cancel_all(ServerShutdown). Verify all 5 tokens are cancelled.

23. **test_cancel_child_independence** — Create cancel pair. Create child from token. Cancel child. Verify parent NOT cancelled. Cancel parent. Verify child is cancelled.

24. **test_batch_envelope_encode_decode** — Create BatchEnvelope with 3 BatchRequests. Encode to bytes. Decode from bytes. Verify len() == 3 and total_payload_bytes matches.

25. **test_batch_response_error_tracking** — Create BatchResponse::error(). Verify is_error() returns true. Create BatchResponse::success(). Verify is_error() returns false.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-TRANS-CONN-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For ConnectionId, use ConnectionId(1), ConnectionId(2) etc.
- For Frame, use Frame { header: FrameHeader { ... }, payload: vec![] } or a constructor if available

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
