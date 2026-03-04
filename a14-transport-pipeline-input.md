# Task: Write transport_pipeline_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-transport` crate focusing on congestion window control, circuit breaker state machine, and request pipeline middleware.

## File location
`crates/claudefs-security/src/transport_pipeline_security_tests.rs`

## Module structure
```rust
//! Transport pipeline/congestion/circuit breaker security tests.
//!
//! Part of A10 Phase 14: Transport pipeline security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_transport::{
    CongestionAlgorithm, CongestionConfig, CongestionState, CongestionStats, CongestionWindow,
};
use claudefs_transport::circuitbreaker::{
    CircuitState as TransCircuitState, CircuitBreakerConfig as TransCBConfig,
    CircuitBreaker as TransCircuitBreaker,
    DEFAULT_FAILURE_THRESHOLD, DEFAULT_SUCCESS_THRESHOLD, DEFAULT_OPEN_DURATION_MS,
};
use claudefs_transport::pipeline::{
    PipelineDirection, StageAction, PipelineError, StageConfig, PipelineConfig,
    PipelineRequest, StageResult, PipelineResult, PipelineStatsSnapshot, Pipeline,
    StageProcessor, PassthroughStage, RejectStage, HeaderStage, StageId,
};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests. Try alternate paths like `claudefs_transport::congestion::CongestionWindow` if top-level fails.

## Existing tests to AVOID duplicating
- `transport_security_tests.rs`: cert auth, zero-copy pool, flow control, backpressure
- `transport_deep_security_tests.rs`: auth time, frame validation, request dedup, rate limiting
- `transport_conn_security_tests.rs`: migration, mux, keepalive, deadline, cancel, hedge, batch

DO NOT duplicate these. Focus on congestion, circuit breaker, pipeline.

## Test categories (25 tests total)

### Category 1: Congestion Window Control (5 tests)

1. **test_congestion_initial_slow_start** — Create CongestionWindow::new(CongestionConfig::default()). Verify state() is SlowStart. Verify window_size() == initial_window (65536).

2. **test_congestion_window_growth_on_ack** — Create window. Send 1000 bytes. Ack 1000 bytes with rtt_us=10000. Verify window_size() increased (slow start doubles). Send and ack more. Verify window continues to grow.

3. **test_congestion_loss_reduces_window** — Create window. Send data. Ack to grow window. Record a loss. Verify window_size() decreased. Verify state changes to Recovery or CongestionAvoidance.

4. **test_congestion_min_window_floor** — Create window. Record many losses. Verify window_size() never goes below min_window (4096). (FINDING: min window prevents complete stall).

5. **test_congestion_stats_tracking** — Create window. Send and ack data. Verify stats().total_sent and total_acked updated. Record loss. Verify total_lost and loss_events incremented.

### Category 2: Transport Circuit Breaker (5 tests)

6. **test_trans_circuit_breaker_defaults** — Create TransCircuitBreaker::default(). Verify state() == Closed. Verify failure_count() == 0. Verify can_execute() returns true.

7. **test_trans_circuit_breaker_opens_on_failures** — Create breaker with default config (failure_threshold=5). Record 4 failures. Verify still Closed. Record 5th failure. Verify Open. Verify can_execute() returns false.

8. **test_trans_circuit_breaker_half_open** — Create breaker. Record failures to open. Wait for open_duration or manually verify half-open logic. Verify state transitions correctly.

9. **test_trans_circuit_breaker_reset** — Create breaker. Record failures until Open. Call reset(). Verify state() == Closed. Verify failure_count() == 0. Verify can_execute() returns true.

10. **test_trans_circuit_breaker_success_recovers** — Create breaker. Open it via failures. When in HalfOpen (after duration), record successes equal to success_threshold. Verify returns to Closed.

### Category 3: Pipeline Stage Management (5 tests)

11. **test_pipeline_add_passthrough_stage** — Create Pipeline::new(PipelineConfig::default()). Add PassthroughStage. Verify stage_count() == 1. Create PipelineRequest. Execute. Verify final_action is Continue.

12. **test_pipeline_reject_stage** — Create pipeline. Add RejectStage::new(opcode=42). Create request with opcode=42. Execute. Verify final_action is Reject. Create request with opcode=1. Execute. Verify Continue (different opcode).

13. **test_pipeline_max_stages_limit** — Create pipeline with max_stages=2. Add 2 stages (OK). Try add 3rd. Verify PipelineError::TooManyStages. (FINDING: stage limit prevents unbounded pipeline growth).

14. **test_pipeline_duplicate_stage_id** — Create pipeline. Add stage with id=1. Try add another stage with id=1. Verify PipelineError::DuplicateStageId.

15. **test_pipeline_enable_disable** — Create pipeline. Add stage with id=1. Disable stage 1. Execute request. Verify disabled stage is skipped. Enable stage 1. Execute. Verify stage is now active.

### Category 4: Pipeline Execution & Stats (5 tests)

16. **test_pipeline_execution_order** — Create pipeline. Add stages with order 10, 5, 20. Execute. Verify stages executed in order (5, 10, 20) by checking stage_results order.

17. **test_pipeline_header_stage** — Create pipeline. Add HeaderStage::new(b"PREFIX".to_vec()). Create request with payload b"data". Execute. Verify final_payload starts with PREFIX prepended.

18. **test_pipeline_stats_tracking** — Create pipeline with passthrough and reject stages. Execute 3 requests (2 pass, 1 rejected). Verify stats().total_requests == 3, total_rejections == 1.

19. **test_pipeline_remove_stage** — Create pipeline with 2 stages. Remove first stage. Verify stage_count() == 1. Execute. Verify only remaining stage processes.

20. **test_pipeline_request_metadata** — Create PipelineRequest with metadata. Add stage that reads metadata. Execute. Verify metadata accessible during processing.

### Category 5: Config Defaults & Edge Cases (5 tests)

21. **test_congestion_config_defaults** — Create CongestionConfig::default(). Verify initial_window == 65536. Verify min_window == 4096. Verify max_window == 16MB. Verify aimd_decrease_factor between 0.0 and 1.0.

22. **test_pipeline_config_defaults** — Create PipelineConfig::default(). Verify max_stages > 0. Verify max_payload_bytes > 0. Document fail_open and track_stage_timing defaults.

23. **test_trans_cb_config_defaults** — Verify DEFAULT_FAILURE_THRESHOLD == 5. Verify DEFAULT_SUCCESS_THRESHOLD == 3. Verify DEFAULT_OPEN_DURATION_MS == 30000.

24. **test_pipeline_empty_execute** — Create pipeline with no stages. Execute request. Verify final_action is Continue (empty pipeline passes through). Verify stages_executed == 0.

25. **test_congestion_can_send** — Create window. Verify can_send(1000) returns true initially. Send window_size() bytes. Verify can_send(1) returns false (window full). Verify available_window() == 0.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-TRANS-PIPE-XX: description`
- If a type is not public, skip that test and add an alternative
- DO NOT use any async code — all tests are synchronous
- Use `assert!`, `assert_eq!`, `matches!`
- For PipelineRequest: PipelineRequest::new(request_id, opcode, payload, PipelineDirection::Inbound)
- For StageConfig: StageConfig::new(id, "name", order, PipelineDirection::Inbound)

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
