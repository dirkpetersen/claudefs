# Phase 2 Infrastructure & Deployment Guide

**Target Completion:** April 15, 2026
**Agents Active:** 7 builders (A5, A6, A7, A8 + A1/A2/A3/A4 fixes) + A9, A10, A11
**Test Cluster Size:** 10 nodes (1 orchestrator + 9 spot instances)

## Phase 2 Test Cluster Architecture

### Node Breakdown

| Role | Count | Instance Type | vCPU | RAM | NVMe/Storage |
|------|-------|---------------|------|-----|--------------|
| **Orchestrator (persistent)** | 1 | c7a.2xlarge | 8 | 16 GB | 100 GB gp3 |
| **Storage servers (site A)** | 3 | i4i.2xlarge | 8 | 64 GB | 1875 GB NVMe |
| **Storage servers (site B)** | 2 | i4i.2xlarge | 8 | 64 GB | 1875 GB NVMe |
| **FUSE client (test runner)** | 1 | c7a.xlarge | 4 | 8 GB | 50 GB gp3 |
| **NFS/SMB client (multi-protocol)** | 1 | c7a.xlarge | 4 | 8 GB | 50 GB gp3 |
| **Cloud conduit (cross-site relay)** | 1 | t3.medium | 2 | 4 GB | 20 GB gp3 |
| **Jepsen controller (chaos injection)** | 1 | c7a.xlarge | 4 | 8 GB | 50 GB gp3 |

**Total: 10 nodes, ~$26/day in spot costs**

### Cluster Topology

```
              Orchestrator (persistent)
                      |
          ____________|____________
         |                         |
    Site A Raft                Site B Replication
    (3 storage nodes)          (2 storage nodes)
    ├─ Primary Metadata        └─ Replica Metadata
    ├─ Data Stripes (4+2 EC)      Data Stripes (replicas)
    └─ Write Journal             Write Journal
         |                           |
    (cloud conduit gRPC/mTLS) ←------→

    Test Clients
    ├─ FUSE client (pjdfstest, fsx, xfstests)
    ├─ NFS/SMB client (Connectathon, multi-proto)
    └─ Jepsen controller (chaos: partition, kill, delay)
```

## Deployment Workflow

### 1. Provision Cluster

```bash
# From developer laptop
cfs-dev up --phase 2

# This will:
# 1. Reuse or provision orchestrator (c7a.2xlarge, persistent)
# 2. Provision 9 spot instances (storage, clients, conduit, jepsen)
# 3. Run bootstrap scripts on each node (orchestrator-user-data.sh, storage-node-user-data.sh, client-node-user-data.sh)
# 4. Install compiled ClaudeFS binaries on each node
# 5. Start cfs-watchdog and cfs-supervisor on orchestrator
```

### 2. Build & Deploy

**On Orchestrator:**
```bash
# Watchdog checks agents every 2 minutes
tmux new-session -d -s cfs-watchdog "cd /home/cfs/claudefs && /opt/cfs-watchdog.sh 2"

# Supervisor runs Claude Code every 15 minutes via cron
*/15 * * * * cfs /opt/cfs-supervisor.sh >> /var/log/cfs-agents/supervisor.log 2>&1

# Cost monitor checks spend every 15 minutes
*/15 * * * * root /opt/cfs-cost-monitor.sh
```

**Agent Launcher (cfs-agent-launcher.sh):**
```bash
# Launches A5, A6, A7, A8, A9, A10 in parallel tmux sessions
/opt/cfs-agent-launcher.sh --phase 2 --agent A5
/opt/cfs-agent-launcher.sh --phase 2 --agent A6
/opt/cfs-agent-launcher.sh --phase 2 --agent A7
# ... etc
```

### 3. Multi-Node Cluster Initialization

