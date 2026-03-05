# ClaudeFS Observability & Monitoring

**Status:** Phase 3 Implementation (2026-03-05)
**Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-05

---

## Overview

ClaudeFS implements comprehensive observability through:
- **Metrics**: Prometheus metrics exported from all crates
- **Tracing**: Distributed tracing via OpenTelemetry
- **Logging**: Structured JSON logging with levels
- **Dashboards**: Grafana dashboards for real-time monitoring
- **Alerting**: Alert rules for SLOs and anomalies

## Architecture

```
┌─────────────────────────────────────────────────────┐
│ ClaudeFS Components (A1-A8)                         │
│ ├─ Metrics: prometheus client                        │
│ ├─ Traces: opentelemetry SDK                        │
│ └─ Logs: structured JSON (tracing-subscriber)       │
└──────────────┬──────────────────────────────────────┘
               │
        ┌──────┴───────┬──────────────┐
        │              │              │
   ┌────▼────┐   ┌────▼────┐   ┌────▼────┐
   │Prometheus│  │Jaeger   │   │Loki     │
   │exporter  │  │exporter │   │exporter │
   └────┬─────┘  └────┬────┘   └────┬────┘
        │             │             │
        └─────┬───────┴─────┬───────┘
              │             │
         ┌────▼───┐    ┌────▼───┐
         │Prometheus│  │Loki    │
         │server    │  │server  │
         └────┬─────┘  └────┬───┘
              │             │
         ┌────▼─────────────▼────┐
         │ Grafana Dashboard     │
         │ (alerts, insights)    │
         └───────────────────────┘
```

## Metrics Collection

### Per-Crate Metrics

Each crate exports metrics relevant to its subsystem:

#### A1: Storage (claudefs-storage)
- `io_depth_limiter_depth_current` — Current I/O queue depth
- `io_depth_limiter_depth_min` — Minimum configured depth
- `io_depth_limiter_mode` — Mode (Healthy=0, Degraded=1, Critical=2)
- `nvme_command_latency_us` — Histogram of command latencies
- `nvme_commands_total` — Total commands executed
- `nvme_errors_total` — Total errors by error_type

#### A2: Metadata (claudefs-meta)
- `raft_log_entries_total` — Log entries replicated
- `raft_state_machine_apply_duration_us` — Apply latency
- `kvstore_get_duration_us` — KV store read latency
- `kvstore_put_duration_us` — KV store write latency
- `shard_connections_total` — Active client connections

#### A3: Data Reduction (claudefs-reduce)
- `dedup_chunks_processed_total` — Chunks through dedup pipeline
- `dedup_hit_ratio` — Percentage of matched chunks
- `compress_ratio` — Compression ratio achieved
- `crypto_operations_total` — Encryption/decryption ops

#### A4: Transport (claudefs-transport)
- `rpc_latency_us` — RPC request latency histogram
- `connection_pool_size` — Active connections in pool
- `rdma_throughput_mbps` — RDMA data throughput
- `bandwidth_shaper_rate_limit_mbps` — Configured rate limits

#### A5: FUSE (claudefs-fuse)
- `fuse_operations_total` — Total FUSE operations by op_type
- `fuse_operation_latency_us` — Latency histogram by operation
- `fuse_readahead_hits` — Cache hits for readahead
- `fuse_cache_memory_bytes` — Memory used by FUSE cache

#### A6: Replication (claudefs-repl)
- `replication_queue_depth` — Pending replication entries
- `replication_lag_seconds` — Replication delay estimate
- `replication_errors_total` — Errors by error_type

#### A7: Gateway (claudefs-gateway)
- `nfs_operations_total` — NFS operations by op_type
- `s3_api_calls_total` — S3 API calls by method
- `gateway_connection_pool_size` — Active gateway connections

#### A8: Management (claudefs-mgmt)
- `prometheus_scrape_duration_us` — Prometheus scrape time
- `event_pipeline_latency_us` — Event processing latency
- `event_queue_depth` — Pending events in queue

### Common Metrics (All Crates)

