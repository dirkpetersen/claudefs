# Phase 33 Block 3: Distributed Tracing Integration
## OpenCode Implementation Prompt

**Target:** OpenTelemetry/Jaeger integration for latency attribution
**Output:** ~500 LOC (source) + 16 tests

## Context
Implement distributed tracing across the data reduction pipeline with per-stage latency attribution. Integrate with OpenTelemetry (OTEL) and export traces to Jaeger for visualization.

## Key Components

### 1. TracingContext (150 LOC)
```rust
pub struct TracingContext {
    otel_context: otel::Context,
    trace_id: u64,
    span_id: u64,
}

pub struct SpanGuard {
    span: Box<dyn Span>,
    start_time: Instant,
}

impl SpanGuard {
    pub fn end_with_metric(&self, latency_ms: u64);
}
```

### 2. Per-Stage Histograms (150 LOC)
- Dedup stage latency histogram (p50, p99)
- Compression stage latency histogram
- Encryption stage latency histogram
- Write stage latency histogram

### 3. Jaeger Export (200 LOC)
- Export spans to Jaeger collector
- Batch span export for efficiency
- Trace ID propagation across RPC boundaries

## Test Categories (16 tests)

1. **Span propagation** (4 tests)
   - test_span_propagation_inline_dedup
   - test_span_propagation_async_gc
   - test_span_propagation_cross_node
   - test_span_propagation_nested_spans

2. **Latency attribution** (4 tests)
   - test_latency_dedupe_stage
   - test_latency_compress_stage
   - test_latency_encrypt_stage
   - test_latency_write_stage

3. **Metrics export** (4 tests)
   - test_histogram_metrics_dedupe
   - test_histogram_metrics_all_stages
   - test_trace_export_jaeger
   - test_otel_context_sampling

4. **Integration testing** (4 tests)
   - test_trace_cluster_write_path
   - test_trace_similarity_lookup
   - test_trace_s3_tiering
   - test_trace_performance_correlation

## Generate
- `crates/claudefs-reduce/src/tracing_context.rs` — Span management
- `crates/claudefs-reduce/src/jaeger_exporter.rs` — Jaeger integration
- `crates/claudefs-reduce/tests/cluster_tracing_integration.rs` — 16 tests

All tests marked #[ignore], documentation comments required.