**On Storage Nodes (Site A):**
```bash
# Storage node 1 (bootstrap leader)
cfs server \
  --cluster-id "claudefs-phase2" \
  --node-id "storage-a-1" \
  --listen 0.0.0.0:9400 \
  --data-dir /data/nvme0 \
  --seed-nodes "storage-a-1:9400,storage-a-2:9400,storage-a-3:9400" \
  --site-id "site-a" \
  --replication-target "conduit-1:9401"

# Storage nodes 2 & 3 join cluster
cfs server join --seed storage-a-1:9400 --token $CLUSTER_SECRET
```

**On Cloud Conduit (gRPC relay for Site B):**
```bash
cfs-conduit \
  --site-local "site-a:9401" \
  --site-remote "site-b:9401" \
  --listen 0.0.0.0:9402
```

### 4. Test Execution

**A9: POSIX Validation**
```bash
# From FUSE client node
cfs mount /mnt/cfs-fuse \
  --server storage-a-1:9400 \
  --cache-ttl 5s

# Run test suites
pjdfstest /mnt/cfs-fuse 2>&1 | tee pjdfstest.log
fsx -d /tmp/fsx-trace -f /mnt/cfs-fuse/fsx-test -N 100000
xfstests -d /mnt/cfs-fuse -x generic/001
```

**A9: Multi-Protocol Tests**
```bash
# From NFS client node
mount -t nfs -o vers=3,proto=tcp storage-a-1:/data /mnt/cfs-nfs
cd /mnt/cfs-nfs
connectathon -h storage-a-1 -n
```

**A9: Jepsen Distributed Consistency**
```bash
# From Jepsen controller
jepsen test \
  --nodes "storage-a-1,storage-a-2,storage-a-3,storage-b-1,storage-b-2" \
  --workload metadata-ops \
  --nemesis partition,pause,kill \
  --time-limit 300
```

### 5. Monitoring & Debugging

**Prometheus Metrics (A8 Management):**
```bash
curl http://storage-a-1:9800/metrics | grep claudefs
```

**Distributed Tracing (A11 or integrated):**
```bash
# View traces for cross-site replication
cfs admin trace --since 1h --filter "replication"
```

**Logs:**
```bash
# On orchestrator
tail -f /var/log/cfs-agents/watchdog.log
tail -f /var/log/cfs-agents/supervisor.log
tail -f /var/log/cfs-agents/a9-validation.log
```

## Phase 2 Features Being Tested

### By A5 (FUSE Client)
- ✅ FUSE v3 daemon integration with A2 metadata
- ✅ FUSE passthrough mode (6.8+ kernel)
- ✅ Client-side metadata caching with leases
- ✅ Data I/O through A4 transport
- ✅ Concurrent client handling

### By A6 (Replication)
- ✅ Cross-site journal replication (async)
- ✅ gRPC cloud conduit for Site B relay
- ✅ UID/GID mapping for heterogeneous clusters
- ✅ Conflict detection and Last-Write-Wins resolution
- ✅ Crash recovery and journal replay

### By A7 (Gateways)
- ✅ NFSv3 gateway (RPC translation)
- ✅ pNFS layouts for parallel data I/O
- ✅ S3 API endpoint (optional)
- ✅ Samba VFS plugin (C, GPLv3) for SMB3

### By A8 (Management)
- ✅ Prometheus exporter (metrics collection)
- ✅ Parquet indexer (metadata lake)
- ✅ DuckDB analytics gateway
- ✅ Web UI (React dashboard)
- ✅ CLI admin commands

### By A9 (Validation)
- ✅ pjdfstest: 1200+ POSIX filesystem tests
- ✅ fsx: Crash consistency, random ops
- ✅ xfstests: Comprehensive NFS/filesystem suite
- ✅ Connectathon: Multi-protocol interop
- ✅ Jepsen: Distributed consistency under chaos
- ✅ FIO: Performance benchmarks

### By A10 (Security)
- ✅ Unsafe code review (A1, A4, A5)
- ✅ RPC protocol fuzzing (libfuzzer)
- ✅ Cryptographic audit (A3 encryption)
- ✅ TLS/mTLS validation
- ✅ Penetration testing (A8 admin API)

