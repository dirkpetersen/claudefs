# ClaudeFS Replication Operations Guide

## Quick Start

### 30-Second Cluster Overview

ClaudeFS replication operates in an active-active dual-site configuration:

```
┌─────────────────┐       ┌─────────────────┐
│   Site A (DC1)  │       │   Site B (DC2)  │
│                 │       │                 │
│  - FUSE client  │◄─────►│  - FUSE client  │
│  - Meta server  │  RPC  │  - Meta server  │
│  - Storage      │       │  - Storage      │
└────────┬────────┘       └────────┬────────┘
         │                         │
         └──────────┬──────────────┘
                    │
            ┌───────▼───────┐
            │   S3 Backend  │
            │   (tiered)    │
            └───────────────┘
```

- **Primary write path**: Site accepts writes → writes to local journal → replicates to peer via RPC
- **Quorum**: 2-of-2 for strong consistency; 1-of-2 for graceful degradation
- **Conflict resolution**: Last-Write-Wins (LWW) based on wall-clock timestamp

### Health Status Check Commands

```bash
# Check replication status
cfs admin repl status

# Check replication lag
cfs admin repl lag

# Check site health
cfs admin site health

# Check failover status
cfs admin failover status
```

### Common Alerts and Quick Fixes

| Alert | Cause | Quick Fix |
|-------|-------|-----------|
| `replication_lag_high` | Network congestion or slow disk | Increase bandwidth or add disk throughput |
| `split_brain_detected` | Network partition | Run `cfs admin repl resolve-split-brain` |
| `site_unreachable` | Network outage or site down | Verify network, then trigger failover |
| `quorum_lost` | Both sites unreachable | Recover network, then force sync |

---

## Monitoring & Metrics

### Replication Lag Tracking

Key metrics from `claudefs-repl` crate:

```
repl_journal_lag_seconds{site="A"}      # How far behind peer
repl_write_latency_ms{pct="p99"}        # 99th percentile write latency  
repl_replication_throughput_mbps        # Current throughput
repl_queue_depth                        # Pending entries to replicate
```

### Alert Thresholds

| Metric | Warning | Critical |
|--------|---------|----------|
| Replication lag | >60s | >300s |
| Write latency (p99) | >100ms | >500ms |
| Queue depth | >1000 | >10000 |
| Site unreachable | 30s | 60s |

### Prometheus Queries

```promql
# Replication lag per site
rate(repl_journal_lag_seconds[5m])

# Write latency percentiles
histogram_quantile(0.99, rate(repl_write_latency_ms_bucket[1m]))

# Split-brain events
increase(repl_split_brain_total[1h])
```

---

## Failover Procedures

### Automatic Failover

When health checks detect site failure:

1. **Detection** (5000ms default interval)
   - 3 consecutive failed health checks → site marked degraded
   - Continued failures → site marked offline

2. **Quorum Consensus** (~1000ms)
   - Remaining site takes quorum
   - Fencing token issued to prevent split-brain

3. **Metadata Switchover** (~1000ms)
   - FailoverController promotes replica to primary
   - Client connections redirected

4. **Verification** (~2000ms)
   - Confirm writes succeed on new primary
   - Monitor replication restart

**Total expected failover time**: <5 seconds

### Manual Failover

Trigger manual failover when:
- Automated failover fails to complete
- Site recovery is uncertain
- Planned migration required

```bash
# Initiate manual failover
cfs admin failover trigger --site B --reason "planned maintenance"

# Verify failover completed
cfs admin failover status
```

### Failover Timing Expectations

| Phase | Expected Time |
|-------|---------------|
| Health check detection | 5s (configurable) |
| Quorum consensus | 1s |
| Metadata switchover | 1s |
| Client reconnection | 2s |
| **Total** | **<5s** |

### Validation Steps Post-Failover

1. Verify writes succeed:
   ```bash
   cfs admin repl verify-write
   ```

2. Check replication restarted:
   ```bash
   cfs admin repl lag
   ```

3. Confirm client connectivity:
   ```bash
   cfs admin client status
   ```

---

## Split-Brain Troubleshooting

### Symptoms and Detection

Split-brain occurs when:
- Network partition divides the cluster
- Both sites continue accepting writes independently
- Journals diverge with different sequences

Detection via SplitBrainResolver:
```rust
let mut resolver = SplitBrainResolver::new(2);
resolver.detect(site_a, site_b, seq_a, seq_b);
```

### Automatic Resolution Strategies

#### Last-Write-Wins (Default)

Accept writes from site with highest journal sequence:

```rust
resolver.resolve(ResolutionStrategy::LastWriteWins)?;
```

**Use when**: Single site clearly ahead, no data loss acceptable for older writes

#### Quorum-Based

Accept writes from majority partition:

```rust
resolver.resolve(ResolutionStrategy::QuorumBased)?;
```

**Use when**: Cluster has 3+ sites, majority still connected

#### Manual Resolution

Operator chooses which site to trust:

```rust
resolver.resolve(ResolutionStrategy::Manual { chosen_site_id: 1 })?;
```

**Use when**: Automated resolution unclear, need operator judgment

### Verification After Resolution

1. Check resolution audit trail:
   ```bash
   cfs admin repl resolution-history
   ```

2. Verify journal reconciliation:
   ```bash
   cfs admin repl verify-reconciliation
   ```

3. Confirm split-brain healed:
   ```bash
   cfs admin repl state  # Should show "Normal"
   ```

---

## Performance Tuning

### Replication Lag Targets

| SLA Tier | Target Lag | Maximum Lag |
|----------|------------|-------------|
| Gold | <5s | 30s |
| Silver | <30s | 60s |
| Bronze | <60s | 300s |

