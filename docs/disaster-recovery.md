# ClaudeFS Disaster Recovery Guide

**Phase 3 Operations:** Comprehensive disaster recovery procedures, RTO/RPO targets, and recovery processes for all failure scenarios.

---

## Disaster Recovery Overview

ClaudeFS is designed for **high availability** with minimal data loss. The distributed architecture means most failures don't cause data loss—only loss of performance or access.

### Availability Tiers

| Failure Scenario | Affected | RTO | RPO | Automated? |
|------------------|----------|-----|-----|-----------|
| Single node (data) | Replication factor | 2 min | 0 | Yes |
| Single node (Raft) | Leader re-election | 5 sec | 0 | Yes |
| Majority quorum loss | Site inaccessible | 30 min | 1 min | Manual |
| Full site loss | Failover to other site | 5 min | 5 min | Yes |
| Metadata corruption | Data accessible, but corrupted | 1 hour | Minutes (snapshot) | Manual |
| S3 backend loss (cache mode) | Rebuild from flash | N/A | 0 | Partial |
| Complete cluster loss | Restore from backup + S3 | 2+ hours | 1 min | Manual |

---

## RTO and RPO Targets

### RTO (Recovery Time Objective)

**Goal:** Time from failure detection to service restoration

| Failure | RTO | Method |
|---------|-----|--------|
| Single storage node | 2 min | Automatic Raft re-election |
| Raft leader loss | 5 sec | Automatic new leader elected |
| Client disconnection | < 1 sec | Client auto-reconnect |
| Temporary network partition | 3 min | Quorum re-established |
| Permanent node loss | 10 min | Data re-replicated from peers |
| Full single-site loss | 5 min | Failover to secondary site |
| Metadata corruption | 1 hour | Restore from snapshot + journal replay |
| Complete cluster loss | 2+ hours | Rebuild from S3 + journal |

### RPO (Recovery Point Objective)

**Goal:** Maximum acceptable data loss

| Failure | RPO | Why |
|---------|-----|-----|
| Single node (Raft) | 0 sec | Raft replicates synchronously |
| Single node (data) | 1 min | EC data on multiple nodes |
| Cross-site (async replication) | 5 min | Replication lag tolerance |
| S3 backend loss (cache) | 0 (full redundancy in flash) | Data in 3+ places |
| Metadata backup snapshot | 24 hours | Daily backup to S3 |

---

## Failure Scenarios and Recovery

### Scenario 1: Single Storage Node Failure

**Symptoms:**
- Node goes offline (network error, hardware failure, OS crash)
- Client sees latency spike, then successful reconnect to other nodes
- Raft leader election may trigger (if leader failed)

**RTO:** 2 minutes (automatic)
**RPO:** 0 seconds (data safe on other replicas)

**Detection:**
```bash
cfs admin status
# Output: Node "storage-1" is DOWN (detected by SWIM, age 15s)

# Check Raft status
cfs admin raft-status
# Shard 001: Leader=storage-2 (re-elected, 5s ago)
```

**Automatic Recovery:**
1. SWIM gossip detects failure (3–5 sec)
2. Raft groups with no leader trigger new election
3. Other replicas continue serving traffic
4. Clients reconnect automatically

**Manual Verification:**
```bash
# 1. Wait for automatic recovery
sleep 30

# 2. Verify cluster is healthy
cfs admin status

# 3. Check if downed node can be brought back
ssh -i ~/.ssh/cfs-key ec2-user@storage-1 "sudo systemctl status cfs-storage"

# 4. If not recoverable, remove permanently:
cfs admin node-drain storage-1
cfs admin node-remove storage-1
```

**Prevention:**
- Monitor node CPU, memory, disk space
- Set up health check alerts
- Configure automatic restart on crash (systemd)

---

### Scenario 2: Raft Leader Loss (Within Site)

**Symptoms:**
- Temporary increase in metadata operation latency
- No data loss
- New leader elected within 5 seconds

**RTO:** 5 seconds (automatic)
**RPO:** 0 seconds (Raft replicated)

**Detection:**
```bash
# Leader crash detected by heartbeat timeout
cfs admin raft-status
# Shard 001: Leader=None (election in progress)
# (after 5 sec)
# Shard 001: Leader=storage-3 (new leader elected)
```

