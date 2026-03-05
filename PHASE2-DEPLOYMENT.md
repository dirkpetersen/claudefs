# ClaudeFS Phase 2: Multi-Node Cluster Deployment & POSIX Validation

**Status:** 🟡 IN PROGRESS | **Date:** 2026-03-05 | **Agent:** A11 (Infrastructure & CI)

---

## Overview

Phase 2 activates the full 9-node test cluster with multi-site replication, cross-node Raft consensus, and comprehensive POSIX compliance validation. This document describes the deployment process and testing workflow.

## Cluster Architecture

### Node Breakdown (9 total + 1 orchestrator)

```
┌─────────────────────────────────────────────────────────────────┐
│ Orchestrator (1)  — c7a.2xlarge (persistent)                   │
│ - Claude agents running (A1-A11)                                │
│ - Build and deployment control                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ SITE A (3 storage nodes) — i4i.2xlarge with 1.8TB NVMe         │
├─────────────────────────────────────────────────────────────────┤
│ - storage-a-1: Raft leader candidate                           │
│ - storage-a-2: Raft replicas (consensus quorum)                │
│ - storage-a-3: Raft replicas                                    │
│ - Multi-Raft with 256 virtual shards (D4 architecture)         │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ SITE B (2 storage nodes) — i4i.2xlarge with 1.8TB NVMe         │
├─────────────────────────────────────────────────────────────────┤
│ - storage-b-1: Standby + async replication target              │
│ - storage-b-2: Standby + async replication target              │
│ - Cross-site journal replication via cloud conduit             │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ TEST NODES                                                       │
├─────────────────────────────────────────────────────────────────┤
│ - fuse-client (c7a.xlarge): FUSE mount, pjdfstest, fsx, xfstests
│ - nfs-client (c7a.xlarge): NFS/SMB mount testing               │
│ - cloud-conduit (t3.large): Cross-site journal relay           │
│ - jepsen-controller (c7a.xlarge): Distributed consistency tests│
└─────────────────────────────────────────────────────────────────┘
```

### Instance Details

| Node Role | Count | Type | CPU | RAM | Storage | Cost/mo |
|-----------|-------|------|-----|-----|---------|---------|
| Storage (A+B) | 5 | i4i.2xlarge | 8 | 64GB | 1.8TB NVMe | $1,500 |
| FUSE Client | 1 | c7a.xlarge | 4 | 8GB | 50GB | $200 |
| NFS Client | 1 | c7a.xlarge | 4 | 8GB | 50GB | $200 |
| Cloud Conduit | 1 | t3.large | 2 | 8GB | 20GB | $100 |
| Jepsen | 1 | c7a.xlarge | 4 | 8GB | 50GB | $200 |
| **Total Spot** | **9** | — | — | — | — | **$2,200/mo** |
| **Orchestrator** | **1** | c7a.2xlarge | 8 | 16GB | 100GB | **$300/mo** |
| **TOTAL** | **10** | — | — | — | — | **$2,500/mo** |

**Budget:** Running continuously = $2,500/mo; Nightly 8-hour window = ~$275/mo; Spot savings ~70%.

## Quick Start: Phase 2 Deployment

### Prerequisite

