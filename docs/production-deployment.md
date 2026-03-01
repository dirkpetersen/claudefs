# ClaudeFS Production Deployment Guide

**Phase 3 Operations:** Comprehensive procedures for deploying ClaudeFS to production environments.

---

## Production Cluster Topologies

### Small Cluster (3 Storage Nodes)

**Use case:** Single site, high availability, 10–100 TB usable capacity

```
┌─────────────────────────────────────────┐
│        AWS Region (us-west-2a)          │
│                                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐│
│  │ Storage  │  │ Storage  │  │ Storage  ││
│  │ Node 1   │  │ Node 2   │  │ Node 3   ││
│  │ (Raft)   │  │ (Raft)   │  │ (Raft)   ││
│  └──────────┘  └──────────┘  └──────────┘│
│        ▲              ▲              ▲    │
│        │ Raft Consensus (3-way)      │    │
│        └──────────┬──────────────────┘    │
│                   │                       │
│              ┌────────────┐               │
│              │   Client   │               │
│              │ (FUSE v3)  │               │
│              └────────────┘               │
│                   │                       │
│           ┌───────┴────────┐              │
│           │                │              │
│       ┌───────────┐    ┌────────────┐    │
│       │ Prometheus│    │  DuckDB    │    │
│       │Grafana    │    │  Parquet   │    │
│       └───────────┘    └────────────┘    │
└─────────────────────────────────────────┘

         ┌──────────────┐
         │  S3 Backend  │
         │   (Tiering)  │
         └──────────────┘
```

**Specifications:**
- **Compute:** `i4i.2xlarge` (8 vCPU, 64 GB RAM) ×3
- **Storage:** 1.875 TB NVMe per node = 5.625 TB raw per node
- **Network:** 100 Gbps placement group (optimal latency)
- **HA:** Raft 3-way consensus (lose 1, still operational)
- **EC:** 4+2 Reed-Solomon (1.5x overhead) when full
- **Metadata:** 3-way Raft replication (all 3 nodes have full metadata)
- **Total usable:** ~8 TB (after EC overhead) or ~15 TB (cache mode)
- **Monthly cost:** ~$80 (on-demand), ~$35 (spot)

---

### Medium Cluster (5 Storage + 2-Site)

**Use case:** Multi-site replication, 100 TB–1 PB usable, disaster recovery

```
┌─────────────────────────────────┐     ┌─────────────────────────────────┐
│      Site A (us-west-2)         │     │      Site B (us-east-1)         │
│                                 │     │                                 │
│  ┌──────────┬──────────┬──────┐ │     │  ┌──────────┬──────────┐        │
│  │ Storage  │ Storage  │Store │ │     │  │ Storage  │ Storage  │        │
│  │ Node 1   │ Node 2   │ 3    │ │     │  │ Node 4   │ Node 5   │        │
│  │ (Raft)   │ (Raft)   │(Raft)│ │     │  │ (Raft)   │ (Raft)   │        │
│  └──────────┴──────────┴──────┘ │     │  └──────────┴──────────┘        │
│        Raft Consensus (3-way)   │     │   Raft Consensus (2-way)        │
│                                 │     │                                 │
│    Metadata: Primary            │     │   Metadata: Follower (hot)      │
│    Data: Mixed (EC 4+2)         │     │   Data: Replicas                │
│                                 │     │                                 │
└─────────────────────────────────┘     └─────────────────────────────────┘
        │ Journal Replication           │
        └──────────────┬─────────────────┘
                       │
                  ┌────────────┐
                  │   Conduit  │
                  │ (gRPC Mux) │
                  └────────────┘
                       │
              ┌────────┴────────┐
              │                 │
          ┌──────────┐      ┌────────────┐
          │ Clients  │      │ S3 Backend │
          │(FUSE v3) │      │(Tiering)   │
          └──────────┘      └────────────┘
```

**Specifications:**
- **Site A:** 3×`i4i.2xlarge` (Raft leader)
- **Site B:** 2×`i4i.2xlarge` (Raft follower for cross-site quorum)
- **Conduit:** 1×`t3.medium` (gRPC relay)
- **Replication:** Async journal + data push to Site B
- **Failover:** Site B can become primary (RTO ~5 min)
- **Total usable:** ~20 TB (after EC overhead) or ~40 TB (cache mode)
- **Monthly cost:** ~$150 (on-demand), ~$70 (spot)
- **RTO/RPO:** RTO ~5 min single-node, RTO ~2 min cross-site failover; RPO ~1 min

---

### Large Cluster (10+ Nodes, Multiple Regions)

**Use case:** Enterprise deployment, 1–100 PB usable, multi-region redundancy

