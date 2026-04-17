# ClaudeFS Scaling & Capacity Planning Guide

**For:** Capacity planners, SREs, architects
**Updated:** 2026-04-17

---

## Capacity Planning Overview

ClaudeFS is designed to scale elastically from 3 nodes to 100+ nodes. This guide helps operators plan scaling and manage cluster growth.

---

## When to Scale

### Scale Up (Add Nodes)

**Trigger 1: Storage Capacity**

```bash
# Check current usage
cfs admin show-capacity

# Threshold for scaling:
# Usage > 80% → Plan scale within 1 week
# Usage > 90% → Scale immediately
```

**Trigger 2: Latency Degradation**

```bash
# Check p99 latency trend
prometheus_query 'histogram_quantile(0.99, storage_latency_seconds)'

# Scale if:
# P99 latency >1s for >10 minutes → add nodes

# OR: CPU/disk I/O at 80%+
# Scale if:
# Node CPU > 80% OR Disk I/O > 80%
```

**Trigger 3: Throughput Bottleneck**

```bash
# Check if throughput is capped
prometheus_query 'rate(operations_total[1m])'

# Baseline: 1000-5000 ops/sec per node

# Scale if:
# Observed throughput < (nodes × target-per-node)
# AND CPU/disk not at 100%
```

### Scale Down (Remove Nodes)

**Only when:**
- Cost cutting approved
- Storage < 40% full
- Performance target still met
- Cluster has redundancy (>3 nodes)

**Procedure:**

```bash
# 1. Drain node (migrate data away)
cfs admin drain-node storage-node-9

# 2. Wait for 100% completion
watch 'cfs admin show-drain-status'

# 3. Remove from cluster
cfs admin remove-node storage-node-9

# 4. Terminate instance
aws ec2 terminate-instances --instance-ids i-xxxxx
```

---

## Scaling Cluster

### Phase 1: Planning (1-2 hours)

**Checklist:**

- [ ] Assess current capacity: `cfs admin show-capacity`
- [ ] Project growth: `storage_growth_percent_per_week`
- [ ] Calculate target size: `(current_usage / 0.75) = target_capacity`
- [ ] Determine node count: `target_capacity / 1.5TB = num_nodes` (1.5TB per i4i.2xlarge)
- [ ] Check AWS quota: `aws service-quotas get-service-quota --service-code ec2 --quota-code L-1216C47A`
- [ ] Estimate cost increase
- [ ] Get approval (if over budget)

**Example:**

```
Current state:
  - 5 storage nodes (each 1.5TB = 7.5TB total)
  - Usage: 6TB (80%)
  - Cost: $1200/month

Target state:
  - Usage: 6TB at 60% = 10TB needed capacity
  - Nodes needed: 10TB / 1.5TB = 7 nodes
  - Scale: 5 → 7 nodes (add 2)
  - New cost: $1200 × (7/5) = $1680/month (+$480/month)
```

### Phase 2: Provision New Nodes (15-20 minutes)

```bash
# Provision new nodes
cfs-dev scale --nodes 7

# Monitor provisioning
watch 'cfs-dev status'

# Expected:
# - Nodes: 7 provisioned
# - New nodes joining cluster (SWIM discovery)
# - Rebalancing starting automatically
```

### Phase 3: Rebalancing (10-30 minutes)

```bash
# Monitor rebalancing progress
watch 'cfs admin show-rebalancing-status'

# Expected output:
# Rebalancing: in-progress
# Shards moved: 45/256
# ETA: 12 minutes

# For large cluster (100+ nodes):
# - Can take up to 2 hours
# - Monitor latency during rebalancing
# - Client traffic continues (no downtime)
```

### Phase 4: Validation (10-15 minutes)

```bash
# Verify balance achieved
cfs admin show-node-storage

# Expected:
# Each node has roughly equal storage ±5%

# Check latency returned to normal
curl -s 'http://prometheus:9090/api/v1/query?query=histogram_quantile(0.99,storage_latency_seconds)' | jq

# Expected: p99 latency <1000ms

# Test client operations
fsx -N 1000 /mnt/data/test.img

# Expected: all operations succeed
```

---

## Cluster Size Recommendations

### By Use Case

| Use Case | Min Nodes | Recommended | Max Nodes |
|----------|-----------|-------------|-----------|
| **Development** | 3 | 5 | 10 |
| **Staging** | 3 | 7 | 20 |
| **Production (small)** | 5 | 7 | 20 |
| **Production (large)** | 10 | 20 | 100+ |
| **Multi-site** | 5 (site A) + 2 (site B) | 7 + 3 | 50 + 20 |

### By Storage Capacity

