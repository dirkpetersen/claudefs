# A11 Infrastructure & CI — Phase 4 Plan

**Agent:** A11 Infrastructure & CI
**Status:** 🟡 **PHASE 4 PLANNING**
**Date:** 2026-04-17
**Model:** Haiku 4.5 (orchestration) + OpenCode for complex infra-as-code

---

## Overview

Phase 4 focuses on **production deployment readiness and operational automation**. Phase 3 delivered multi-node testing infrastructure; Phase 4 hardens operations, integrates metrics, and enables production deployments with automated recovery and scaling.

---

## Phase 3 Recap

✅ **Phase 3 Complete:**
- CI/CD optimization: 50% build time reduction (20-30 min → <15 min)
- Health monitoring infrastructure: 40+ tests, cluster aggregation ready
- Multi-node test orchestration: failover testing, e2e automation
- Operational documentation: 2,500+ lines (troubleshooting, runbooks, scaling)
- Infrastructure tools: 2 scripts (failover tests, test orchestrator)

---

## Phase 4 Goals

### Primary Goals

1. **Production Deployment Automation** — Terraform/Pulumi modules for AWS infrastructure
   - ClaudeFS cluster provisioning (5+N storage nodes, 2 clients, 1 conduit)
   - VPC, subnets, security groups, IAM roles
   - Auto-scaling group for storage nodes (with controlled scaling)
   - CloudFormation or Pulumi exports for reproducible deployments

2. **Operational Metrics & Observability Integration** — Connect all crates to centralized monitoring
   - Prometheus scrape configs for all services
   - Per-crate metrics collection (A1-A8 integration)
   - Grafana dashboard updates with real operational metrics
   - Alert rules for critical conditions (high CPU, memory, latency)

3. **Automated Recovery & Self-Healing** — Execute corrective actions without manual intervention
   - health.rs recovery action execution (CPU reduction, memory shrinking, node restart)
   - Graceful shutdown and recovery procedures
   - Automatic backup rotation and cleanup
   - Dead node detection and removal from cluster

4. **Deployment & Release Pipeline** — Streamlined binary releases
   - Build production binaries (cfs, cfs-client, cfs-mgt)
   - Multi-arch support (x86_64, aarch64)
   - Artifact signing and verification
   - Staged rollout: canary → 10% → 50% → 100%

### Secondary Goals

5. **Cost Monitoring & Optimization**
   - AWS cost tracking dashboard (daily/weekly spend)
   - Resource utilization analysis
   - Spot instance savings optimization
   - Reserved capacity planning

6. **Disaster Recovery & Backup**
   - Automated snapshot scheduling
   - Cross-region replication setup
   - Recovery time objective (RTO) testing
   - Backup retention policies

---

## Phase 4 Breakdown: 6 Blocks

### Block 1: Infrastructure-as-Code Foundation (Days 1-2)

**Goal:** Terraform/Pulumi modules for AWS ClaudeFS cluster deployment

**Deliverables:**
1. **AWS infrastructure module** (`tools/terraform/modules/claudefs-cluster/`)
   - VPC with dual AZ support
   - Security groups for storage/client/conduit
   - IAM roles for EC2 nodes
   - CloudWatch integration

2. **Node provisioning** (`tools/terraform/modules/claudefs-node/`)
   - Storage node configuration (EBS, NVMe, kernel tuning)
   - Client node configuration (FUSE mount points)
   - Conduit node configuration (replication parameters)

3. **Auto-scaling group** for storage nodes
   - Scale up/down based on capacity thresholds
   - Graceful node addition (data rebalancing)
   - Node decommissioning procedures

4. **Network infrastructure**
   - Private VPC for cluster traffic
   - NAT gateway for S3 access
   - VPN/bastion for admin access

**Implementation notes:**
- Use Terraform for declarative infrastructure
- Outputs: cluster endpoint, node IPs, load balancer DNS
- Store state in S3 with DynamoDB locking
- Support multi-region deployments

**Tests:** Manual deployment testing on AWS (5-node cluster)

**Reference:** See `docs/DEPLOYMENT-GUIDE.md` (to be created)

---

### Block 2: Metrics Integration with All Crates (Days 3-4)

**Goal:** All 8 crates export Prometheus metrics; dashboards show real operational data

**Integration work per crate:**

