# A6 Replication — Operational Runbook

**Owner:** Agent A6 (Replication)
**Last Updated:** 2026-03-01
**Audience:** Operations, DevOps, System Administrators
**Status:** Production Ready

---

## Quick Reference

### Healthy Cluster Indicators
- Replication lag <500ms on all sites
- 0 conflicts in last hour
- All sites HEALTHY in health dashboard
- No errors in replication logs

### Emergency Procedures
- **Site down:** Automatic failover triggered within 5 seconds
- **Replication stuck:** Check network connectivity, restart conduit service
- **Split-brain detected:** Manual intervention required, see split-brain section

---

## Pre-Deployment Checklist

Before deploying ClaudeFS with replication in production:

- [ ] **NTP/PTP Setup:** Ensure <100ms clock skew across all sites
  ```bash
  # Verify clock sync
  chronyc tracking  # on each node
  # All "RMS offset" should be <100ms
  ```

- [ ] **Network Connectivity:** Verify all sites can reach each other on port 9500
  ```bash
  # From site A to site B
  nc -zv site-b 9500
  # Expected: Connection to site-b:9500 succeeded!
  ```

- [ ] **TLS Certificates:** Verify mTLS certificates are deployed and valid
  ```bash
  # Check certificate validity
  cfs admin tls status
  # All certificates should have expiry >30 days
  ```

- [ ] **Monitoring Setup:** Prometheus scraping A6 metrics endpoint
  ```bash
  # Check Prometheus config includes A6
  grep "claudefs-repl" /etc/prometheus/prometheus.yml
  ```

- [ ] **Disk Space:** Ensure 20% free space on metadata partition
  ```bash
  df -h /var/claudefs
  # Used <80%
  ```

- [ ] **Memory:** Verify sufficient RAM for replication (min 2GB recommended)
  ```bash
  free -h
  # Available should be >2GB
  ```

---

## Day 1: Deployment

### Step 1: Deploy Primary Site (Site A)

```bash
# 1. Stop any existing claudefs-repl service
systemctl stop claudefs-repl

# 2. Start new replication engine
systemctl start claudefs-repl

# 3. Check status
cfs admin replication status
# Expected output:
# Site: site-a (LOCAL)
# Status: READY
# Role: PRIMARY
# Lag: 0ms
# Peers: site-b [DOWN] (expected on first deploy)
```

### Step 2: Deploy Replica Site (Site B)

```bash
# 1. On site B, start replication engine (connecting to site-a)
systemctl start claudefs-repl

# 2. Enroll as replica (this syncs from site A)
cfs admin replication enroll --primary site-a

# 3. Monitor enrollment progress
cfs admin replication bootstrap-status
# Expected: Phase transitions: IDLE → ENROLLING → SNAPSHOT → CATCHUP → COMPLETE

# 4. When complete, verify sync
cfs admin replication status
# Expected output:
# Site: site-b (LOCAL)
# Status: READY
# Role: REPLICA
# Lag: <100ms
# Peers: site-a [HEALTHY]
```

### Step 3: Verify Both Sites

```bash
# On both sites, check status
for site in site-a site-b; do
    echo "=== $site ==="
    ssh $site "cfs admin replication status"
done

# Expected: Both READY, lag <500ms
```

---

## Daily Operations

### Morning Check (Every Day)

```bash
#!/bin/bash
# Check replication health

echo "=== Replication Status ==="
cfs admin replication status

echo ""
echo "=== Replication Lag ==="
cfs admin replication lag

echo ""
echo "=== Recent Conflicts (last hour) ==="
cfs admin replication audit --since "1 hour ago" --filter "conflict" | wc -l

echo ""
echo "=== Site Health ==="
cfs admin replication health

echo ""
echo "=== Journal Size ==="
du -h /var/claudefs/journal

echo ""
echo "=== Prometheus Metrics ==="
curl -s http://localhost:9090/api/v1/query?query='claudefs_replication_lag_ms' | jq '.data.result'
```

