# A11 Phase 4 Block 2: Prometheus Metrics Integration (All Crates)

## Objective
Integrate Prometheus metrics collection from all 8 crates (A1-A8) into centralized monitoring infrastructure. All crates should export metrics on standard `/metrics` endpoint with proper naming, labels, and SLA alert thresholds.

## Current State
- A8 (Management) crate has metrics.rs with Gauge, Counter, Histogram types
- Web API has `/metrics` endpoint that calls `state.metrics.render_prometheus()`
- Other crates have minimal metrics infrastructure

## Deliverables

### 1. Per-Crate Metrics Export (All 8 Crates)

#### A1 Storage Engine (claudefs-storage)
Metrics to export:
- `storage_io_queue_depth` (gauge): Current queue length
- `storage_io_latency_ms` (histogram): I/O operation latency
- `storage_block_allocator_free_bytes` (gauge): Free space in allocator
- `storage_gc_activity` (counter): Number of GC runs
- `storage_nvme_throughput_mbps` (gauge): Current throughput
- `storage_write_amplification` (gauge): Write amplification ratio

Implementation: Create `src/metrics.rs` module with MetricsExporter struct
Export via: tokio task that updates shared Arc<Metrics>

#### A2 Metadata Service (claudefs-meta)
Metrics to export:
- `metadata_raft_commits` (counter): Total Raft commits
- `metadata_kv_ops` (counter): Total KV operations
- `metadata_shard_distribution` (gauge): Operations per shard
- `metadata_transaction_latency_ms` (histogram): Transaction latency
- `metadata_raft_leader_changes` (counter): Leadership elections
- `metadata_quorum_health` (gauge): 0=unhealthy, 1=healthy

Implementation: Instrument existing Raft code, KV store ops
Export via: Include in mgmt crate scraper

#### A3 Data Reduction (claudefs-reduce)
Metrics to export:
- `reduce_dedup_ratio` (gauge): Dedup compression ratio (0.0-1.0)
- `reduce_compression_ratio` (gauge): Compression ratio (0.0-1.0)
- `reduce_tiering_rate` (counter): Files moved to S3 per hour
- `reduce_similarity_detection_time_ms` (histogram): Similarity search latency
- `reduce_pipeline_stage_latency_ms` (histogram, label: stage): Per-stage latency

Implementation: Existing metrics module; add Prometheus export
Export via: Include in mgmt scraper

#### A4 Transport (claudefs-transport)
Metrics to export:
- `transport_rpc_latency_ms` (histogram, label: method): Per-method latency
- `transport_bandwidth_mbps` (gauge): Current bandwidth
- `transport_trace_aggregation_samples` (counter): Samples collected
- `transport_router_score` (gauge): Router health score (0-100)
- `transport_connection_pool_size` (gauge): Active connections
- `transport_rdma_vs_tcp_ratio` (gauge): Percentage using RDMA

Implementation: Instrument existing RPC code, trace_aggregator module
Export via: Include in mgmt scraper

#### A5 FUSE Client (claudefs-fuse)
Metrics to export:
- `fuse_operations_per_sec` (gauge): FUSE ops/sec
- `fuse_cache_hit_ratio` (gauge): Client cache hit % (0-100)
- `fuse_passthrough_mode_percentage` (gauge): % of ops in passthrough (0-100)
- `fuse_quota_usage_bytes` (gauge): Per-tenant usage
- `fuse_syscall_latency_ms` (histogram, label: syscall): Per-syscall latency

Implementation: Instrument existing FUSE handler code
Export via: Include in mgmt scraper

#### A6 Replication (claudefs-repl)
Metrics to export:
- `replication_journal_lag_ms` (gauge): Cross-site lag
- `replication_failover_count` (counter): Number of failovers
- `replication_cross_site_latency_ms` (histogram): Round-trip latency
- `replication_conflict_rate` (gauge): Conflicts per minute
- `replication_batch_size_bytes` (gauge): Current batch size

