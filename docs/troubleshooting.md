# ClaudeFS Troubleshooting Guide

Solutions for common issues encountered during ClaudeFS deployment and operation.

## Cluster Provisioning Issues

### Terraform Apply Fails

**Symptom:** `terraform apply` errors out with AWS API errors.

**Solutions:**

1. **Check AWS credentials**
   ```bash
   aws sts get-caller-identity  # Verify credentials work
   ```

2. **Check IAM permissions** — ensure you have EC2, VPC, and SecurityGroup permissions
   ```bash
   aws ec2 describe-instances  # Should work
   ```

3. **Region issues**
   ```bash
   aws ec2 describe-availability-zones --region us-west-2
   ```

4. **Spot price exceeded**
   ```bash
   aws ec2 describe-spot-price-history --product-descriptions "Linux/UNIX" \
     --instance-type i4i.2xlarge --region us-west-2
   # Increase spot_max_price in terraform.tfvars or disable spot instances
   ```

### Instances Launch But Don't Complete Status Checks

**Symptom:** `cfs-dev status` shows instances running but stuck on status checks.

**Causes & Solutions:**

1. **User data script errors**
   ```bash
   # SSH to instance (if SSH is working)
   ssh -i ~/.ssh/cfs-key.pem ec2-user@<instance-ip>
   tail -100 /var/log/user-data.log
   dmesg | tail -20
   ```

2. **ENI issues** (network interface)
   ```bash
   aws ec2 describe-network-interfaces --filters "Name=attachment.instance-id,Values=<instance-id>"
   ```

3. **AMI issues** — Ubuntu image may not exist in region
   ```bash
   aws ec2 describe-images --owners 099720109477 \
     --filters "Name=name,Values=ubuntu/images/hvm-ssd-gp3/ubuntu-questing-25.10*"
   ```

## Cluster Initialization Issues

### Storage Nodes Won't Join Cluster

**Symptom:** `cfs server join` times out or fails.

**Diagnosis:**

```bash
# On seed node (storage-a-1)
ssh storage-a-1
ps aux | grep cfs  # Check if server is running
journalctl -u claudefs -n 50

# On joining node
cfs server join --seed storage-a-1:9400 --token $CLUSTER_SECRET -v
```

**Solutions:**

1. **Check port connectivity**
   ```bash
   # From joining node
   nc -zv storage-a-1 9400  # Test TCP connection
   ```

2. **Token mismatch** — regenerate cluster secret
   ```bash
   # On orchestrator
   cfs admin cluster-secret regenerate
   echo $CLUSTER_SECRET
   ```

3. **Firewall/Security Group** — verify inbound rules
   ```bash
   aws ec2 describe-security-groups --group-ids <sg-id>
   # Check that port 9400-9410 are open between nodes
   ```

### Raft Not Electing Leader

**Symptom:** Metadata operations timeout, cluster unstable.

**Causes:**

1. **Not enough nodes** — need 3 for quorum (minimum for Raft)
   ```bash
   cfs admin nodes status
   # Should show 3+ nodes
   ```

2. **Node clock skew** — NTP not synchronized
   ```bash
   timedatectl status  # Check each node
   ntpq -p  # View NTP peers
   ```

3. **Raft log corruption** — needs restart
   ```bash
   # On all nodes, stop server
   systemctl stop claudefs

   # Wipe Raft log and restart
   rm -rf /data/claudefs/raft*
   systemctl start claudefs

   # Rejoin cluster
   cfs server join --seed storage-a-1:9400
   ```

## FUSE Mount Issues

### Mount Command Fails

**Symptom:** `cfs mount` returns error.

**Common causes:**

1. **Storage cluster not ready**
   ```bash
   cfs admin nodes status  # Should show 3+ healthy nodes
   cfs admin health  # Should report "healthy"
   ```

2. **Wrong endpoint**
   ```bash
   cfs mount /mnt/cfs --server storage-a-1:9400 --debug
   ```

3. **Permissions on mount point**
   ```bash
   ls -ld /mnt/cfs
   sudo chmod 755 /mnt/cfs
   ```