| Capacity | Node Count | Cost/Month | Latency | Throughput |
|----------|------------|-----------|---------|-----------|
| 10 TB | 7 | $1600 | <500ms | 5K ops/sec |
| 50 TB | 34 | $8000 | <500ms | 25K ops/sec |
| 100 TB | 67 | $16000 | <500ms | 50K ops/sec |
| 500 TB | 334 | $80000 | <1s | 250K ops/sec |

### By Performance Requirements

| Target p99 Latency | Node Count (for 50TB) | Notes |
|---|---|---|
| <200ms (ultra-low) | 50+ | Premium instance types, full flash |
| <500ms (low) | 34 | Standard i4i.2xlarge instances |
| <1s (normal) | 20 | Can handle 80% utilization |
| <2s (acceptable) | 15 | Lower cost, trade latency |

---

## Metadata Shard Rebalancing

When cluster size changes significantly, metadata shards auto-rebalance.

### Shard Rebalancing Process

**Automatic (no action needed):**

```
Cluster grows from 5 → 7 nodes:
  1. New nodes join (SWIM gossip)
  2. Raft groups detect topology change
  3. Metadata shards migrate to new nodes (async)
  4. Rebalancing completes when all shards have new replicas

Time: Typically 10-30 minutes
Impact: Minimal (background, doesn't block client ops)
```

### Monitor Rebalancing

```bash
# Check shard distribution
cfs admin show-shard-distribution

# Expected output (pre-rebalance):
# Shard 0: node-1, node-2, node-3
# Shard 1: node-2, node-3, node-4
# ... (clustered on old nodes)

# After rebalancing:
# Shard 0: node-1, node-5, node-7
# Shard 1: node-2, node-6, node-1
# ... (distributed across all nodes)

# Verify completion
cfs admin show-rebalancing-status

# Expected: Rebalancing: complete
```

---

## Cost Management

### Cost Breakdown

**For 7-node cluster (10 TB capacity):**

| Component | Count | Unit Cost | Monthly |
|-----------|-------|-----------|---------|
| Storage nodes | 7 | $160/mo | $1120 |
| Orchestrator | 1 | $300/mo | $300 |
| Networking | - | - | $50 |
| S3 (backups) | 10TB | $0.05/GB | $500 |
| Data transfer | est. 1TB | $0.02/GB | $20 |
| CloudWatch | - | - | $10 |
| **Total** | - | - | **$2000** |

### Cost Optimization

**Strategy 1: Use Spot Instances**

```bash
# All storage nodes are spot (60-90% discount)
# On-demand cost: $160/node/month
# Spot cost: $50/node/month (60% discount)
# Savings: 7 nodes × $110 = $770/month

# Risk: Spot instances terminate (managed by cfs-dev auto-recovery)
# Acceptable for dev/test, requires HA setup for production
```

**Strategy 2: Right-size Instances**

```bash
# Current: i4i.2xlarge (2x $80 + $80 compute)
# Option A: i4i.xlarge (smaller, $40/mo each)
#   Pros: Save 50% cost
#   Cons: Latency may increase 2x, throughput halved

# Option B: i4i.4xlarge (larger, $200/mo each)
#   Pros: Better latency, more throughput
#   Cons: More cost, overkill if not fully utilized

# Recommendation: Measure before/after latency on smaller instance
```

**Strategy 3: Data Tiering**

```bash
# Archive old data to S3
cfs admin tier-data --older-than 30days --destination s3

# S3 cost: $0.05/GB/month (vs $0.1/GB/month on NVMe)
# Savings: 50% for archived data

# Trade-off: Slower access to tiered data (~100ms additional latency)
```

**Strategy 4: Compression & Dedup**

```bash
# Enable inline compression (enabled by default)
# Reduces storage 50-70% on typical workloads
# CPU cost: <5% overhead

# Enable deduplication (enabled by default)
# Reduces storage 20-40% on redundant data
# Benefits with:
#   - Backups (highly deduped)
#   - Container images (many identical layers)
#   - Database snapshots
```

### Cost Monitoring

```bash
# Check current spend
aws ce get-cost-and-usage \
  --time-period Start=$(date -d 'first day of this month' +%Y-%m-%d),End=$(date +%Y-%m-%d) \
  --granularity DAILY \
  --metrics BlendedCost

# Set budget alerts
aws budgets create-budget \
  --account-id $(aws sts get-caller-identity | jq -r '.Account') \
  --budget BudgetName=claudefs-production,BudgetLimit=2000,TimeUnit=MONTHLY

# Enable cost anomaly detection
aws ce put-anomaly-monitor \
  --anomaly-monitor MonitorName=claudefs,MonitorType=DIMENSIONAL,MonitorSpecification='{ ... }'
```

---

## Disaster Recovery & Redundancy

### Minimum Cluster Configuration

**For HA (High Availability):**

