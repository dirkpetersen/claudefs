# Prometheus Metrics Integration Guide

**For:** Crate Owners A1-A8
**Prepared by:** A11 Infrastructure & CI
**Date:** 2026-03-06

This guide outlines the standard approach for integrating Prometheus metrics into each ClaudeFS crate.

## Architecture Overview

```
Crate Metrics (A1-A8)
    ↓
lazy_static REGISTRY (Prometheus)
    ↓
HTTP Exporter (Tokio)
    ↓
/metrics endpoint (port 9001-9008)
    ↓
Prometheus scraper
    ↓
Grafana dashboards & alerts
```

## Per-Crate Integration Template

Each crate should:

1. **Add Prometheus dependency** to `Cargo.toml`
2. **Create metrics module** (`src/metrics.rs`)
3. **Export metrics** in `lib.rs`
4. **Spawn exporter task** during daemon startup
5. **Instrument code** with metric updates

### Step 1: Add Dependency

In `crates/claudefs-{crate}/Cargo.toml`, add:

```toml
[dependencies]
prometheus = "0.13"
lazy_static = "1.4"
```

### Step 2: Create Metrics Module

Create `src/metrics.rs`:

```rust
use lazy_static::lazy_static;
use prometheus::{Registry, Counter, Histogram, IntGauge, HistogramOpts, CounterOpts, IntGaugeOpts, TextEncoder, Encoder};
use anyhow::Result;

// Create global registry
lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // Example metrics for A1 (Storage)
    pub static ref STORAGE_OPERATIONS_TOTAL: Counter =
        Counter::with_opts(
            CounterOpts::new("storage_operations_total", "Total storage operations"),
        ).expect("Failed to create storage_operations_total metric");

    pub static ref STORAGE_LATENCY: Histogram =
        Histogram::with_opts(
            HistogramOpts::new("storage_latency_ms", "Storage operation latency in milliseconds")
                .buckets(vec![1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0]),
        ).expect("Failed to create storage_latency metric");

    pub static ref STORAGE_IO_DEPTH_CURRENT: IntGauge =
        IntGauge::with_opts(
            IntGaugeOpts::new("storage_io_depth_current", "Current I/O queue depth"),
        ).expect("Failed to create io_depth_current metric");

    pub static ref STORAGE_BYTES_READ_TOTAL: Counter =
        Counter::with_opts(
            CounterOpts::new("storage_bytes_read_total", "Total bytes read from storage"),
        ).expect("Failed to create bytes_read_total metric");

    pub static ref STORAGE_BYTES_WRITTEN_TOTAL: Counter =
        Counter::with_opts(
            CounterOpts::new("storage_bytes_written_total", "Total bytes written to storage"),
        ).expect("Failed to create bytes_written_total metric");

    pub static ref STORAGE_ERRORS_TOTAL: Counter =
        Counter::with_opts(
            CounterOpts::new("storage_errors_total", "Total storage errors"),
        ).expect("Failed to create errors_total metric");
}

/// Register all metrics with the global registry
pub fn register_metrics() -> Result<()> {
    REGISTRY.register(Box::new(STORAGE_OPERATIONS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(STORAGE_LATENCY.clone()))?;
    REGISTRY.register(Box::new(STORAGE_IO_DEPTH_CURRENT.clone()))?;
    REGISTRY.register(Box::new(STORAGE_BYTES_READ_TOTAL.clone()))?;
    REGISTRY.register(Box::new(STORAGE_BYTES_WRITTEN_TOTAL.clone()))?;
    REGISTRY.register(Box::new(STORAGE_ERRORS_TOTAL.clone()))?;
    Ok(())
}

/// Gather metrics in Prometheus text format
pub fn gather_metrics() -> Result<String> {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
```

### Step 3: Export Metrics from lib.rs

In `src/lib.rs`, add:

```rust
pub mod metrics;
pub use metrics::{REGISTRY, register_metrics, gather_metrics};
```

### Step 4: Spawn HTTP Exporter

In the crate's startup code (typically `bin/main.rs` or within a daemon initialization), spawn the exporter:

```rust
use tokio::net::TcpListener;
use axum::{Router, routing::get, response::IntoResponse};

async fn start_metrics_server(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/metrics", get(metrics_handler));

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    let _server = axum::serve(listener, app)
        .await?;

    Ok(())
}

async fn metrics_handler() -> impl IntoResponse {
    match gather_metrics() {
        Ok(metrics) => (axum::http::StatusCode::OK, metrics),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)),
    }
}

// In your daemon/server startup:
tokio::spawn(start_metrics_server(9001)); // A1 uses 9001
```

### Step 5: Instrument Code

Throughout your crate, update metrics:

```rust
use crate::metrics::*;

// In hot path functions:
pub fn read_block(block_id: u64) -> Result<Vec<u8>> {
    let start = Instant::now();

    STORAGE_OPERATIONS_TOTAL.inc();

    // Actual read operation
    let data = do_read(block_id)?;

    STORAGE_BYTES_READ_TOTAL.inc_by(data.len() as u64);
    let elapsed_ms = start.elapsed().as_millis() as f64;
    STORAGE_LATENCY.observe(elapsed_ms);

    Ok(data)
}

// Update gauge periodically (in background task):
pub async fn update_io_depth_gauge() {
    loop {
        let depth = get_current_io_depth();
        STORAGE_IO_DEPTH_CURRENT.set(depth as i64);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
```

