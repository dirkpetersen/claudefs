# ClaudeFS Debugging & Troubleshooting Runbook

**For:** Operations team, SREs, and agent troubleshooters
**Updated:** 2026-04-17

---

## Quick Decision Tree

**Problem**

→ **Choose category:**

- [Build Failures](#build-failures) — Cargo errors, compile errors
- [Test Failures](#test-failures) — Unit tests, integration tests, flaky tests
- [Cluster Health](#cluster-health) — Node down, replication lag, high latency
- [Performance Degradation](#performance-degradation) — Throughput down, latency up
- [Replication Issues](#cross-site-replication-lag) — Multi-site sync problems
- [Agent Crashes](#agent-session-crashes) — Agent tmux session died

---

## Build Failures

### Symptom: `cargo build` fails locally

**Step 1: Check error message**

```bash
cargo build 2>&1 | grep "^error:"
```

**Step 2: Classify error**

| Error Pattern | Cause | Fix |
|---|---|---|
| `error[E0433]: unresolved module` | Missing module import | Add `mod` or `use` statement |
| `error[E0425]: cannot find value` | Undefined variable/function | Check spelling, import scope |
| `error[E0308]: mismatched types` | Type mismatch | Review function signature, cast if needed |
| `error: failed to resolve` | Dependency issue | Run `cargo update`, check `Cargo.lock` |
| `error: linking with ...` failed | Library not found | Install system dependency |

**Step 3: Auto-fix if possible**

```bash
# For simple errors
cargo fix --allow-dirty

# For type errors (manual)
cargo build 2>&1 | head -20  # Read the full error
# ... manually fix the code ...
cargo build
```

**Step 4: If stuck >30 minutes**

- Check if supervisor is running: `/opt/cfs-supervisor.sh`
- Check OpenCode fixes log: `tail -50 /var/log/cfs-agents/opencode-fixes.log`
- Create GitHub issue with full error output

### Symptom: CI build fails but local passes

**Likely cause:** Environment difference (build cache, dependencies)

**Solution:**

```bash
# 1. Clear build cache locally
rm -rf target/

# 2. Rebuild
cargo build --release

# 3. If still fails: report to A11
# If passes: likely GitHub cache issue, force refresh in Actions tab
```

### Symptom: Compilation takes >20 minutes

**Likely cause:** Cache miss or large incremental rebuild

**Solution:**

```bash
# Check if it's incremental or clean
cargo build -vv 2>&1 | grep -E "(Compiling|Fresh)"  # "Fresh" = cached, "Compiling" = rebuilding

# If mostly "Compiling": cache miss
# Clear and rebuild:
rm -rf target/
cargo build --release -j 4  # Limit parallelism to avoid OOM

# If stuck: kill and investigate
pkill -f cargo
# Check disk space: df -h
# Check RAM: free -h
```

---

## Test Failures

### Symptom: `cargo test` fails

**Step 1: Reproduce locally**

```bash
# Run with single thread for consistent output
cargo test --lib -- --test-threads=1

# Run specific test
cargo test -p claudefs-storage io_depth_limiter -- --nocapture
```

**Step 2: Check for flakiness**

```bash
# Run 5 times in a row
for i in {1..5}; do
  cargo test --lib --quiet || break
done

# If fails inconsistently: flaky test
# If fails consistently: real bug
```

**Step 3: Debug output**

```bash
# Add RUST_BACKTRACE for stack traces
RUST_BACKTRACE=1 cargo test --lib -- --nocapture

# Use test output
cargo test --lib -- --nocapture 2>&1 | grep -A 5 "thread.*panicked"
```

**Step 4: Isolate the failure**

```bash
# Test just the failing module
cargo test -p claudefs-storage io_depth

# Run with verbose to see all assertions
cargo test -p claudefs-storage io_depth -- --nocapture

# Check the test code
cat crates/claudefs-storage/src/io_depth_limiter.rs | grep -A 20 "#\[test\]"
```

### Symptom: Tests pass locally but fail in CI

**Likely cause:** Timing-sensitive test, async/await issue, or race condition

**Solution:**

```bash
# 1. Run multiple times
cargo test --lib

# 2. Run with cargo-nextest (catches race conditions)
cargo install cargo-nextest
cargo nextest run --lib

# 3. If still fails in CI: check CI logs for environment differences
# (e.g., slower runner, different timing)

# 4. Add explicit waits/timeouts if timing-sensitive
tokio::time::sleep(Duration::from_millis(100)).await;
tokio::time::timeout(Duration::from_secs(5), operation).await?;
```

### Symptom: "test panicked" but no clear error

**Check panic message**

```bash
cargo test --lib -- --nocapture 2>&1 | grep -B 5 "panicked"
```

**Common panic messages:**

| Panic | Likely Cause | Fix |
|---|---|---|
| `assertion failed` | assert! failed | Check assertion condition |
| `unwrap()` called on None | Expected Some, got None | Add error handling |
| `index out of bounds` | Vector/slice access error | Check bounds |
| `lock poisoned` | Mutex/RwLock deadlock | Review lock scope |

### Symptom: Test timeout (>60 sec)

**Likely cause:** Deadlock, infinite loop, or hanging I/O

```bash
# Kill the test
pkill -f "cargo test"

# Run with timeout to catch it faster
timeout 10 cargo test --lib

# Check for infinite loops in test code
grep -n "loop\|while true" crates/*/src/lib.rs
```

---

## Cluster Health

### Symptom: Node appears offline

**Step 1: Check node reachability**

```bash
# SSH to orchestrator
ssh cfs@orchestrator

# List instances
aws ec2 describe-instances --filters Name=tag:role,Values=storage \
  --query 'Reservations[].Instances[].{id:InstanceId,state:State.Name}'

# Check specific node
aws ec2 get-console-output --instance-id i-xxxxx | tail -20
```

**Step 2: Check ClaudeFS daemon**

```bash
# SSH to node
ssh ubuntu@storage-node-1

# Check if daemon running
ps aux | grep -E "cfs|daemon"

# Check logs
journalctl -u claudefs -n 50

# Restart daemon
systemctl restart claudefs
```

**Step 3: Check networking**

```bash
# From orchestrator
ping storage-node-1
nc -zv storage-node-1 9000  # Check RPC port

# Check firewall rules
aws ec2 describe-security-groups
```

### Symptom: Node in "degraded" state (high latency)

**Check node metrics**

```bash
# Check CPU
ssh ubuntu@storage-node-1 "top -bn1 | head -10"

# Check memory
ssh ubuntu@storage-node-1 "free -h"

# Check disk I/O
ssh ubuntu@storage-node-1 "iostat -x 1 3"

# Check network
ssh ubuntu@storage-node-1 "iftop -n -t -c 1"
```

**Recovery actions:**

```bash
# If high CPU: reduce worker threads
ssh ubuntu@storage-node-1 "cfs admin set-worker-threads 4"

# If high memory: flush caches
ssh ubuntu@storage-node-1 "sync && echo 3 | sudo tee /proc/sys/vm/drop_caches"

# If high disk I/O: check for stuck operations
ssh ubuntu@storage-node-1 "cfs admin show-io-operations"
```

### Symptom: Raft quorum lost

**Check Raft status**

```bash
# On any storage node
cfs admin show-raft-status

# Output should show:
# Node 1: leader
# Node 2: follower
# Node 3: follower
```

**If quorum lost (e.g., 1 node up, 2 down):**

```bash
# Check which nodes are down
cfs admin show-node-status

# Option 1: Recover from backup (if available)
cfs repair --from-backup backup.tar.gz

# Option 2: Rebuild quorum (data loss risk!)
# WARNING: Only if you know what you're doing
# Contact A11/SRE before proceeding
cfs admin rebuild-quorum --force
```

---

## Performance Degradation

### Symptom: Throughput dropped 50%

**Step 1: Check node metrics**

```bash
# Sample every 5 seconds for 1 minute
prometheus_query() {
  curl "http://prometheus:9090/api/v1/query?query=$1" | jq '.data.result[].value'
}

prometheus_query 'rate(io_operations_total[1m])'  # Operations/sec
prometheus_query 'rate(storage_bytes_written[1m])'  # Bytes/sec
```

**Step 2: Identify bottleneck**

| Metric | Bottleneck | Fix |
|---|---|---|
| CPU 95%+ | CPU | Reduce parallelism, background tasks |
| Memory 90%+ | RAM | Flush caches, reduce buffer sizes |
| Disk I/O 100% | I/O | Enable SSD, reduce fsync frequency |
| Network 90%+ | Network | Add bandwidth, reduce replication |
| Latency 10s+ | Queueing | Reduce load, add nodes |

**Step 3: Take corrective action**

```bash
# Example: High CPU
ssh ubuntu@node-1

# Reduce worker threads
cfs admin set-worker-threads 2

# Pause background compaction
cfs admin pause-compaction

# Monitor recovery
watch 'cfs admin show-metrics | grep cpu'

# When recovered, restore
cfs admin set-worker-threads 8
cfs admin resume-compaction
```

### Symptom: Latency spike (p99 latency 5s → 10s)

**Check for blocking operations**

```bash
# See which operations are slow
cfs admin show-slow-operations --threshold 1s

# Example output:
# metadata_create: 2.3s
# storage_write: 1.5s
# raft_append: 0.8s

# Focus on slowest operation and trace it
cfs admin trace metadata_create
```

**Common causes:**

```bash
# 1. GC (garbage collection) pause
cfs admin show-gc-status

# 2. Raft fsync (disk sync)
cfs admin show-raft-fsync-latency

# 3. Network latency (check replication lag)
cfs admin show-replication-lag

# 4. Queuing (too much load)
cfs admin show-queue-depth
```

---

## Cross-Site Replication Lag

### Symptom: Replication latency >5 sec (target <1 sec)

**Step 1: Check replication status**

```bash
# On metadata leader (site A)
cfs admin show-replication-status

# Output should show:
# Site A: 0ms (local)
# Site B: <1000ms (target)
```

**Step 2: Identify bottleneck**

```bash
# Check conduit network
cfs admin show-conduit-status

# Check journal queue size
cfs admin show-journal-queue --site-b

# Check site B capacity
ssh ubuntu@site-b-node-1
cfs admin show-replication-backpressure
```

**Step 3: Take corrective action**

```bash
# If network issue:
# Check connectivity
ping site-b-node-1
iperf3 -c site-b-node-1  # Network throughput test

# If site B overloaded:
ssh ubuntu@site-b-node-1
cfs admin set-worker-threads 8
cfs admin set-write-batch-size 100  # Increase batching

# If journal queue large:
# Reduce writes from clients (back-pressure)
# Or scale up site B
```

### Symptom: Replication conflict detected

**Check conflict log**

```bash
# View conflicts
cfs admin show-conflicts

# Conflicts indicate write-write conflicts (last-write-wins)
# Example:
# inode 12345: write at 1000ms from site-a vs 1001ms from site-b
# Resolution: site-b's write won (newer timestamp)
```

**Recovery:**

```bash
# If conflicts are expected (acceptable):
# No action needed (last-write-wins is policy)

# If conflicts are unacceptable:
# 1. Review client write patterns
# 2. Implement application-level conflict resolution
# 3. Document in runbook
```

---

## Agent Session Crashes

### Symptom: Agent tmux session dead

**Check session status**

```bash
tmux list-sessions | grep cfs-a5

# If missing: session crashed
```

**Restart agent**

```bash
# Restart specific agent
/opt/cfs-agent-launcher.sh --agent A5

# Verify
tmux list-sessions | grep cfs-a5

# Tail logs to verify it's running
tail -f ~/claudefs-a5.log
```

**If repeatedly crashing:**

```bash
# Check agent log for error
tail -100 ~/claudefs-a5.log | grep -i error

# Check system resources
ps aux | head -5  # Check memory usage

# If memory issue: restart orchestrator
#  Contact A11 for support
```

---

## Emergency Procedures

### Emergency: Cluster completely down

**Triage**

```bash
# 1. Check AWS account
aws ec2 describe-instances --query 'Reservations[].Instances[].{id:InstanceId,state:State.Name}' | jq

# 2. Check cost-monitor didn't kill instances
grep "killed.*spot" /var/log/cfs-agents/*.log

# 3. Check if manual shutdown occurred
grep "shutdown\|terminate" /var/log/cfs-agents/*.log
```

**Recovery**

```bash
# 1. Provision new cluster
cd /home/cfs/claudefs
cfs-dev up

# 2. Deploy from backup
cfs-dev deploy --from-backup latest

# 3. Verify
cfs-dev status
cfs-dev test --posix  # Quick test
```

### Emergency: Data loss or corruption

**Containment**

```bash
# 1. STOP all writes immediately
cfs admin set-read-only true

# 2. DO NOT restart cluster yet

# 3. Call incident lead
echo "INCIDENT: Data loss detected at $(date)" > /tmp/incident.txt
# ... contact person ...

# 4. Preserve logs/snapshots
tar czf /tmp/claudefs-state-$(date +%s).tar.gz /var/log/cfs-agents
aws s3 cp /tmp/claudefs-state-*.tar.gz s3://claudefs-backups/incidents/
```

**Recovery (SRE only)**

```bash
# Only proceed under incident lead guidance

# Option 1: Restore from backup
cfs-dev restore-from-backup --date 2026-04-16-00-00

# Option 2: Rebuild from EC (erasure coding)
# For read-only data: can reconstruct from EC stripes
cfs admin rebuild-from-ec --verify

# Option 3: Accept data loss and move forward
# (Last resort, only under incident lead approval)
cfs admin accept-data-loss --confirm-uuid <incident-id>
```

---

## Monitoring & Alerting

### Enable Alerts for Key Metrics

**In Prometheus:**

```yaml
groups:
  - name: claudefs.rules
    interval: 30s
    rules:
      # High latency alert
      - alert: HighLatency
        expr: histogram_quantile(0.99, rate(storage_latency_seconds[5m])) > 1
        for: 5m
        annotations:
          summary: "P99 latency >1s for 5 min"

      # Replication lag
      - alert: HighReplicationLag
        expr: max(replication_lag_seconds) > 5
        for: 2m
        annotations:
          summary: "Cross-site replication lag >5s"

      # Node offline
      - alert: NodeOffline
        expr: up{job="storage"} == 0
        for: 1m
        annotations:
          summary: "Storage node offline"
```

**Subscribe to notifications:**

```bash
# In Alertmanager (send to Slack/PagerDuty)
# See docs/OPERATIONS_RUNBOOK.md for alerting setup
```

---

## Performance Profiling

### Profile slow operations

```bash
# Enable detailed tracing
export RUST_LOG=claudefs=debug

cfs daemon

# In another terminal
cfs client mount -target /mnt/data

# Run operation
cd /mnt/data
time dd if=/dev/zero of=testfile bs=1M count=100

# Check logs for slow operations
journalctl -u claudefs | grep -i slow
```

### Use Prometheus for long-term analysis

```bash
# Query latency over time
prometheus_query() {
  curl "http://prometheus:9090/api/v1/query_range?query=$1&start=$(date -d '1 hour ago' +%s)&end=$(date +%s)&step=60" | jq
}

# Get p99 latency over last hour
prometheus_query 'histogram_quantile(0.99, rate(storage_latency_seconds[5m]))'
```

---

## Prevention: Regular Health Checks

**Daily:**

```bash
# Morning check
cfs admin show-cluster-status

# Verify no alerts
curl http://prometheus:9090/api/v1/alerts | jq '.data.alerts | length'  # Should be 0

# Check replication lag
cfs admin show-replication-lag --max-ms 1000  # Should all be <1s
```

**Weekly:**

```bash
# Backup integrity check
cfs-dev backup --verify

# Disaster recovery drill
# (restore from backup to separate environment)
```

**Monthly:**

```bash
# Performance regression check
# Compare metrics to last month's baseline

# Security audit
# Check for CVEs: cargo audit

# Capacity planning
# Review growth trends, plan scaling
```

---

## Reference: Common Error Codes

| Code | Meaning | Action |
|------|---------|--------|
| `ENOENT` | File not found | Check path, verify access |
| `EACCES` | Permission denied | Check auth tokens, RBAC |
| `ENOSPC` | No space left | Check disk usage, tier data to S3 |
| `ETIMEDOUT` | Operation timeout | Check latency, network |
| `EIO` | I/O error | Check node health, disk errors |

---

## Getting Help

**For agent issues:**
- Check `/var/log/cfs-agents/*.log`
- Run `/opt/cfs-supervisor.sh` to auto-fix
- Post in #engineering Slack

**For cluster issues:**
- Follow procedures in this runbook
- Escalate to A11/SRE if blocked

**For production incidents:**
- Page incident lead (PagerDuty)
- Follow `/home/cfs/INCIDENT_RESPONSE_PLAN.md`
- Preserve logs/evidence
- Post-mortem after resolution

---

**Last Updated:** 2026-04-17
**Maintained By:** A11 Infrastructure & CI
**Co-Authored-By:** Claude Haiku 4.5 <noreply@anthropic.com>
