# A4 Transport: Prometheus Metrics Exporter Integration

## Context

A11 Phase 4 Block 2 requires all crates to export Prometheus metrics. A4 (Transport) needs to provide:
- RPC latency (histogram)
- Bandwidth (counter)
- Trace aggregation statistics
- Router scores/decisions
- Connection pool metrics
- Backpressure signals
- QoS metrics

Current state:
- `crates/claudefs-transport/src/metrics.rs`: Basic counter/gauge module with MetricsSnapshot
- Transport subsystem tracks: requests, responses, bytes, errors, retries, timeouts, connections, health checks
- No Prometheus client integration yet

## Requirements

### 1. Create Prometheus Exporter Module (`prometheus_exporter.rs`)

Create a new module that:
- Depends on `prometheus` crate (add to Cargo.toml: `prometheus = "0.13"`)
- Provides a `PrometheusTransportMetrics` struct that wraps all transport metrics
- Exports metrics via `/metrics` HTTP endpoint (compatible with standard Prometheus scraping)
- Implements `std::fmt::Display` for Prometheus text format output

### 2. Metrics to Export (Prometheus types)

**Counters:**
- `transport_requests_sent_total` — total requests sent (counter)
- `transport_requests_received_total` — total requests received (counter)
- `transport_responses_sent_total` — total responses sent (counter)
- `transport_responses_received_total` — total responses received (counter)
- `transport_errors_total` — total errors (counter with error_type label)
- `transport_retries_total` — total retries (counter)
- `transport_timeouts_total` — total timeouts (counter)
- `transport_connections_opened_total` — connections opened (counter)
- `transport_connections_closed_total` — connections closed (counter)
- `transport_health_checks_total` — health checks performed (counter)
- `transport_health_checks_failed_total` — health checks failed (counter)

**Gauges:**
- `transport_active_connections` — currently active connections (gauge)
- `transport_bytes_sent_total` — total bytes sent (counter)
- `transport_bytes_received_total` — total bytes received (counter)

**Histograms (if available from trace_aggregator):**
- `transport_rpc_latency_ms` — RPC latency in milliseconds
- `transport_trace_critical_path_ms` — critical path latency from trace aggregation

**Gauges (if available from connection pool stats):**
- `transport_pool_connections_idle` — idle connections in pool
- `transport_pool_connections_acquired` — acquired connections in pool
- `transport_pool_wait_time_ms` — wait time for connection from pool

**Gauges (if available from backpressure coordinator):**
- `transport_backpressure_level` — current backpressure level (enum as number: 0=Ok, 1=Slow, 2=Degraded, 3=Overloaded)
- `transport_backpressure_signals_emitted_total` — signals emitted (counter)

**Gauges (if available from QoS):**
- `transport_qos_bandwidth_limit_mbps` — configured bandwidth limit
- `transport_qos_current_bandwidth_mbps` — current bandwidth usage

### 3. Integration Points

The new module needs to:
1. Accept references to existing metrics collectors (TransportMetrics, trace_aggregator, pool stats, etc.)
2. Implement `fn scrape(&self) -> String` that returns Prometheus text format
3. Register with a centralized metrics registry (or use global registry pattern)
4. Be callable from management/HTTP API to serve `/metrics` endpoint

### 4. Cargo.toml Changes

Add dependency:
```toml
prometheus = "0.13"
```

### 5. Files to Create/Modify

**New files:**
- `crates/claudefs-transport/src/prometheus_exporter.rs` — Prometheus exporter implementation

**Modified files:**
- `crates/claudefs-transport/Cargo.toml` — add prometheus dependency
- `crates/claudefs-transport/src/lib.rs` — export prometheus_exporter module

### 6. Testing Requirements

Tests should verify:
1. PrometheusTransportMetrics can be created with metrics references
2. Scrape output contains all expected metric lines
3. Scrape output follows Prometheus text format (HELP, TYPE, value lines)
4. Metrics values are correctly reflected in output
5. No panics on concurrent metric updates during scraping

### 7. Design Notes

- Use builder pattern for metric registration (allow selective export of subsets)
- Lazy-load metrics (don't export zero-value metrics by default, or do if explicitly enabled)
- Thread-safe (metrics can be updated concurrently with scraping)
- No lock contention during scrape (use atomic reads only)
- Support labels (e.g., error_type label for different error kinds)

### 8. Example Usage (in management API)

```rust
let transport_metrics = PrometheusTransportMetrics::new(
    &transport.metrics,
    &transport.trace_aggregator,
    &transport.pool_stats,
);

// In HTTP handler for GET /metrics
let prometheus_text = transport_metrics.scrape();
response.body(prometheus_text)
```

### 9. Documentation

Add module-level doc comments explaining:
- Metric names and their meanings
- Labels used (if any)
- How to scrape from Prometheus
- Integration with Grafana

## Implementation Approach

Use OpenCode to generate:
1. Complete `prometheus_exporter.rs` with all metrics and proper formatting
2. Updated `lib.rs` exports
3. Updated `Cargo.toml` with dependencies
4. Comprehensive unit tests for the exporter

Output should include:
- Prometheus format validation (proper HELP, TYPE, value syntax)
- All counter, gauge, and histogram types properly formatted
- Thread-safe metric collection
- No performance overhead for metric collection

## Success Criteria

✅ All transport metrics exported to Prometheus text format
✅ Can be integrated into management API for `/metrics` endpoint
✅ All tests passing
✅ No breaking changes to existing transport APIs
✅ Metrics values reflect actual system state
✅ Can scale to handle high request volumes without lock contention