**Expected output:**
- Status: READY on all sites
- Lag: <500ms on all sites
- Conflicts: 0 or very low
- All sites: HEALTHY
- Journal size: <10GB (should stabilize)

### Periodic Tasks

**Every 4 hours:**
```bash
# Check for stale journal entries
cfs admin replication journal-status
# Expected: compaction_pending = false

# If compaction pending, trigger manually
cfs admin replication gc --aggressive
```

**Every 24 hours:**
```bash
# Check audit trail for anomalies
cfs admin replication audit --since "24 hours ago" | grep -E "ERROR|CONFLICT" | tail -20

# Review replication metrics
cfs admin replication metrics summary
```

**Every week:**
```bash
# Full health check
cfs admin replication health --detailed

# Verify TLS certificates not expiring soon
cfs admin tls status --warn-days 30

# Test manual failover (in staging)
cfs admin replication failover --to site-b --dry-run
```

---

## Monitoring Dashboard

### Key Metrics to Watch

| Metric | Healthy | Warning | Critical |
|--------|---------|---------|----------|
| Replication lag | <500ms | 500-2000ms | >2000ms |
| Error rate | 0/min | <1/min | >1/min |
| Conflicts/hour | 0-5 | 5-20 | >20 |
| Network latency | <50ms | 50-200ms | >200ms |
| Journal size | <5GB | 5-10GB | >10GB |
| CPU per repl task | <5% | 5-10% | >10% |
| Memory per repl task | <100MB | 100-300MB | >300MB |

### Grafana Dashboard Queries

```promql
# Replication lag (per site)
claudefs_replication_lag_ms{site="site-a"}

# Error rate
rate(claudefs_replication_errors_total[5m])

# Throughput (entries/sec)
rate(claudefs_replication_entries_processed[1m])

# Network bandwidth (bytes/sec)
rate(claudefs_replication_bytes_sent[1m])
```

### Alerting Rules

```yaml
# prometheus.yml alert rules for replication

- alert: ReplicationLagHigh
  expr: claudefs_replication_lag_ms > 5000
  for: 5m
  annotations:
    summary: "Replication lag > 5 seconds"

- alert: ReplicationErrorRate
  expr: rate(claudefs_replication_errors_total[5m]) > 1
  for: 1m
  annotations:
    summary: "Replication errors detected"

- alert: SplitBrainDetected
  expr: claudefs_replication_split_brain == 1
  for: 0m
  annotations:
    summary: "CRITICAL: Split-brain detected"
```

---

## Troubleshooting Guide

### Issue 1: High Replication Lag (>2 seconds)

**Symptoms:** Dashboard shows lag increasing, clients noticing stale reads

**Diagnosis:**
```bash
# Check network connectivity
ping -c 5 site-b
# latency should be <50ms

# Check conduit service status
systemctl status claudefs-repl --full
# should be active (running)

# Check error logs
journalctl -u claudefs-repl -n 100 | grep -i error

# Check compression ratio
cfs admin replication metrics | grep compression_ratio
# if <0.5, consider disabling compression
```

**Solutions:**
1. **Network latency high:** Contact network team, check switch logs
2. **Conduit service down:** Restart it: `systemctl restart claudefs-repl`
3. **Compression too aggressive:** Disable or switch algorithm:
   ```bash
   cfs admin replication config --compression none
   # Restart to apply
   systemctl restart claudefs-repl
   ```
4. **Primary overloaded:** Check A2 write load, may need to scale

### Issue 2: Write Conflicts Detected

**Symptoms:** `conflict_count > 0` in metrics, conflicts in audit trail

**Diagnosis:**
```bash
# View conflicts
cfs admin replication audit --filter "conflict" --tail 10

# Check if due to clock skew
chronyc tracking
# look for "RMS offset" - if >100ms, clock skew is issue

# Check active-active mode
cfs admin replication config
# look for "active_active_mode"
```

**Solutions:**
1. **Clock skew issue:** Fix NTP/PTP, resync clocks
2. **Active-active conflict:** Expected in active-active mode, verify LWW is working
3. **Partition healed:** Conflicts during partition heal are expected

