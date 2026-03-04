# A11: Phase 2 Infrastructure & CI Implementation Plan

**Agent:** A11 | **Status:** ACTIVE (Phase 2) | **Target:** March 2026

## Overview

Phase 2 infrastructure work scales ClaudeFS from single-node testing (Phase 1) to multi-node cluster validation. This document outlines the infrastructure roadmap, dependencies, and implementation priorities.

## Current State (End of Phase 1)

✅ **Complete:**
- Orchestrator node (persistent c7a.2xlarge)
- Basic cfs-dev CLI (up/down/status/logs/ssh/cost)
- GitHub Actions CI/CD pipelines (6 workflows)
- Terraform infrastructure templates
- Watchdog/supervisor/cost-monitor automation
- Comprehensive operational documentation (OPERATIONAL_RUNBOOK.md)
- Cost optimization guide (COST_OPTIMIZATION_GUIDE.md)
- CI/CD pipeline guide (CI_CD_GUIDE.md)

## Phase 2 Goals

1. **Full test cluster provisioning & automation** — deploy 9 spot instances (5 storage + 2 clients + 1 conduit + 1 Jepsen)
2. **Robust spot instance lifecycle** — automatic failover to on-demand, budget-aware scaling
3. **Multi-node deployment** — build and deploy ClaudeFS across all nodes simultaneously
4. **Enhanced monitoring** — cross-cluster health checks, node-to-node communication validation
5. **Preemptible node automation** — spot instance interruption handling, automatic replacement
6. **CI/CD integration** — trigger full cluster tests via GitHub Actions

## Architecture: Multi-Node Test Cluster

```
┌─────────────────────────────────────────────────────────────┐
│ Orchestrator (c7a.2xlarge, on-demand, always running)       │
│ ├── Claude Code agents (A1-A11)                             │
│ ├── cfs-dev CLI controller                                  │
│ ├── cfs-watchdog (2-min heartbeat)                          │
│ └── GitHub Actions CI/CD                                   │
├─────────────────────────────────────────────────────────────┤
│ SITE A — Replication Source (3 storage nodes)              │
│ ├── storage-node-1 (i4i.2xlarge, spot)                     │
│ ├── storage-node-2 (i4i.2xlarge, spot)                     │
│ └── storage-node-3 (i4i.2xlarge, spot)                     │
├─────────────────────────────────────────────────────────────┤
│ SITE B — Replication Target (2 storage nodes)              │
│ ├── storage-node-4 (i4i.2xlarge, spot)                     │
│ └── storage-node-5 (i4i.2xlarge, spot)                     │
├─────────────────────────────────────────────────────────────┤
│ CLIENT NODES                                                │
│ ├── fuse-client (c7a.xlarge, spot) — pjdfstest, fsx        │
│ └── nfs-client (c7a.xlarge, spot) — Connectathon, SMB      │
├─────────────────────────────────────────────────────────────┤
│ CROSS-SITE INFRASTRUCTURE                                  │
│ ├── conduit (t3.medium, spot) — gRPC cloud relay           │
│ └── jepsen-controller (c7a.xlarge, spot) — chaos testing   │
└─────────────────────────────────────────────────────────────┘
```

**Total: 10 nodes** (1 persistent + 9 preemptible)

## Implementation Tasks

### 1. Enhanced Spot Instance Provisioning (PRIORITY: HIGH)

**Goal:** Provision all 9 spot instances with automatic fallback to on-demand.

**Current state:** Basic Terraform templates exist, but spot handling is minimal.

**Requirements:**
- Spot fleet request with diversified instance types for availability
- On-demand fallback if spot instances not available
- Budget-aware (stop spinning up if daily spend exceeds threshold)
- Instance tagging for role identification (storage, client, conduit, jepsen)
- Health check validation before returning to CLI

**Implementation:**
1. Enhance `tools/terraform/main.tf` — add spot fleet request for diversified types
2. Enhance `tools/terraform/storage-nodes.tf` — add spot/on-demand fallback logic
3. Enhance `tools/terraform/client-nodes.tf` — similar fallback for client nodes
4. Add `tools/spot-fleet-manager.sh` — monitor spot requests, handle interruption notices
5. Add `tools/instance-health-checker.sh` — validate SSH, storage readiness before returning

