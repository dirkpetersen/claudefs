# ClaudeFS Monitoring Stack

Complete observability solution for ClaudeFS using Docker Compose. Includes metrics (Prometheus), dashboards (Grafana), distributed tracing (Jaeger), log aggregation (Loki), and alerting (AlertManager).

## 🚀 Quick Start (1 minute)

```bash
cd /home/cfs/claudefs/monitoring
docker-compose up -d

# Verify all services running
docker ps | grep -E "prometheus|grafana|jaeger|loki|alertmanager"
```

## 📊 Services

| Service | Port | URL | Purpose |
|---------|------|-----|---------|
| **Prometheus** | 9090 | http://localhost:9090 | Time series database & metrics scraper |
| **Grafana** | 3000 | http://localhost:3000 | Dashboards & alerts (login: admin/admin) |
| **Jaeger** | 16686 | http://localhost:16686 | Distributed tracing & span visualization |
| **Loki** | 3100 | http://localhost:3100 | Log aggregation & querying |
| **AlertManager** | 9093 | http://localhost:9093 | Alert routing to Slack/PagerDuty/email |

## 🔧 Configuration Files

| File | Purpose |
|------|---------|
| `prometheus.yml` | Scrape targets, retention, alert rules |
| `alerts.yml` | 25+ alert rules (performance, cost, health) |
| `alertmanager.yml` | Notification routing configuration |
| `docker-compose.yml` | Container service definitions |
| `loki-config.yml` | Log ingestion and retention settings |
| `promtail-config.yml` | Log shipping from system logs |
| `grafana/provisioning/` | Datasources & pre-built dashboards (auto-loaded) |

## 📈 Pre-Built Dashboards

**All dashboards auto-load at http://localhost:3000:**

1. **Cluster Health** — System overview
   - Node status (green/yellow/red)
   - CPU, memory, disk gauges
   - Operations/sec & latency (p50/p99)

2. **Storage Performance** — A1 metrics
   - NVMe IOPS and throughput
   - I/O queue depth trends
   - Read/write latency percentiles
   - Error rates

3. **Metadata & Consensus** — A2 metrics
   - Raft leader election status
   - Operation latency & throughput
   - Replication lag per shard
   - Uncommitted log entries

4. **Cost Tracking** — AWS/infrastructure metrics
   - Daily spend & projected monthly cost
   - Active instance count
   - Cost breakdown by service (EC2, Bedrock, Storage)
   - Spot vs on-demand comparison

## 🔗 Metrics Collection Architecture

```
ClaudeFS Crates (A1-A8)
        ↓
Prometheus Client Libraries
        ↓
/metrics HTTP Endpoints (ports 9001-9008)
        ↓
Prometheus Scraper (every 15 seconds)
        ↓
Time Series Database
        ↓
Grafana Queries ← AlertManager Rules ← Query Engine
```

### Exporter Ports

Each crate exposes metrics on:

```
A1 (Storage)        → 9001
A2 (Metadata)       → 9002
A3 (Reduction)      → 9003
A4 (Transport)      → 9004
A5 (FUSE)           → 9005
A6 (Replication)    → 9006
A7 (Gateway)        → 9007
A8 (Management)     → 9008
```

### Test a Metrics Endpoint

```bash
# Start a crate with metrics (e.g., A1 on port 9001)
cargo run --bin claudefs-server

# In another terminal, query metrics
curl http://127.0.0.1:9001/metrics | head -20

# Expected output (Prometheus text format):
# # HELP storage_operations_total Total storage operations
# # TYPE storage_operations_total counter
# storage_operations_total 1234
# storage_latency_ms_bucket{le="1.0"} 100
# ...
```

## 🚨 Alert Examples

**Pre-configured alerting rules (25+):**

- **HighLatency** — p99 latency exceeds 100ms
- **HighErrorRate** — Error rate > 1%
- **HighCPU** — CPU usage > 80%
- **HighMemory** — Memory usage > 90%
- **DiskSpaceLow** — Available space < 10%
- **RaftReplicationLag** — Log entries behind > 5000
- **LeaderElection** — No Raft leader elected
- **DailySpendHigh** — AWS spend > $100/day

