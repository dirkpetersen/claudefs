# ClaudeFS Replication Procedures

## Step-by-Step Failover

### Prerequisites Checklist

Before triggering failover, verify:
- [ ] Current site is truly unreachable (not network flapping)
- [ ] Replica site is healthy and accessible
- [ ] No ongoing writes that would be lost
- [ ] Client applications can handle failover
- [ ] Monitoring is in place to track event

### Commands to Execute

#### 1. Verify Current State

```bash
# Check current replication state
$ cfs admin repl status

Output:
Site A: ActiveReadWrite (healthy)
Site B: StandbyReadOnly (healthy)
Replication lag: 2s
```

#### 2. Trigger Failover

```bash
# Trigger manual failover from Site A to Site B
$ cfs admin failover trigger --from-site=A --to-site=B --reason="primary unreachable"

Output:
Failover initiated
Estimated time: <5s
```

#### 3. Monitor Progress

```bash
# Watch failover progress
$ cfs admin failover watch --timeout=30s

Output:
[14:32:01] Phase 1: Detecting site failure... OK (1.2s)
[14:32:02] Phase 2: Quorum consensus... OK (0.8s)
[14:32:03] Phase 3: Metadata switchover... OK (1.1s)
[14:32:04] Phase 4: Client reconnection... OK (1.8s)
[14:32:05] Failover completed successfully
```

#### 4. Verify New Primary

```bash
# Verify Site B is now primary
$ cfs admin site mode --site=B

Output:
Site B: ActiveReadWrite (primary)
```

### Verification Steps

1. **Write Test**
   ```bash
   $ cfs admin repl test-write
   
   Output: Write confirmed on new primary
   ```

2. **Replication Restart**
   ```bash
   $ cfs admin repl lag
   
   Output: Lag: 0s (catching up)
   ```

3. **Client Connectivity**
   ```bash
   $ cfs admin client status
   
   Output: 3 clients connected, all healthy
   ```

### Rollback Procedures

If failover fails or issues occur:

```bash
# Cancel failover (if still in progress)
$ cfs admin failover cancel

# Rollback to original primary (if clients had issues)
$ cfs admin failover rollback --to-site=A

# Force original site offline (if it's causing issues)
$ cfs admin site offline --site=A --force
```

---

## Manual Recovery Procedures

### Bringing a Failed Site Back Online

#### Scenario: Site B was offline, now network is restored

```bash
# 1. Verify site is reachable
$ cfs admin site ping --site=B

Output: Site B reachable (latency: 5ms)

# 2. Register site with cluster
$ cfs admin site add --site=B --address=10.0.1.5:9000

Output: Site B registered

# 3. Verify health
$ cfs admin site health --site=B

Output: Site B: healthy

# 4. Verify catch-up
$ cfs admin repl catchup-status --site=B

Output: Catching up: 500 entries behind
```

#### Recovery Timeline

| Phase | Time |
|-------|------|
| Network restore | ~30s |
| Site registration | ~5s |
| Health verification | ~5s |
| Journal catch-up | 30s-5min (depends on lag) |

### Catching Up a Lagging Site

#### Scenario: Site B is 60s behind Site A

```bash
# 1. Check current lag
$ cfs admin repl lag --site=B

Output: Lag: 60s (1000 entries)

# 2. Trigger aggressive catch-up
$ cfs admin repl catchup --site=B --mode=aggressive

Output:
[14:35:01] Starting aggressive catch-up
[14:35:02] Transfer rate: 50MB/s
[14:35:45] Caught up: 0 entries behind

# 3. Verify sync complete
$ cfs admin repl status

Output: Both sites synchronized
```

#### Catch-up Modes

| Mode | Use Case | Speed |
|------|----------|-------|
| Normal | Behind <5min | 10MB/s |
| Aggressive | Behind 5-30min | 50MB/s |
| Force | Behind >30min | 100MB/s (may impact primary) |

### Force-Syncing Journals

If journals are severely out of sync:

```bash
# Force journal sync from primary to replica
$ cfs admin repl force-sync --source=A --target=B

Output:
[14:40:01] Starting force sync
[14:40:15] Transferred 500MB
[14:40:16] Verifying integrity... OK
[14:40:17] Force sync complete
```

### Handling Persistent Failures

If site repeatedly fails health checks:

```bash
# 1. Investigate root cause
$ cfs admin site diagnostics --site=B

Output:
- Disk latency: 50ms (high)
- Network errors: 12 (intermittent)
- Memory pressure: 80%

# 2. Take site offline
$ cfs admin site offline --site=B

# 3. Resolve underlying issue (disk/network/memory)

# 4. Bring site back online
$ cfs admin site online --site=B
```

---

## Disaster Recovery (Dual-Site Failure)

### Scenario: Both Sites Down

When both sites are unreachable, recover from S3-backed copy:

#### 1. Verify S3 Connectivity