**Estimated effort:** 3-4 hours

### 2. Multi-Node Deployment Pipeline (PRIORITY: HIGH)

**Goal:** Build ClaudeFS once on orchestrator, deploy across all cluster nodes.

**Current state:** Build happens only on orchestrator.

**Requirements:**
- Cross-compile or build on representative architecture
- Distribute binary to all nodes (storage + clients + conduit)
- Start services in correct order (storage first, then metadata, then clients)
- Validate service startup and inter-node communication
- Automated rollback if any node fails to start

**Implementation:**
1. Create `tools/deploy-cluster.sh` — orchestrator-side deployment manager
   - Build: `cargo build --release`
   - Distribute: scp binary to all nodes
   - Start services: ssh into each node with correct config
   - Validate: check service status, test inter-node RPC
2. Enhance `cfs-dev` with `deploy` subcommand
   - `cfs-dev deploy [--skip-build]` — rebuild and redeploy all nodes
   - `cfs-dev deploy --node <name>` — single node redeployment
3. Add service templates for each node role:
   - `tools/storage-node-systemd.service` — cfs server with storage role
   - `tools/client-node-systemd.service` — cfs mount with FUSE
   - `tools/conduit-systemd.service` — cross-site replication conduit

**Estimated effort:** 4-5 hours

### 3. Preemptible Node Lifecycle Automation (PRIORITY: HIGH)

**Goal:** Handle spot instance interruption gracefully; automatic replacement.

**Current state:** Spot interruption not handled; manual recovery required.

**Requirements:**
- Detect spot interruption notice (2-minute warning)
- Graceful shutdown: flush state, notify remaining nodes
- Automatic launch replacement instance
- Re-balance cluster topology after replacement
- Alert administrator if replacement exceeds budget

**Implementation:**
1. Create `tools/spot-interruption-handler.sh` — runs on each spot instance
   - Monitor EC2 metadata for termination notice
   - Call graceful shutdown sequence on local cfs service
   - Signal orchestrator to launch replacement
   - Export node state to S3 for recovery
2. Enhance `cfs-dev` with replacement logic in `status` command
   - Check for missing nodes (expected vs actual)
   - Automatically launch replacements if budget allows
   - Show replacement progress
3. Add S3 state export for each node:
   - Metadata snapshots before shutdown
   - Raft log checkpoints
   - Allow recovery from backup

**Estimated effort:** 3-4 hours

### 4. Enhanced Monitoring & Health Checks (PRIORITY: MEDIUM)

**Goal:** Validate cluster health across all nodes; detect issues early.

**Current state:** Basic monitoring via Prometheus only.

**Requirements:**
- Node-to-node connectivity checks (RPC/transport layer)
- Service status validation (storage, metadata, replication)
- Data consistency checks (read same data from multiple nodes)
- Latency profiling between nodes
- Alert thresholds and escalation

**Implementation:**
1. Create `tools/cluster-health-check.sh` — comprehensive cluster validator
   - SSH to each node, check service status
   - From orchestrator, test RPC calls to each node
   - Measure inter-node latency
   - Validate data replication state
   - Generate health report
2. Enhance `cfs-dev status` with detailed health output:
   - Node status (running, pending, stopping)
   - Service status per node
   - RPC connectivity matrix
   - Replication lag (if applicable)
3. Add health check integration to Prometheus:
   - Export node health as metrics
   - Set up alerting rules for critical conditions
   - Track historical health trends

**Estimated effort:** 2-3 hours

### 5. CI/CD Integration for Multi-Node Tests (PRIORITY: MEDIUM)

**Goal:** Automate full-cluster tests via GitHub Actions.

**Current state:** CI/CD runs single-node builds; multi-node testing manual.

**Requirements:**
- GitHub Actions workflow trigger
- Provision full cluster, deploy ClaudeFS
- Run POSIX test suites (pjdfstest, fsx, xfstests)
- Run Connectathon (multi-protocol)
- Collect results, generate report
- Clean up cluster at end

**Implementation:**
1. Create `.github/workflows/test-cluster-full.yml` — full cluster test workflow
   - Trigger: manual + nightly schedule + PR with label `test:full-cluster`
   - Provision cluster via Terraform
   - Deploy via `cfs-dev deploy`
   - Run test suite (A9 output)
   - Upload results to GitHub Artifacts
   - Post summary to PR/commit