### Mount Succeeds But Files Not Visible

**Symptom:** Mount point is empty, no files visible.

**Solutions:**

1. **Check FUSE status**
   ```bash
   df /mnt/cfs
   mount | grep cfs
   ```

2. **FUSE daemon crashed** — check logs
   ```bash
   journalctl -u cfs-mount -n 50
   ps aux | grep cfs
   ```

3. **Metadata not replicated** — check replication status
   ```bash
   cfs admin replication status
   ```

### High Latency on FUSE Mount

**Symptom:** File operations are slow (>1s).

**Investigation:**

```bash
# From client
strace -c cfs ls /mnt/cfs  # Profile system calls

# Check metadata latency
cfs admin metrics | grep "latency_p99"

# Check client-to-server ping
ping -c 10 storage-a-1

# Check if client is using passthrough mode (kernel 6.8+)
mount | grep cfs
# Should show "fuse.fuse.cfs" with "direct_io" flag
```

**Solutions:**

1. **Enable passthrough mode** (kernel 6.8+)
   ```bash
   uname -r  # Must be >= 6.8
   cfs mount --passthrough /mnt/cfs --server storage-a-1:9400
   ```

2. **Adjust cache TTL**
   ```bash
   cfs mount --cache-ttl 30s /mnt/cfs --server storage-a-1:9400
   # Increase TTL (default 5s) to cache more aggressively
   ```

3. **Check network** — packet loss or high latency
   ```bash
   iperf3 -c storage-a-1 -R  # Measure bandwidth
   ```

## Replication Issues

### Replication Lag Increasing

**Symptom:** `claudefs_replication_lag_us` metric keeps increasing.

**Causes & Solutions:**

1. **Network issues** — high latency or packet loss
   ```bash
   # From site A storage node
   ping -c 100 -i 0.1 storage-b-1 | grep loss
   mtr -r -c 100 storage-b-1
   ```

2. **Cloud conduit slow** — check gRPC latency
   ```bash
   ssh conduit
   journalctl -u cfs-conduit -n 50
   cfs admin conduit status
   ```

3. **Site B storage nodes slow** — check I/O latency
   ```bash
   ssh storage-b-1
   cfs admin metrics | grep "io_latency"
   ```

### Replication Conflict Detection

**Symptom:** Alerts about "conflicting writes" to same file on both sites.

**Resolution:**

1. **Check conflict log**
   ```bash
   cfs admin replication conflicts
   ```

2. **Last-Write-Wins resolution** — newer write wins
   ```bash
   cfs admin file stat /path/to/file
   # Check timestamp and which site owns it
   ```

3. **Manual conflict resolution** (if needed)
   ```bash
   # Rename conflicting file and keep newer version
   cfs admin replication resolve --path /conflict/file --keep newer
   ```

## Performance Issues

### Low IOPS / Throughput

**Symptoms:** FIO benchmark shows <100 IOPS or <100 MB/s.

**Investigation:**

```bash
# Check I/O metrics
cfs admin metrics | grep "io_"

# Check CPU usage (should be 80%+ for saturated I/O)
top -b -n 1 | head -20

# Check memory pressure
free -h

# Check disk queue depth
iostat -x 1 5 /dev/nvme*
```

**Solutions:**

1. **Increase client parallelism**
   ```bash
   # Run multiple FIO jobs
   fio --numjobs=4 --iodepth=32 --rw=read --bs=4k
   ```

2. **Check NVMe health**
   ```bash
   nvme smart-log /dev/nvme0n1
   # Check "Data Units Read", media errors
   ```

3. **Enable RDMA** (if hardware available)
   ```bash
   # For InfiniBand or RoCE clusters
   cfs server --transport rdma --rdma-device mlx5_0
   ```

### High CPU Usage

**Symptoms:** CPU at 100%, single-threaded bottleneck.

**Investigation:**

```bash
# Identify hot functions
perf record -F 99 -p $(pidof cfs) -g -- sleep 30
perf report

# Check lock contention
cfs admin metrics | grep "lock"
```

**Solutions:**