```
Site A (3 nodes)    Site B (3 nodes)    Site C (4 nodes)
├─ Raft Group 1     ├─ Raft Group 2     ├─ Raft Group 3
└─ Metadata         └─ Metadata         └─ Metadata + Client

Async Replication: A ←→ B ←→ C (full mesh via conduits)
EC Distribution: 4+2 across 3 sites
Metadata Consistency: Within-site strong, cross-site eventual (LWW)
```

**Recommendations:**
- Use **256 virtual shards** (or 1024 for 100+ nodes)
- Each shard: 3-way Raft on different nodes/sites
- Metadata is **fully replicated** across all sites (important!)
- Prefer odd number of nodes per site (3, 5, 7) for Raft quorum
- Consider dedicated NVMe for metadata (separate from data)
- Regional failover requires manual intervention (no automatic cross-region)

---

## Day-1 Operations Checklist

### Pre-Deployment Verification
- [ ] AWS account prepared with appropriate IAM roles and permissions
- [ ] Security groups configured for inter-node communication (Raft, RPC, replication)
- [ ] SSH key pairs generated and stored in AWS Secrets Manager
- [ ] SSL/TLS certificates generated and stored in cluster CA (encrypted)
- [ ] S3 bucket created and configured (versioning, retention policy)
- [ ] Terraform code reviewed and variables configured
- [ ] Network topology validated (latency, bandwidth)

### Deployment
- [ ] Run `terraform apply` to provision cluster
- [ ] Verify all nodes reach EC2 "running" state
- [ ] SSH into orchestrator, verify all nodes are reachable
- [ ] Bootstrap first storage node: `cfs server --config config/prod.toml`
- [ ] Verify node logs show Raft group initialization
- [ ] Join remaining nodes: `cfs server --config config/prod.toml --join node1:9400`
- [ ] Verify Raft quorum elected (check logs for "leader elected")

### Health Checks
- [ ] All nodes report "healthy" via `cfs admin status`
- [ ] Metadata shards show 3-way replication (check `cfs admin metadata-info`)
- [ ] Raft leader is elected for each shard
- [ ] All nodes can ping each other (RPC health)
- [ ] S3 backend is accessible (test write/read via `cfs admin s3-check`)

### Baseline Metrics
- [ ] Prometheus scrape targets all green (0 nodes down)
- [ ] Grafana dashboard displays cluster overview
- [ ] CPU usage baseline: < 10% idle nodes
- [ ] Memory usage baseline: < 50% per node
- [ ] NVMe health baseline: all drives healthy, no warnings
- [ ] Network latency baseline: < 1ms (intra-site), < 10ms (inter-site)

### Alerts Configuration
- [ ] Configure SNS topic for critical alerts
- [ ] Set up 24/7 on-call rotation with runbooks
- [ ] Test alert delivery (generate synthetic alert)
- [ ] Document alert escalation paths

### Client Onboarding
- [ ] Issue one-time enrollment tokens for FUSE clients
- [ ] Clients mount successfully: `cfs mount --token <token> /mnt/claudefs`
- [ ] Verify client can read/write files
- [ ] Test multi-client concurrent I/O

### Documentation
- [ ] Record cluster UUID and save to secure location
- [ ] Document node IP addresses and Raft shard assignments
- [ ] Create operational runbook specific to this cluster
- [ ] Document any custom configuration or non-defaults
- [ ] Record S3 bucket name, credentials, retention policy

---

## Deployment Procedure by Cluster Size

### Single-Node Development (Testing Only)
```bash
# 1. Provision orchestrator with user-data
aws ec2 run-instances --image-id ami-xyz --instance-type c7a.2xlarge \
  --user-data file://tools/orchestrator-user-data.sh --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=cfs-orchestrator}]'

# 2. SSH into orchestrator
ssh -i ~/.ssh/cfs-key ec2-user@<orchestrator-ip>

# 3. Start a single storage node
cfs server --config config/dev.toml

# 4. From another terminal, mount client
cfs mount --token <one-time-token> /mnt/claudefs
```

### Small Cluster (3 Nodes, Single Site)
```bash
# 1. Prepare Terraform
cd tools/terraform
cat > dev.tfvars << 'EOF'
aws_region       = "us-west-2"
cluster_name     = "claudefs-prod-1"
instance_type    = "i4i.2xlarge"
storage_nodes    = 3
spot_instances   = true
enable_monitoring = true
EOF

# 2. Provision infrastructure
terraform init
terraform plan -var-file=dev.tfvars -out=tfplan
terraform apply tfplan

# 3. SSH into each node and run bootstrap script
for node in storage-1 storage-2 storage-3; do
  ssh -i ~/.ssh/cfs-key ec2-user@${node} 'bash ~/bootstrap-storage.sh'
done

# 4. From orchestrator, initialize cluster
cfs cluster init --bootstrap-node storage-1 --cluster-name claudefs-prod-1 --storage-quota 15TB

# 5. Add remaining nodes
cfs node add --node storage-2
cfs node add --node storage-3

# 6. Verify cluster health
cfs admin status
cfs admin metadata-info
```

