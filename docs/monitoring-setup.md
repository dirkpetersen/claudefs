# ClaudeFS Monitoring & Observability Setup

Complete guide for setting up monitoring, logging, and distributed tracing for ClaudeFS clusters.

## Architecture Overview

ClaudeFS monitoring uses a three-layer approach:

1. **Metrics (Real-time)** — Prometheus for operational telemetry
2. **Logs (Streaming)** — Structured logging via `tracing` crate
3. **Traces (Distributed)** — W3C Trace Context for request tracing

## Metrics Collection (Prometheus)

### Prometheus Architecture

```
┌─────────────────────────────────────────┐
│ ClaudeFS Storage Nodes                  │
│ ├─ Node 1 (:9800/metrics)              │
│ ├─ Node 2 (:9800/metrics)              │
│ └─ Node 3 (:9800/metrics)              │
└─────────────────────────────────────────┘
         ↓ (scrape every 15s)
┌─────────────────────────────────────────┐
│ Prometheus Server (on orchestrator)     │
│ ├─ Data retention: 15 days              │
│ ├─ Storage: ~50GB                       │
│ └─ Alerts: AlertManager rules           │
└─────────────────────────────────────────┘
         ↓ (query API)
┌─────────────────────────────────────────┐
│ Grafana Dashboards                      │
│ ├─ Cluster Health                       │
│ ├─ Per-Node Performance                 │
│ ├─ I/O Metrics                          │
│ └─ Data Reduction Pipeline              │
└─────────────────────────────────────────┘
```

### Metrics Exported by ClaudeFS

#### Storage Node Metrics

| Metric | Type | Labels | Notes |
|--------|------|--------|-------|
| `claudefs_io_ops_total` | Counter | `node`, `device`, `op` (read/write) | Total I/O operations |
| `claudefs_io_bytes_total` | Counter | `node`, `device`, `op` | Total I/O bytes |
| `claudefs_io_latency_us` | Histogram | `node`, `device`, `op` | I/O latency in microseconds (p50/p99/p999) |
| `claudefs_raft_commit_latency_us` | Histogram | `node`, `shard` | Raft commit latency |
| `claudefs_raft_log_size_bytes` | Gauge | `node`, `shard` | Raft log size |
| `claudefs_metadata_ops_total` | Counter | `node`, `op` (create/delete/rename) | Metadata operations |
| `claudefs_dedupe_hit_ratio` | Gauge | `node` | Deduplication hit ratio (0-1) |
| `claudefs_compression_ratio` | Gauge | `node` | Compression ratio (size_before/size_after) |
| `claudefs_s3_queue_depth` | Gauge | `node` | S3 flush queue depth |
| `claudefs_s3_latency_us` | Histogram | `node` | S3 upload latency |
| `claudefs_replication_lag_us` | Gauge | `node`, `site` | Cross-site replication lag |
| `claudefs_flash_usage_ratio` | Gauge | `node` | Flash usage (0-1) |

#### Transport Metrics

| Metric | Type | Labels | Notes |
|--------|------|--------|-------|
| `claudefs_rpc_calls_total` | Counter | `node`, `method`, `status` | RPC call count |
| `claudefs_rpc_latency_us` | Histogram | `node`, `method` | RPC latency |
| `claudefs_connection_pool_size` | Gauge | `node`, `peer` | Active connections per peer |
| `claudefs_tls_connections_total` | Counter | `node`, `type` (mTLS/TLS) | TLS connection count |
| `claudefs_qos_permits_available` | Gauge | `node`, `class` | QoS permits available per workload class |

#### NVMe Health Metrics

| Metric | Type | Labels | Notes |
|--------|------|--------|-------|
| `claudefs_nvme_endurance_percent` | Gauge | `node`, `device` | Remaining endurance (0-100%) |
| `claudefs_nvme_media_errors_total` | Counter | `node`, `device` | NVMe media errors |
| `claudefs_nvme_temperature_celsius` | Gauge | `node`, `device` | NVMe temperature |
| `claudefs_nvme_spare_capacity_percent` | Gauge | `node`, `device` | Spare capacity remaining |

### Setting Up Prometheus

#### 1. Install Prometheus on Orchestrator