## Performance Targets (Phase 2 Baseline)

| Metric | Target | Notes |
|--------|--------|-------|
| **Metadata op latency (p99)** | <50 ms | Local Raft commit |
| **Data I/O bandwidth (single client)** | >1 GB/s | NVMe → client passthrough |
| **Cross-site replication lag** | <100 ms | Async journal to Site B |
| **Jepsen: consistency violations** | 0 | Strict linearizability under partition |
| **Test suite duration** | <4 hours | Full pjdfstest + fsx + xfstests |
| **Infrastructure cost** | <$26/day spot | 5% of $100 daily budget |

## Rollout Strategy

### Week 1 (Onboarding)
- A5 FUSE: wires A2+A4 into FUSE daemon, basic mount/unmount
- A6 Replication: journal tailer, gRPC conduit skeleton
- A7 Gateways: NFSv3 translation layer
- A8 Management: Prometheus exporter
- A9: pjdfstest wrapper, CI integration
- A10: starts `unsafe` code audit

### Week 2 (Integration)
- A5 FUSE: passthrough mode, client caching, concurrent ops
- A6 Replication: full cross-site journal sync, conflict handling
- A7 Gateways: pNFS layouts, NFSv3 working
- A8 Management: Parquet indexer, DuckDB gateway
- A9: fsx, xfstests on multi-node cluster
- A10: fuzzing setup, RPC protocol audit

### Week 3 (Testing & Hardening)
- A5/A6: multi-node failover scenarios
- A7: SMB3 via Samba VFS plugin
- A8: Web UI, CLI admin commands
- A9: Connectathon, Jepsen setup
- A10: TLS/mTLS review, penetration tests

### Week 4 (Performance & Hardening)
- FIO benchmarks: read/write throughput, latency
- Jepsen consistency tests: partition + kill
- Long-running soak tests (overnight fsx)
- Bug fixes from all test findings

## Troubleshooting

### Cluster won't bootstrap

1. Check orchestrator logs:
   ```bash
   tail /var/log/cfs-bootstrap.log
   ```

2. Verify spot instances are running:
   ```bash
   cfs-dev status
   ```

3. Check Secrets Manager access:
   ```bash
   aws secretsmanager get-secret-value --secret-id cfs/github-token --region us-west-2
   ```

### Tests failing

1. Check agent session:
   ```bash
   cfs-dev logs --agent A9
   ```

2. Check cargo build:
   ```bash
   cd /home/cfs/claudefs && cargo check
   ```

3. Check test output:
   ```bash
   cd /mnt/cfs-fuse && pjdfstest . 2>&1 | tail -50
   ```

### Budget exceeded

Cost monitor will auto-terminate spot instances at $100/day.

To manually check:
```bash
cfs-dev cost
```

To extend budget (dev only):
```bash
# Modify AWS Budget via console or AWS CLI
aws budgets update-budget --account-id 405644541454 --new-budget Limit=200
```

## Deployment Checklist

- [ ] Phase 1 tests passing (551 tests)
- [ ] All builder agents ready (A5, A6, A7, A8 code reviewed)
- [ ] Test cluster provisioned (cfs-dev up --phase 2)
- [ ] Orchestrator: watchdog + supervisor running
- [ ] Storage cluster: 3-node + 2-node sites initialized
- [ ] Cloud conduit: gRPC relay operational
- [ ] Clients: mount points ready
- [ ] Jepsen: controller online
- [ ] A9 tests: pjdfstest baseline passing
- [ ] A10 security: initial unsafe code audit complete
- [ ] Monitoring: Prometheus metrics flowing
- [ ] Cost monitor: <$20/day baseline established

## Next Steps: Phase 3

See [phase3-production.md](phase3-production.md) for production readiness checklist.

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
**Status:** ✅ Ready for Phase 2 start