| Crate | Existing Metrics | New Metrics | Integration |
|-------|-----------------|-------------|-------------|
| A1 (Storage) | Queue depth, I/O latency | Block allocator free space, GC activity | Prometheus exporter |
| A2 (Metadata) | Raft commits, KV ops | Shard distribution, txn latency | Prometheus exporter |
| A3 (Reduce) | Dedup ratio, compression | Tiering rate, similarity detection | DuckDB → Parquet |
| A4 (Transport) | RPC latency, bandwidth | Trace aggregation, router scores | Existing trace_aggregator |
| A5 (FUSE) | FUSE ops/sec, cache hits | Passthrough mode %, quota usage | Tokio task metrics |
| A6 (Repl) | Journal lag, failovers | Cross-site latency, conflict rate | gRPC metrics |
| A7 (Gateway) | NFSv3/pNFS ops | Protocol distribution, error rate | Prometheus middleware |
| A8 (Mgmt) | DuckDB queries, Web API | Admin API latency, auth failures | Existing infrastructure |

**Deliverables:**
1. Per-crate Prometheus exporter configuration
2. Standardized metrics naming (prometheus_client best practices)
3. Updated Grafana dashboards with real metric queries
4. Alert rules for SLA violations (p99 latency > 100ms, etc.)

**Integration method:**
- All crates use `prometheus` crate for Prometheus client
- Export on `/metrics` endpoint (port 9090+ for daemons)
- Central Prometheus scrape config (`monitoring/prometheus.yml`)

**Tests:**
- Run local 5-node cluster, verify all metrics exported
- Check dashboard queries return non-zero values
- Validate alert rules trigger appropriately

---

### Block 3: Automated Recovery Actions (Days 5-6)

**Goal:** health.rs recovery actions actually execute corrective measures

**Deliverables:**

1. **health.rs recovery execution**
   - Extend `health.rs` with action callbacks
   - CPU too high → reduce worker thread count
   - Memory > 90% → shrink buffer caches, evict cold data
   - Disk full → trigger emergency cleanup, alert admin

2. **Graceful shutdown procedures**
   - Drain in-flight operations before shutdown
   - Clean state checkpoint to durable storage
   - Coordinated shutdown across cluster
   - Recovery on restart

3. **Dead node detection & removal**
   - Heartbeat timeout threshold (configurable, default 30s)
   - Auto-remove from Raft quorum after 3 missed heartbeats
   - Alert admin but don't panic
   - Trigger automatic rebalancing

4. **Automatic backup rotation**
   - Daily snapshot → S3 (retain 7 days)
   - Weekly snapshot → Glacier (retain 90 days)
   - Cleanup old snapshots automatically
   - Verify backups monthly (restore test)

**Implementation approach:**
- New module: `crates/claudefs-mgmt/src/recovery_actions.rs`
- Integrates with `health.rs` via callback interface
- Uses A1/A2/A4 APIs to execute corrective measures
- Logs all actions for audit trail

**Tests:**
- Simulate high CPU scenario → verify worker reduction
- Simulate memory pressure → verify cache shrinking
- Kill storage node → verify auto-detection and removal
- Verify backup rotation timing

---

### Block 4: Deployment & Release Pipeline (Days 7-8)

**Goal:** Automated binary building, signing, and staged rollout

**Deliverables:**

1. **Production binary building**
   - `cargo build --release` for all crates
   - Strip debug symbols (reduce binary size)
   - Multi-arch: x86_64 (primary), aarch64 (secondary)
   - Output: `cfs-server-v1.0.0-x86_64.tar.gz`, etc.

2. **Binary signing & verification**
   - Sign with GPG key (stored in AWS Secrets Manager)
   - Create `.asc` file for each binary
   - Publish to GitHub Releases with signed manifest
   - Verify signatures before deployment

3. **Staged rollout automation** (GitHub Actions workflow)
   - Canary: deploy to 1 test node, run POSIX suite for 24h
   - 10% rollout: 1 storage node + 1 client
   - 50% rollout: 3 storage nodes + 1 client
   - 100% rollout: full cluster (10+ nodes)
   - Automatic rollback on health check failure

4. **Release notes generation**
   - Aggregate all commits since last release
   - Extract highlights from CHANGELOG
   - Generate `RELEASE-NOTES.md` automatically
   - Link to full commit history

**Implementation:**
- GitHub Actions workflows: `.github/workflows/release.yml`
- New tools: `tools/build-release.sh`, `tools/rollout.sh`
- Integration with A9's test orchestrator for canary phase

**Tests:**
- Build release binaries locally, verify they run
- Test binary signing and verification
- Simulate staged rollout on test cluster

---

### Block 5: Cost Monitoring & Optimization (Days 9)

**Goal:** Track AWS spend, identify savings, plan capacity

**Deliverables:**

1. **AWS Cost Dashboard** (Grafana + CloudWatch)
   - Daily spend tracker (target: <$10/day dev, <$50/day production)
   - Cost by resource type (EC2, EBS, NAT, data transfer)
   - Spot savings tracking (should save 50-70%)
   - 30-day trend projection

2. **Cost optimization recommendations**
   - Reserved instances analysis (if 6-month commitment makes sense)
   - Unused resources cleanup (dangling EBS, unattached NICs)
   - Data transfer optimization (minimize cross-AZ traffic)
   - Spot interruption handling