**Test alert locally:**

```bash
# 1. Trigger high CPU (60 sec)
stress-ng --cpu 8 --timeout 60s &

# 2. Check Prometheus alerts
curl http://localhost:9090/alerts | grep -i CPU

# 3. Check AlertManager
curl http://localhost:9093/api/v1/alerts
```

## 📝 Crate Integration (A1-A8)

Each crate must export metrics to be monitored. See [METRICS-INTEGRATION-GUIDE.md](../docs/METRICS-INTEGRATION-GUIDE.md) for detailed instructions.

**Quick summary:**

1. Add `prometheus = "0.13"` to `Cargo.toml`
2. Create `src/metrics.rs` with metric definitions
3. Spawn `/metrics` endpoint (Tokio/Axum)
4. Call metric operations in hot paths
5. Verify: `curl http://127.0.0.1:900X/metrics`

Example:

```rust
use crate::metrics::*;

pub fn read_block(id: u64) -> Result<Vec<u8>> {
    let start = Instant::now();
    STORAGE_OPERATIONS_TOTAL.inc();

    let data = do_read(id)?;

    STORAGE_BYTES_READ_TOTAL.inc_by(data.len() as u64);
    STORAGE_LATENCY.observe(start.elapsed().as_millis() as f64);
    Ok(data)
}
```

## 🔍 Useful PromQL Queries

```promql
# Operations per second
rate(storage_operations_total[1m])

# p99 latency
histogram_quantile(0.99, rate(storage_latency_ms_bucket[5m]))

# Error rate percentage
(rate(storage_errors_total[1m]) / rate(storage_operations_total[1m])) * 100

# I/O queue depth
storage_io_depth_current

# Metadata operations by type
rate(metadata_operations_total{operation=~"create|delete"}[1m])

# Raft replication lag
raft_replication_lag{job="metadata"}

# AWS daily cost
aws_daily_cost_usd

# Uptime
time() - process_start_time_seconds
```

## 🧪 Troubleshooting

### Metrics Not Appearing

```bash
# 1. Is crate running and exporting?
curl http://127.0.0.1:9001/metrics

# 2. Prometheus scrape config correct?
curl http://localhost:9090/api/v1/targets

# 3. Prometheus logs
docker logs prometheus
```

### Grafana Dashboards Empty

```bash
# 1. Prometheus datasource working?
curl http://localhost:9090/api/v1/query?query=up

# 2. Restart Grafana to reload provisioning
docker restart grafana

# 3. Check logs
docker logs grafana
```

### High Memory (Prometheus)

```bash
# Check cardinality (number of unique metrics)
curl 'http://localhost:9090/api/v1/cardinality' | jq .

# Reduce retention if needed
docker exec prometheus prometheus --storage.tsdb.retention.time=3d
```

## 🛠️ Custom Dashboards

Create custom dashboards in Grafana:

```
1. Go to http://localhost:3000
2. Click "+" → Dashboard
3. Add Panel → Prometheus datasource
4. Enter PromQL query
5. Save → Export JSON
6. Commit to version control
```

## 🧹 Cleanup

```bash
# Stop all services
docker-compose down

# Remove volumes (deletes all metrics history)
docker-compose down -v

# Prune Docker system
docker system prune -a --volumes
```

## 📚 Documentation

- [OBSERVABILITY.md](../docs/OBSERVABILITY.md) — Complete architecture & setup
- [HEALTH-MONITORING.md](../docs/HEALTH-MONITORING.md) — Health monitoring system design
- [BUILD-METRICS.md](../docs/BUILD-METRICS.md) — Build performance metrics
- [METRICS-INTEGRATION-GUIDE.md](../docs/METRICS-INTEGRATION-GUIDE.md) — Per-crate integration guide

## 🆘 Need Help?

1. Check logs: `docker logs <service>`
2. Verify connectivity: `curl <service:port>`
3. See integration guide: METRICS-INTEGRATION-GUIDE.md
4. Contact: A11 Infrastructure & CI

---

**Agent:** A11 Infrastructure & CI
**Version:** 2.0
**Last Updated:** 2026-03-06
**Status:** Production Ready
