# A11 Phase 4 Block 2: Create Missing Grafana Dashboards

## Objective
Create 4 additional Grafana dashboards for Phase 4 Block 2 metrics integration to complete the monitoring coverage across all 8 ClaudeFS crates.

## Current Status
✅ Existing dashboards (already created):
- 01-cluster-health.json: Cluster node status, uptime
- 02-storage-performance.json: I/O latency, throughput, queue depth
- 03-metadata-consensus.json: Raft commits, leader changes, quorum health
- 04-cost-tracking.json: AWS costs, spot vs on-demand, resource allocation

❌ Missing dashboards (to create):
- 05-data-reduction.json: Dedup ratio, compression ratio, tiering activity, pipeline latency
- 06-replication.json: Cross-site lag, failovers, conflict rate, batch size
- 07-transport.json: RPC latency, bandwidth utilization, connection pool, RDMA ratio
- 08-fuse-gateway.json: FUSE ops/sec, cache hit ratio, passthrough %, gateway error rate, NFS/SMB connections

## Dashboard Requirements

### Dashboard 5: Data Reduction (05-data-reduction.json)
**Purpose:** Monitor dedup, compression, tiering pipeline performance
**Panels:**
1. **Dedup Ratio Gauge** (top-left)
   - Metric: `reduce_dedup_ratio` (0.0-1.0, show as %)
   - Thresholds: Green 0.5+, Yellow 0.3-0.5, Red <0.3
   - Unit: percent

2. **Compression Ratio Gauge** (top-right)
   - Metric: `reduce_compression_ratio` (0.0-1.0)
   - Thresholds: Green 0.4+, Yellow 0.2-0.4, Red <0.2
   - Unit: percent

3. **Tiering Activity Graph** (middle-left)
   - Metric: `rate(reduce_tiering_rate[5m])` (files/sec moved to S3)
   - Line graph, 6-hour time range
   - Y-axis: files per minute

4. **Pipeline Stage Latencies** (middle-right)
   - Metrics: `reduce_pipeline_stage_latency_ms{stage=~"dedup|compress|encrypt|store"}`
   - Stacked bar chart showing latency per stage
   - Labels: dedup, compress, encrypt, store

5. **Reduction Efficiency Over Time** (bottom)
   - Metric: `reduce_dedup_ratio` + `reduce_compression_ratio` (combined)
   - Line graph, 24-hour range
   - Show legend with both lines

### Dashboard 6: Replication (06-replication.json)
**Purpose:** Monitor cross-site replication health
**Panels:**
1. **Cross-Site Lag Gauge** (top-left)
   - Metric: `replication_journal_lag_ms`
   - Thresholds: Green <1000ms, Yellow 1000-5000ms, Red >5000ms
   - Unit: milliseconds

2. **Failover Count Counter** (top-middle)
   - Metric: `replication_failover_count` (total)
   - Large stat display
   - Unit: count

3. **Site Health Status** (top-right)
   - Metrics: `up{job="claudefs-repl"}` for each site
   - Status panels (green/red)
   - Show: Site A, Site B

4. **Cross-Site Latency Histogram** (middle)
   - Metric: `replication_cross_site_latency_ms`
   - P50, P95, P99 percentiles displayed
   - Sparkline trends

5. **Conflict Rate** (bottom-left)
   - Metric: `replication_conflict_rate` (conflicts per minute)
   - Single-value stat
   - Color threshold: Green <1, Yellow 1-5, Red >5

6. **Journal Replication Lag Trend** (bottom-right)
   - Metric: `replication_journal_lag_ms`
   - Line graph, 24-hour view
   - Alert line at 5000ms threshold

### Dashboard 7: Transport (07-transport.json)
**Purpose:** Monitor network and RPC performance
**Panels:**
1. **RPC Latency Gauge** (top-left)
   - Metric: `histogram_quantile(0.95, rate(transport_rpc_latency_ms_bucket[5m]))`
   - Display: p95 latency
   - Thresholds: Green <50ms, Yellow 50-100ms, Red >100ms
   - Unit: milliseconds

2. **Bandwidth Utilization** (top-right)
   - Metric: `transport_bandwidth_mbps`
   - Gauge: 0-1000 Mbps scale
   - Thresholds: Green <500Mbps, Yellow 500-800Mbps, Red >800Mbps
   - Unit: Mbps