3. **Budget alerts**
   - Daily threshold: $15/day → Slack warning
   - Weekly threshold: $100/week → escalate
   - Monthly threshold: $400/month → halt new deployments

**Implementation:**
- New Grafana dashboard: `monitoring/grafana/cost-dashboard.json`
- AWS Cost Explorer integration
- Automated report generation (weekly email)

---

### Block 6: Disaster Recovery & Testing (Day 10)

**Goal:** Verify RTO/RPO targets, test cross-region failover

**Deliverables:**

1. **Backup & recovery procedures**
   - Daily snapshot to S3 (RPO: 24h)
   - Weekly encrypted snapshot to Glacier
   - Cross-region backup replication (optional for Phase 4)
   - Recovery time measurement: RTO target <30 min

2. **Disaster recovery testing**
   - Monthly DR drill: simulate region failure
   - Restore from backup, verify data consistency
   - Document recovery procedures
   - Update RTO/RPO in SLA documentation

3. **Compliance & audit**
   - Backup retention policies (meet regulatory requirements)
   - Encryption at rest and in transit
   - Access logging (who accessed what backup)
   - Regular compliance audits

**Tests:**
- Provision temporary cluster in different region
- Restore from backup, run POSIX suite
- Measure recovery time
- Verify data integrity post-recovery

---

## Phase 4 Success Criteria

✅ **All blocks complete:**

- [ ] Block 1: Terraform modules deploy 5-node test cluster successfully
- [ ] Block 2: All 8 crates export metrics; Grafana dashboards show real data
- [ ] Block 3: health.rs recovery actions execute; dead node auto-removed
- [ ] Block 4: Release pipeline builds, signs, and deploys v1.0.0 binaries
- [ ] Block 5: Cost dashboard shows <$15/day spend (dev cluster)
- [ ] Block 6: DR drill succeeds; RTO <30 min verified

**Build & test:**
- `cargo build --release` succeeds with zero warnings
- `cargo test --release` passes all tests
- `cargo clippy --release` shows zero new warnings
- Multi-node test cluster: POSIX suite passes, Jepsen tests green

**Documentation:**
- Deployment guide: from cloud CLI to running cluster in <30 min
- Operations runbook: updated with recovery procedures
- Disaster recovery guide: step-by-step recovery procedures

---

## Dependencies & Blockers

**Current blockers (from Phase 3):**
- GitHub Issue #27: A8 web_api.rs Send+Sync errors (blocks mgmt crate compilation)
  - **Impact:** health.rs tests can't run until A8 fixes this
  - **Resolution:** A8 to use interior mutability (RwLock/Mutex)

**Future blockers likely:**
- A1-A8 metrics integration requires cooperation from each builder agent
- Terraform modules need AWS account + credentials
- GPG signing key management (store in Secrets Manager)

---

## Estimated Effort

- **Total:** 10 days (5-6 blocks in parallel where possible)
- **Critical path:** Block 1 (infrastructure) → Block 2 (metrics) → Block 3 (recovery)
- **Parallel:** Block 5 (cost monitoring) can start immediately
- **Follow-up:** Block 6 (DR testing) needs infrastructure from Block 1

---

## Success Metrics for Phase 4

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Deployment time** | <5 min | Terraform apply time |
| **Cluster health** | 100% | All nodes healthy, quorum active |
| **Metrics coverage** | 100% | All 8 crates exporting metrics |
| **Dashboard uptime** | 99%+ | Grafana dashboard availability |
| **Recovery automation** | >80% | % of recovery actions auto-executed |
| **Cost tracking** | <$20/day | Dev cluster daily spend |
| **RTO verification** | <30 min | Measured from backup to operational |
| **Build time** | <20 min | Release build time (from clean) |

---

## Next Steps

1. **Resolve A8 blocker** — Contact A8 to fix web_api.rs (GitHub Issue #27)
2. **Approve plan** — Get developer sign-off on Phase 4 roadmap
3. **Start Block 1** — Write Terraform modules for AWS infrastructure
4. **Parallel: Block 5** — Set up AWS Cost Dashboard
5. **Coordinate with A1-A8** — Collect metrics integration requirements

---

## Reference Documents

- `docs/A11-PHASE3-COMPLETION.md` — Phase 3 final summary
- `docs/CI_TROUBLESHOOTING.md` — CI/CD troubleshooting guide
- `docs/DEPLOYMENT-GUIDE.md` — (to be created)
- `docs/DISASTER-RECOVERY.md` — (to be created)
- `.github/workflows/ci-build.yml` — Current CI workflow
- `tools/cfs-failover-test.sh` — Failover testing script
- `tools/cfs-test-orchestrator.sh` — Test orchestrator script