## Port Assignment

Each crate gets a dedicated metrics port:

| Agent | Crate | Port | Metrics |
|-------|-------|------|---------|
| **A1** | claudefs-storage | 9001 | I/O ops, latency, queue depth |
| **A2** | claudefs-meta | 9002 | Raft consensus, metadata ops |
| **A3** | claudefs-reduce | 9003 | Dedup, compression, encryption |
| **A4** | claudefs-transport | 9004 | RPC calls, RDMA, TCP |
| **A5** | claudefs-fuse | 9005 | FUSE syscalls, cache hits |
| **A6** | claudefs-repl | 9006 | Replication lag, conflicts |
| **A7** | claudefs-gateway | 9007 | NFS/pNFS/S3 operations |
| **A8** | claudefs-mgmt | 9008 | Queries, exports, health |

## Prometheus Configuration

The orchestrator will auto-configure Prometheus to scrape each port. Manual update:

In `monitoring/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'storage'
    static_configs:
      - targets: ['127.0.0.1:9001']
    scrape_interval: 15s

  - job_name: 'metadata'
    static_configs:
      - targets: ['127.0.0.1:9002']
    scrape_interval: 15s

  # ... (A3-A8 similarly)
```

## Testing Your Metrics

After implementing metrics in your crate:

```bash
# 1. Start your crate daemon with metrics enabled
cargo run --bin claudefs-server

# 2. Test endpoint in another terminal
curl http://127.0.0.1:9001/metrics | head -30

# Expected output:
# # HELP storage_operations_total Total storage operations
# # TYPE storage_operations_total counter
# storage_operations_total 1234
# # HELP storage_latency_ms Storage operation latency in milliseconds
# # TYPE storage_latency_ms histogram
# storage_latency_ms_bucket{le="1.0"} 100
# ...
```

## Recommended Metrics per Crate

### A1: Storage Engine
- `storage_operations_total` (counter)
- `storage_latency_*` (histogram)
- `storage_io_depth_current` (gauge)
- `storage_read_iops_total`, `storage_write_iops_total` (counters)
- `storage_bytes_read_total`, `storage_bytes_written_total` (counters)
- `storage_errors_total` (counter)

### A2: Metadata Service
- `metadata_operations_total{operation="create|read|update|delete"}` (counter)
- `metadata_latency_*` (histogram)
- `raft_node_state{shard, node}` (gauge: 0=follower, 1=leader, 2=candidate)
- `raft_replication_lag{shard, follower}` (gauge)
- `raft_uncommitted_entries{shard}` (gauge)

### A3: Data Reduction
- `dedup_fingerprints_total` (gauge)
- `dedup_matches_total` (counter)
- `compression_ratio` (gauge)
- `encryption_operations_total` (counter)

### A4: Transport
- `rpc_calls_total{method, status}` (counter)
- `rpc_latency_*` (histogram)
- `rdma_operations_total{operation}` (counter)
- `tcp_connections_active` (gauge)

### A5: FUSE Client
- `fuse_operations_total{operation}` (counter)
- `fuse_latency_*` (histogram)
- `cache_hits_total`, `cache_misses_total` (counters)

### A6: Replication
- `replication_lag_entries{shard, destination}` (gauge)
- `replication_ops_total{operation}` (counter)
- `conflict_detected_total` (counter)

### A7: Protocol Gateways
- `nfs_operations_total{operation}` (counter)
- `pnfs_layouts_active` (gauge)
- `s3_operations_total{operation}` (counter)

### A8: Management
- `parquet_rows_written` (counter)
- `query_latency_*` (histogram)
- `export_errors_total` (counter)

## Common Pitfalls

1. **Registration Errors:** Call `register_metrics()` before any metrics operations
2. **Thread Safety:** All metrics must be thread-safe (use `lazy_static`)
3. **High Cardinality:** Avoid metric labels with unbounded cardinality (e.g., filenames)
4. **Performance:** Keep metric updates fast (atomic operations, not allocations)

## References

- [Prometheus Rust Client](https://github.com/prometheus/client_rust)
- [Prometheus Best Practices](https://prometheus.io/docs/practices/instrumentation/)
- [Grafana Dashboard JSON Model](https://grafana.com/docs/grafana/latest/dashboards/json-model/)

## Next Steps

1. **Implement in A1** (Storage) — Start here for foundation metrics
2. **Roll out A2-A8** — Each crate owner implements following this template
3. **Test locally** — Verify metrics appear in Prometheus UI (port 9090)
4. **Create dashboards** — Use metrics to build per-crate visualizations
5. **Define SLOs** — Set thresholds for alert rules

---

**Questions?** Reach out to A11 Infrastructure team
**Status:** Template ready, awaiting per-crate implementation
