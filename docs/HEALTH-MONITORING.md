# ClaudeFS Cluster Health Monitoring

**Status:** Phase 3 Design Document
**Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-05

---

## Overview

The Cluster Health Monitor is a distributed health check service that:
1. Monitors node health (CPU, memory, disk, network)
2. Detects hardware failures and degradation
3. Coordinates automatic recovery actions
4. Provides health-based routing decisions
5. Integrates with AWS spot instance lifecycle events

## Architecture

```
┌─────────────────────────────────────────┐
│ ClaudeFS Nodes (Storage, Metadata, etc) │
├─────────────────────────────────────────┤
│  Health Check Daemon (per node)         │
│  ├─ CPU monitor                         │
│  ├─ Memory monitor                      │
│  ├─ Disk I/O monitor                    │
│  └─ Network latency monitor             │
└──────────────┬──────────────────────────┘
               │ (health metrics)
        ┌──────▼─────────┐
        │ Health Manager │ (elected leader)
        │ ├─ Collect metrics
        │ ├─ Detect anomalies
        │ ├─ Coordinate recovery
        │ └─ Update routing
        └──────┬─────────┘
               │
        ┌──────▼──────────┐
        │ Remediation     │
        │ Actions         │
        ├─ Restart node   │
        ├─ Migrate data   │
        ├─ Terminate node │
        └─ Request new node
```

## Health States

### Node Health Levels

```
HEALTHY (Green)
  ├─ CPU: <70%
  ├─ Memory: <80%
  ├─ Disk: <90%
  └─ Latency: <50ms

DEGRADED (Yellow)
  ├─ CPU: 70-85%
  ├─ Memory: 80-95%
  ├─ Disk: 90-98%
  └─ Latency: 50-100ms

CRITICAL (Red)
  ├─ CPU: >85%
  ├─ Memory: >95%
  ├─ Disk: >98%
  └─ Latency: >100ms

OFFLINE (Black)
  └─ Node unreachable for >30s
```

### Cluster Health Levels

```
FULLY_HEALTHY: All nodes HEALTHY
DEGRADED:      1+ nodes DEGRADED, quorum maintained
CRITICAL:      Quorum at risk or 1+ nodes CRITICAL
UNHEALTHY:     <3 nodes reachable
```

## Health Checks

### CPU Monitor

```rust
pub struct CPUMonitor {
    thresholds: CPUThresholds,
    history: VecDeque<CPUSample>,

    // Thresholds
    degraded_threshold: 70%,
    critical_threshold: 85%,
    sample_interval: 10s,
    history_size: 6 (1 minute),
}

// Alert if:
// - 5-min average > degraded threshold
// - Any single measurement > critical threshold
// - Trending upward (derivative > 2%/min)
```

### Memory Monitor

```rust
pub struct MemoryMonitor {
    // Track RSS (resident set size) and VSZ (virtual size)
    // Per-crate breakdown: storage, meta, gateway, etc.

    // Alert if:
    // - RSS > 80% of available
    // - RSS growing unabated (leak detection)
    // - Single spike to 95%+ (OOM risk)
    // - Memory fragmentation > 30%
}
```

### Disk I/O Monitor

```rust
pub struct DiskIOMonitor {
    // Track per filesystem: utilization, latency, error rate

    // Alert if:
    // - Used space > 90%
    // - Util latency > threshold
    // - Read/write error rate elevated
    // - SMART health score degrading
}
```

### Network Monitor

```rust
pub struct NetworkMonitor {
    // Track latency to all peer nodes
    // Detect path degradation or asymmetry

    // Alert if:
    // - Latency to any peer > 100ms
    // - Packet loss > 0.1%
    // - Path asymmetry > 50%
    // - Network interface errors
}
```

## Recovery Actions

### Automatic Recovery

| Condition | Action | Decision |
|-----------|--------|----------|
| High memory (>90%) | Reduce cache sizes | Auto |
| High CPU (>85%) | Throttle non-essential tasks | Auto |
| Disk full (>95%) | Trigger compaction/cleanup | Auto |
| Network latency (>200ms) | Use alternate path (if available) | Auto |
| Node offline (>30s) | Remove from routing | Auto |
| Raft quorum at risk | Alert and pause new mutations | Manual |
| Hardware failure (SMART) | Terminate and request new node | Manual+Auto |

