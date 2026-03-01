# ClaudeFS Operational Procedures & Runbooks

**Phase 3 Operations:** Comprehensive runbooks for day-to-day cluster operations, scaling, maintenance, and emergency procedures.

**Target Audience:** ClaudeFS operators, SREs, and on-call engineers.

---

## Table of Contents

1. [Daily Operational Tasks](#daily-operational-tasks)
2. [Node Management](#node-management)
3. [Cluster Scaling](#cluster-scaling)
4. [Maintenance & Updates](#maintenance--updates)
5. [Emergency Procedures](#emergency-procedures)
6. [Debugging & Log Analysis](#debugging--log-analysis)
7. [Performance Tuning](#performance-tuning)
8. [Backup & Recovery](#backup--recovery)
9. [Metrics & Alert Interpretation](#metrics--alert-interpretation)
10. [Escalation Procedures](#escalation-procedures)

---

## Daily Operational Tasks

### 1. Morning Health Check (Start of Shift)

**Estimated Time:** 5-10 minutes

**Purpose:** Verify cluster is healthy, no overnight issues

**Procedure:**

```bash
# 1. Check cluster status
cfs admin status

# Expected output:
# Cluster: claudefs-prod-1 (3 nodes)
# Status: HEALTHY
# Raft Groups: 256/256 healthy
# Replication Lag: < 100ms

# 2. Check individual node status
cfs admin node-status --all

# 3. Check for any overnight alerts
cfs admin alerts --since 24h

# 4. Verify connectivity to S3 backend
cfs admin s3-health

# 5. Check disk usage trends
cfs admin storage stats --json | jq '.nodes[] | {name, usage_pct}'

# 6. View Grafana dashboard
# Open: https://grafana.claudefs.internal:3000/d/cluster-health
# Look for:
#   - Green status on all panels
#   - No spike in error rates
#   - Latency p99 < 50ms
```

**Success Criteria:**
- ✅ Cluster status: HEALTHY
- ✅ All Raft groups: HEALTHY
- ✅ Replication lag: < 1 minute
- ✅ No active critical alerts
- ✅ Disk usage stable (< 85%)

**If Issues Found:**
- See [Debugging & Log Analysis](#debugging--log-analysis)
- Page on-call engineer if critical

### 2. Midday Capacity Check (Business Hours)

**Estimated Time:** 2-3 minutes

**Purpose:** Ensure cluster isn't hitting capacity limits

**Procedure:**

```bash
# Check current usage vs. configured limits
cfs admin capacity-check

# Output should show:
# Storage: 45% of 100TB
# Metadata: 2.1M inodes (3% of 64M limit)
# Replication lag: 50ms (below 5min target)
# QoS: All workload classes within limits

# Alert if:
# - Storage > 80%
# - Replication lag > 5 min
# - Any workload class at limit
```

**Action if Capacity Concern:**
- Trigger eviction to S3 if in cache mode: `cfs admin evict-to-s3 --aggressive`
- Scale out if needed (see [Cluster Scaling](#cluster-scaling))

### 3. Evening Summary (End of Shift)

**Estimated Time:** 5 minutes

**Purpose:** Document shifts and hand off to night shift

**Procedure:**

```bash
# Generate operational summary
cfs admin stats --summary --json > /var/log/claudefs/shift-summary-$(date +%Y%m%d).json

# Check for any warnings or degradation
cfs admin health-report --period 8h

# File summary in shared log:
# Date: [date]
# Shift: [morning/evening/night]
# Status: [HEALTHY/DEGRADED/CRITICAL]
# Key Events: [list of significant events]
# Handoff Notes: [anything for next shift]

# Post to ops channel:
# slack-post "#ops-handoff" < /tmp/shift-summary.txt
```

---

## Node Management

### Adding a Storage Node

**Estimated Time:** 10-15 minutes

**Prerequisites:**
- Hardware provisioned and network configured
- Node meets kernel/OS requirements (Ubuntu 24.04+ with kernel 6.20+)
- SSH access configured
- Sufficient capacity in cluster for rebalancing

**Procedure:**

```bash
# 1. Generate cluster join token (1-time use, expires in 1 hour)
TOKEN=$(cfs admin cluster-token --node-type storage)
echo $TOKEN  # Save for step 4

# 2. Provision the node with cloud-init (or manually)
aws ec2 run-instances \
  --image-id ami-ubuntu-24.04 \
  --instance-type i4i.2xlarge \
  --user-data "#!/bin/bash
    export JOIN_TOKEN=$TOKEN
    ./tools/storage-node-user-data.sh"

# 3. Wait for node to start (2-3 minutes)
# Monitor: aws ec2 describe-instances --query 'Reservations[].Instances[].State.Name'

# 4. Verify node availability
cfs admin node-status --pending

# 5. Trigger rebalancing (async, takes 10-60 minutes depending on data size)
cfs admin rebalance --target <new-node-id>

# 6. Monitor rebalancing progress
cfs admin rebalance-status --watch
# Press Ctrl+C to stop watching

# 7. Verify rebalancing complete
cfs admin raft-status | grep "leader_count"
# All shards should be re-replicated across new node
```

**Verification:**

```bash
# Node is healthy and participating
cfs admin node-status <node-id>
# Expected: Status: HEALTHY, Role: STORAGE, Shards: ~86 (256/3 nodes)

# All Raft groups healthy
cfs admin raft-status | grep "UNHEALTHY"
# Expected: (empty output = all healthy)

# Metadata distributed
cfs admin shard-stats | grep -c "healthy"
# Expected: 256 (all shards)
```

**Rollback (if needed):**

```bash
# Drain the node first
cfs admin node-drain <node-id> --migrate-data

# Wait for draining
cfs admin node-status <node-id> | grep "State"
# Expected: State: DRAINED

# Remove from cluster
cfs admin node-remove <node-id>

# Terminate instance
aws ec2 terminate-instances --instance-ids <instance-id>
```

### Removing a Storage Node (Graceful Drain)

**Estimated Time:** 30-60 minutes (depends on data size)

**Purpose:** Prepare node for maintenance or decommissioning

**Procedure:**

```bash
# 1. Start graceful drain (moves data off node to other nodes)
cfs admin node-drain <node-id> --timeout 1h

# 2. Monitor draining progress
cfs admin node-status <node-id> --watch
# Look for: Drained percentage increasing to 100%

# 3. Wait for completion or timeout
# If drain completes: proceed to step 4
# If timeout: data stays on node, can force-remove (data loss)

# 4. Verify data is distributed
cfs admin shard-distribution <node-id>
# Expected: "0 shards (node removed)"

# 5. Remove node from cluster
cfs admin node-remove <node-id> --confirmed

# 6. Terminate instance (if in cloud)
aws ec2 terminate-instances --instance-ids <instance-id>
```

**Verification:**

```bash
# Cluster still healthy
cfs admin status | grep "Status: HEALTHY"

# Remaining nodes have 50% more shards (3→2 nodes)
cfs admin shard-stats | grep "replicas"
# Expected: All shards have 2 replicas (3→2 nodes means reduced redundancy)
```

### Node Failure Detection & Recovery

**Automatic Recovery (No Action Needed):**

The cluster automatically detects and recovers from node failures:

1. **Detection Phase (0-5 seconds):**
   - SWIM gossip detects node is offline
   - Raft groups on failed node lose heartbeats
   - Leader election triggered

2. **Recovery Phase (5-60 seconds):**
   - New Raft leaders elected
   - Data re-replicated from surviving replicas
   - Client redirects to new leaders

3. **Rebalancing (1-24 hours, depending on data size):**
   - Background process re-replicates data across cluster
   - Maintains EC 4+2 striping

**Operator Verification:**

```bash
# Check failed node recovery
cfs admin node-status <failed-node-id>
# Expected: Status: DOWN (age: 15s), Recovery: IN_PROGRESS

# Monitor recovery completion
cfs admin rebalance-status
# Expected: Progress bar showing rebalancing percentage

# Verify cluster healthy
cfs admin raft-status | grep -c "LEADER"
# Expected: 256 (all Raft groups have leaders = healthy)
```

---

## Cluster Scaling

### Horizontal Scale-Out (Add Nodes)

**When to Scale Out:**
- Storage capacity > 80%
- IOPS/throughput approaching limits
- Preparing for predicted growth

**Procedure:**

Follow [Adding a Storage Node](#adding-a-storage-node) × N

**Best Practices:**
- Add nodes in groups of 3 (maintains Raft quorum stability)
- Allow rebalancing to complete between additions (avoids network storms)
- Monitor replication lag during scale-out

### Horizontal Scale-In (Remove Nodes)

**When to Scale In:**
- Cluster is oversized for current workload
- Cost optimization needed

**Procedure:**

1. Follow [Removing a Storage Node](#removing-a-storage-node) for each node
2. Verify cluster remains healthy after each removal
3. Allow rebalancing between removals

**Risks:**
- Reduces redundancy until rebalancing completes
- May trigger data migrations (high disk I/O)

### Vertical Scale-Up (Upgrade Node Hardware)

**When to Upgrade:**
- Hot node (high latency, high CPU)
- NVMe capacity exhausted

**Procedure:**

```bash
# 1. Start graceful drain
cfs admin node-drain <node-id> --timeout 1h

# 2. Wait for drain to complete
cfs admin node-status <node-id> --watch

# 3. Stop the node
cfs admin node-stop <node-id>

# 4. Upgrade hardware (resize instance, upgrade NVMe)
# For AWS EC2:
aws ec2 stop-instances --instance-ids <instance-id>
# [Upgrade hardware in AWS console]
aws ec2 start-instances --instance-ids <instance-id>

# 5. Wait for node to boot (2-3 minutes)
sleep 180

# 6. Rejoin cluster
cfs admin node-rejoin <node-id>

# 7. Monitor recovery
cfs admin rebalance-status --watch
```

---

## Maintenance & Updates

### Kernel & OS Updates

**Frequency:** Monthly patch updates, yearly major updates

**Procedure (Rolling Update):**

```bash
# 1. Set maintenance window (announce to users)
cfs admin set-maintenance-window --start "2026-03-15T02:00Z" --duration 4h

# 2. Remove first node from cluster
cfs admin node-drain node-1 --timeout 1h

# 3. Stop ClaudeFS on node
ssh node-1 "sudo systemctl stop claudefs"

# 4. Apply updates
ssh node-1 "sudo apt-get update && apt-get upgrade -y"

# 5. Reboot if needed
ssh node-1 "sudo reboot"

# 6. Wait for boot (2-3 minutes)
sleep 180

# 7. Rejoin cluster
cfs admin node-rejoin node-1

# 8. Monitor recovery
cfs admin rebalance-status --watch

# 9. Repeat for nodes 2, 3, etc.

# 10. Exit maintenance window
cfs admin clear-maintenance-window
```

**Verification:**

```bash
# All nodes updated
cfs admin node-status --all | grep "kernel_version"

# Cluster healthy
cfs admin status | grep "Status: HEALTHY"
```

### ClaudeFS Binary Updates

**Types:**
- Patch (bug fixes, minor features) — can do rolling update
- Minor (new features) — usually backward-compatible
- Major (schema changes) — requires full restart

**Procedure (Rolling Update for Patch/Minor):**

```bash
# 1. Update binaries on one node
ssh node-1 "cd /opt/claudefs && curl -O latest-binary && chmod +x cfs"

# 2. Stop ClaudeFS on that node
ssh node-1 "sudo systemctl stop claudefs"

# 3. Wait for clients to reconnect (5-10 seconds)
sleep 10

# 4. Start with new binary
ssh node-1 "sudo systemctl start claudefs"

# 5. Monitor for errors
cfs admin node-status node-1 --watch

# 6. Repeat for other nodes
```

**Procedure (Full Restart for Major):**

```bash
# 1. Announce maintenance window
cfs admin set-maintenance-window --start "2026-03-15T02:00Z" --duration 2h

# 2. Gracefully stop all nodes
cfs admin cluster-stop --graceful --timeout 5m

# 3. Update binaries everywhere
for node in $(cfs admin node-list); do
  ssh $node "cd /opt/claudefs && curl -O latest-binary && chmod +x cfs"
done

# 4. Start all nodes
cfs admin cluster-start

# 5. Monitor cluster bootstrap (5-30 seconds)
watch -n 1 "cfs admin status"

# 6. Clear maintenance window
cfs admin clear-maintenance-window
```

---

## Emergency Procedures

### Breakglass Access (Emergency Recovery)

**When to Use:** Cluster is down/unavailable, normal access procedures failing

**Procedure:**

```bash
# 1. Get emergency access certificate from AWS Secrets Manager
aws secretsmanager get-secret-value --secret-id cfs/breakglass-cert \
  --region us-west-2 | jq -r .SecretString > /tmp/breakglass.crt

# 2. SSH to node using emergency key
ssh -i /path/to/emergency-key.pem -o "StrictHostKeyChecking=no" ubuntu@node-1

# 3. Use breakglass certificate for cluster commands
export CLAUDEFS_CERT=/tmp/breakglass.crt
cfs admin status

# 4. Perform recovery operations as needed
# See: Metadata Corruption Recovery, Full Cluster Recovery, etc.
```

**Important:**
- Breakglass access is audited (all commands logged)
- Certificate valid only for 1 hour
- Must file incident report after use

### Metadata Corruption Detection & Recovery

**Symptoms:**
- Inconsistent file metadata (stat returns different results)
- Files appearing/disappearing unexpectedly
- Raft state machine corrupted

**Detection:**

```bash
# Automatic detection (runs hourly)
cfs admin fsck --detect-corruption

# Manual check if suspected
cfs admin fsck --mode read-only
```

**Recovery (from latest snapshot):**

```bash
# 1. Identify corruption age
cfs admin fsck --detect-corruption --json | jq '.corruption_age'

# 2. Find latest clean snapshot
cfs admin snapshot list --json | jq -r '.[] | select(.verified==true) | .name' | head -1

# 3. Initiate recovery from snapshot
cfs admin metadata-recover --from-snapshot <snapshot-name> \
  --confirmed \
  --no-client-access

# 4. Monitor recovery
cfs admin metadata-recover --status --watch

# 5. When recovery complete, clients can reconnect
cfs admin allow-client-access

# 6. Verify integrity
cfs admin fsck --mode read-only
```

**RPO:** Data loss = time since last snapshot (default: daily)

### Full Cluster Failure Recovery

**When Needed:** Complete data loss, all nodes down, unrecoverable

**Prerequisites:**
- S3 backend has data (cache mode) or cross-site replica
- Backup snapshots available

**Recovery Procedure (from S3 + Backup):**

```bash
# 1. Provision new cluster infrastructure (same spec as original)
# See docs/production-deployment.md for topology

# 2. Initialize first node
cfs server bootstrap \
  --cluster-name claudefs-prod-1 \
  --initial-nodes 3 \
  --shards 256

# 3. Add additional nodes
TOKEN=$(cfs admin cluster-token --node-type storage)
# [Join nodes 2 and 3 with TOKEN]

# 4. Wait for cluster to stabilize
watch -n 1 "cfs admin status"

# 5. Restore from S3 backend
cfs admin restore-from-s3 \
  --s3-bucket claudefs-backups \
  --recovery-point latest \
  --timeout 12h

# 6. Monitor restoration
cfs admin restore-status --watch

# 7. Verify data integrity
cfs admin fsck --mode read-only

# 8. Enable client access
cfs admin allow-client-access
```

**ETA:** 2-12 hours depending on cluster size and data volume

### Raft Quorum Loss (Majority Down)

**Symptoms:**
- Cluster reports "QUORUM_LOST"
- Raft groups cannot elect leaders
- No metadata operations possible

**Automatic Recovery (if 2+ of 3 come back):**
- Quorum re-established
- Leader re-elected
- Metadata service resumes

**Manual Recovery (all 3 permanently down):**

```bash
# 1. Assess permanent damage
# If > 1/3 of data nodes permanently down: must use backup recovery

# 2. Force recovery from known-good state (DANGEROUS - data loss possible)
cfs admin force-raft-recovery \
  --recovery-point last-known-good \
  --skip-verification \
  --confirmed

# 3. Monitor cluster restart
watch -n 1 "cfs admin status"

# 4. Verify data integrity
cfs admin fsck
```

**RPO:** From last Raft log checkpoint (typically < 1 min)

---

## Debugging & Log Analysis

### Enabling Debug Logging

**Procedure:**

```bash
# Temporary (until node restart)
cfs admin set-log-level --module claudefs::metadata --level DEBUG

# Persistent (survives restart)
ssh node-1 "sudo sed -i 's/RUST_LOG=info/RUST_LOG=debug/g' /etc/systemd/system/claudefs.service"
ssh node-1 "sudo systemctl daemon-reload && systemctl restart claudefs"
```

### Collecting Diagnostic Bundle

**Purpose:** Full system state for analysis

**Procedure:**

```bash
# Generate diagnostic bundle
cfs admin diagnostic-bundle --node <node-id> > /tmp/claudefs-diagnostic.tar.gz

# Contents include:
# - ClaudeFS logs (last 24h)
# - System metrics (CPU, memory, disk)
# - Raft state dumps
# - TCP connection states
# - Process status
# - Kernel logs

# For multi-node issue
for node in $(cfs admin node-list); do
  cfs admin diagnostic-bundle --node $node > /tmp/diag-$node.tar.gz
done
```

### Analyzing High Latency

**Procedure:**

```bash
# 1. Check distributed traces
cfs admin trace-search --operation create_file --percentile 99 --last 1h

# 2. Identify slow path
# Look for: Which component (RPC, disk I/O, Raft consensus) is slow?

# 3. Check resource utilization
cfs admin metrics --node <node-id> \
  --metric "cpu_usage,memory_usage,disk_io_latency,network_latency"

# 4. Profile slow operation
cfs admin profile --operation <op> --duration 30s

# 5. Generate flamegraph
cfs admin flamegraph > /tmp/claudefs-flame.svg
# Open in browser to visualize call stack
```

### Analyzing High CPU

**Procedure:**

```bash
# 1. Identify hot process
cfs admin process-top --node <node-id>

# 2. Profile with perf
cfs admin perf-profile --duration 30s --cpu

# 3. Generate flamegraph
cfs admin perf-generate-flamegraph --output /tmp/cpu-flame.svg

# 4. Check for pathological workload
cfs admin workload-analysis --node <node-id>
# Look for: Excessive dedup work, compression, GC
```

---

## Performance Tuning

### CPU Optimization

**High-CPU Symptoms:**
- CPU usage > 70% sustained
- Latency > 20ms p99

**Tuning Steps:**

```bash
# 1. Check for dedup/compression overhead
cfs admin metrics | grep "dedupe_cpu_pct,compress_cpu_pct"

# 2. Adjust dedup aggressiveness
cfs admin config set dedupe.mode=selective  # Instead of "always"
cfs admin config set dedupe.min_file_size 1M  # Skip small files

# 3. Reduce compression for high-concurrency workloads
cfs admin config set compression.level=1  # Instead of 6

# 4. Enable CPU affinity
cfs admin config set cpu.affinity=true
cfs admin config set cpu.io_threads=$(nproc --all)

# 5. Verify improvement
cfs admin metrics | grep "cpu_usage"
```

### Disk I/O Optimization

**High-Latency Symptoms:**
- I/O latency > 5ms p99
- Raft write latency > 10ms

**Tuning Steps:**

```bash
# 1. Check NVMe queue depth
cfs admin hardware-stats | grep "nvme_queue_depth"

# 2. Increase queue depth
cfs admin config set nvme.queue_depth=128  # From 32

# 3. Enable write combining
cfs admin config set raft.write_combining=true
cfs admin config set data.write_combining=true

# 4. Verify improvement
cfs admin metrics | grep "io_latency"
```

### Network Optimization

**High-Latency Symptoms:**
- RPC latency > 2ms p99
- Replication lag > 100ms

**Tuning Steps:**

```bash
# 1. Check network health
cfs admin network-health | grep "latency,packet_loss"

# 2. Enable TCP no-delay
cfs admin config set network.tcp_nodelay=true

# 3. Increase socket buffer sizes
cfs admin config set network.tcp_sndbuf=16M  # Increase from 4M
cfs admin config set network.tcp_rcvbuf=16M

# 4. Enable RDMA if available
cfs admin config set transport.enable_rdma=true

# 5. Verify improvement
cfs admin metrics | grep "rpc_latency"
```

---

## Backup & Recovery

### Daily Backup Procedure (Automated)

**Frequency:** Daily at 02:00 UTC

**What's Backed Up:**
- Metadata snapshots (KV store + Raft log)
- Configuration files
- Cluster keys (CA certificate, node certificates)

**Procedure (Manual Trigger):**

```bash
# Trigger backup
cfs admin backup create \
  --destination s3://claudefs-backups \
  --retention 30d \
  --compress

# Monitor
cfs admin backup-status --watch

# Verify backup
cfs admin backup list | tail -5
```

### Point-in-Time Recovery

**RPO Target:** < 1 minute

**Procedure:**

```bash
# 1. List available backups
cfs admin backup list --json | jq '.[].timestamp'

# 2. Select target recovery point
BACKUP_ID="2026-03-15T02:15Z"

# 3. Restore metadata to point in time
cfs admin metadata-restore --from-backup $BACKUP_ID \
  --no-client-access \
  --confirmed

# 4. Monitor restoration
cfs admin metadata-restore --status --watch

# 5. Verify data integrity
cfs admin fsck

# 6. Enable client access
cfs admin allow-client-access
```

---

## Metrics & Alert Interpretation

### Critical Alerts

| Alert | Meaning | Action |
|-------|---------|--------|
| `raft_leader_unavailable` | No leader for Raft group | Check node status, restart if hung |
| `replication_lag_high` | Cross-site lag > 5 min | Check network, increase replication workers |
| `metadata_corruption_detected` | fsck found inconsistency | Initiate metadata recovery |
| `flash_usage_critical` | Storage > 95% | Evict to S3 or scale out |
| `node_down` | Node offline > 5 min | Check hardware, network, manually recover if needed |

### Performance Metrics

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| `create_file_latency_p99` | < 10ms | 10-50ms | > 50ms |
| `metadata_replicate_latency_p99` | < 5ms | 5-20ms | > 20ms |
| `io_read_latency_p99` | < 2ms | 2-10ms | > 10ms |
| `replication_lag_s3` | < 30s | 30s-5min | > 5min |
| `cpu_usage_avg` | < 50% | 50-70% | > 70% |

### Custom Query Examples

```bash
# Last hour: create operation latency percentiles
cfs admin query --metric operation_latency \
  --operation create \
  --window 1h \
  --percentiles "50,99,99.9"

# Last 24h: disk I/O trend
cfs admin query --metric io_ops_total \
  --window 24h \
  --interval 1h

# Per-node: replication lag
cfs admin query --metric replication_lag \
  --by node
```

---

## Escalation Procedures

### Level 1: On-Call Operator

**Handles:**
- Health checks (all green)
- Capacity warnings (> 75%)
- Performance alerts (latency > 50ms)
- Node temporarily down

**Actions:**
- Execute runbooks from this guide
- Collect diagnostic data
- Escalate if not resolved in 15 min

### Level 2: Senior SRE / Engineer

**Handles:**
- Raft quorum loss
- Metadata corruption
- Rebalancing failures
- Performance mysteries (root cause analysis)

**Actions:**
- Deep debugging (see [Debugging & Log Analysis](#debugging--log-analysis))
- Code review if suspected bug
- Escalate if not resolved in 1 hour

### Level 3: Engineering Team Lead

**Handles:**
- Full cluster failure
- Major data loss incidents
- Architecture-level issues
- Post-incident review

**Actions:**
- Invoke disaster recovery procedures
- Coordinate multi-team response
- File incident with timeline and root cause

---

## Appendix: Quick Command Reference

```bash
# Cluster status
cfs admin status                        # Overall health
cfs admin node-status --all             # All nodes
cfs admin raft-status                   # Raft groups
cfs admin shard-stats                   # Metadata distribution

# Scaling
cfs admin cluster-token                 # Generate join token
cfs admin node-drain <id> --timeout 1h  # Prepare for removal
cfs admin rebalance-status              # Monitor data movement

# Maintenance
cfs admin set-log-level --level DEBUG   # Enable debug logs
cfs admin diagnostic-bundle             # Collect all diagnostics
cfs admin fsck                          # Check metadata integrity

# Backup & Recovery
cfs admin backup create                 # Manual backup
cfs admin metadata-recover              # From snapshot
cfs admin restore-from-s3               # Full cluster restore

# Performance
cfs admin metrics --json                # Current metrics
cfs admin profile --duration 30s        # CPU/memory profile
cfs admin flamegraph                    # Visualization

# Debugging
cfs admin trace-search --operation <op> # Find slow operations
cfs admin perf-profile                  # Performance profiling
```

---

**Last Updated:** 2026-03-01
**Maintained By:** A11 Infrastructure & CI
**Review Frequency:** Quarterly
**Escalation Contact:** cfs-oncall@company.com