**Automatic Recovery:**
1. Raft nodes detect leader gone (heartbeat timeout 150–300 ms)
2. Candidate nodes start election
3. Node with highest term wins (usually most recent follower)
4. New leader begins accepting writes immediately

**Manual Recovery (if election stalls):**
```bash
# Check election status
cfs admin raft-debug --shard 001

# Force new election (last resort)
cfs admin raft-force-election --shard 001
```

**Prevention:**
- Increase heartbeat frequency (reduces election time)
- Monitor Raft latency, leader transitions
- Alert on multiple leader losses (indicates instability)

---

### Scenario 3: Majority Quorum Loss (2 of 3 Nodes Down)

**Symptoms:**
- All metadata operations fail: "Leader not available"
- Data on remaining node is inaccessible
- Cluster is effectively offline

**RTO:** 30 minutes (manual intervention)
**RPO:** 1 minute (journal available on surviving node)

**Prevention (Better than Recovery!):**
```bash
# 1. Ensure automated backups are running
cfs admin snapshot-metadata --interval 1h --destination s3://backups

# 2. Monitor cluster health
cfs admin raft-status  # Should show 3 healthy nodes

# 3. Set up alerts for node failures
# Alert when any node is down > 5 minutes
```

**Recovery Option 1: Bring Back Lost Nodes**
```bash
# Fastest if nodes are recoverable (hardware hiccup)

# 1. Reboot node 1
ssh -i ~/.ssh/cfs-key ec2-user@storage-1 "sudo reboot"

# 2. Wait for boot
sleep 30

# 3. Start ClaudeFS daemon
ssh -i ~/.ssh/cfs-key ec2-user@storage-1 "sudo systemctl start cfs-storage"

# 4. Check Raft recovery
cfs admin raft-status
```

**Recovery Option 2: Force Leader on Surviving Node**
```bash
# Use only if other nodes are permanently lost

# 1. Check which node is still alive
cfs admin status
# Output: storage-3 is UP

# 2. DANGER: Force storage-3 as leader (may lose data from nodes 1 & 2)
# This is DANGEROUS - only do if nodes 1 & 2 are confirmed destroyed
cfs admin raft-force-leader --shard * --leader storage-3

# 3. Verify cluster operates (with 1-node quorum)
cfs admin status

# 4. Bring up new nodes immediately
# Run terraform to provision new nodes
terraform apply

# 5. Add new nodes to cluster
cfs node add --node storage-1-new
cfs node add --node storage-2-new

# 6. Verify full quorum restored
cfs admin raft-status
```

**Recovery Option 3: Restore from Snapshot**
```bash
# If you have an older snapshot and can tolerate data loss

# 1. Download snapshot from S3
aws s3 cp s3://cfs-backups/metadata/metadata-20260228.tar.gz /tmp/

# 2. Create brand new cluster from scratch (or on disaster recovery site)
cfs cluster init --bootstrap-node storage-1-new \
  --cluster-name claudefs-prod-dr

# 3. Restore metadata from snapshot
cfs admin restore-metadata --from-snapshot /tmp/metadata-20260228.tar.gz

# 4. Verify consistency
cfs admin metadata-consistency-check

# 5. Replay recent journal entries (if available)
cfs admin replay-journal --from <timestamp>
```

---

### Scenario 4: Full Site Failure (All 3 Nodes Down)

**Symptoms:**
- Complete cluster unavailable
- All clients see connection errors
- Secondary site (if configured) should take over

**RTO:** 5 minutes (automatic failover if 2-site setup)
**RPO:** 5 minutes (cross-site replication lag)

**Single-Site Setup (No Failover):**
```bash
# 1. Determine root cause
# - Hardware failure? Contact data center
# - Network issue? Check firewall, DNS, routes
# - Software crash? Check logs

# 2. Wait for manual recovery
# - Replace faulty hardware
# - Restart nodes from backup AMI

# 3. Bring cluster back up
for node in storage-1 storage-2 storage-3; do
  aws ec2 start-instances --instance-ids <id>
done

# 4. Wait for boot and service start (5 minutes)

# 5. Verify cluster recovers automatically
cfs admin status
cfs admin raft-status
```