### Recovery Workflow

```
1. Detect unhealthy state (automatic)
2. Verify not transient (5s check)
3. Alert administrators
4. Attempt automatic remediation
5. If remediation fails (60s timeout):
   - Quarantine node (remove from cluster)
   - Request replacement node
   - Rebalance data/metadata
6. Once recovered, rejoin cluster
```

## Integration Points

### AWS Spot Instance Events

Listen to AWS EC2 Instance State Change Notifications:

```
spot-instance-interruption-warning
  ↓
100 second countdown to termination
  ↓
Graceful shutdown:
  1. Drain connections
  2. Complete in-flight operations
  3. Trigger Raft snapshot
  4. Announce leaving cluster
  5. Exit cleanly
```

### Kubernetes Integration (Future)

If deployed on K8s:
- Pod liveness/readiness probes
- StatefulSet coordination
- Persistent Volume Mount monitoring
- Resource quota adherence

## Deployment

### Single Node (Development)

Health checks run as background task in main daemon:

```bash
./target/release/cfs daemon --enable-health-checks
```

### Multi-Node Cluster (Production)

Health Manager elected via Raft consensus:

```yaml
health_monitoring:
  enabled: true
  check_interval_seconds: 10
  recovery_enabled: true

  thresholds:
    cpu_degraded_percent: 70
    cpu_critical_percent: 85
    memory_degraded_percent: 80
    memory_critical_percent: 95
    disk_degraded_percent: 90
    disk_critical_percent: 98
    network_latency_ms: 100
    offline_timeout_seconds: 30
```

## Metrics Exposed

Per node:

```promql
node_health_state{node_id, state}
node_cpu_percent
node_memory_percent
node_disk_percent
node_network_latency_ms
node_last_check_timestamp_seconds
node_recovery_attempts_total
node_errors_total{error_type}
```

Cluster:

```promql
cluster_health_state{state}
cluster_healthy_nodes_count
cluster_degraded_nodes_count
cluster_offline_nodes_count
cluster_quorum_active{quorum_id}
```

## Alerting Rules

```yaml
- alert: NodeHighCPU
  expr: node_cpu_percent > 85
  for: 5m

- alert: NodeHighMemory
  expr: node_memory_percent > 90
  for: 3m

- alert: NodeHighLatency
  expr: node_network_latency_ms > 100
  for: 5m

- alert: QuorumAtRisk
  expr: cluster_healthy_nodes_count + cluster_degraded_nodes_count < 3
  for: 1m

- alert: NodeOffline
  expr: cluster_offline_nodes_count > 0
  for: 2m
```

## Troubleshooting

### Node stays in CRITICAL

1. Check actual resource usage: `top`, `free`, `df`
2. Verify no stuck processes
3. Review logs for OOM killer events
4. Check for resource leaks in code
5. If persistent: terminate and replace node

### Cluster health degraded

1. Verify all nodes reachable: `cfs cluster status`
2. Check network connectivity: `ping`, `traceroute`
3. Review latency metrics in Grafana
4. Check for correlated failures (software update, etc)
5. Manual failover if necessary

### False positives (flaky health checks)

- Increase check intervals (reduce noise)
- Increase degradation thresholds
- Require multiple consecutive failures before alerting
- Implement exponential backoff on repeated checks

## Future Enhancements

- [ ] Machine learning for anomaly detection
- [ ] Predictive failure detection (SMART trends)
- [ ] Automatic node scaling (cloud-native)
- [ ] Circuit breaker patterns per component
- [ ] Fine-grained resource tracking per operation
- [ ] Cost-aware recovery (minimize cloud spend)

---

## Implementation Status

**Phase 3 (Current):**
- [ ] Health monitoring agent per node
- [ ] Health manager election
- [ ] Automatic recovery actions
- [ ] Alerting integration

**Phase 4 (Planned):**
- [ ] Spot instance lifecycle handling
- [ ] K8s integration
- [ ] ML-based anomaly detection
- [ ] Production SLO dashboards
