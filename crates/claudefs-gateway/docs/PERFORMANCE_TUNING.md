# A7 Gateway Performance Tuning Guide

This guide covers production tuning for the ClaudeFS Protocol Gateway.

## Connection Pool Configuration

The connection pool (`gateway_conn_pool.rs`) manages backend connections. Tune based on network capacity and workload.

### Configuration Parameters

```yaml
connection_pool:
  # Minimum connections per backend node
  # Keep >= 2 for resilience during node restarts
  min_per_node: 4
  
  # Maximum connections per backend node
  # Set based on concurrent client count
  # Rule of thumb: client_count / node_count * 2
  max_per_node: 32
  
  # Close connections idle longer than this (ms)
  # Balance: lower = less resource usage, higher = more reuse
  max_idle_ms: 300000  # 5 minutes
  
  # Connection timeout (ms)
  connect_timeout_ms: 5000
  
  # Health check interval (ms)
  health_check_interval_ms: 30000
```

### Tuning by Network Speed

| Network Speed | Recommended max_per_node |
|---------------|-------------------------|
| 1 Gbps        | 10                      |
| 10 Gbps       | 32                      |
| 25 Gbps       | 64                      |
| 100 Gbps RDMA | 128                     |

### Connection Pool Metrics

Monitor via Prometheus:
```promql
gateway_active_connections{protocol="nfs3"}
gateway_backend_errors_total
```

High `backend_errors_total` may indicate need for larger pool or circuit breaker tuning.

## Circuit Breaker Settings

The circuit breaker (`gateway_circuit_breaker.rs`) prevents cascading failures.

### Configuration Parameters

```yaml
circuit_breaker:
  # Open circuit after this many consecutive failures
  failure_threshold: 5
  
  # Close circuit after this many successes in half-open state
  success_threshold: 2
  
  # Time in open state before attempting recovery (ms)
  open_duration_ms: 30000
  
  # Operation timeout (ms)
  timeout_ms: 5000
```

### Tuning Guidelines

| Workload | failure_threshold | open_duration_ms | timeout_ms |
|----------|-------------------|------------------|------------|
| Latency-sensitive | 3 | 15000 | 2000 |
| Batch/throughput | 10 | 60000 | 10000 |
| Mixed | 5 | 30000 | 5000 |

### Circuit Breaker Metrics

```promql
# Monitor circuit states
gateway_circuit_breaker_state{backend="meta-1"}

# Track rejected requests
rate(gateway_rejected_requests_total[5m])
```

## Quota Enforcement

The quota system (`quota.rs`) enforces storage limits with soft/hard thresholds.

### Configuration

```yaml
quota:
  soft_limit_grace_period_seconds: 604800  # 7 days
  
  # Per-user default limits
  user_defaults:
    bytes_soft: 10737418240   # 10 GB
    bytes_hard: 12884901888   # 12 GB
    inodes_soft: 10000
    inodes_hard: 12000
  
  # Per-group default limits  
  group_defaults:
    bytes_soft: 107374182400  # 100 GB
    bytes_hard: 128849018880  # 120 GB
```

### Grace Period Behavior

When a user exceeds their soft limit:
1. Warning is logged
2. Operations continue for grace period
3. After grace period, hard limit is enforced

## Metadata Caching

The attribute cache (`nfs_cache.rs`) reduces metadata load on A2.

### Configuration

```yaml
cache:
  nfs_attr_cache:
    # Maximum cached entries
    max_entries: 10000
    
    # Default TTL in seconds
    default_ttl_secs: 30
    
    # Eviction policy: lru, lfu, or fifo
    eviction: lru
```

### Tuning Cache by Workload

| Workload Pattern | max_entries | default_ttl_secs |
|------------------|-------------|------------------|
| Many small files | 50000 | 10 |
| Few large files  | 5000 | 120 |
| Mixed            | 10000 | 30 |
| Read-heavy       | 20000 | 60 |
| Write-heavy      | 5000 | 5 |

### Cache Hit Rate Target

Monitor hit rate and tune accordingly:
```promql
# Calculate hit rate
rate(gateway_cache_hits_total[5m]) / 
(rate(gateway_cache_hits_total[5m]) + rate(gateway_cache_misses_total[5m]))
```

Target: > 80% hit rate for most workloads.

## SMB Multi-Channel Optimization

For high-throughput SMB workloads (`smb_multichannel.rs`).

### Configuration

