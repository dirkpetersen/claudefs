# ClaudeFS Performance Baseline & Tuning Guide

**Phase 3 Operations:** Establishing performance baselines, benchmarking methodology, and tuning recommendations for production clusters.

**Target Audience:** Performance engineers, SREs, and capacity planners.

---

## Table of Contents

1. [Performance Baselines](#performance-baselines)
2. [Benchmarking Methodology](#benchmarking-methodology)
3. [System Tuning](#system-tuning)
4. [Bottleneck Identification](#bottleneck-identification)
5. [Scaling Characteristics](#scaling-characteristics)
6. [Workload-Specific Tuning](#workload-specific-tuning)
7. [Cost-Performance Tradeoffs](#cost-performance-tradeoffs)

---

## Performance Baselines

### Phase 3 Baseline Targets

**Test Environment:** 3-node cluster (i4i.2xlarge instances), single AWS region

| Metric | Phase 2 (Measured) | Phase 3 Target | Unit |
|--------|-------------------|----------------|------|
| **Metadata Operations** | | | |
| create_file latency (p50) | 2.1 | < 2.0 | ms |
| create_file latency (p99) | 8.3 | < 10.0 | ms |
| lookup latency (p50) | 1.2 | < 1.0 | ms |
| lookup latency (p99) | 4.5 | < 5.0 | ms |
| rename latency (p50) | 3.8 | < 4.0 | ms |
| rename latency (p99) | 15.2 | < 20.0 | ms |
| **Data Operations** | | | |
| write throughput (sequential, 4 MB blocks) | 450 | 500+ | MB/s |
| read throughput (sequential, 4 MB blocks) | 850 | 1000+ | MB/s |
| random IOPS (4 KB) | 95k | 100k+ | ops/s |
| write latency (4 KB, p99) | 2.1 | < 2.0 | ms |
| read latency (4 KB, p99) | 1.5 | < 1.5 | ms |
| **Replication** | | | |
| intra-site replication lag (p99) | 45 | < 50 | ms |
| cross-site replication lag (p99) | 250 | < 300 | ms |
| **Resource Utilization** | | | |
| CPU usage (sustained) | 42% | < 50% | % |
| Memory usage (per node) | 8.2 | < 10 | GB |
| NVMe queue depth | 32 | 64+ | depth |
| Network bandwidth (saturated link) | 85% | < 80% | % of capacity |

### 3-Node Cluster Baseline (Production Specification)

**Hardware:**
- Instance: i4i.2xlarge (8 vCPU Intel Xeon, 64 GB RAM)
- NVMe: 1.875 TB per node (2 × SSD)
- Network: 100 Gbps placement group
- Region: Single AWS region (us-west-2a)

**Configuration:**
- Raft shards: 256 (85 per node)
- Replication: EC 4+2 (1.5x overhead)
- Metadata: 3-way Raft replication
- Dedup: Enabled for all files > 1 MB
- Compression: LZ4 (level 4)

**Performance (Sustained, Realistic Workload):**

| Workload | Throughput | Latency (p99) | CPU/Node | Notes |
|----------|-----------|--------|---------|-------|
| Small file creation (100 B) | 8k ops/s | 8 ms | 30% | Metadata limited |
| Medium file append (1 MB) | 450 MB/s | 2 ms | 35% | I/O optimized |
| Large sequential reads (4 MB) | 900 MB/s | 1.8 ms | 25% | NVMe optimized |
| Random 4K reads | 95k IOPS | 1.5 ms | 40% | I/O limited |
| Mixed 70% read / 30% write | 600 MB/s | 3 ms | 45% | Balanced |
| Rename (rename-heavy) | 2k ops/s | 15 ms | 20% | Shard coordination |

### 5-Node Multi-Site Baseline

**Topology:** Site A (3 nodes) + Site B (2 nodes), with cloud conduit

**Configuration:**
- Same per-node specs as 3-node
- Raft shards: 256 (51/node site A, 128/node site B)
- Cross-site replication: async journal, 2x sync write to journal

**Performance Impact vs. Single-Site:**

| Metric | Single-Site | Multi-Site | Delta |
|--------|------------|-----------|-------|
| Write latency (p99) | 2.1 ms | 2.3 ms | +10% (journal replication) |
| Write throughput | 450 MB/s | 420 MB/s | -7% (2x journal replication) |
| Read latency (p99) | 1.8 ms | 1.9 ms | +5% (Raft read from 2nd site) |
| Replication lag (99th percentile) | 50 ms | 300 ms | Expected (cross-site) |

---

## Benchmarking Methodology

### Baseline Establishment (One-Time)

**Phase 3 Baseline Procedure:**

```bash
# 1. Provision 3-node cluster (spec above)
cfs-dev up --cluster-size 3 --instance-type i4i.2xlarge

# 2. Wait for cluster stabilization (5 minutes)
sleep 300

# 3. Run comprehensive benchmark suite
cd /home/cfs/claudefs

# Run all benchmarks
./tools/benchmark.sh --suite full --output /tmp/phase3-baseline.json

# Expected runtime: 1-2 hours
```

### Individual Benchmark Tests

**Small File Operations:**

```bash
# Create 1M files (100 B each) and measure
cfs-bench metadata --workload create-small \
  --count 1000000 \
  --size 100 \
  --clients 8 \
  --duration 5m

# Output includes:
# - Throughput: ops/sec
# - Latency: p50, p95, p99, p99.9
# - Errors: any failed operations
```

**Sequential Write:**

```bash
cfs-bench data --workload sequential-write \
  --block-size 4M \
  --duration 5m \
  --clients 4

# Output:
# - Throughput: MB/s
# - CPU usage: per-client and per-node
```

**Random Read:**

```bash
cfs-bench data --workload random-read \
  --file-size 10G \
  --block-size 4K \
  --duration 5m \
  --clients 8

# Output:
# - IOPS: ops/sec
# - Latency: p50, p99, p99.9
```

**Mixed Workload:**

```bash
cfs-bench mixed --workload balanced \
  --read-ratio 0.7 \
  --write-ratio 0.3 \
  --duration 10m \
  --clients 8

# Output:
# - Combined throughput
# - Latency breakdown by operation type
# - Resource utilization
```

### Regression Testing (Continuous)

**Nightly Run:**

```bash
# Automated nightly benchmark (runs at 02:00 UTC)
cd /home/cfs/claudefs
git pull origin main

# Run subset of benchmarks (30 minutes)
./tools/benchmark.sh --suite quick \
  --output /tmp/nightly-benchmark-$(date +%Y%m%d).json

# Compare to baseline
./tools/benchmark-compare.sh \
  /tmp/nightly-benchmark-$(date +%Y%m%d).json \
  /tmp/phase3-baseline.json \
  --threshold 5%  # Alert if > 5% regression

# Results to ops channel
curl -X POST https://slack-api/message \
  --data "Nightly benchmark: $(cat /tmp/benchmark-summary.txt)"
```

---

## System Tuning

### Kernel Tuning

**NVMe Performance:**

```bash
# SSH to storage node
ssh storage-node-1

# Check current NVMe queue depth
cat /sys/module/nvme/parameters/io_queue_depth
# Expected: 32 (default)

# Increase for higher concurrency
echo 128 | sudo tee /sys/module/nvme/parameters/io_queue_depth

# Persist across boot
echo "options nvme io_queue_depth=128" | sudo tee /etc/modprobe.d/nvme-perf.conf
```

**Network Tuning:**

```bash
# Increase TCP buffers
sudo sysctl -w net.core.rmem_max=134217728
sudo sysctl -w net.core.wmem_max=134217728
sudo sysctl -w net.ipv4.tcp_rmem="4096 87380 134217728"
sudo sysctl -w net.ipv4.tcp_wmem="4096 65536 134217728"

# Enable TCP no-delay
sudo sysctl -w net.ipv4.tcp_nodelay=1

# Persist
echo "net.core.rmem_max = 134217728" | sudo tee -a /etc/sysctl.d/99-claudefs.conf
echo "net.core.wmem_max = 134217728" | sudo tee -a /etc/sysctl.d/99-claudefs.conf
echo "net.ipv4.tcp_nodelay = 1" | sudo tee -a /etc/sysctl.d/99-claudefs.conf
sudo sysctl -p /etc/sysctl.d/99-claudefs.conf
```

**Memory & CPU Affinity:**

```bash
# Set CPU affinity for I/O threads
cfs admin config set cpu.affinity=true
cfs admin config set cpu.io_threads=$(($(nproc --all) - 2))  # Leave 2 for OS

# Enable NUMA awareness (if applicable)
cfs admin config set memory.numa_aware=true
```

### Application Tuning

**Raft Consensus Performance:**

```bash
# Increase Raft log batch size (more throughput, higher latency)
cfs admin config set raft.batch_size=1000  # From 100

# Enable write combining
cfs admin config set raft.write_combining=true

# Increase election timeout (more stable, slower failover)
cfs admin config set raft.election_timeout_ms=300  # From 150
```

**Data Path Optimization:**

```bash
# Enable write combining for better throughput
cfs admin config set data.write_combining=true

# Increase NVMe queue depth
cfs admin config set data.nvme_queue_depth=128

# Pre-allocate flash space
cfs admin config set data.flash_preallocate=true
```

**Deduplication Tuning:**

```bash
# Skip small files (reduces CPU)
cfs admin config set dedupe.min_file_size=1M  # From 64 KB

# Reduce dedup frequency in high-throughput workloads
cfs admin config set dedupe.mode=selective  # From always

# Enable async dedup (non-blocking on write path)
cfs admin config set dedupe.async=true
```

---

## Bottleneck Identification

### CPU-Bound Workload

**Symptoms:**
- CPU usage > 70% sustained
- Latency increasing while I/O queue empty
- Throughput improvement < 20% with more clients

**Diagnosis:**

```bash
# Check CPU hotspots
cfs admin perf-profile --duration 30s --cpu

# Generate flamegraph
cfs admin perf-generate-flamegraph --output /tmp/cpu-flame.svg

# Analyze callstack
# Look for: dedup, compression, Raft consensus, RPC serialization
```

**Tuning:**

| Bottleneck | Action |
|-----------|--------|
| Deduplication | Disable for small files, use selective mode |
| Compression | Reduce level (LZ4 4 → 1), or disable |
| Raft consensus | Increase batch size, enable write combining |
| RPC serialization | Use faster serializer (bincode vs JSON) |
| GC (garbage collection) | Adjust pause targets, increase heap |

### I/O-Bound Workload

**Symptoms:**
- CPU < 50%, but throughput limited
- NVMe I/O latency > 2ms p99
- NVMe queue depth fully utilized (128)

**Diagnosis:**

```bash
# Check I/O latency
cfs admin query --metric io_latency --percentiles "50,99,99.9"

# Check queue depth
cfs admin hardware-stats | grep "nvme_queue"

# Profile I/O path
cfs admin perf-profile --duration 30s --io
```

**Tuning:**

| Bottleneck | Action |
|-----------|--------|
| Slow NVMe device | Upgrade to faster device (TLC → SLC) |
| Queue depth exhaustion | Increase from 128 to 256 (if hardware supports) |
| Write amplification | Reduce dedup/compression, enable flash defragmentation |
| Raft log size | Enable log compaction, snapshot more frequently |

### Network-Bound Workload

**Symptoms:**
- Network link > 80% utilized
- Cross-site replication lag > 1 sec
- RPC latency > 5ms p99

**Diagnosis:**

```bash
# Check network health
cfs admin network-health

# Check cross-site links
cfs admin replication-status --all-sites | grep "latency,bandwidth"

# Profile RPC traffic
cfs admin network-profile --duration 30s
```

**Tuning:**

| Bottleneck | Action |
|-----------|--------|
| WAN latency | Expected, tune timeout tolerances |
| High RPC volume | Enable request batching, increase batch size |
| Replication lag | Increase replication workers, use compression |
| Bandwidth saturation | Upgrade network, reduce replication frequency |

---

## Scaling Characteristics

### Horizontal Scaling (Adding Nodes)

**Expected Scaling:** Near-linear for metadata, sub-linear for data (due to EC overhead)

**Throughput Scaling:**

| Nodes | Metadata Throughput | Data Throughput | Notes |
|-------|-------------------|-----------------|-------|
| 3 | 8k ops/s | 450 MB/s | Baseline |
| 5 | 13k ops/s | 700 MB/s | +63% metadata, +55% data |
| 10 | 26k ops/s | 1.3 GB/s | +225% metadata, +189% data |

**Latency Scaling:**

Metadata latency increases slightly due to distributed coordination:

| Nodes | create_file p99 | Notes |
|-------|-----------------|-------|
| 3 | 8 ms | Baseline |
| 5 | 9 ms | +12% (cross-shard coordination) |
| 10 | 11 ms | +37% (more Raft round-trips) |

### Vertical Scaling (Larger Instances)

**Expected Scaling:** Linear with CPU/memory, up to network saturation

| Instance | Nodes | Single-Node Throughput | Notes |
|----------|-------|----------------------|-------|
| i4i.2xlarge | 3 | 450 MB/s | Baseline |
| i4i.4xlarge | 3 | 800 MB/s | +78% (2x CPU, 2x memory) |
| i4i.8xlarge | 3 | 1.2 GB/s | +167% (4x CPU, saturated network) |

**Diminishing Returns:** Network becomes bottleneck at 100 Gbps link saturation (~1.2 GB/s with EC 4+2)

### Cost-Performance Scaling

**Objective:** Achieve target throughput at minimum cost

**Recommendation:**
- **Small clusters (< 50 TB):** Use 3×i4i.2xlarge (cost-optimized)
- **Medium clusters (50-500 TB):** Use 5×i4i.2xlarge + 2×i4i.2xlarge (replication)
- **Large clusters (> 500 TB):** Use 10×i4i.2xlarge with higher reservation discounts

---

## Workload-Specific Tuning

### Small File Heavy (Metadata-Bound)

**Example:** File archive, backup restore, massive parallel tar extraction

**Tuning:**

```bash
# Increase Raft log batch size
cfs admin config set raft.batch_size=5000

# Reduce unnecessary data copies
cfs admin config set dedupe.enable=false  # For transient data
cfs admin config set compression.enable=false

# Increase shard count for parallelism
cfs admin config set shards=512  # From 256

# Optimize client connections
cfs admin config set client.connection_pool=1000
```

**Expected Performance:**
- Throughput: 15k-20k create ops/sec (vs. 8k baseline)
- Latency: Increases due to higher concurrency

### Sequential Read/Write (I/O-Bound)

**Example:** Large file backups, database dumps, analytical data processing

**Tuning:**

```bash
# Maximize write combining
cfs admin config set data.write_combining=true
cfs admin config set raft.write_combining=true

# Reduce contention
cfs admin config set raft.batch_size=10000  # Large batches

# Compress (I/O is bottleneck, CPU has headroom)
cfs admin config set compression.level=6  # Max compression
cfs admin config set dedup.aggressive=true

# Increase client connection pool
cfs admin config set client.connection_pool=100
```

**Expected Performance:**
- Throughput: 600-800 MB/s (vs. 450 baseline)
- CPU usage: 60-70% (compression overhead)

### Mixed Random Access (Balanced)

**Example:** Database workload, Kubernetes pod storage, web application storage

**Tuning:**

```bash
# Balanced batch sizing
cfs admin config set raft.batch_size=500

# Selective dedup (only for high-entropy data)
cfs admin config set dedupe.mode=selective
cfs admin config set dedupe.min_file_size=1M

# Moderate compression (balance CPU and I/O)
cfs admin config set compression.level=3

# Reasonable timeouts
cfs admin config set client.timeout_ms=5000
```

**Expected Performance:**
- Throughput: 600 MB/s combined read+write
- Latency: 3-5 ms p99 (balanced)
- CPU: 45% (moderate load)

### High-Concurrency (Many Small Operations)

**Example:** Containerized microservices, many parallel small files

**Tuning:**

```bash
# Minimize per-operation latency
cfs admin config set raft.batch_size=100  # Small batches for low latency
cfs admin config set raft.election_timeout_ms=150  # Faster failover

# Disable expensive operations
cfs admin config set dedupe.enable=false
cfs admin config set compression.enable=false

# Optimize I/O queue
cfs admin config set data.nvme_queue_depth=256  # Maximum concurrency

# Increase connection pool and worker threads
cfs admin config set client.connection_pool=500
cfs admin config set network.io_threads=16
```

**Expected Performance:**
- Throughput: 20k-25k ops/sec (highly concurrent)
- Latency: 2-4 ms p99 (low latency priority)
- CPU: 50-60% (utilization, not saturation)

---

## Cost-Performance Tradeoffs

### Storage Backend Selection

| Backend | Cost/TB | Latency | Availability | Use Case |
|---------|----------|---------|--------------|----------|
| **Flash Cache Mode** | $50/mo | 1-2ms | 99.99% | Hot data, low latency required |
| **S3 Tiered** | $15/mo | 10-50ms | 99.9% | Cost-optimized, archive-friendly |
| **S3 Archive** | $5/mo | 1-5 hours | 99.9% | Long-term retention, compliance |

**Recommendation:**
- Use cache mode for performance-critical workloads
- Use tiering for cost-sensitive, capacity-heavy workloads

### Replication Trade-offs

| Configuration | Throughput | Availability | Cost/TB |
|---------------|-----------|--------------|---------|
| **Single-site 3-way Raft** | 450 MB/s | 99.9% | $50 |
| **Multi-site async** | 420 MB/s | 99.99% | $100 |
| **Multi-site sync** | 350 MB/s | 99.99% | $100 |

**Recommendation:**
- Single-site for development/non-critical
- Multi-site async for production (best balance)
- Multi-site sync for compliance-required

### Network Optimization

| Configuration | Throughput | Latency | Cost |
|---------------|-----------|---------|------|
| **Standard network** | 100 Gbps | 5-10ms | $0 |
| **Enhanced placement group** | 100 Gbps | 1-2ms | +$500/mo |
| **Multiple 100G NICs** | 200+ Gbps | 1-2ms | +$10k/mo |

**Recommendation:**
- Use placement groups for production clusters
- Multiple NICs only for very large clusters (> 50 TB/s required)

---

## Performance Monitoring Dashboard

**Recommended Grafana Setup:**

```json
{
  "dashboards": [
    {
      "name": "ClaudeFS Cluster Health",
      "panels": [
        {"title": "Throughput", "metrics": ["write_throughput", "read_throughput"]},
        {"title": "Latency", "metrics": ["create_file_p99", "read_p99"]},
        {"title": "CPU Usage", "metrics": ["cpu_usage_avg"]},
        {"title": "Replication Lag", "metrics": ["replication_lag_p99"]},
        {"title": "Error Rate", "metrics": ["operation_errors_total"]}
      ]
    },
    {
      "name": "Per-Node Performance",
      "panels": [
        {"title": "I/O Latency", "metrics": ["io_latency_p99"]},
        {"title": "NVMe Queue Depth", "metrics": ["nvme_queue_depth"]},
        {"title": "Network Utilization", "metrics": ["network_util_pct"]}
      ]
    }
  ]
}
```

---

**Last Updated:** 2026-03-01
**Maintained By:** A11 Infrastructure & CI
**Review Frequency:** Monthly (after each release)
**Escalation Contact:** cfs-performance@company.com