3. **Active Connections Pool** (middle-left)
   - Metric: `transport_connection_pool_size`
   - Stat display, show max capacity vs current
   - Color if >80% utilized

4. **RPC Method Latency Breakdown** (middle-right)
   - Metrics: `transport_rpc_latency_ms{method=~"read|write|metadata"}`
   - Heatmap or stacked area chart
   - Compare: read, write, metadata operations
   - Time range: 6 hours

5. **RDMA vs TCP Usage** (middle-bottom)
   - Metric: `transport_rdma_vs_tcp_ratio`
   - Pie chart: % RDMA vs % TCP
   - Show absolute connection counts

6. **Transport Error Rate** (bottom)
   - Metrics: `rate(transport_errors[5m])`
   - Line graph with alert threshold at 1% errors
   - Show total errors and errors by type (timeout, connection, throttle)

### Dashboard 8: FUSE & Gateway (08-fuse-gateway.json)
**Purpose:** Monitor client FUSE and protocol gateway performance
**Panels:**
1. **FUSE Operations/sec** (top-left)
   - Metric: `fuse_operations_per_sec`
   - Gauge: 0-100k ops/sec scale
   - Sparkline trend
   - Unit: ops/sec

2. **FUSE Cache Hit Ratio** (top-middle)
   - Metric: `fuse_cache_hit_ratio`
   - Gauge: 0-100% scale
   - Thresholds: Green >80%, Yellow 50-80%, Red <50%
   - Unit: percent

3. **FUSE Passthrough Mode %** (top-right)
   - Metric: `fuse_passthrough_mode_percentage`
   - Gauge: 0-100%
   - Show efficiency gain (passthrough is faster)

4. **Gateway Protocol Distribution** (middle-left)
   - Metrics: `gateway_nfsv3_ops_per_sec`, `gateway_pnfs_ops_per_sec`, `gateway_smb_connection_count`
   - Pie chart or bar chart showing distribution
   - Labels: NFSv3, pNFS, SMB

5. **Gateway Error Rate** (middle-right)
   - Metric: `rate(gateway_error_rate[5m])`
   - Line graph, 24-hour view
   - Alert threshold at 1% errors (red line)
   - Show error types if available

6. **FUSE Syscall Latency** (middle-bottom)
   - Metrics: `fuse_syscall_latency_ms{syscall=~"read|write|open|close|lookup"}`
   - Stacked bar showing latency per syscall type
   - Compare read, write, metadata operations

7. **Quota Usage by Tenant** (bottom-left)
   - Metric: `fuse_quota_usage_bytes` (if labeled by tenant)
   - Stacked area chart or gauge for top N tenants
   - Show used vs limit

8. **Active SMB Connections** (bottom-right)
   - Metric: `gateway_smb_connection_count`
   - Stat display
   - Sparkline trend over 6 hours

## Technical Requirements

### File Locations
- Dashboard files: `monitoring/grafana/provisioning/dashboards/json/`
- Each JSON file follows Grafana 8.0+ format
- Datasource: "Prometheus" (must match provisioning config)

### Common Specifications
- Time range: Default 6 hours (configurable by user)
- Refresh: 30 seconds
- Rows: Approximately 3-4 rows per dashboard
- Width: Panels span 12 units (half-width = 6, full-width = 12, third = 4)

### Error Handling
- Show "No data" gracefully if metric not available
- Don't break dashboard if metric missing
- Use sensible defaults for gauges/thresholds

### Panel Defaults
```json
{
  "datasource": "Prometheus",
  "targets": [
    {
      "expr": "METRIC_NAME",
      "refId": "A"
    }
  ],
  "fieldConfig": {
    "defaults": {
      "color": { "mode": "palette-classic" },
      "unit": "short"
    }
  }
}
```

## Success Criteria
1. All 4 dashboards JSON files created in correct location
2. Each dashboard loads without errors in Grafana 8.0+
3. All panels have valid Prometheus queries
4. Proper color themes and thresholds applied
5. Panels readable and well-organized
6. Titles and help text included

## References
- Existing dashboards: monitoring/grafana/provisioning/dashboards/json/
- Prometheus metrics: Documented in metrics.rs files across all 8 crates
- Phase 4 Block 2 spec: a11-phase4-block2-input.md