2. Enhance cost-monitor to pause non-essential tasks during cluster tests
3. Create `.github/workflows/test-cluster-quick.yml` — subset for faster feedback
   - Test only critical paths
   - Single-node pjdfstest subset
   - 10-15 minute target

**Estimated effort:** 2-3 hours

### 6. Documentation & Operational Guides (PRIORITY: MEDIUM)

**Goal:** Document Phase 2 infrastructure changes.

**Requirements:**
- Multi-node deployment guide for operators
- Troubleshooting guide for common cluster issues
- Architecture diagram with network topology
- Runbook for spot instance replacement
- Performance baseline expectations

**Implementation:**
1. Create `docs/DEPLOYMENT_GUIDE.md` (400 lines)
   - Prerequisites (AWS account, SSH keys)
   - Step-by-step cluster provisioning
   - Deployment validation
   - Common issues and fixes
2. Update `docs/OPERATIONAL_RUNBOOK.md` with Phase 2 procedures
   - Multi-node cluster monitoring
   - Node failure handling
   - Rebalancing procedures
3. Create `docs/CLUSTER_ARCHITECTURE.md` (300 lines)
   - Network topology diagram
   - Node roles and responsibilities
   - Raft consensus across sites
   - Replication pipeline

**Estimated effort:** 2-3 hours

## Dependencies

### Builder Agents (A1-A8)

- **A1 (Storage):** Must have stable io_uring implementation for multi-node testing
- **A2 (Metadata):** Raft consensus must handle 3+ nodes (not just single-node)
- **A4 (Transport):** RPC protocol must work reliably across network (not just localhost)
- **A5 (FUSE):** Mount automation for multi-node FUSE clients
- **A6 (Replication):** Cloud conduit must work across separate sites

### Cross-Cutting Agents

- **A9 (Test & Validation):** Provide multi-node test suites (pjdfstest, Connectathon)
- **A10 (Security):** Validate mTLS across multi-node setup

## Success Criteria

Phase 2 is complete when:

1. ✅ Full cluster (9 nodes) provisioned and running reliably (>99% uptime)
2. ✅ Multi-node ClaudeFS deployment working end-to-end
3. ✅ Spot interruption handling tested (manual kill of one node, automatic recovery)
4. ✅ Multi-node POSIX test suite passing (pjdfstest, fsx core tests)
5. ✅ Connectathon passing (NFS, SMB client compatibility)
6. ✅ Replication working (data written to site A, visible on site B within <5s)
7. ✅ Health checks detecting and alerting on failures
8. ✅ Full cluster CI/CD workflow integrated

## Timeline

- **Week 1:** Tasks 1, 2 (provisioning + deployment)
- **Week 2:** Tasks 3, 4 (lifecycle + monitoring)
- **Week 3:** Tasks 5, 6 (CI/CD + documentation)
- **Verification:** A9 runs full POSIX test suite; all tests passing by end of Phase 2

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Spot instance availability in us-west-2 | Diversify instance types, consider regional fallback |
| Network partition between sites | Add chaos testing (Jepsen partition tests) in Phase 3 |
| Cost overruns during multi-node testing | Strict schedule enforcement, destroy cluster after tests |
| Replication consistency issues | A6/A10 responsible for validation; extensive testing |

## Rollback Plan

If Phase 2 infrastructure becomes unstable:

1. Tear down all spot nodes: `cfs-dev down`
2. Keep orchestrator running for debugging
3. Fall back to Phase 1 (single-node) test mode
4. File GitHub Issues for each blocker
5. Collaborate with affected agents (A2/A4/A6)

## Next Phase (Phase 3)

Phase 3 (Production Readiness) will focus on:
- Long-running soak tests (48+ hour endurance)
- Jepsen split-brain and partition recovery tests
- CrashMonkey crash consistency validation
- Performance benchmarking (FIO workloads)
- Security penetration testing
- Full documentation (deployment guide, troubleshooting)

---

**Owner:** A11 (Infrastructure & CI)
**Model:** Claude Haiku 4.5
**Last Updated:** 2026-03-04