```rust
// Module lifecycle
module_initialized_timestamp_seconds
module_errors_total{error_type,crate_name}

// Memory usage
module_memory_allocated_bytes{crate_name}
module_memory_resident_bytes{crate_name}

// Thread/task counts
module_spawned_tasks_total{crate_name}
module_active_tasks{crate_name}

// Configuration
module_config_value{setting_name,crate_name}
```

## Distributed Tracing

### Trace Setup

Each request/operation is traced end-to-end:

```rust
use opentelemetry::{global, Context};
use opentelemetry::trace::{Span, Tracer};

#[tracing::instrument(skip(client))]
async fn handle_read(client: &Client, offset: u64, len: usize) -> Result<Vec<u8>> {
    // Spans are created automatically by #[instrument]
    // Parent-child relationship tracked via OpenTelemetry
    Ok(vec![])
}
```

### Trace Exporters

**Jaeger** for distributed tracing:
- Endpoint: `http://jaeger:14268/api/traces`
- Service name: `claudefs`
- Sampling: Probabilistic (0.1 = 10% of traces)

**Spans per operation:**
- `storage.read` → `nvme.command` → `device.wait`
- `metadata.get` → `raft.replicate` → `kvstore.lookup`
- `fuse.read` → `cache.lookup` → `storage.read`

## Logging

### Log Format

Structured JSON logging with levels:

```json
{
  "timestamp": "2026-03-05T10:15:30Z",
  "level": "INFO",
  "message": "I/O depth adjustment",
  "target": "claudefs_storage::io_depth_limiter",
  "mode": "Degraded",
  "previous_depth": 32,
  "new_depth": 16,
  "reason": "high_latency",
  "span": {
    "name": "check_and_adjust",
    "id": "0x123abc"
  }
}
```

### Log Levels

- `ERROR`: Errors requiring attention (storage failures, timeouts)
- `WARN`: Degraded operation (mode transitions, retries)
- `INFO`: Normal milestones (startup, shutdown, config changes)
- `DEBUG`: Detailed operation traces (useful for debugging)
- `TRACE`: Very detailed (spans, individual operations)

### Export

Logs are exported to **Loki**:
- Push via `loki_push_api` from agents
- Query via Grafana Loki datasource
- Retention: 30 days

## Dashboards

### Main Dashboard (`main-cluster-health.json`)

Displays:
- **Cluster Overview**: Node status, capacity, health
- **Performance**: Throughput, latency p50/p99/max
- **I/O Subsystem**: Queue depth, latency, errors
- **Metadata**: Transaction latency, replication lag
- **Reduction**: Dedup hit rate, compression ratio
- **Resource Usage**: CPU, memory, disk on each node

### Per-Service Dashboards

1. **Storage Dashboard** (`storage-performance.json`)
   - NVMe performance (IOPS, latency)
   - Queue depth mode transitions
   - Error rates and retry counts

2. **Metadata Dashboard** (`metadata-consensus.json`)
   - Raft consensus metrics
   - Write latency, throughput
   - Replication lag

3. **Transport Dashboard** (`transport-networking.json`)
   - RPC latency distribution
   - Bandwidth shaping (rate limits)
   - Connection pool utilization

4. **Cost Dashboard** (`cost-and-resources.json`)
   - Daily infrastructure costs
   - Spot instance pricing
   - EC2 on-demand vs spot ratio
   - Optimization recommendations

## Alerting

### Alert Rules

Configure in `/etc/prometheus/alerts.yml`:

```yaml
groups:
  - name: claudefs.rules
    rules:
      - alert: HighIOLatency
        expr: histogram_quantile(0.99, io_latency_us) > 10000
        for: 5m
        annotations:
          summary: "High I/O latency ({{ $value }}us)"

      - alert: ReplicationLag
        expr: replication_lag_seconds > 30
        for: 2m
        annotations:
          summary: "High replication lag: {{ $value }}s"

      - alert: DedupeHitRateLow
        expr: dedup_hit_ratio < 0.2
        for: 15m
        annotations:
          summary: "Low dedup efficiency: {{ $value }}%"
```

### SLOs

Define in `monitoring/slos.yaml`:

```yaml
slos:
  - name: read_latency_p99
    target: 0.99
    threshold_ms: 50
    window: 5m
    alert: ReadLatencyP99

  - name: replication_consistency
    target: 0.999
    threshold_lag_seconds: 10
    window: 1h
    alert: ReplicationConsistency
```

## Running Locally

### Prerequisites

```bash
# Install stack via Docker Compose
docker-compose -f monitoring/docker-compose.yml up -d

# Services available at:
# - Prometheus:  http://localhost:9090
# - Grafana:     http://localhost:3000
# - Jaeger:      http://localhost:16686
# - Loki:        http://localhost:3100
```

### ClaudeFS Configuration

Enable metrics export in `config.yaml`:

```yaml
observability:
  metrics:
    enabled: true
    prometheus_addr: "127.0.0.1:9090"
    scrape_interval_seconds: 15

  tracing:
    enabled: true
    jaeger_endpoint: "http://127.0.0.1:14268/api/traces"
    sampling_rate: 0.1

  logging:
    level: "info"
    format: "json"
    export_to_loki: true
    loki_endpoint: "http://127.0.0.1:3100"
```

Start ClaudeFS and metrics will flow to Prometheus:

```bash
./target/release/cfs daemon --config config.yaml
```

## Querying Metrics

### Prometheus Query Examples

```promql
# Current read latency (p99)
histogram_quantile(0.99, rate(storage_read_latency_us[5m]))

# 24h average throughput
rate(storage_bytes_read_total[24h])

# Storage capacity remaining
(storage_capacity_bytes - storage_used_bytes) / storage_capacity_bytes * 100

# Replication lag across all nodes
max(replication_lag_seconds)

# Cost estimate (hourly spend * 24)
(aws_ec2_hourly_cost_usd + aws_bedrock_hourly_cost_usd) * 24
```

### Grafana Dashboard Building

1. Navigate to `http://localhost:3000`
2. Create new dashboard
3. Add Prometheus datasource
4. Create panels with queries above
5. Set thresholds and alerts

## Performance Baseline

Expected metrics in healthy cluster:

| Metric | Target | Warning | Critical |
|--------|--------|---------|----------|
| Read latency (p99) | <50ms | 100ms | 200ms |
| Write latency (p99) | <100ms | 200ms | 500ms |
| Replication lag | <5s | 30s | 60s |
| Dedup hit ratio | >50% | 20% | <5% |
| CPU utilization | <50% | 75% | 90% |
| Memory utilization | <60% | 80% | 95% |
| Disk utilization | <70% | 85% | 95% |

## Troubleshooting

### Prometheus not scraping metrics

Check:
```bash
curl http://localhost:9090/api/v1/targets
# Should show all crate endpoints as "UP"
```

### High I/O latency

1. Check `io_depth_limiter_mode` — if "Critical", NVMe is degraded
2. Check `nvme_errors_total` — verify no hardware errors
3. Check `compression_ratio` — if <50%, reduce compression
4. Check `replication_lag_seconds` — verify network health

### Low dedup efficiency

1. Check workload characteristics (variance in data)
2. Increase `chunk_size` if too aggressive
3. Check `similarity_tier_hit_ratio` — if low, may need tuning
4. Verify enough memory allocated for dedup index

## Integration with CI/CD

Metrics are automatically published to GitHub:
- Nightly benchmark workflow uploads to artifacts
- Cost monitoring workflow stores daily spend
- Test results include performance metrics
- Release artifacts include pre-built Grafana dashboards

See `.github/workflows/benchmark.yml` and `cost-monitoring.yml`.

## Future Enhancements

- [ ] OpenTelemetry Collector for centralized gathering
- [ ] Custom metrics for application-level semantics
- [ ] Anomaly detection via ML (isolation forests)
- [ ] eBPF-based kernel-level tracing
- [ ] Integration with Datadog/New Relic for production deployments
- [ ] Custom cardinality limits to prevent OOM

---

**Next Steps:**
1. Deploy monitoring stack locally
2. Configure Grafana dashboards
3. Set alerting thresholds
4. Document operational runbooks