### Checkpoint Frequency

Reduce journal size by increasing checkpoint frequency:

```toml
[replication]
checkpoint_interval_ms = 5000  # Default: 10s → 5s for lower lag
```

### Compression Settings

Enable compression for low-bandwidth links:

```toml
[replication.compression]
enabled = true
algorithm = "lz4"  # lz4 (fast) or zstd (better ratio)
```

### Network Bandwidth Optimization

```toml
[replication.network]
max_concurrent_streams = 16
buffer_size_kb = 256
```

---

## SLA Definitions

### Recovery Point Objective (RPO)

- **Target**: <1 minute
- **Definition**: Maximum acceptable data loss in a disaster
- **Guarantee**: Journal replicated within 60s under normal conditions

### Recovery Time Objective (RTO)

- **Target**: <5 seconds
- **Definition**: Time to restore write capability after site failure
- **Guarantee**: Automatic failover completes within 5s

### Consistency Guarantees

- **Strong consistency**: All writes visible after quorum confirmation
- **Post-failover**: No lost writes, consistent state across sites

### Availability

- **Target**: 99.95% (excluding planned maintenance)
- **Calculation**: (Total Minutes - Downtime) / Total Minutes × 100
- **Exclusions**: Scheduled maintenance, force majeure

---

## Common Scenarios & Remediation

### "Replication is Lagging Behind"

**Diagnosis**:
```bash
cfs admin repl lag
cfs admin metrics repl_queue_depth
```

**Actions**:
1. Check network throughput: `cfs admin net stats`
2. Check disk latency: `iostat -x 1`
3. Increase checkpoint frequency if journal too large
4. Add network bandwidth if saturated

**Resolution**:
- Normal lag: automatic catch-up after load decreases
- Persistent lag: investigate underlying cause (disk/network)

---

### "Split-Brain Detected"

**Symptoms**:
```
ERROR: split-brain detected between site A and site B
```

**Immediate Actions**:
```bash
# View split-brain details
cfs admin repl split-brain status

# Choose resolution strategy
cfs admin repl resolve-split-brain --strategy=lww
# or
cfs admin repl resolve-split-brain --strategy=manual --site=A
```

**Verification**:
```bash
# Confirm resolution
cfs admin repl state

# Check audit trail
cfs admin repl resolution-history
```

---

### "One Site is Slow"

**Diagnosis**:
```bash
# Check site-specific metrics
cfs admin site metrics --site=B

# Check health
cfs admin site health --site=B
```

**Common Causes**:
- Disk saturation: check `iostat`
- Network congestion: check `netstat`
- High CPU: check `top`

**Tuning**:
```toml
[replication]
# Reduce write rate to prevent disk saturation
backpressure_threshold = 10000

# Increase timeout for slow sites
health_check_timeout_ms = 15000
```

---

### "Cluster Lost Quorum"

**Symptoms**:
```
ERROR: no quorum - both sites unreachable
```

**Recovery from S3**:
1. Verify S3 connectivity:
   ```bash
   cfs admin s3 check-connection
   ```

2. Force restore from S3:
   ```bash
   cfs admin disaster-recovery restore --source=s3
   ```

3. Verify restoration:
   ```bash
   cfs admin repl verify-state
   ```

**Timeline**: ~5-10 minutes depending on data size

---

## Integration with Management (A8)

### Grafana Dashboards

- **Replication Overview**: Lag, throughput, queue depth
- **Site Health**: Status, health check results, failover count
- **Split-Brain Events**: Detection, resolution, audit trail

### Alert Integration

Split-brain triggers SNS notification:
```
SNS Topic: arn:aws:sns:us-west-2:123456789012:claudefs-alerts
Subject: [CRITICAL] Split-Brain Detected
```

### CLI Commands

```bash
# Replication status
cfs admin repl status
cfs admin repl lag

# Failover management
cfs admin failover status
cfs admin failover trigger --site=B
cfs admin failover cancel

# Split-brain resolution
cfs admin repl split-brain status
cfs admin repl resolve-split-brain --strategy=lww

# Site management
cfs admin site health
cfs admin site add --address=10.0.0.5 --port=9000
cfs admin site remove --site=B
```

---

## Testing & Validation

### Failover Testing

```bash
# Simulate site failure
cfs admin failover simulate-failure --site=B

# Verify failover time
cfs admin failover measure-time
```

### Split-Brain Testing

```bash
# Create split-brain scenario
cfs admin repl create-split-brain --site-a=1 --site-b=2

# Test resolution
cfs admin repl resolve-split-brain --strategy=lww
```

---

## Appendix: Module Reference

### FailoverController

High-level failover orchestration:

```rust
use claudefs_repl::FailoverController;

let mut controller = FailoverController::new(config);
controller.record_failure(site_id)?;
if controller.should_failover(site_id) {
    controller.trigger_failover(site_id)?;
}
```

### SplitBrainResolver

Automated split-brain detection and resolution:

```rust
use claudefs_repl::{SplitBrainResolver, ResolutionStrategy};

let mut resolver = SplitBrainResolver::new(2);
if resolver.detect(site_a, site_b, seq_a, seq_b) {
    resolver.resolve(ResolutionStrategy::LastWriteWins)?;
}
```

### OperationalRunbook

Procedural guidance for operators:

```rust
use claudefs_repl::{OperationalRunbook, OperationalScenario};

let mut runbook = OperationalRunbook::new();
let steps = runbook.handle_scenario(OperationalScenario::PrimarySiteDown);
// Steps include: verify replica, promote, update routing, verify clients
```