### Multi-Site Cluster (3+2 Nodes, Two Sites)
```bash
# 1. Prepare Terraform with multi-site configuration
cd tools/terraform
cat > prod-2site.tfvars << 'EOF'
aws_region_primary   = "us-west-2"
aws_region_secondary = "us-east-1"
cluster_name         = "claudefs-prod-2site"
storage_nodes_site_a = 3
storage_nodes_site_b = 2
conduit_region       = "us-west-2"
enable_replication   = true
enable_monitoring    = true
EOF

# 2. Provision both sites
terraform init
terraform plan -var-file=prod-2site.tfvars -out=tfplan
terraform apply tfplan

# 3. Bootstrap Site A (primary)
for node in us-west-2-storage-{1,2,3}; do
  ssh -i ~/.ssh/cfs-key ec2-user@${node} 'bash ~/bootstrap-storage.sh --role metadata-leader'
done

# 4. Initialize Site A cluster
cfs cluster init --bootstrap-node us-west-2-storage-1 \
  --cluster-name claudefs-prod-2site --storage-quota 30TB

# 5. Add Site B nodes
cfs node add --node us-east-1-storage-1 --site site-b
cfs node add --node us-east-1-storage-2 --site site-b

# 6. Enable cross-site replication
cfs admin enable-replication --local-site site-a --remote-site site-b

# 7. Verify replication health
cfs admin replication-status
```

---

## Version Upgrade Procedures

### Canary Upgrade (Single Node)
```bash
# 1. Update binary on one non-leader node
ssh -i ~/.ssh/cfs-key ec2-user@storage-2 'cfs self-update --version v1.1.0'

# 2. Restart node (gracefully drain shards to other nodes first)
cfs admin drain-node storage-2
ssh -i ~/.ssh/cfs-key ec2-user@storage-2 'sudo systemctl restart cfs-storage'

# 3. Monitor for 30 minutes
cfs admin status
# Check metrics: CPU, memory, I/O latency

# 4. If healthy, proceed to rolling upgrade
# If degradation observed, rollback immediately:
ssh -i ~/.ssh/cfs-key ec2-user@storage-2 'cfs self-update --version v1.0.0'
cfs admin undrain-node storage-2
```

### Rolling Upgrade (All Nodes)
```bash
# 1. For each node (one at a time):
for node in storage-1 storage-2 storage-3; do
  echo "Upgrading $node"
  cfs admin drain-node $node
  ssh -i ~/.ssh/cfs-key ec2-user@$node 'cfs self-update --version v1.1.0'
  ssh -i ~/.ssh/cfs-key ec2-user@$node 'sudo systemctl restart cfs-storage'
  cfs admin undrain-node $node
  sleep 60  # Wait for node to stabilize
  cfs admin status  # Verify health
done

# 2. Verify all nodes are healthy
cfs admin metadata-info
```

### Emergency Rollback
```bash
# If upgrade causes data corruption or severe issues:

# 1. Stop all clients immediately
for client in $(cfs admin list-clients); do
  echo "Disconnecting $client"
  cfs admin disconnect-client $client
done

# 2. Trigger automatic rollback (if available)
cfs admin emergency-rollback

# 3. Manual rollback (last resort)
for node in storage-1 storage-2 storage-3; do
  ssh -i ~/.ssh/cfs-key ec2-user@$node 'cfs self-update --version v1.0.0'
  ssh -i ~/.ssh/cfs-key ec2-user@$node 'sudo systemctl restart cfs-storage'
done

# 4. Verify Raft consistency
cfs admin metadata-consistency-check
```

---

## Backup and Restore Procedures

### Metadata Backup (Daily)
```bash
# 1. Snapshot Raft journal from leader
cfs admin snapshot-metadata --output /backup/metadata-$(date +%Y%m%d).tar.gz

# 2. Upload to S3 (with encryption)
aws s3 cp /backup/metadata-*.tar.gz s3://cfs-backups/metadata/ \
  --sse AES256 --storage-class GLACIER

# 3. Retention: 30 days (glacier transition after 7 days)
```

### Metadata Restore (Disaster Recovery)
```bash
# 1. On new cluster, download backup
aws s3 cp s3://cfs-backups/metadata/metadata-20260301.tar.gz /tmp/

# 2. Extract and restore
cfs admin restore-metadata --from-archive /tmp/metadata-20260301.tar.gz \
  --target-cluster new-cluster

# 3. Verify consistency
cfs admin metadata-consistency-check

# 4. Replay recent journal entries (if available)
cfs admin replay-journal --from <timestamp> --to now
```

