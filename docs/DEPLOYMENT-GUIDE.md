# ClaudeFS Production Deployment Guide

**Version:** Phase 4 (Infrastructure & CI)  
**Author:** A11 Infrastructure & CI  
**Status:** Complete for development/staging environments  

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Infrastructure Provisioning](#infrastructure-provisioning)
3. [Post-Deployment Configuration](#post-deployment-configuration)
4. [Monitoring & Observability](#monitoring--observability)
5. [Operations & Maintenance](#operations--maintenance)
6. [Disaster Recovery](#disaster-recovery)
7. [Troubleshooting](#troubleshooting)

---

## Quick Start

### Prerequisites

1. **AWS Account** with appropriate permissions
2. **Terraform** 1.6+
3. **AWS CLI** v2 configured with credentials
4. **SSH Key** pre-created in AWS EC2 (store locally as `~/.ssh/cfs-key.pem`)
5. **Git** with ClaudeFS repository cloned

### One-Command Deployment

```bash
# Development environment (default)
cd tools/terraform
terraform init
terraform apply -var-file="environments/dev/terraform.tfvars" \
  -var="ssh_key_name=cfs-key"

# Wait for deployment (~10-15 minutes)
# Output: orchestrator IP, storage node IPs, etc.
```

### Verify Cluster Health

```bash
# SSH into orchestrator
ssh -i ~/.ssh/cfs-key.pem ubuntu@<orchestrator-ip>

# Check cluster status
cfs status

# View node health
cfs nodes

# Test FUSE mount
cfs mount /mnt/claudefs
df -h /mnt/claudefs
```

---

## Infrastructure Provisioning

### Architecture Overview

```
┌─────────────────────────────────────────────────┐
│  AWS VPC (10.0.0.0/16)                          │
├─────────────────────────────────────────────────┤
│                                                 │
│  Public Subnets (NAT outbound)                  │
│  ├── Orchestrator (c7a.2xlarge)                 │
│  └── Bastion host (t3.medium)                   │
│                                                 │
│  Private Subnets (Internal only)                │
│  ├── Storage Site A (3-5 i4i.2xlarge nodes)   │
│  ├── Storage Site B (2-4 i4i.2xlarge nodes)   │
│  ├── FUSE Client (c7a.xlarge)                 │
│  ├── NFS Client (c7a.xlarge)                   │
│  └── Conduit (t3.medium)                       │
│                                                 │
│  Security Groups                                │
│  ├── Cluster (TCP 9400-9410, UDP 9400-9410)   │
│  ├── Monitoring (TCP 9800, 3000)              │
│  ├── SSH (TCP 22)                             │
│  └── Replication (TCP 5051-5052)              │
│                                                 │
└─────────────────────────────────────────────────┘
```

### Environment Configurations

| Environment | Cluster Size | Spot Instances | Budget | Use Case |
|-------------|--------------|----------------|--------|----------|
| **dev** | 3+2 nodes | Yes (enabled) | $100/day | Development, CI/CD |
| **staging** | 5+3 nodes | Yes (enabled) | $150/day | Testing, validation |
| **prod** | 5+5 nodes | No (on-demand) | $500/day | Production workloads |

### Step-by-Step Provisioning

#### 1. Initialize Terraform

```bash
cd tools/terraform
terraform init
# Downloads AWS provider, initializes backend (first run creates S3+DynamoDB)
```

#### 2. Plan Deployment

```bash
# Development
terraform plan -var-file="environments/dev/terraform.tfvars" \
  -var="ssh_key_name=cfs-key" -out=tfplan

# Review changes, verify instance counts and types
cat tfplan | grep 'aws_instance\|aws_autoscaling'
```

#### 3. Deploy Infrastructure

```bash
terraform apply tfplan

# This will:
# 1. Create VPC, subnets, security groups
# 2. Spin up orchestrator instance
# 3. Create launch templates and ASGs for storage nodes
# 4. Provision client nodes
# 5. Set up CloudWatch monitoring
# 6. Configure auto-scaling policies

# Typical duration: 10-15 minutes
```

#### 4. Capture Outputs

```bash
terraform output -raw orchestrator_public_ip > /tmp/orchestrator_ip.txt
terraform output -json > /tmp/deployment_outputs.json
```

### Remote State Backend

The first `terraform apply` creates S3 and DynamoDB resources for remote state:

```
S3 Bucket: claudefs-terraform-state-<ACCOUNT_ID>-us-west-2
DynamoDB Table: claudefs-terraform-locks
```

Future runs will use remote state automatically. To verify:

```bash
terraform state list   # Shows all managed resources
terraform state show aws_s3_bucket.terraform_state  # Inspects state file
```

---

## Post-Deployment Configuration

### 1. SSH into Orchestrator

```bash
ORCH_IP=$(terraform output -raw orchestrator_public_ip)
ssh -i ~/.ssh/cfs-key.pem ubuntu@$ORCH_IP
```

### 2. Verify Nodes Are Running

```bash
# Inside orchestrator
aws ec2 describe-instances --filters "Name=tag:Project,Values=claudefs" \
  --query 'Reservations[*].Instances[*].[Tags[?Key==`Name`].Value|[0],State.Name]' \
  --output table
```

### 3. Initialize ClaudeFS Cluster

```bash
# Configure cluster parameters
cfs config set --cluster-name "claudefs-dev-1" \
  --raft-heartbeat-interval 100ms \
  --raft-election-timeout 300ms

# Add storage nodes to cluster
for ip in $(aws ec2 describe-instances \
  --filters "Name=tag:Site,Values=A" \
  --query 'Reservations[*].Instances[*].PrivateIpAddress' \
  --output text); do
  cfs node add --ip $ip --role storage
done

# Verify cluster formed
cfs status   # Should show "healthy" with 3+ storage nodes
```

### 4. Start FUSE Client

```bash
# On FUSE client node
mkdir -p /mnt/claudefs
cfs mount --mount-point /mnt/claudefs \
  --metadata-server orchestrator:9801 \
  --cache-size 4G

# Verify mount
mount | grep claudefs
df -h /mnt/claudefs
```

### 5. Create Test Data

```bash
# Create test files
for i in {1..10}; do
  echo "Test data $i" > /mnt/claudefs/test-$i.txt
done

# Verify files exist
ls -la /mnt/claudefs/
```

---

## Monitoring & Observability

### 1. Prometheus Metrics

Metrics endpoint (managed node):
```
http://orchestrator:9800/metrics
```

Common metrics to monitor:
```
# Storage I/O
storage_io_latency_ms_bucket
storage_io_queue_depth

# Metadata
metadata_raft_commits_total
metadata_quorum_health

# Replication
replication_journal_lag_ms
replication_failover_count_total

# Gateway
gateway_nfsv3_ops_total
gateway_error_rate
```

### 2. Grafana Dashboards

Pre-configured dashboards accessible at:
```
http://orchestrator:3000
```

Default credentials: `admin / admin`

Key dashboards:
- **Cluster Health**: Nodes, storage capacity, quorum status
- **Performance**: Latencies, throughput, cache hit ratios
- **Data Reduction**: Dedup/compression ratios, tiering activity
- **Replication**: Cross-site lag, failover count, conflicts
- **Cost Tracking**: EC2 hourly cost, spot vs on-demand

### 3. CloudWatch Integration

CloudWatch metrics for infrastructure:
- CPU utilization (per instance)
- Disk usage (per volume)
- Network throughput (in/out)
- Auto-scaling group activity

Alarms configured for:
- Scale-up at 70% CPU, scale-down at 20%
- Alerts if allocator free space < 10%
- Alerts if cross-site lag > 5 seconds

### 4. Structured Logging

All nodes send logs to:
```
/var/log/cfs/
  ├── storage.log         # A1 Storage Engine
  ├── metadata.log        # A2 Metadata Service
  ├── reduce.log          # A3 Data Reduction
  ├── transport.log       # A4 Transport
  ├── fuse.log            # A5 FUSE Client
  ├── replication.log     # A6 Replication
  ├── gateway.log         # A7 Protocol Gateways
  └── management.log      # A8 Management
```

Access logs from orchestrator:
```bash
ssh orchestrator "tail -f /var/log/cfs/cluster.log"
```

---

## Operations & Maintenance

### Scaling Storage Nodes

#### Manual Scale-Up

```bash
# Edit terraform configuration
terraform apply -var-file="environments/dev/terraform.tfvars" \
  -var="storage_site_a_count=5"

# Wait for new nodes to launch and join cluster
watch -n 5 'cfs nodes | grep -c "healthy"'
```

#### Manual Scale-Down (with Data Rebalancing)

```bash
# Cordon node to prevent new operations
cfs node cordon <node-id>

# Wait for in-flight operations to complete
sleep 60

# Drain data from node
cfs node drain <node-id>

# Once drained, terminate in AWS
aws ec2 terminate-instances --instance-ids <instance-id>

# Remove from cluster
cfs node remove <node-id>
```

### Node Maintenance

#### Rolling Updates

```bash
# Stage new binary on orchestrator
scp ./target/release/cfs-storage orchestrator:/tmp/

# For each storage node:
for node in node1 node2 node3; do
  # Cordon and drain
  cfs node cordon $node
  cfs node drain $node
  
  # Update binary
  ssh $node "sudo systemctl stop cfs-storage"
  ssh $node "sudo mv /tmp/cfs-storage /usr/local/bin/"
  ssh $node "sudo systemctl start cfs-storage"
  
  # Uncordon
  cfs node uncordon $node
  
  # Wait for recovery
  sleep 30
done
```

#### Backup & Restore

Daily snapshots to S3:
```bash
# Backup
cfs snapshot create --backup-location s3://claudefs-backups/daily/ \
  --retention 7  # Keep 7 daily backups

# Restore (in disaster scenario)
cfs snapshot restore --source s3://claudefs-backups/daily/2026-04-18/
```

---

## Disaster Recovery

### Backup Strategy

**RPO (Recovery Point Objective):** 24 hours  
**RTO (Recovery Time Objective):** <30 minutes

Backups are automated and stored in S3:
- Daily snapshots: retained for 7 days
- Weekly snapshots: retained for 90 days (Glacier)

### Recovery Procedure

1. **Verify Backup Integrity**
   ```bash
   cfs backup verify s3://claudefs-backups/latest/
   ```

2. **Provision New Cluster** (same geography or different region)
   ```bash
   terraform apply -var-file="environments/prod/terraform.tfvars" \
     -var="aws_region=us-east-1"  # Different region for DR
   ```

3. **Restore from Backup**
   ```bash
   cfs snapshot restore --source s3://claudefs-backups/latest/ \
     --target-cluster <new-cluster-endpoint>
   ```

4. **Verify Data Integrity**
   ```bash
   cfs verify-consistency --sample-size 1000  # Spot check 1000 files
   ```

5. **Update DNS/Routing**
   ```bash
   aws route53 change-resource-record-sets \
     --zone-id Z... \
     --change-batch file://dns-update.json
   ```

Expected time to fully restored cluster: 20-30 minutes

---

## Troubleshooting

### Cluster Won't Start

**Symptom:** `cfs status` shows "unhealthy" or "no leader"

**Debug:**
```bash
# Check node logs
ssh storage-node-1 "journalctl -u cfs-storage -n 50"

# Verify networking
cfs nodes | grep -E "ip|port"

# Check security groups
aws ec2 describe-security-groups --filter "Name=tag:Project,Values=claudefs"
```

**Solutions:**
- Verify security group allows TCP 9400-9410 between nodes
- Check node DNS resolution: `nslookup metadata-node`
- Ensure Raft quorum: must have 2+ healthy nodes in site A

### High I/O Latency

**Symptom:** `storage_io_latency_ms` > 500ms in Grafana

**Debug:**
```bash
# Check disk utilization
df -h /mnt/nvme
# Check NVMe queue depth
cat /sys/block/nvme*/queue/nr_requests

# Check CPU utilization
top -b -n 1 | head -20
```

**Solutions:**
- Scale-up (trigger via: `stress-ng --cpu 100%` for 5 min)
- Check for disk defragmentation: `fstrim /mnt/nvme`
- Review slow query logs: `tail -f /var/log/cfs/slow-queries.log`

### Cross-Site Lag > 5s

**Symptom:** Replication alerts firing

**Debug:**
```bash
# Check network latency
ssh storage-site-a "ping -c 10 storage-site-b" | grep -E "avg|loss"

# Check conduit status
cfs conduit status

# Check replication queue
cfs replication queue-depth
```

**Solutions:**
- Verify Network ACLs allow replication ports (5051-5052)
- Check if conduit is overloaded (scale up or add more)
- Review cross-region routing if multi-region deployment

### Storage Allocation Failure

**Symptom:** `LowAllocatorFree` alert firing, writes failing

**Debug:**
```bash
# Check allocator state
cfs allocator status

# Check disk usage
storage-node# df -h

# Check for zombie files (snapshots being deleted)
cfs snapshot list --show-cleanup-progress
```

**Solutions:**
- Trigger emergency cleanup: `cfs gc run --emergency`
- Scale up storage nodes to add capacity
- Identify and move large files to S3 tiering

---

## Next Steps

### Immediate (Post-Deployment)

1. [ ] Verify all nodes healthy
2. [ ] Create test data
3. [ ] Run POSIX test suite: `cfs-test-orchestrator.sh`
4. [ ] Configure backup retention policy
5. [ ] Document team runbooks

### Before Production

1. [ ] Run 24-hour soak test with production workload
2. [ ] Execute DR drill (restore from backup)
3. [ ] Performance benchmark (FIO, CrashMonkey)
4. [ ] Security audit (penetration test, fuzzing)
5. [ ] Load testing (ramp up to expected peak)

### Ongoing

1. [ ] Monitor CloudWatch dashboards daily
2. [ ] Review logs for errors/warnings
3. [ ] Run weekly DR tests (automate via cron)
4. [ ] Update runbooks based on operational experience
5. [ ] Plan capacity upgrades based on trends

---

## Support & References

- **Architecture Decisions:** `docs/decisions.md`
- **Terraform Configuration:** `tools/terraform/README.md`
- **Phase 4 Planning:** `docs/A11-PHASE4-PLAN.md`
- **Operations Runbooks:** `docs/OPERATIONS.md` (coming)
- **API Documentation:** `docs/API.md` (coming)

---

**Last Updated:** 2026-04-17  
**Phase:** 4 (Infrastructure & CI)  
**Status:** ✅ Complete for dev/staging, production-ready for prod environment