```bash
# SSH to orchestrator
cfs-dev ssh

# Download Prometheus (latest LTS)
cd /opt
sudo wget https://github.com/prometheus/prometheus/releases/download/v2.50.0/prometheus-2.50.0.linux-amd64.tar.gz
sudo tar xzf prometheus-2.50.0.linux-amd64.tar.gz
sudo ln -s prometheus-2.50.0.linux-amd64 prometheus
cd prometheus
```

#### 2. Create Prometheus Configuration

Create `/opt/prometheus/prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s
  external_labels:
    cluster: claudefs-phase2
    environment: dev

# AlertManager configuration
alerting:
  alertmanagers:
    - static_configs:
        - targets:
            - localhost:9093

rule_files:
  - "rules/*.yml"

scrape_configs:
  # Storage nodes
  - job_name: "claudefs-storage"
    static_configs:
      - targets:
          - "storage-a-1.internal:9800"
          - "storage-a-2.internal:9800"
          - "storage-a-3.internal:9800"
          - "storage-b-1.internal:9800"
          - "storage-b-2.internal:9800"
    relabel_configs:
      - source_labels: [__address__]
        regex: "([^:]+):.*"
        target_label: instance

  # Jepsen metrics
  - job_name: "jepsen"
    static_configs:
      - targets:
          - "jepsen.internal:9800"

  # Prometheus self-monitoring
  - job_name: "prometheus"
    static_configs:
      - targets:
          - localhost:9090
```

#### 3. Create Alert Rules

Create `/opt/prometheus/rules/claudefs.yml`:

```yaml
groups:
  - name: claudefs-alerts
    interval: 1m
    rules:
      # Storage health
      - alert: NodeDown
        expr: up{job="claudefs-storage"} == 0
        for: 5m
        annotations:
          summary: "Node {{ $labels.instance }} is down"

      - alert: NVMeHealthDegraded
        expr: claudefs_nvme_endurance_percent < 10
        for: 10m
        annotations:
          summary: "NVMe {{ $labels.device }} endurance < 10% on {{ $labels.instance }}"

      - alert: HighMediaErrors
        expr: increase(claudefs_nvme_media_errors_total[1h]) > 10
        for: 5m
        annotations:
          summary: "High NVMe media errors on {{ $labels.instance }}"

      # Replication lag
      - alert: ReplicationLagHigh
        expr: claudefs_replication_lag_us > 100000  # > 100ms
        for: 5m
        annotations:
          summary: "Replication lag to {{ $labels.site }} is {{ $value }}us"

      # Storage capacity
      - alert: FlashNearFull
        expr: claudefs_flash_usage_ratio > 0.85
        for: 5m
        annotations:
          summary: "Flash is {{ $value | humanizePercentage }} full on {{ $labels.instance }}"

      - alert: FlashCritical
        expr: claudefs_flash_usage_ratio > 0.95
        for: 1m
        annotations:
          summary: "Flash is CRITICAL ({{ $value | humanizePercentage }} full) on {{ $labels.instance }}"

      # Metadata performance
      - alert: RaftCommitLatencyHigh
        expr: histogram_quantile(0.99, claudefs_raft_commit_latency_us) > 100000  # > 100ms
        for: 5m
        annotations:
          summary: "Raft commit latency p99 > 100ms on {{ $labels.node }}"

      # I/O performance degradation
      - alert: ReadLatencyHigh
        expr: histogram_quantile(0.99, claudefs_io_latency_us{op="read"}) > 10000  # > 10ms
        for: 10m
        annotations:
          summary: "Read latency p99 > 10ms on {{ $labels.device }}"

      # S3 tiering issues
      - alert: S3QueueBacklogged
        expr: claudefs_s3_queue_depth > 10000
        for: 5m
        annotations:
          summary: "S3 flush queue backlogged on {{ $labels.node }} ({{ $value }} objects)"
```

#### 4. Start Prometheus Service

```bash
# Create systemd service
sudo tee /etc/systemd/system/prometheus.service > /dev/null <<EOF
[Unit]
Description=Prometheus
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=prometheus
WorkingDirectory=/opt/prometheus
ExecStart=/opt/prometheus/prometheus --config.file=/opt/prometheus/prometheus.yml --storage.tsdb.path=/var/lib/prometheus
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable prometheus
sudo systemctl start prometheus

# Verify
curl http://localhost:9090/-/healthy
```

## Grafana Dashboards

### Install Grafana

