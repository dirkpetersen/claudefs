# ClaudeFS Quick Reference

Concise commands and procedures for common Phase 2 operations.

## Cluster Provisioning

### Provision Phase 2 Test Cluster

```bash
# Using cfs-dev CLI
cfs-dev up --phase 2

# Or using Terraform directly
cd tools/terraform
terraform init
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars with your settings
terraform apply

# Verify
cfs-dev status
terraform output cluster_info
```

### Tear Down Cluster

```bash
# Keep orchestrator, remove spot instances
cfs-dev down

# Destroy everything
terraform destroy -auto-approve
```

## Connection & Access

### SSH to Nodes

```bash
# Orchestrator (management node)
cfs-dev ssh
ssh -i ~/.ssh/cfs-key.pem ec2-user@<orchestrator-ip>

# Storage node
ssh -i ~/.ssh/cfs-key.pem ec2-user@<storage-a-1-ip>

# Get all SSH commands
terraform output ssh_commands
```

### SSH Key Setup (one-time)

```bash
# Create AWS key pair
aws ec2 create-key-pair --key-name cfs-key --region us-west-2 \
  --query 'KeyMaterial' --output text > ~/.ssh/cfs-key.pem
chmod 600 ~/.ssh/cfs-key.pem

# Or use existing key
export CFS_KEY_NAME=my-existing-key
```

## Cluster Initialization

### Bootstrap Multi-Node Cluster

```bash
# On storage-a-1 (seed node)
cfs server \
  --cluster-id "claudefs-phase2" \
  --node-id "storage-a-1" \
  --listen 0.0.0.0:9400 \
  --data-dir /mnt/nvme/claudefs \
  --seed-nodes "storage-a-1:9400,storage-a-2:9400,storage-a-3:9400" \
  --site-id "site-a" \
  --replication-target "conduit:9401"

# On other nodes in site A
cfs server join --seed storage-a-1:9400 --token $CLUSTER_SECRET

# On site B (after site A is ready)
cfs server \
  --cluster-id "claudefs-phase2" \
  --node-id "storage-b-1" \
  --listen 0.0.0.0:9400 \
  --data-dir /mnt/nvme/claudefs \
  --seed-nodes "storage-b-1:9400,storage-b-2:9400" \
  --site-id "site-b" \
  --replication-source "conduit:9401"

cfs server join --seed storage-b-1:9400 --token $CLUSTER_SECRET
```

### Check Cluster Health

```bash
# From orchestrator
cfs admin nodes status
cfs admin health

# Check specific shard
cfs admin shard status --shard-id 0

# Check replication status
cfs admin replication status
```

## FUSE Mount

### Mount ClaudeFS

```bash
# One-time enrollment (generates certificate)
cfs mount --enroll --server storage-a-1:9400 \
  --token <one-time-enrollment-token> /mnt/cfs

# Subsequent mounts (uses stored certificate)
cfs mount --server storage-a-1:9400 /mnt/cfs

# Unmount
umount /mnt/cfs

# With passthrough mode (kernel 6.8+)
cfs mount --passthrough --server storage-a-1:9400 /mnt/cfs
```

### Test FUSE Mount

```bash
# Basic file operations
touch /mnt/cfs/test.txt
echo "hello" > /mnt/cfs/test.txt
cat /mnt/cfs/test.txt

# POSIX tests
cd /mnt/cfs
pjdfstest .          # POSIX filesystem tests
fsx -f test-file -N 1000  # Random operations
xfstests -d . -x generic/001  # Extended tests
```

## Monitoring

### Prometheus & Grafana

```bash
# Access Prometheus
http://orchestrator-ip:9090

# Access Grafana
http://orchestrator-ip:3000  # Default: admin/admin

# Query metrics
curl http://orchestrator-ip:9090/api/v1/query?query=up
curl http://storage-a-1:9800/metrics | head -20

# Check targets
curl http://orchestrator-ip:9090/api/v1/targets | jq
```

### Logs

```bash
# Cluster logs
cfs admin logs --since 1h --tail 100

# Node-specific logs
ssh storage-a-1 'journalctl -u claudefs -n 50 -f'

# Agent logs (on orchestrator)
tail -f /var/log/cfs-agents/a2.log
tail -f /var/log/cfs-agents/watchdog.log
```

## Performance Testing

### Run POSIX Tests

```bash
# Mount and test
cfs mount --server storage-a-1:9400 /mnt/cfs-test

# Run pjdfstest (most comprehensive)
pjdfstest /mnt/cfs-test 2>&1 | tee pjdfstest.log

# Run fsx (crash consistency)
fsx -d /tmp/fsx-trace -f /mnt/cfs-test/fsx-file -N 100000

# Run xfstests
xfstests -d /mnt/cfs-test -x generic -q
```

### Run FIO Benchmarks

```bash
# Baseline sequential read
fio --name=seqread --ioengine=libaio --rw=read \
    --bs=4m --size=1g --numjobs=1 --iodepth=32 \
    --runtime=60s --filename=/mnt/cfs/fio-test

# Random IOPS
fio --name=randread --ioengine=libaio --rw=randread \
    --bs=4k --size=1g --numjobs=4 --iodepth=32 \
    --runtime=60s --filename=/mnt/cfs/fio-test

# Mixed read/write
fio --name=mixed --ioengine=libaio --rw=randrw --rwmixread=70 \
    --bs=4k --size=1g --numjobs=4 --iodepth=16 \
    --runtime=60s --filename=/mnt/cfs/fio-test
```