**If conflicts continue:**
```bash
# Review which inode has most conflicts
cfs admin replication audit --filter "conflict" | jq -r '.inode_id' | sort | uniq -c | sort -rn

# Check that inode on both sites
cfs admin meta lookup <inode_id>
# compare result across sites
```

### Issue 3: Replication Stuck (Not Making Progress)

**Symptoms:** Lag increasing linearly, no entries being replicated

**Diagnosis:**
```bash
# Check cursor position (should be increasing)
cfs admin replication cursor status

# Check if blocked on compression
cfs admin replication metrics | grep compression_queue_size
# if >100, compression is bottleneck

# Check if blocked on network
netstat -an | grep 9500
# look for CLOSE_WAIT or ESTABLISHED connections

# Check TLS handshake
cfs admin tls status
# certificates should be valid
```

**Solutions:**
1. **Conduit process hung:** Kill and restart
   ```bash
   systemctl restart claudefs-repl
   ```

2. **Compression bottleneck:** Disable compression
   ```bash
   cfs admin replication config --compression none
   ```

3. **Network stuck:** Check firewall rules, restart network
   ```bash
   # Verify port 9500 is open
   sudo ufw allow 9500
   # Restart replication service
   systemctl restart claudefs-repl
   ```

4. **TLS certificate issue:** Regenerate and redeploy
   ```bash
   cfs admin tls renew-certificates
   # Restart all replication services
   ```

### Issue 4: Split-Brain Detected

**Symptoms:** `claudefs_replication_split_brain == 1`, error messages about fencing

**This is a CRITICAL issue requiring immediate action.**

**Diagnosis:**
```bash
# Confirm split-brain
cfs admin replication status --detailed

# Check network partition
ping -c 5 site-b
# will likely fail if in split-brain

# Check fencing token
cfs admin replication split-brain status
# look for "fenced=true"
```

**Recovery Procedure:**
1. **Identify which site should be primary** (usually the one with more recent writes)
2. **Manually force resolution:**
   ```bash
   # On the WRONG site (to demote it):
   cfs admin replication failover --force --to site-a

   # Wait 5 seconds for network to heal
   sleep 5

   # Verify recovery
   cfs admin replication status
   ```

3. **Verify data consistency:**
   ```bash
   # On primary
   cfs admin meta consistency-check

   # On replica
   cfs admin meta consistency-check

   # Compare results (should match)
   ```

4. **If data inconsistency remains:**
   ```bash
   # Full rebuild from primary to replica
   cfs admin replication enroll --primary site-a --force-rebuild
   ```

### Issue 5: Audit Trail Bloated (Old Events Not Being Purged)

**Symptoms:** `/var/claudefs/audit` growing, disk space warning

**Diagnosis:**
```bash
# Check audit retention policy
cfs admin replication config | grep audit_retention

# Check audit trail size
du -sh /var/claudefs/audit

# Check oldest entry
cfs admin replication audit --oldest
```

**Solution:**
```bash
# Run garbage collection
cfs admin replication gc --audit-only

# Or adjust retention policy (e.g., 30 days)
cfs admin replication config --audit-retention 30d

# Restart to apply
systemctl restart claudefs-repl
```

---

## Scaling Operations

### Adding a Third Site (Site C)

```bash
# 1. On site C, initialize replication engine
systemctl start claudefs-repl

# 2. Enroll as replica
cfs admin replication enroll --primary site-a

# 3. Monitor enrollment
cfs admin replication bootstrap-status

# 4. When complete, verify
cfs admin replication status

# 5. On all sites, verify peer list updated
cfs admin replication health
# should show site-a, site-b, site-c
```

### Removing a Site

```bash
# 1. On the site to remove
cfs admin replication shutdown --graceful

# 2. On other sites, verify removal
cfs admin replication health
# should no longer show the removed site

# 3. Monitor lag to ensure replication still working
cfs admin replication lag
```

---

## Disaster Recovery

### Scenario 1: Primary Site Down

