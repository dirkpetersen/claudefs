# ClaudeFS Deployment Runbook

Operational procedures for deploying and managing ClaudeFS clusters.

## Prerequisites

- AWS Account with appropriate IAM permissions (see `tools/iam-policies/`)
- SSH key pair in AWS EC2 (`cfs-key` recommended)
- GitHub token with appropriate scopes (see `docs/ci-cd.md`)
- Rust 1.70+ toolchain
- `cfs-dev` CLI installed (from `tools/cfs-dev`)

## Cluster Deployment

### Quick Start: Single-Node Development Cluster

```bash
# 1. Provision orchestrator and start agents
cfs-dev up --phase 2

# 2. Check status
cfs-dev status

# 3. Stream agent logs
cfs-dev logs --agent A2

# 4. SSH into orchestrator
cfs-dev ssh

# 5. From orchestrator, start a storage node
cd /home/cfs/claudefs
cfs server --config config/dev.toml

# 6. From your laptop, test FUSE mount
cfs mount --token <one-time-token> /mnt/claudefs
```

### Multi-Node Cluster (3+1 Raft + 2 replicas)

For production-like testing with 2 sites:

```bash
# Create topology config
cat > ~/.cfs/cluster-config.yaml << 'EOF'
sites:
  - name: site-a
    nodes: 3
    instance_type: i4i.2xlarge
    availability_zone: us-west-2a
  - name: site-b
    nodes: 2
    instance_type: i4i.2xlarge
    availability_zone: us-west-2b

clients:
  - instance_type: c7a.xlarge
    count: 1

conduit:
  instance_type: t3.medium
  count: 1

jepsen_controller:
  instance_type: c7a.xlarge
EOF

# Provision cluster
cfs-dev up --topology ~/.cfs/cluster-config.yaml

# View node inventory
cfs-dev status --detailed
```

## Node Management

### Adding a New Storage Node

```bash
# 1. Provision new instance (orchestrator handles this)
cfs-dev node add --instance-type i4i.2xlarge

# 2. Node automatically joins cluster via bootstrap token
# 3. Shards rebalance automatically (A2 scaling manager)

# Check rebalancing progress
cfs admin status

# Monitor shard migration
cfs admin node list --show-shards
```

### Removing a Storage Node (Graceful Drain)

```bash
# 1. Mark node for draining
cfs admin node drain <node-id>

# 2. Monitor drain progress (blocks writes, drains data)
cfs admin status --watch

# 3. Once drained, terminate instance
aws ec2 terminate-instances --instance-ids i-xxxxx

# 4. Shards rebalance to remaining nodes
```

### Node Failure Recovery

**Automatic:** If a node fails, SWIM membership detection (~10s) marks it dead.
- Raft leader re-replicates affected shards
- S3 tiering: if node has only replicas, data recoverable from S3
- If node had primary: brief unavailability until leader elected

**Manual Recovery:**
```bash
# 1. Check node status
cfs admin status

# 2. If dead and not recovered:
cfs admin node recover <node-id> --from-s3

# 3. Node rebuilds state from S3 + Raft log
```

## Data Management

### Creating Snapshots

```bash
# Create point-in-time snapshot
cfs admin snapshot create /data --name weekly-backup

# List snapshots
cfs admin snapshot list

# Restore from snapshot (to new mount)
cfs admin snapshot restore weekly-backup /mnt/data-restored
```

### Disaster Recovery (Full Cluster Loss)

Assuming all nodes failed but S3 has data (cache mode, per D5):

```bash
# 1. Provision new cluster
cfs-dev up --phase 2 --new-cluster

# 2. Rebuild from S3
cfs admin repair --from-s3 s3://your-bucket

# 3. Wait for rebuild to complete
cfs admin status --watch

# 4. Mount and verify
cfs mount /mnt/data
ls -la /mnt/data/
```

## Monitoring

### Prometheus Metrics

Metrics available at `http://storage-node:9090/metrics`:

```bash
# Common queries
curl http://node1:9090/metrics | grep claudefs_

# Key metrics:
# - claudefs_inode_count — total inodes
# - claudefs_used_bytes — space used (flash + tiered)
# - claudefs_raft_term — Raft term (leadership changes = increasing)
# - claudefs_replication_lag_ms — cross-site lag
# - claudefs_operation_latency_ms — p50/p95/p99
```

### Grafana Dashboards

Default dashboards available at `http://orchestrator:3000/`:

- **Cluster Overview:** node count, space, ops/sec
- **Storage Latency:** read/write latency distributions
- **Replication:** lag, conflicts, throughput
- **Data Reduction:** dedup rate, compression ratio, encryption overhead

## Maintenance

### Regular Tasks

| Task | Frequency | Impact |
|------|-----------|--------|
| Update metadata snapshots | Daily | None (background) |
| Compact Raft log | Weekly | Brief CPU spike |
| GC dedup index | Daily | Background |
| S3 tiering threshold check | Hourly | Flash usage monitored |

### Upgrades

**Zero-Downtime Rolling Upgrade:**
```bash
# 1. Drain first node
cfs admin node drain <node-id>

# 2. Upgrade binary
scp cfs node1:/tmp/ && ssh node1 'systemctl restart claudefs'

# 3. Verify node rejoins
cfs admin status

# 4. Repeat for each node
```

### Emergency Procedures

#### Cluster Split Brain (Should Not Happen)

If Raft detects split brain (two sites both think they're leader):

```bash
# 1. Check membership
cfs admin status

# 2. If split, last-write-wins conflict resolution triggers
# 3. Administrator can query conflict log
cfs admin conflicts list

# 4. Manual resolution via `cfs admin conflicts resolve`
```

#### Flash Out of Space

If flash reaches critical (>95% full):

```bash
# 1. Check usage
cfs admin status

# 2. Force S3 tiering immediately
cfs admin tier flush --mode aggressive

# 3. Monitor tiering progress
cfs admin status --watch

# 4. If still critical: reject new writes (clients get ENOSPC)
```

#### Replication Lag Growing

If replication lag exceeds SLA:

```bash
# 1. Check conduit status
cfs admin replication status

# 2. Check network latency to site B
ping site-b.internal

# 3. Check conduit logs
ssh conduit 'tail -f /var/log/claudefs/conduit.log'

# 4. If conduit is dead: restart
ssh conduit 'systemctl restart cfs-conduit'
```

## Performance Tuning

### Cache Sizing (FUSE Clients)

```bash
# Increase metadata cache for workloads with many small files
cfs mount --cache-size 100M /mnt/data

# Increase I/O buffer pool
export CFS_IO_BUFFER_POOL_SIZE=1G
cfs mount /mnt/data
```

### QoS Configuration

Per `docs/decisions.md` D3, configure bandwidth limits:

```bash
# Set batch job max bandwidth to 100 MB/s
cfs admin qos set batch --max-bandwidth 100M

# Set interactive priority 2x higher than batch
cfs admin qos set interactive --weight 100 --weight batch 50
```

### Storage Node Tuning

See `tools/storage-node-user-data.sh` for kernel tuning:

```bash
# Key tunings:
# - io_uring queue depth (NVMe passthrough)
# - CPU affinity for per-core NVMe queues
# - Memory swappiness (disable for storage nodes)
# - Network buffer tuning (for cross-site replication)
```

## Troubleshooting Quick Reference

| Symptom | Likely Cause | Action |
|---------|-------------|--------|
| High metadata latency | Raft leader slow | Check node load, consider scaling |
| Replication lag | Network congestion | Check network metrics, increase conduit bandwidth |
| FUSE mount fails | Metadata node down | `cfs admin status`, check logs |
| Data loss on node failure | Missing S3 backup | Verify cache mode is active, S3 tiering working |
| Split brain alert | Network partition | Check site-to-site connectivity |

## References

- `tools/cfs-dev` — cluster management CLI
- `docs/decisions.md` — architecture decisions
- `docs/metadata.md` — metadata service internals
- `CHANGELOG.md` — version history and known issues