## Cluster Management

### Node Operations

```bash
# Add new node
cfs admin node add --node-id storage-a-4 --site site-a

# Remove node (drain shards first)
cfs admin node drain --node-id storage-a-3
cfs admin node remove --node-id storage-a-3

# Check migration status
cfs admin migration status

# Force rebalance
cfs admin rebalance --num-shards 256
```

### Quotas

```bash
# Set user quota
cfs admin quota set --user alice --limit 100GB

# Set group quota
cfs admin quota set --group engineering --limit 1TB

# List quotas
cfs admin quota list

# Check usage
cfs admin quota usage --user alice
```

## Maintenance

### Snapshot Operations

```bash
# Create snapshot
cfs admin snapshot create --name "before-upgrade"

# List snapshots
cfs admin snapshot list

# Restore snapshot
cfs admin snapshot restore --name "before-upgrade" --path /data/restored

# Delete snapshot
cfs admin snapshot delete --name "before-upgrade"
```

### Backup & Recovery

```bash
# Export to S3
cfs admin export --path /important/data --s3-bucket claudefs-backups

# Import from S3
cfs admin import --s3-bucket claudefs-backups --path /restored/data

# Full cluster backup
cfs admin cluster backup --s3-bucket claudefs-backups --output backup.tar.gz
```

## Cost Management

### Monitor Spending

```bash
# Check cluster cost
cfs-dev cost

# Detailed AWS spend
aws ce get-cost-and-usage \
  --time-period Start=$(date -d '7 days ago' +%Y-%m-%d),End=$(date +%Y-%m-%d) \
  --granularity DAILY --metrics "UnblendedCost"

# Check budget
aws budgets describe-budget --account-id $(aws sts get-caller-identity --query Account --output text) \
  --budget-name cfs-daily-100
```

### Reduce Costs

```bash
# Stop spot instances (save ~$8/day)
cfs-dev down

# Start spot instances
cfs-dev up

# Check spot prices
aws ec2 describe-spot-price-history --instance-types i4i.2xlarge \
  --product-descriptions "Linux/UNIX" --region us-west-2 \
  --query 'SpotPriceHistory[0:10]'
```

## Troubleshooting

### Common Issues

```bash
# Cluster won't bootstrap
ssh storage-a-1 'journalctl -u claudefs -n 50'
nc -zv storage-a-2 9400  # Check connectivity

# FUSE mount slow
mount | grep cfs  # Check mount options
cfs admin metrics | grep latency  # Check latency

# Raft not electing leader
timedatectl status  # Check NTP sync
cfs admin nodes status  # Check which nodes are up

# Replication lag high
cfs admin replication status  # Check sync status
ping -c 10 storage-b-1  # Check network
```

### Diagnostic Commands

```bash
# Full system diagnosis
cfs admin health
cfs admin config show
cfs admin nodes status
cfs admin shard status
cfs admin replication status
cfs admin migration status

# Performance analysis
cfs admin metrics --since 1h | grep -E "latency|iops"

# Raft logs
cfs admin raft dump --shard-id 0 > raft-shard-0.log
```

## Agent Development (A9–A11)

### Run Tests

```bash
# Unit tests (fast)
cargo test --lib

# Integration tests
cargo test --test '*'

# Specific test
cargo test --lib metadata::raftservice::test_not_leader_error

# With output
cargo test -- --nocapture

# Benchmark
cargo bench --bench metadata_perf
```

### Run Clippy

```bash
# Check all
cargo clippy --all

# Fix automatically
cargo clippy --fix --all --allow-dirty

# Deny warnings
cargo clippy --all -- -D warnings
```

### Check Build

```bash
# Quick check
cargo check --all

# Full release build
cargo build --release

# Build specific crate
cargo build -p claudefs-meta --release
```

## Documentation

### Build Docs

```bash
# Generate and open docs
cargo doc --no-deps --open

# Check doc examples
cargo test --doc

# Build with private items
cargo doc --no-deps --document-private-items
```

### View Docs

```bash
# GitHub
https://github.com/dirkpetersen/claudefs/tree/main/docs

# Key files
- docs/decisions.md — Architecture decisions
- docs/agents.md — Agent responsibilities
- docs/management.md — Monitoring architecture
- docs/monitoring-setup.md — Prometheus setup
- docs/troubleshooting.md — Common issues
- docs/cost-optimization.md — Cost savings
```

## Quick Facts

- **Total tests:** 821 passing (A2: 447, A3: 60, A4: 223, A5: 58, others: 33)
- **Target deployment:** <10 minutes (full cluster)
- **Cost:** $71-86/day (dev/test), $100-150/day production
- **Supported kernels:** 5.14+ (RHEL 9+), passthrough on 6.8+
- **Default shards:** 256 (1 Raft group per shard)
- **Replication model:** Raft (3-way) for metadata, EC 4+2 for data
- **Transport:** TCP (io_uring), RDMA (libfabric) optional

## References

- **GitHub:** https://github.com/dirkpetersen/claudefs
- **CLAUDE.md:** Agent development guidelines
- **Deployment Runbook:** docs/deployment-runbook.md
- **CI/CD:** docs/ci-cd.md
- **Infrastructure:** tools/terraform/README.md

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