**Expected behavior:**
- Automatic failover triggered within 5 seconds
- Replica site (B) promotes to PRIMARY
- Clients redirected to site B

**Operator actions:**
```bash
# On site B (new primary)
cfs admin replication status
# Should show: Role: PRIMARY, Peers: site-a [DOWN]

# Investigate what happened on site A
ssh site-a
systemctl status claudefs
journalctl -u claudefs -n 50

# Once site A is fixed and online
cfs admin replication rejoin --primary site-b
# Site A will sync from site B as new replica
```

### Scenario 2: Complete Cluster Failure

**If both sites go down:**

```bash
# 1. Bring primary site (A) up first
systemctl start claudefs-repl

# 2. Verify data integrity
cfs admin meta consistency-check

# 3. Bring replica site (B) online
systemctl start claudefs-repl

# 4. Let B catch up from A
# This should happen automatically, monitor:
cfs admin replication lag

# 5. Once lag <100ms, both are healthy
```

### Scenario 3: Data Corruption on One Site

**If inode corruption detected:**

```bash
# 1. Identify which site has correct version
cfs admin meta lookup <inode_id> --detailed
# Compare output across both sites

# 2. On corrupted site, force sync from healthy site
cfs admin replication resync --inode <inode_id>

# 3. Monitor repair progress
cfs admin replication audit --filter "resync" --tail 20

# 4. Verify repair completed
cfs admin meta consistency-check
```

---

## Performance Tuning

### Reducing Replication Latency

```bash
# 1. Check current configuration
cfs admin replication config

# 2. Optimize batch settings
cfs admin replication config --batch-size 1000  # increase from default 500
cfs admin replication config --batch-timeout-ms 50  # decrease from default 100

# 3. Disable compression if not compressible data
cfs admin replication config --compression none

# 4. Increase worker threads (if CPU available)
cfs admin replication config --worker-threads 8

# 5. Restart to apply
systemctl restart claudefs-repl

# 6. Monitor latency improvement
cfs admin replication lag
```

### Reducing Network Bandwidth

```bash
# 1. Enable compression (if data is compressible)
cfs admin replication config --compression lz4

# 2. Enable journal compaction
cfs admin replication gc --aggressive

# 3. Monitor bandwidth savings
cfs admin replication metrics | grep bandwidth
```

---

## Maintenance Windows

### Planned Replication Service Restart

```bash
# 1. Announce maintenance to users
# 2. On secondary site, restart (will cause temporary failover)
systemctl restart claudefs-repl

# 3. Monitor failover completion
cfs admin replication status
# should show PRIMARY role moved to site A

# 4. On primary site, restart
systemctl restart claudefs-repl

# 5. Monitor convergence
cfs admin replication lag
# should show lag going back to normal

# 6. When both sides healthy, announce maintenance complete
```

### Rolling Certificate Update

```bash
# 1. Generate new certificates
cfs admin tls renew-certificates

# 2. On each site, restart replication service
for site in site-a site-b; do
    ssh $site "systemctl restart claudefs-repl"
    sleep 2
    ssh $site "cfs admin replication status"
done

# 3. Verify full cluster recovered
cfs admin replication health
```

---

## Contacts & Escalation

- **A6 Replication Owner:** Agent A6
- **Repository:** https://github.com/dirkpetersen/claudefs
- **Crate:** `claudefs-repl`
- **Emergency:** Check Split-Brain section first

---

## Appendix: Common Commands

```bash
# Status checks
cfs admin replication status
cfs admin replication lag
cfs admin replication health

# Operational
cfs admin replication failover --to site-b
cfs admin replication enroll --primary site-a
cfs admin replication resync --inode <id>

# Monitoring
cfs admin replication metrics
cfs admin replication audit --since "1 hour ago"

# Configuration
cfs admin replication config
cfs admin replication config --compression lz4
cfs admin replication config --batch-size 1000

# Troubleshooting
journalctl -u claudefs-repl -n 100 -f
cfs admin replication debug --trace
cfs admin meta consistency-check
```