Implementation: Instrument journal tailer, gRPC conduit
Export via: Include in mgmt scraper

#### A7 Protocol Gateways (claudefs-gateway)
Metrics to export:
- `gateway_nfsv3_ops_per_sec` (gauge): NFSv3 ops/sec
- `gateway_pnfs_ops_per_sec` (gauge): pNFS ops/sec
- `gateway_protocol_distribution` (gauge, label: protocol): % per protocol
- `gateway_error_rate` (gauge): Errors per minute
- `gateway_smb_connection_count` (gauge): Active SMB connections

Implementation: Instrument protocol handler code
Export via: Include in mgmt scraper

#### A8 Management (claudefs-mgmt)
Metrics to export (existing):
- `mgmt_duckdb_query_latency_ms` (histogram): Query response time
- `mgmt_web_api_latency_ms` (histogram, label: endpoint): Per-endpoint latency
- `mgmt_admin_api_auth_failures` (counter): Failed authentications
- `mgmt_cluster_health_score` (gauge): Overall cluster health (0-100)

Implementation: Already in metrics.rs; enhance with more detail
Export via: Existing `/metrics` endpoint

### 2. Standardized Prometheus Export Format

All crates should output Prometheus text format:

```
# HELP storage_io_latency_ms I/O operation latency in milliseconds
# TYPE storage_io_latency_ms histogram
storage_io_latency_ms_bucket{le="100"} 150
storage_io_latency_ms_bucket{le="500"} 1245
storage_io_latency_ms_bucket{le="+Inf"} 1500
storage_io_latency_ms_sum 450000
storage_io_latency_ms_count 1500

# HELP storage_block_allocator_free_bytes Free space in block allocator
# TYPE storage_block_allocator_free_bytes gauge
storage_block_allocator_free_bytes 15728640000
```

Implementation: Implement `render_prometheus() -> String` on metrics struct

### 3. Prometheus Scrape Configuration

Create `monitoring/prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: "claudefs-storage"
    static_configs:
      - targets: ["storage-nodes:9801"]
  
  - job_name: "claudefs-metadata"
    static_configs:
      - targets: ["metadata-nodes:9802"]
  
  - job_name: "claudefs-management"
    static_configs:
      - targets: ["management:9800"]
    # Other crates export via management aggregator

alerting:
  alertmanagers:
    - static_configs:
        - targets: ["alertmanager:9093"]

rule_files:
  - "alert-rules.yml"
```

### 4. Grafana Dashboards

Create `monitoring/grafana/` with JSON dashboards:

#### cluster-health.json
- Cluster health score (0-100)
- Node count (storage/metadata/client)
- Storage capacity (used/free)
- Network latency heatmap

#### performance.json
- p50/p95/p99 latencies (FUSE, NFS, RPC)
- Throughput (FUSE ops/sec, bandwidth)
- Cache hit ratios

#### data-reduction.json
- Dedup ratio (%)
- Compression ratio (%)
- Tiering activity (files/hour)
- Reduction pipeline stages (latency breakdown)

#### replication.json
- Cross-site lag (ms)
- Failover count (total)
- Conflict rate (per min)
- Site A vs Site B node health

#### cost-tracking.json
- EC2 hourly cost
- Storage cost (GB-hours)
- Data transfer cost
- Spot vs on-demand comparison

### 5. Alert Rules

Create `monitoring/alert-rules.yml`:

```yaml
groups:
  - name: storage_alerts
    rules:
      - alert: HighIOLatency
        expr: storage_io_latency_ms_bucket{le="1000"} / storage_io_latency_ms_count > 0.5
        for: 5m
        annotations:
          summary: "{{ $value }}% of I/O > 1s"
      
      - alert: LowAllocatorFree
        expr: storage_block_allocator_free_bytes / 17179869184 < 0.1
        for: 10m
        annotations:
          summary: "Block allocator < 10% free"
      
  - name: metadata_alerts
    rules:
      - alert: RaftQuorumUnhealthy
        expr: metadata_quorum_health == 0
        for: 1m
        annotations:
          summary: "Raft quorum lost"
      
  - name: replication_alerts
    rules:
      - alert: HighCrossSiteLag
        expr: replication_journal_lag_ms > 5000
        for: 5m
        annotations:
          summary: "Cross-site lag > 5s"
      
  - name: gateway_alerts
    rules:
      - alert: HighGatewayErrorRate
        expr: rate(gateway_error_rate[5m]) > 0.01
        for: 5m
        annotations:
          summary: "Gateway error rate > 1%"
```