**Multi-Site Setup (With Failover):**
```bash
# 1. Detect primary site failure
cfs admin replication-status
# Output: Primary site unreachable, Secondary is current leader

# 2. Failover is automatic (if configured)
# Secondary site continues operating
# Clients may experience brief reconnect

# 3. Applications can continue on secondary site
# RTO is automatic (< 5 sec), RPO is replication lag (typically < 5 min)

# 4. Once primary is recovered, sync back with secondary:
cfs admin replication-sync --source secondary --target primary

# 5. Switch leadership back to primary (optional)
cfs admin failback --from secondary --to primary
```

---

### Scenario 5: Metadata Corruption

**Symptoms:**
- Checksum failures: "Inode checksum mismatch: expected 0x123, got 0x456"
- Operations fail with "data integrity error"
- Data may be readable but metadata is corrupt

**RTO:** 1 hour (restore from snapshot)
**RPO:** 1 day (if daily snapshots)

**Root Causes:**
- Hardware error (bit flip in RAM or storage)
- Bug in metadata operation (code defect)
- Incomplete write followed by crash
- Incorrect fsync semantics

**Detection:**
```bash
# Run consistency check
cfs admin metadata-consistency-check

# Output indicates problems:
# Inode 12345: corrupted (expected_size=1024, actual=512)
# Inode 67890: invalid parent pointer
```

**Recovery:**
```bash
# 1. Identify scope of corruption
cfs admin metadata-consistency-check --verbose > corruption-report.txt

# 2. If fixable, auto-repair
cfs admin metadata-repair --auto-fix

# 3. Verify repair succeeded
cfs admin metadata-consistency-check

# 4. If auto-repair fails, restore from snapshot
#    (this means losing recent operations)

# Download latest snapshot
aws s3 ls s3://cfs-backups/metadata/ | tail -5
# 2026-02-28: metadata-20260228.tar.gz
# 2026-02-29: metadata-20260229.tar.gz  (corrupt snapshot!)
# 2026-02-28: metadata-20260228.tar.gz  (use this, 1 day older)

# Restore from clean snapshot
cfs admin restore-metadata --from-snapshot \
  /tmp/metadata-20260228.tar.gz

# Verify
cfs admin metadata-consistency-check
```

**Prevention:**
- Enable checksums on all metadata writes
- Regular consistency checks (daily)
- Snapshot metadata daily
- Monitor hardware health (SMART, memtest)
- Keep backups on multiple storage media

---

### Scenario 6: Network Partition (Site A ↔ Site B Split)

**Symptoms:**
- Cross-site replication lag = infinite
- Both sites continue operating (within their own quorum)
- Divergent writes create conflicts

**RTO:** 0 seconds (both sites operational)
**RPO:** N/A (writes were successful on both sides, conflicts on merge)

**Automatic Behavior (Split-Brain Safe):**
```
Before partition:
  Site A: [inode 123: name="file.txt", modtime=t1, leader=storage-1]
  Site B: [inode 123: name="file.txt", modtime=t1, replica]

Network partition occurs (30 sec)

Scenario 1: Modify on Site A
  Site A: [inode 123: name="file-a.txt", modtime=t2, leader=storage-1]
  Site B: [inode 123: name="file.txt", modtime=t1, replica] (stale)

Scenario 2: Modify on Site B (different attribute)
  Site A: [inode 123: attr.owner=alice, modtime=t3]
  Site B: [inode 123: attr.owner=bob, modtime=t4]

Partition heals (after 2 min)

On heal:
  Site A and B sync metadata
  LWW conflict resolution applies
  cfs admin show-conflicts reports:
    - Inode 123: Site A won (modtime=t2 > t1)
    - Inode 456: CONFLICT - attr.owner differs
      Site A: owner=alice (t3)
      Site B: owner=bob (t4)
```

**Detection:**
```bash
# Check replication status
cfs admin replication-status
# Cross-site lag: unreachable (partition detected)

# After partition heals
cfs admin replication-status
# Lag resolving... [████████░░]

# Check for conflicts
cfs admin show-conflicts
```

**Recovery:**
```bash
# 1. Examine conflicts
cfs admin show-conflicts --verbose

# 2. Resolve using LWW (automatic)
cfs admin resolve-conflicts --strategy last-write-wins

# 3. For manual resolution
cfs admin resolve-conflicts --strategy manual \
  --inode 456 --winner site-a  # Use Site A's version

# 4. Verify both sites converge
cfs admin metadata-consistency-check --cross-site
```

**Prevention:**
- Prefer writing on primary site only (reduce divergence)
- Monitor network partition detection
- Set up alerts for replication lag > 10 sec
- Document conflict resolution policy