```bash
# On orchestrator
sudo apt-get install -y grafana-server

# Enable and start
sudo systemctl enable grafana-server
sudo systemctl start grafana-server

# Access at http://orchestrator-ip:3000 (default: admin/admin)
```

### Create Dashboards

#### Cluster Health Dashboard

```json
{
  "dashboard": {
    "title": "ClaudeFS Cluster Health",
    "panels": [
      {
        "title": "Nodes Up",
        "targets": [
          {
            "expr": "count(up{job=\"claudefs-storage\"} == 1)"
          }
        ]
      },
      {
        "title": "Flash Usage",
        "targets": [
          {
            "expr": "claudefs_flash_usage_ratio * 100"
          }
        ],
        "alert": {
          "conditions": [
            {"evaluator": {"params": [85], "type": "gt"}}
          ]
        }
      },
      {
        "title": "Replication Lag",
        "targets": [
          {
            "expr": "claudefs_replication_lag_us / 1000"  // Convert to ms
          }
        ]
      },
      {
        "title": "IOPS (Last Hour)",
        "targets": [
          {
            "expr": "rate(claudefs_io_ops_total[1h])"
          }
        ]
      },
      {
        "title": "I/O Latency (p99)",
        "targets": [
          {
            "expr": "histogram_quantile(0.99, claudefs_io_latency_us) / 1000"
          }
        ]
      }
    ]
  }
}
```

### Important Dashboards to Create

1. **Cluster Overview** — nodes, capacity, replication lag
2. **I/O Performance** — throughput, latency, IOPS
3. **Metadata Operations** — Raft latency, operation counts
4. **Data Reduction** — dedupe ratio, compression ratio
5. **Hardware Health** — NVMe endurance, temperature, media errors
6. **S3 Tiering** — queue depth, upload latency
7. **Network** — RPC latency, connection pools, QoS metrics

## Structured Logging

### Tracing Configuration

ClaudeFS uses the `tracing` crate for structured logging. Enable in configuration:

```toml
[logging]
level = "info"  # debug, info, warn, error
format = "json"  # json or text
output = "stdout"  # stdout or file://path
distributed_tracing = true
```

### Distributed Tracing (W3C Trace Context)

Trace context is automatically propagated across RPC boundaries:

```rust
// In Rust code
use tracing::{info, instrument};

#[instrument(skip(data))]
async fn write_file(path: &str, data: &[u8]) -> Result<()> {
    info!("writing file", path = %path, size = data.len());
    // ... operation ...
    Ok(())
}
```

Trace context headers:
- `traceparent`: `00-<trace-id>-<span-id>-<flags>`
- `tracestate`: vendor-specific trace state

### Collecting Logs

```bash
# Stream logs from a node
ssh -i ~/.ssh/cfs-key.pem ec2-user@storage-a-1 \
  journalctl -u claudefs -f -n 100

# Export logs to centralized logging (e.g., Loki)
# Configure via CloudWatch Logs agent or Fluent Bit
```

## Cost-Saving Monitoring Tips

1. **Adjust scrape interval** — reduce from 15s to 60s for less critical clusters
2. **Retention policy** — keep 7 days instead of 15 for development
3. **Alert sampling** — don't alert on every small spike
4. **Metrics export** — only export metrics you're actively monitoring

## Troubleshooting Monitoring

### Prometheus Not Scraping Metrics

```bash
# Check Prometheus targets
curl http://localhost:9090/api/v1/targets | jq

# Check connectivity to storage nodes
ssh storage-a-1 curl http://localhost:9800/metrics | head -20

# Check Prometheus logs
journalctl -u prometheus -n 50
```

### Grafana Not Showing Data

```bash
# Verify Prometheus datasource
# In Grafana: Configuration > Data Sources > Prometheus
# Test connection and verify it shows "Datasource is working"

# Check Grafana logs
journalctl -u grafana-server -n 50

# Query Prometheus directly
curl 'http://localhost:9090/api/v1/query?query=up{job="claudefs-storage"}'
```

### High Memory Usage

```bash
# Check Prometheus memory
ps aux | grep prometheus

# Reduce retention
curl -X POST http://localhost:9090/-/reload

# Or in prometheus.yml
storage:
  tsdb:
    retention:
      time: 7d  # Reduce from 15d
```

## Next Steps

- See `docs/ci-cd.md` for CI/CD monitoring integration
- See `docs/management.md` for metadata search and analytics
- See `tools/cfs-dev logs` for agent-specific logging

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