Ensure you have:
- AWS credentials with EC2/VPC permissions (via `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
- SSH key configured (set `CFS_KEY_NAME` or pass `--key` to `cfs-dev`)
- GitHub CLI (`gh`) installed for workflow triggers

### Deployment Steps

```bash
# 1. Provision Orchestrator (one-time)
cfs-dev up --phase 2

# 2. Trigger full multi-node deployment via GitHub Actions
cfs-dev cluster deploy

# 3. Monitor deployment (GitHub Actions: https://github.com/dirkpetersen/claudefs/actions)
# — Takes ~15 min: Terraform provision + Binary build + Deploy + Service startup

# 4. Validate cluster health
cfs-dev cluster status
cfs-dev health full

# 5. Run POSIX compliance tests
# — Automatically runs on deployed cluster (pjdfstest, fsx, xfstests)
# — Results collected in GitHub Actions artifacts

# 6. Monitor cluster health continuously
cfs-dev health monitor 60  # Every 60 seconds
```

## Manual Deployment (Orchestrator SSH)

If GitHub Actions is unavailable, deploy manually from orchestrator:

```bash
# SSH into orchestrator
cfs-dev ssh

# Build release binary
cd ~/claudefs
cargo build --release --bin cfs

# Deploy to all storage/client nodes
tools/deploy-cluster.sh deploy

# Start services
tools/deploy-cluster.sh start-services

# Validate
tools/deploy-cluster.sh validate

# Run cluster health check
tools/cluster-health-check.sh
```

## POSIX Compliance Testing

Phase 2 runs comprehensive POSIX test suites against the deployed cluster:

### Test Suites

| Suite | Files | Scenarios | Time | Purpose |
|-------|-------|-----------|------|---------|
| **pjdfstest** | Basic POSIX ops | 847 tests | ~15 min | Inode ops, directory semantics, permissions |
| **fsx** | Stress test | Random reads/writes | ~10 min | Crash recovery, data consistency |
| **xfstests** | XFS test suite | 400+ tests | ~20 min | Standard FS conformance |
| **Connectathon** | NFS tests | 60 tests | ~5 min | Multi-client coordination |
| **Jepsen** | Consistency tests | Partition/heal | ~30 min | Linearizability, split-brain recovery |

### Running Tests Manually

```bash
# SSH into FUSE client
cfs-dev ssh fuse-client

# Mount ClaudeFS
mkdir -p /mnt/cfs
cfs mount \
  --server storage-a-1:9400 \
  --server storage-a-2:9400 \
  --server storage-a-3:9400 \
  --transport tcp \
  /mnt/cfs

# Run pjdfstest
cd ~/claudefs
pjdfstest -p /mnt/cfs basic

# Run fsx
fsx -N 10000 -l 1000000 /mnt/cfs/fsx-test

# Run xfstests (if installed)
sudo ./check -g auto /mnt/cfs
```

## Cluster Lifecycle

### State Transitions

```
┌──────────────────────────────────────────────────┐
│ Initial: No Cluster                              │
└──────────────────────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────┐
│ "cfs-dev up --phase 2"                           │
│ → Provision orchestrator (persistent)            │
│ → Tag with Project=claudefs, Phase=2             │
└──────────────────────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────┐
│ "cfs-dev cluster deploy"                         │
│ → Trigger deploy-multinode.yml workflow          │
│ → Terraform provision 9 spot instances           │
│ → Build release binary                           │
│ → Deploy and start services                      │
│ → Run POSIX validation tests                     │
└──────────────────────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────┐
│ Cluster Running                                   │
│ → Watchdog monitors spot instance interruptions  │
│ → Spot-fleet-manager replaces failed nodes       │
│ → Supervisor fixes any build/agent issues        │
└──────────────────────────────────────────────────┘
                        ↓
┌──────────────────────────────────────────────────┐
│ "cfs-dev cluster destroy" (manual)               │
│ → Terraform destroy all 9 instances              │
│ → Keep orchestrator running                      │
└──────────────────────────────────────────────────┘
```

### Cost Optimization

**Always Running:**
- Orchestrator only: $10/day ($300/mo)

**Nightly Runs (8 hours, schedule: 20:00-04:00 UTC):**
- Orchestrator + 9 spot nodes: ~$40/night
- Monthly cost: ~$275 (spot savings vs on-demand)

**Manual on-Demand:**
- Provision cluster via `cfs-dev cluster deploy`: Pay only for test duration
- Destroy via `cfs-dev cluster destroy`: Stops all charges

**Monitoring:**
- `cfs-dev cost` — Show today's spend and monthly total
- Cost monitor script — Auto-kill cluster if daily spend > $100

## Monitoring & Debugging

### Health Check

```bash
# Quick status
cfs-dev status
cfs-dev cluster status

# Comprehensive health report
cfs-dev health full
# Saves report to: ~/.cfs/health-reports/health-$(date +%s).json

# Continuous monitoring (60-sec intervals)
cfs-dev health monitor 60

# Connectivity test (inter-node RPC)
cfs-dev health connectivity

# Cross-site replication status
cfs-dev health replication
```

### Logs

```bash
# Stream orchestrator agent logs
cfs-dev logs --agent A1  # Storage engine
cfs-dev logs --agent A2  # Metadata service
cfs-dev logs --agent A4  # Transport

# Stream cluster node logs (via SSH)
cfs-dev ssh storage-a-1
tail -f /var/log/claudefs/cfs.log

# Collect all logs from deployment
tools/cluster-health-check.sh --collect-logs /tmp/cfs-logs
tar czf cfs-logs-$(date +%Y%m%d-%H%M%S).tar.gz /tmp/cfs-logs
```

### Troubleshooting

| Issue | Diagnosis | Resolution |
|-------|-----------|-----------|
| Nodes not starting | Check AWS console for startup errors | `cfs-dev ssh <node>` → check `/var/log/cloud-init-output.log` |
| Raft not electing leader | Check metadata service logs | Restart metadata daemon on all 3 storage-a nodes |
| Client mount failing | Network connectivity | `cfs-dev health connectivity` → verify security groups |
| Tests failing | Check POSIX logs | `cfs-dev ssh fuse-client` → review `/var/log/claudefs/posix-*.log` |
| Spot instance terminated | Expected on preemptible nodes | Watchdog auto-replaces within 2 min |

## Phase 2 Milestones

- [ ] **Week 1:** Terraform validated, deploy-multinode.yml tested, cfs-dev cluster commands working
- [ ] **Week 2:** Multi-node cluster provisioning working end-to-end
- [ ] **Week 3:** POSIX validation suites passing on live cluster (80%+ pass rate)
- [ ] **Week 4:** Cross-site replication validated, failover tested, production-ready documentation

## Integration with CI/CD

### Automated Schedules

```yaml
# Nightly (01:00 UTC) — Full workspace + integration tests
tests-all.yml

# Weekly (02:00 UTC) — Multi-node cluster deployment + POSIX validation
deploy-multinode.yml (on schedule)

# Per-commit — Unit/parallel tests (fast path)
tests-parallel.yml
ci-build.yml
```

### GitHub Actions Workflow

```
PR → ci-build (5 min) + tests-parallel (10 min) → Merge
↓
Every night → tests-all (45 min) + a9-tests (30 min)
↓
Every week → deploy-multinode (60 min: provision + deploy + POSIX validation)
↓
GitHub Releases → Deployment artifacts, changelog, metrics
```

## Next Steps (Phase 2 Continuation)

1. **Spot Instance Lifecycle** — Integrate spot-fleet-manager with watchdog for automatic failover
2. **Multi-Client Testing** — FUSE + NFS/SMB clients concurrently reading/writing
3. **Jepsen Tests** — Distributed consistency verification (split-brain, healing)
4. **Performance Benchmarks** — FIO throughput, latency, scalability under load
5. **Security Validation** — A10 fuzzing, penetration testing of management API

## References

- **Architecture:** `docs/decisions.md` (D1-D10)
- **Infrastructure:** `INFRASTRUCTURE.md`
- **Agent Plan:** `docs/agents.md` (Phase 2 section)
- **Developer Guide:** `tools/cfs-dev help`
- **Terraform:** `tools/terraform/main.tf`
- **Bootstrap Scripts:** `tools/*-user-data.sh`

---

**Owner:** A11 (Infrastructure & CI) | **Status:** Phase 2 Foundation Complete | **Last Updated:** 2026-03-05