---

### Scenario 7: S3 Backend Unavailable (Cache Mode)

**Symptoms:**
- S3 writes fail (timeout or auth error)
- Local flash continues operating (flash acts as cache)
- Alert: "S3 write queue depth > 1M"

**RTO:** 0 seconds (continue with flash)
**RPO:** Depends on S3 recovery, data is safe in flash

**Behavior:**
```
Normal operation:
  Client write → Flash (ack) → Background S3 write

S3 outage:
  Client write → Flash (ack) → S3 write fails, enqueue
  Keep writing to flash until high watermark (80% full)

  At high watermark: Switch to write-through mode
  Client write → Flash → WAIT for S3 write → ack
  (Slower but no data loss)

Critical (95% full):
  Alert: "Flash critical, S3 unreachable, write-through active"

At 100% full:
  Return ENOSPC (no space) to clients
  Stop accepting writes until S3 recovers
```

**Recovery:**
```bash
# 1. Diagnose S3 issue
cfs admin s3-health
# Output: S3 unreachable, last successful write 30 min ago

# 2. Check if issue is auth, network, or S3 service
aws s3 ls --region us-west-2
# If this hangs or fails, S3 is down

# 3. Wait for S3 to recover (or switch to backup S3)
cfs admin s3-failover --target s3://backup-bucket

# 4. Monitor S3 write queue
cfs admin s3-queue-status
# Queue depth: 500k entries, flushing at 100k/min

# 5. Once S3 is healthy, queue drains automatically
```

**Prevention:**
- Use S3 with cross-region replication for higher availability
- Monitor S3 connectivity from cluster
- Have backup S3 bucket ready (different region)
- Set up alerts for S3 write lag > 5 min

---

### Scenario 8: Complete Cluster Loss (Total Disaster)

**Symptoms:**
- All nodes simultaneously destroyed (data center fire, ransomware, etc.)
- No recovery possible without backups
- Only S3 backend remains

**RTO:** 2+ hours (rebuild from S3)
**RPO:** 1 day (if daily snapshots) or up to 5 min (if hourly snapshots)

**Prerequisites (Backup Strategy):**
```bash
# 1. Daily metadata snapshots to S3
cfs admin snapshot-metadata --output s3://cfs-backups/metadata-$(date +%Y%m%d).tar.gz

# 2. Application data is already on S3 (cache mode) or mirrored (tiered mode)

# 3. Backup cluster configuration
cfs admin export-config > s3://cfs-backups/config-$(date +%Y%m%d).json
```

**Complete Rebuild:**
```bash
# 1. Provision new cluster (manual or via terraform)
cfs-dev up --topology prod-cluster.yaml

# 2. Download last known good snapshot
aws s3 cp s3://cfs-backups/metadata-20260228.tar.gz /tmp/

# 3. Initialize new cluster from scratch
cfs cluster init --bootstrap-node storage-1-new \
  --cluster-name claudefs-prod-restored

# 4. Restore metadata from backup
cfs admin restore-metadata --from-snapshot /tmp/metadata-20260228.tar.gz

# 5. Data is automatically recovered from S3 (cache mode)
# (Data in tiered mode: use cfs admin restore-data --from-s3)

# 6. Verify integrity
cfs admin metadata-consistency-check --verbose

# 7. Replay any journal entries after backup timestamp
# (if journal was backed up separately)

# 8. Clients reconnect, resume operations
```

**Time Breakdown:**
- Provision new cluster: 15 min
- Initialize Raft: 5 min
- Restore metadata: 10 min
- Verify consistency: 5 min
- Client reconnection: 1 min
- **Total: ~36 minutes**

**Minimize RTO:**
- Keep standby cluster in another region (hot backup)
- Automate rebuild with CloudFormation/Terraform
- Pre-stage AMIs with all dependencies

---

## Backup and Restore Procedures

### Backup Strategy

| Component | Frequency | Method | Retention | Location |
|-----------|-----------|--------|-----------|----------|
| Metadata | Daily | Snapshot tar.gz | 30 days | S3 + local |
| KV Store | Daily | Dump to Parquet | 30 days | S3 |
| Audit Logs | Continuous | Stream to S3 | 3 years | S3 + Glacier |
| Application Data | Automatic (cache mode) | Async S3 writes | N/A | S3 (primary) |