### Data Backup (S3 Cache Mode)
```bash
# If using cache mode (D5):
# - Data is automatically asynchronously replicated to S3
# - No separate backup needed; S3 IS the backup

# If using tiered mode:
# - Schedule nightly snapshot → S3
cfs admin snapshot-active-data --output s3://cfs-backups/data-$(date +%Y%m%d)

# Test restore monthly
cfs admin restore-data --from s3://cfs-backups/data-20260301 \
  --target /mnt/recovery-test
```

---

## Emergency Procedures

### Single Node Failure
**RTO:** ~2 minutes (client reconnect)

```bash
# 1. Raft cluster detects failure (timeout ~3 sec)
# 2. Other leaders continue (multi-Raft)
# 3. Affected inodes re-route to other shards

# 4. Repair: Once node is back up
cfs admin node-status storage-1  # Check if online
cfs admin restore-shard-replicas storage-1  # Re-replicate data

# 5. Optional: Drain and remove permanently
cfs admin drain-node storage-1
cfs admin node-remove storage-1
```

### Majority Quorum Loss (2 of 3 nodes down)
**RTO:** ~30 minutes (manual intervention required)

```bash
# 1. Identify surviving node and quorum state
cfs admin raft-status

# 2. If >= 1 node survives with any data, recover:
cfs admin force-leader storage-1  # Make survivor the leader (CAUTION: data loss possible)

# 3. Do NOT do this if you have a snapshot or journal backup
# Instead, restore from backup and rebuild

# 4. Once leader elected, Raft will accept writes
# 5. Bring up replacement nodes (they'll re-sync from leader)
```

### Metadata Corruption
**Detection:** Checksum failures in inode operations

```bash
# 1. Check consistency
cfs admin metadata-consistency-check --verbose

# 2. If fixable, repair:
cfs admin metadata-repair --auto-fix

# 3. If not fixable, restore from backup:
cfs admin restore-metadata --from-backup metadata-20260228.tar.gz

# 4. Verify
cfs admin metadata-consistency-check --verbose
```

### Network Partition (Site A ↔ Site B Split)
**Behavior:** Both sites continue; conflict resolution on heal

```bash
# 1. Detect partition
cfs admin replication-status  # Will show "unreachable"

# 2. Both sites can continue operating (Raft quorum preserved if >= 2 nodes per site)
# 3. Writes are buffered and replayed when partition heals

# 4. On heal, conflict resolution triggers:
cfs admin show-conflicts  # List LWW conflicts
cfs admin resolve-conflicts --strategy last-write-wins  # Apply LWW resolution

# 5. Verify consistency
cfs admin metadata-consistency-check
```

---

## Performance Tuning for Production

### NVMe Optimization
```bash
# 1. Enable NVMe IRQ affinity (per CLAUDE.md kernel.md)
echo <irq> > /proc/irq/<irq>/smp_affinity_list  # Pin to specific CPU

# 2. Increase NVMe queue depth
cfs admin nvme-tune --queue-depth 256

# 3. Enable FDP (Solidigm) for write placement
cfs admin nvme-tune --enable-fdp --fdp-ruamask 0xff

# 4. Monitor health
cfs admin nvme-health
```

### Raft Optimization
```bash
# 1. Tune Raft election timeout (default 150-300ms)
# In config.toml:
[raft]
election_timeout_ms = 200
heartbeat_interval_ms = 50

# 2. Increase log compaction to reduce disk I/O
[raft]
snapshot_interval = 100000  # Increase from default

# 3. Monitor Raft latency
cfs admin raft-metrics  # p50, p99 commit latency
```

### CPU and Memory
```bash
# 1. Set CPU affinity (per-core isolation)
cfs admin cpu-pin --node storage-1 --cpu-mask 0xffff

# 2. Reserve memory for OS (avoid OOM)
# In config.toml:
[memory]
reserved_gb = 4  # Reserve 4 GB for OS

# 3. Monitor cache hit rates
cfs admin cache-stats
```

---

## Monitoring and Alerting

### Critical Alerts to Configure
1. **Node Down** — Any node unreachable > 30 sec
2. **Raft Leader Lost** — No leader elected > 10 sec
3. **Replication Lag** — Cross-site lag > 100ms
4. **Flash Capacity** — > 80% full
5. **NVMe Health** — Predictive failure, wear level
6. **Metadata Size** — Growing unexpectedly
7. **S3 Write Queue** — > 1M entries (cache full)
8. **Memory OOM Pressure** — > 90% used

See `docs/monitoring-setup.md` for complete Prometheus/Grafana setup.

---

## Success Criteria for Production Deployment

✅ All Day-1 checks passing
✅ No errors in logs (3 hours baseline)
✅ All metrics within normal ranges
✅ Raft quorum stable (no leader flaps)
✅ Replication lag < 100ms (cross-site)
✅ Client mounts and I/O operations successful
✅ Backup/restore procedures validated
✅ Runbook procedures tested by ops team