```bash
$ cfs admin s3 check-connection

Output:
S3 endpoint: https://s3.us-west-2.amazonaws.com
Bucket: claudefs-replication
Connection: OK
Latest snapshot: 2026-04-18T14:30:00Z
```

#### 2. List Available Snapshots

```bash
$ cfs admin disaster-recovery list-snapshots

Output:
2026-04-18T14:30:00Z (size: 500GB)
2026-04-18T12:00:00Z (size: 480GB)
2026-04-18T09:00:00Z (size: 450GB)
```

#### 3. Restore from Snapshot

```bash
$ cfs admin disaster-recovery restore \
    --source=s3 \
    --snapshot=2026-04-18T14:30:00Z \
    --target-site=A

Output:
[14:45:00] Starting restore from S3
[14:45:30] Downloaded metadata: 50GB
[14:46:00] Downloaded journals: 200GB
[14:46:30] Downloaded data: 250GB
[14:47:00] Restore complete
[14:47:01] Verification: OK
```

#### 4. Bring Site Online

```bash
$ cfs admin site online --site=A

$ cfs admin repl status

Output:
Site A: ActiveReadWrite (recovered from S3)
Site B: Offline (awaiting recovery)
```

### Timeline Expectations

| Phase | Time |
|-------|------|
| S3 connectivity check | 10s |
| Metadata download (50GB) | 30s |
| Journal download (200GB) | 2-5min |
| Data download (250GB) | 3-10min |
| Verification | 30s |
| **Total** | **5-15 minutes** |

### Data Verification

After restore, verify data integrity:

```bash
# Verify metadata consistency
$ cfs admin verify metadata --site=A

Output: Metadata consistent

# Verify journal integrity
$ cfs admin verify journal --site=A

Output: Journal integrity OK

# Run checksum validation
$ cfs admin verify checksums

Output: All checksums match
```

### Recovery from Backup

If S3 is also unavailable, use local backup:

```bash
# List local backups
$ cfs admin backup list

Output:
/backup/claudefs-20260418.tar.gz (500GB)
/backup/claudefs-20260417.tar.gz (480GB)

# Restore from local backup
$ cfs admin disaster-recovery restore \
    --source=local \
    --path=/backup/claudefs-20260418.tar.gz
```

---

## Validation Checklists

### Post-Failover Validation

- [ ] Write capability confirmed on new primary
- [ ] Replication restarted and catching up
- [ ] No split-brain state detected
- [ ] Client connections successful
- [ ] Metrics showing normal operation
- [ ] Failover event logged in audit trail

```bash
# Quick validation
cfs admin repl verify-write
cfs admin repl status
cfs admin failover history --last=1
```

### Post-Recovery Validation

- [ ] Site health check passing
- [ ] Replication lag at zero
- [ ] No errors in logs
- [ ] Write quorum achievable
- [ ] Metadata synchronized

```bash
# Quick validation
cfs admin site health --site=B
cfs admin repl lag --site=B
cfs admin repl verify-reconciliation
```

### Health Check Verification

Run periodic health checks:

```bash
# Manual health check
$ cfs admin site health --all

Output:
Site A: healthy
  - Disk latency: 5ms
  - Memory: 60%
  - Network: 100Mbps
Site B: healthy
  - Disk latency: 6ms
  - Memory: 55%
  - Network: 100Mbps

# Automated health check (cron)
0 */5 * * * cfs admin health-check --alert-if-failed
```

### Performance Baseline Validation

Establish and verify performance baselines:

```bash
# Record baseline
$ cfs admin metrics baseline record

Output: Baseline recorded at 2026-04-18T14:00:00Z

# Compare current to baseline
$ cfs admin metrics baseline compare

Output:
- Write latency: +5% (within tolerance)
- Replication lag: +2s (within tolerance)
- Throughput: -3% (within tolerance)
```

---

## Emergency Procedures

### Immediate Failover (No Time for Analysis)

```bash
# Force failover immediately
$ cfs admin failover force --to-site=B

# Skip validation for speed
$ cfs admin failover force --to-site=B --no-validate
```

### Stop Replication (Maintenance Mode)

```bash
# Pause replication
$ cfs admin repl pause

# ... perform maintenance ...

# Resume replication
$ cfs admin repl resume
```

### Emergency Split-Brain Resolution

```bash
# Immediate resolution without analysis
$ cfs admin repl resolve-split-brain --strategy=auto --force
```

### Emergency Shutdown (Both Sites)

```bash
# Graceful shutdown
$ cfs admin shutdown --graceful

# Emergency shutdown (force)
$ cfs admin shutdown --emergency
```

---

## Contact & Escalation

For issues not covered by these procedures:

1. **Level 1**: Check logs at `/var/log/claudefs/`
2. **Level 2**: Contact infrastructure team
3. **Level 3**: Escalate to on-call engineer

```
Pager: +1-555-0199
Email: claudefs-ops@company.com
Slack: #claudefs-operations
```