### Metadata Backup Procedure
```bash
#!/bin/bash
# Daily backup script (cron: 0 2 * * * /opt/backup-metadata.sh)

BACKUP_DATE=$(date +%Y%m%d)
BACKUP_FILE="/tmp/metadata-$BACKUP_DATE.tar.gz"

# 1. Create snapshot from Raft leader
cfs admin snapshot-metadata --output $BACKUP_FILE

# 2. Verify integrity
tar -tzf $BACKUP_FILE > /dev/null || exit 1

# 3. Upload to S3 with encryption
aws s3 cp $BACKUP_FILE s3://cfs-backups/metadata/ \
  --sse AES256 \
  --metadata "date=$BACKUP_DATE,cluster=prod"

# 4. Cleanup (keep local copy for 7 days)
find /backups -name "metadata-*.tar.gz" -mtime +7 -delete

# 5. Verify S3 upload
aws s3 ls s3://cfs-backups/metadata/ | grep $BACKUP_DATE || exit 1

echo "Backup successful: $BACKUP_FILE"
```

### Restore Procedure
```bash
#!/bin/bash
# Restore from backup

BACKUP_FILE=${1:-/tmp/metadata-latest.tar.gz}

# 1. Verify backup integrity
tar -tzf $BACKUP_FILE > /dev/null || exit 1

# 2. (Optional) If restoring to existing cluster: snapshot current state first
cfs admin snapshot-metadata --output /tmp/metadata-pre-restore.tar.gz

# 3. Restore metadata
cfs admin restore-metadata --from-snapshot $BACKUP_FILE

# 4. Verify restoration
cfs admin metadata-consistency-check

# 5. Restore application data (if needed)
# For cache mode: automatic (data already on S3)
# For tiered mode: cfs admin restore-data --from-s3

echo "Restore complete"
```

---

## Disaster Recovery Testing

### Monthly DR Drill
```bash
# Schedule: First Saturday of each month, 2 AM UTC (low-traffic window)

# 1. Take full cluster snapshot
cfs admin snapshot-metadata --output /tmp/dr-test-metadata.tar.gz
cfs admin s3-snapshot --output s3://cfs-backups/dr-test-data

# 2. Provision temporary "DR cluster" (small, for testing)
terraform apply -var="cluster_name=cfs-dr-test"

# 3. Restore from snapshot
cfs admin restore-metadata --from-snapshot /tmp/dr-test-metadata.tar.gz

# 4. Verify data
cfs admin metadata-consistency-check
find /mnt/recovery-test -type f | head -100 | xargs md5sum

# 5. Run smoke tests
pjdfstest -c create_file -c open -c write /mnt/recovery-test

# 6. Document issues, update runbooks
# 7. Tear down temporary cluster
terraform destroy -var="cluster_name=cfs-dr-test"
```

### Annual Full Failover Test (Multi-Site Only)
```bash
# Once per year: test complete failover to secondary site

# 1. Coordinate with team, plan maintenance window (4 hours)
# 2. Block all new writes to primary site
cfs admin failover-prepare --source primary --target secondary

# 3. Verify secondary can be promoted to primary
cfs admin failover-test --target secondary

# 4. If tests pass, perform actual failover
cfs admin failover --from primary --to secondary

# 5. Verify secondary is now serving all traffic
cfs admin status

# 6. Once satisfied, failback to primary
cfs admin failback --from secondary --to primary

# 7. Re-sync any divergent data
cfs admin replication-sync --source secondary --target primary
```

---

## Disaster Recovery Checklist

- [ ] Backup strategy documented (frequency, retention)
- [ ] Automated backups running (metadata daily, logs continuous)
- [ ] Backup retention enforced (S3 lifecycle policy)
- [ ] Backup integrity verified (weekly test restore)
- [ ] RTO/RPO targets defined and communicated
- [ ] Recovery procedures documented for all scenarios
- [ ] Emergency contacts and escalation paths documented
- [ ] Breakglass access procedures tested
- [ ] Firewall rules documented for disaster recovery site
- [ ] Alternative S3 bucket provisioned (for S3 failover)
- [ ] Terraform code version-controlled (infrastructure as code)
- [ ] SSH keys distributed securely (not in code)
- [ ] Monthly backup restore test scheduled
- [ ] Annual failover drill scheduled (multi-site)
- [ ] Incident response plan reviewed with team
- [ ] Runbooks published and accessible to operations team