1. **Increase Raft shard count** (if bottleneck is metadata)
   ```bash
   # During cluster creation
   cfs cluster create --num-shards 512  # Default is 256
   ```

2. **Enable io_uring** (should be default)
   ```bash
   cfs admin config | grep uring
   ```

3. **Profiling** — enable debug logging
   ```bash
   RUST_LOG=debug cfs server  # Verbose logging
   ```

## Monitoring & Observability Issues

### Prometheus Not Collecting Metrics

**Symptom:** Grafana dashboards show "no data".

**Solutions:**

1. **Check Prometheus targets**
   ```bash
   curl http://localhost:9090/api/v1/targets
   ```

2. **Check metrics endpoint on storage node**
   ```bash
   curl http://storage-a-1:9800/metrics | head
   ```

3. **Check Prometheus logs**
   ```bash
   journalctl -u prometheus -n 100
   ```

4. **Verify firewall** — port 9800 open?
   ```bash
   nc -zv storage-a-1 9800
   ```

### Logs Missing or Rotated

**Symptom:** Can't find historical logs or logs are truncated.

**Solutions:**

```bash
# Check journald retention
journalctl --disk-usage
journalctl --vacuum-time=30d  # Keep 30 days

# Check file-based logs
ls -lh /var/log/claudefs*
tail -f /var/log/claudefs/cfs.log

# Export logs before they rotate
journalctl -u claudefs -n 100000 > /tmp/claudefs-logs.txt
```

## Data Integrity Issues

### Checksums Failed

**Symptom:** Error messages about "checksum mismatch" during reads.

**Causes:**

1. **Hardware failure** — NVMe media error
   ```bash
   nvme smart-log /dev/nvme0n1 | grep -i error
   dmesg | grep -i nvme
   ```

2. **Software corruption** — bug in data path
   ```bash
   # Rebuild affected blocks
   cfs admin repair --block <block-id>
   ```

**Action:**

1. If hardware error detected, replace NVMe drive
2. Enable enhanced checksums
   ```bash
   cfs cluster config set --enhanced-checksums true
   ```

## Emergency Procedures

### Complete Cluster Failure

**Symptom:** All nodes down or unreachable.

**Recovery:**

1. **Check cluster status**
   ```bash
   cfs admin nodes status
   cfs admin health
   ```

2. **If S3 backup available, rebuild from S3**
   ```bash
   # This is a last-resort recovery
   cfs repair --from-s3 --cluster-id claudefs-phase2
   ```

3. **If Raft log corrupted, reset to known-good state**
   ```bash
   # Dangerous! Only as last resort
   for node in storage-a-{1,2,3}; do
     ssh $node 'rm -rf /data/claudefs/raft* && systemctl restart claudefs'
   done
   ```

### Single Node Failure (Raft)

**Symptom:** One storage node is down or slow.

**Recovery:**

1. **Remove failed node from quorum**
   ```bash
   cfs admin node remove --node-id storage-a-3
   ```

2. **Perform maintenance** or replace node

3. **Rejoin cluster** when ready
   ```bash
   ssh new-node
   cfs server join --seed storage-a-1:9400 --token $CLUSTER_SECRET
   ```

4. **Wait for rebalancing**
   ```bash
   cfs admin migration status  # Monitor progress
   ```

## Support Resources

- **GitHub Issues:** https://github.com/dirkpetersen/claudefs/issues
- **Documentation:** https://github.com/dirkpetersen/claudefs/tree/main/docs
- **Architecture Guide:** `docs/decisions.md`
- **API Reference:** `docs/management.md`

## Common Commands

```bash
# Cluster status
cfs admin nodes status
cfs admin health
cfs admin metrics

# Node operations
cfs admin node drain --node-id storage-a-1
cfs admin node remove --node-id storage-a-1

# Replication
cfs admin replication status
cfs admin replication conflicts

# Performance profiling
cfs admin metrics --since 1h --filter "latency"

# Configuration
cfs admin config show
cfs admin config set --key value

# Emergency
cfs admin cluster-secret regenerate
cfs admin repair --help
```

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
**Revision:** 1.0