### 6. Metrics Collection & Aggregation

Create central metrics collector in A8 that:
1. Scrapes `/metrics` from each crate daemon (or in-process collection)
2. Aggregates metrics for dashboard queries
3. Stores time-series in optional external Prometheus instance
4. Maintains metrics in-memory with retention window

Implementation: Enhance `metrics_collector.rs` to aggregate across crates

### 7. Integration Points

Per-crate changes needed:

**A1 Storage:** 
- Add metrics module with tokio task updating shared metrics
- Export via callback/channel to mgmt

**A2 Metadata:** 
- Instrument Raft consensus, KV store operations
- Export via scraper

**A3 Reduce:** 
- Use existing metrics infrastructure
- Add prometheus export

**A4 Transport:** 
- Instrument trace_aggregator, bandwidth_shaper, adaptive_router
- Export via scraper

**A5 FUSE:** 
- Instrument syscall handler
- Export via scraper

**A6 Replication:** 
- Instrument journal tailer, gRPC conduit
- Export via scraper

**A7 Gateway:** 
- Instrument protocol handlers
- Export via scraper

**A8 Management:** 
- Enhance existing metrics
- Implement aggregator and Prometheus endpoint

## Success Criteria
1. ✅ All 8 crates export metrics on `/metrics` endpoint
2. ✅ Prometheus scrape config successfully collects from all sources
3. ✅ Grafana dashboards display real-time metrics
4. ✅ Alert rules trigger appropriately on test data
5. ✅ Metrics format follows Prometheus best practices
6. ✅ Per-crate metrics naming is consistent and documented
7. ✅ Dashboard queries return non-zero values on test cluster

## Implementation Strategy
- Start with A8 (already has infrastructure)
- Use Arc<Metrics> pattern for thread-safe metrics
- Add tokio tasks for metric updates
- Aggregate in mgmt scraper module
- Test with `curl http://localhost:9800/metrics`

## Delivery Timeline
- Days 1-2: A1, A2, A3 metrics integration
- Days 3-4: A4, A5, A6, A7 metrics integration
- Days 5: Grafana dashboards + alert rules testing
- Day 6: Documentation + full cluster validation

## Testing
1. Local: Spin up single-node cluster, verify `/metrics` endpoint
2. Multi-node: Deploy 5-node cluster, verify all crates exporting
3. Dashboard: Query Grafana, verify data appears
4. Alerts: Trigger test scenarios (high CPU, full disk, etc.), verify alerts fire

## Files to Create/Modify
- crates/claudefs-storage/src/metrics.rs (new)
- crates/claudefs-meta/src/metrics_integration.rs (new)
- crates/claudefs-transport/src/metrics_export.rs (new)
- crates/claudefs-fuse/src/metrics_tracking.rs (new)
- crates/claudefs-repl/src/metrics_hooks.rs (new)
- crates/claudefs-gateway/src/metrics_gateway.rs (new)
- crates/claudefs-mgmt/src/metrics_aggregator.rs (new)
- monitoring/prometheus.yml (new)
- monitoring/alert-rules.yml (new)
- monitoring/grafana/cluster-health.json (new)
- monitoring/grafana/performance.json (new)
- monitoring/grafana/data-reduction.json (new)
- monitoring/grafana/replication.json (new)
- monitoring/grafana/cost-tracking.json (new)

Reference: Phase 4 Block 2 deliverables from docs/A11-PHASE4-PLAN.md