```
Minimum: 5 storage nodes
  - 3 nodes in Site A (Raft quorum)
  - 2 nodes in Site B (replication target)
  - Can survive 1 node failure in Site A (2/3 Raft quorum remains)
  - Can survive total failure of Site B (Site A continues)

Recommended: 7+ nodes
  - 5 nodes in Site A (better performance)
  - 2+ nodes in Site B (better failover)
  - Can survive 2 node failures in Site A (3/5 quorum remains)
  - Multi-node Site B provides redundancy
```

### Multi-Site Setup

**Example: 7-node cluster (production HA)**

```
AWS Region A (us-west-2):
  - 5 storage nodes (i4i.2xlarge spot)
  - 1 orchestrator (c7a.2xlarge on-demand)
  - Total: ~$1500/month

AWS Region B (us-east-1):
  - 2 storage nodes (i4i.2xlarge spot, async replicated)
  - Total: ~$300/month

Cross-region replication:
  - Latency: 30-50ms (typical)
  - Sync strategy: Async (writes ack'd when reached Site A quorum)
  - Recovery: Can switch to Site B as primary within 5 min

Total: ~$1800/month for HA across regions
```

### Failover Procedures

**Site A Primary → Site B Failover:**

```bash
# Step 1: Detect Site A failure
# (automatic via health monitoring)

# Step 2: Promote Site B to primary
cfs admin promote-replica-to-primary --site B

# Step 3: Update client configuration
# Point clients to Site B metadata servers

# Step 4: Re-establish Site A
# (when Site A recovered)

# Recovery time: <5 minutes (automatic failover)
# Data loss: 0 (async replication ensures all writes in Site B)
```

---

## Capacity Planning Template

**Use this to plan your deployment:**

```
CAPACITY PLAN — [Project Name]
Date: [current date]

=== Current State ===
Nodes: [count]
Total capacity: [GB]
Used capacity: [GB]
Usage %: [percent]
P99 latency: [ms]
Peak throughput: [ops/sec]
Cost/month: $[amount]

=== Projected Growth ===
Expected growth rate: [%/month]
Growth period: [months]
Projected usage in [timeframe]: [GB]
Projected peak throughput: [ops/sec]

=== Scaling Decision ===
Trigger point: [when to scale]
Target size: [nodes]
Target capacity: [GB]
Estimated cost increase: $[amount]/month
Projected latency: [ms]

=== Approval ===
Requester: [name]
Approver: [name]
Date approved: [date]
Implementation date: [date]
```

---

## Testing Scaling

### Load Test Before Scaling Production

```bash
# Deploy to staging cluster (same size as prod)
cfs-dev provision-staging --nodes 7

# Create test load
fio --filename=/mnt/data/test \
    --rw=randrw \
    --bs=4k \
    --runtime=3600 \
    --numjobs=32 \
    --iodepth=32

# Monitor metrics during test
watch 'cfs admin show-metrics'

# After test, analyze results
# - Did latency stay <1s?
# - Did throughput meet expectations?
# - Did CPU/disk stay <80%?

# If issues found: adjust parameters, retry
# If successful: proceed with production scaling
```

---

## Rollback Procedures

**If scaling causes issues:**

```bash
# Step 1: Identify the problem
# (e.g., latency degraded, cost spike)

# Step 2: Drain new nodes
for node in node-6 node-7; do
  cfs admin drain-node $node
  sleep 600  # Wait for drain completion
done

# Step 3: Remove new nodes
cfs admin remove-node node-6
cfs admin remove-node node-7

# Step 4: Verify performance restored
cfs admin show-metrics

# Step 5: Investigate root cause
# - Was rebalancing incomplete?
# - Were new nodes under-provisioned?
# - Was metadata shard distribution unbalanced?

# Step 6: Fix issue and retry
```

---

## Monitoring During Scaling

**Key Metrics to Watch:**

```bash
# Create Grafana dashboard showing:
# 1. Node count (should spike up)
# 2. Storage per node (should decrease as rebalancing progresses)
# 3. P99 latency (may spike, then settle)
# 4. Rebalancing progress (should reach 100%)
# 5. CPU/disk I/O per node (monitor for hotspots)

# Alert thresholds during rebalancing:
# - P99 latency >2s for >5min: warning
# - P99 latency >5s for >2min: critical
# - Any node CPU >95% for >10min: warning
```

---

## References

- [OPERATIONS_RUNBOOK.md](OPERATIONS_RUNBOOK.md) — Daily operations
- [DEBUGGING_RUNBOOK.md](DEBUGGING_RUNBOOK.md) — Troubleshooting
- [docs/decisions.md](decisions.md) — D4 Raft topology
- [docs/hardware.md](hardware.md) — Instance type details

---

**Last Updated:** 2026-04-17
**Maintained By:** A11 Infrastructure & CI
**Co-Authored-By:** Claude Haiku 4.5 <noreply@anthropic.com>