```yaml
smb:
  multichannel:
    enabled: true
    
    # Maximum channels per session
    max_channels: 8
    
    # Minimum channels required
    min_channels: 2
    
    # Prefer RDMA when available
    prefer_rdma: true
    
    # Interface selection policy
    # Options: round_robin, weighted_by_speed, prefer_rdma
    channel_selection: weighted_by_speed
```

### NIC Selection for Multi-Channel

Configure multiple network interfaces:

```yaml
  interfaces:
    - name: eth0
      ip: 10.0.0.10
      link_speed_mbps: 25000
      capabilities:
        rdma: true
        rss: true
        tso: true
        checksum_offload: true
    - name: eth1
      ip: 10.0.1.10
      link_speed_mbps: 25000
      capabilities:
        rdma: true
        rss: true
```

### Performance Expectations

| Configuration | Expected Throughput |
|---------------|-------------------|
| Single 10GbE | ~800 MB/s |
| Dual 10GbE multi-channel | ~1.5 GB/s |
| Single 25GbE | ~2 GB/s |
| Dual 25GbE multi-channel | ~3.5 GB/s |
| 100GbE RDMA | ~8 GB/s |

## S3 Multipart Upload Tuning

Large object uploads use multipart (`s3_multipart.rs`).

### Configuration

```yaml
s3:
  # Minimum part size (5MB default - AWS S3 requirement)
  multipart_chunk_min: 5242880
  
  # Maximum parts per upload (10000 max)
  # Default chunk size * max_parts = max object size
  # e.g., 5MB * 10000 = 50GB max object
  
  # Part upload timeout (ms)
  part_timeout_ms: 300000
  
  # Maximum concurrent part uploads
  max_concurrent_parts: 4
```

### Tuning for Throughput

For maximum upload speed:
```yaml
s3:
  multipart_chunk_min: 104857600  # 100MB chunks
  max_concurrent_parts: 8
```

This reduces handshake overhead but uses more memory.

### Monitoring Multipart Uploads

```promql
# Active multipart uploads
gateway_s3_multipart_active

# Failed uploads
gateway_s3_multipart_failed_total
```

## Throughput Expectations

### By Protocol and Hardware

| Protocol | 1 Gbps NIC | 10 Gbps NIC | 25 Gbps NIC | 100 Gbps RDMA |
|----------|------------|-------------|-------------|---------------|
| NFSv3 | 100 MB/s | 800 MB/s | 2 GB/s | - |
| NFSv4 | 90 MB/s | 700 MB/s | 1.8 GB/s | - |
| pNFS | - | 900 MB/s | 2.2 GB/s | 8 GB/s |
| S3 | 80 MB/s | 700 MB/s | 1.8 GB/s | - |
| SMB3 | 90 MB/s | 750 MB/s | 1.9 GB/s | 6 GB/s |

### Latency Targets

| Operation | Target Latency (p99) |
|-----------|---------------------|
| GETATTR | 1 ms |
| LOOKUP | 2 ms |
| READ (4KB) | 0.5 ms |
| READ (1MB) | 3 ms |
| WRITE (4KB) | 0.5 ms |
| WRITE (1MB) | 4 ms |

### Performance Testing

```bash
# NFS throughput test
fio --name=throughput --rw=write --bs=1m --numjobs=4 \
    --filename=/mnt/claudefs/fio-test --direct=1 \
    --ioengine=libaio --runtime=60 --time_based

# NFS latency test  
fio --name=latency --rw=randread --bs=4k --numjobs=1 \
    --filename=/mnt/claudefs/fio-test --direct=1 \
    --ioengine=libaio --runtime=60 --time_based --percentile=99

# S3 benchmark
aws s3 cp --recursive test-dir/ s3://bucket/ \
    --expected-size=10737418240
```

## Resource Recommendations

### CPU

- Minimum: 8 cores
- Recommended: 16+ cores for high throughput
- Key metric: `gateway_cpu_usage_percent`

### Memory

- Base: 2 GB
- Per-connection buffer: 256 KB
- Cache: 1-4 GB depending on workload

Example for 1000 concurrent connections:
```
2GB + (1000 * 256KB) + 2GB cache = ~4.5 GB
```

### Network

- Use RSS-capable NICs for multi-core scaling
- Enable flow control
- Configure jumbo frames (MTU 9000) for LAN

## Next Steps

- [Operations Runbook](OPERATIONS_RUNBOOK.md) - Operational procedures
- [Protocol Notes](PROTOCOL_NOTES.md) - Protocol details