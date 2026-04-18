# A11: Infrastructure & CI — Phase 5 Plan

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING
**Previous Phase:** Phase 4 Block 5 ✅ COMPLETE (2,570+ LOC, 17 tests, all passing)

---

## Executive Summary

Phase 5 focuses on **operational automation and production readiness**. Building on Phase 4's cost monitoring foundation, Phase 5 will deliver:

1. **Terraform automation** for multi-region test cluster provisioning
2. **Preemptible instance lifecycle** management (cost-efficient testing)
3. **GitHub Actions CI/CD** hardening and multi-stage deployment
4. **Monitoring integration** across all subsystems (metrics ↔ alerting ↔ recovery)
5. **GitOps orchestration** for declarative infrastructure

**Target completion:** 3-4 weeks (5 blocks × 3-5 days each)
**Success criteria:** Production-ready cluster with zero manual ops, automated CI/CD

---

## Phase 5 Architecture

### Block 1: Terraform Cluster Provisioning (3-4 days)

**Objective:** Automate 10-node test cluster (1 orchestrator + 9 preemptible)

**Deliverables:**

| Module | Responsibility | LOC | Tests |
|--------|-----------------|-----|-------|
| `terraform/main.tf` | Root module, provider config, backends | 150 | 3 |
| `terraform/orchestrator.tf` | C7a.2xlarge persistent instance | 200 | 4 |
| `terraform/storage_nodes.tf` | 5×i4i.2xlarge with NVMe tagging | 250 | 5 |
| `terraform/client_nodes.tf` | 2×c7a.xlarge (FUSE + NFS clients) | 180 | 3 |
| `terraform/conduit.tf` | t3.medium cloud replication relay | 120 | 2 |
| `terraform/jepsen.tf` | c7a.xlarge Jepsen controller | 130 | 2 |
| `terraform/networking.tf` | VPC, subnets, security groups | 200 | 4 |
| `terraform/monitoring.tf` | CloudWatch, SNS, log groups | 180 | 3 |
| `terraform/modules/` | Reusable: storage, network, compute | 300 | 6 |
| `terraform/variables.tf` | Region, instance types, counts | 100 | 2 |
| `terraform/outputs.tf` | IPs, DNS, cluster topology | 80 | 2 |
| `tools/cfs-terraform.sh` | CLI wrapper (apply, destroy, validate) | 200 | 4 |

**Integration Points:**
- Outputs → tools/cfs-dev for cluster status
- CloudFormation exports for cross-stack references
- SNS → cost monitor alerts
- CloudWatch metrics → Grafana dashboards (A8)

**Test Coverage:**
- ✅ Terraform syntax validation (terraform validate)
- ✅ Instance type availability in region
- ✅ Security group rules (ingress/egress)
- ✅ IAM role attachments
- ✅ VPC subnet sizing (no CIDR conflicts)
- ✅ Cost tags applied correctly
- ✅ Spot instance lifecycle policies
- ✅ Orchestrator persistence tag verified

**Success Criteria:**
- ✅ `terraform apply` creates all 10 nodes in <5 min
- ✅ All nodes tagged with cost attribution
- ✅ Network connectivity verified (ping all nodes)
- ✅ Prometheus scrapes all node metrics
- ✅ Destroy is idempotent (`terraform destroy -auto-approve`)

---

### Block 2: Preemptible Instance Lifecycle Management (3-4 days)

**Objective:** Cost-efficient cluster provisioning with zero manual ops

**Deliverables:**

| Module | Responsibility | LOC | Tests |
|--------|-----------------|-----|-------|
| `tools/cfs-instance-manager.sh` | Spot pricing, lifecycle, replacement | 400 | 6 |
| `tools/cfs-instance-monitor.sh` | Health checks, auto-recovery | 300 | 5 |
| `tools/cfs-disruption-handler.sh` | Spot termination notice handling | 250 | 4 |
| `tools/cfs-node-drain.sh` | Graceful workload migration | 200 | 3 |
| `systemd/cfs-spot-monitor.service` | Daemon for interruption events | 50 | 2 |
| Rust tests | Integration with recovery actions | 400+ | 15 |

**Features:**

1. **Spot Pricing Intelligence:**
   - Query current prices for all instance types
   - Calculate breakeven vs on-demand
   - Auto-launch on price dips (<avg 60%)
   - Termination notice → graceful shutdown

2. **Instance Health:**
   - CloudWatch metrics → Prometheus
   - Failed instances marked for replacement
   - Auto-launch replacement from warm AMI
   - Drain workload before termination

3. **Disruption Handling:**
   - EC2 Instance Metadata Service for termination notices
   - 120s notice → initiate drain
   - Move workload to persistent orchestrator
   - Launch replacement on Spot market

4. **Rollout Coordination:**
   - Integrate with Phase 4 rollout.sh
   - Stage deployments respect instance churn
   - Cost monitor tracks instance lifecycle costs

**Integration Points:**
- Cost monitor (tools/cfs-cost-monitor-enhanced.sh) for pricing
- Terraform for node management
- Prometheus for health metrics
- Recovery actions (A8) for failover
- Deployment pipeline (tools/rollout.sh) for stage progression

**Test Coverage:**
- ✅ Spot pricing queries (mock AWS API)
- ✅ Termination notice handling (2-minute drain window)
- ✅ Node drain without data loss
- ✅ Replacement instance launch and bootstrap
- ✅ Cost calculation for preemption + replacement
- ✅ Concurrent terminations (up to 3 nodes)
- ✅ Cloud conduit failover during drain

**Success Criteria:**
- ✅ Spot interruption → graceful shutdown in <2 min
- ✅ Replacement node online in <3 min
- ✅ Zero data loss during replacement
- ✅ Monthly cost reduction: 60-70% via spot
- ✅ All logs to `/var/log/cfs-instance-*.log`

---

### Block 3: GitHub Actions CI/CD Hardening (3-4 days)

**Objective:** Production-grade CI/CD with automated testing & deployment

**Deliverables:**

| Workflow | Trigger | Jobs | Actions | Tests |
|----------|---------|------|---------|-------|
| `build.yml` | Push to main | 3 (test, clippy, build) | Matrix (stable, nightly) | 5 |
| `test.yml` | Push to main | 5 (unit, integration, posix, jepsen, bench) | Parallel | 8 |
| `security.yml` | Daily + PR | 4 (audit, clippy, SBOM, scan) | cargo-audit, license | 6 |
| `deploy.yml` | Manual trigger | 4 (canary, 10%, 50%, 100%) | `tools/rollout.sh` | 7 |
| `cost-report.yml` | Daily 9am | 2 (analyze, notify) | Generate reports, SNS | 4 |
| `metrics-export.yml` | Every 5min | 1 (collect, export) | Prometheus push | 3 |

**Key Components:**

1. **Build Pipeline:**
   - Rust cache (cargo incremental)
   - LTO for release builds (60% smaller)
   - Binary signing with GPG
   - S3 artifact storage + CloudFront CDN

2. **Test Matrix:**
   - Unit tests: all platforms (ubuntu-latest, macos-latest, windows-latest)
   - Integration tests: ubuntu-latest (io_uring required)
   - POSIX compliance: pjdfstest, xfstests, fsx (full suite on release)
   - Chaos: Jepsen tests (partition tolerance, Byzantine)
   - Benchmarks: FIO perf baseline tracking

3. **Security Scanning:**
   - `cargo audit` for CVEs
   - `cargo clippy` for potential bugs
   - SBOM generation (syft)
   - License compliance (license-rs)
   - Secret scanning (truffleHog)

4. **Deployment Automation:**
   - Manual approval gates before each stage
   - Cost attribution by stage
   - Rollback on health check failure
   - Canary metrics collection

5. **Cost Tracking:**
   - Compute cost per commit (build time × node cost)
   - Test duration tracking
   - Spot instance cost allocation
   - Daily reports to Grafana

**Integration Points:**
- Artifact storage: S3 + CloudFront
- Status reporting: GitHub + Slack (SNS)
- Cost tracking: CloudWatch metrics
- Secrets: AWS Secrets Manager (GPG keys, GitHub token)
- Permissions: OIDC federation (no long-lived keys)

**Test Coverage:**
- ✅ Workflow syntax validation
- ✅ Matrix expansion (platform × rust-version)
- ✅ Artifact upload/download (S3)
- ✅ GPG signature verification
- ✅ Secret masking in logs
- ✅ Concurrent builds (no race conditions)
- ✅ Rollback on security scan failure
- ✅ Cost attribution accuracy

**Success Criteria:**
- ✅ Full test suite completes in <45 min
- ✅ Build + test + security in <60 min total
- ✅ Deploy stage completion in <15 min per stage
- ✅ Zero secrets in logs or artifacts
- ✅ 99%+ uptime (no workflow hangs)
- ✅ SBOM generated + accessible

---

### Block 4: Monitoring Integration & Alerting (3-5 days)

**Objective:** Comprehensive observability: metrics → alerts → recovery

**Deliverables:**

| Component | Purpose | LOC | Tests |
|-----------|---------|-----|-------|
| `tools/cfs-alert-aggregator.sh` | Dedupe & aggregate alerts | 250 | 4 |
| `tools/cfs-runbook.sh` | Automated remediation workflows | 300 | 5 |
| `tools/cfs-metrics-collector.sh` | Custom metric collection | 200 | 3 |
| Grafana dashboards (A8 integration) | Visualization (dashboards created) | 800 | 8 |
| Prometheus alerting rules | Alert thresholds & routing | 150 | 3 |
| Recovery action hooks | Trigger automated recovery | 200+ LOC | 5 |
| Rust integration tests | End-to-end monitoring + recovery | 300+ LOC | 6 |

**Monitoring Layers:**

1. **Infrastructure Metrics:**
   - EC2 node state (running, stopping, terminated)
   - Spot pricing & interruption notices
   - CloudWatch: CPU, memory, disk I/O, network
   - EBS volume health (EBS snapshots)

2. **Application Metrics (from A1-A8):**
   - Storage: IOPS, latency, throughput, cache hit ratio
   - Metadata: Raft consensus health, quorum status
   - Replication: lag, split-brain events, repair actions
   - Transport: RPC latency, connection pool status
   - FUSE: mount status, operation latency, error rate
   - Gateway: NFS/SMB/S3 request counts, error rate
   - Management: query latency, export success rate

3. **System Alerts:**
   - Critical: Node down, quorum loss, split-brain, replication lag >5min
   - Warning: Disk >80%, CPU >70%, replication lag >60s, error rate >1%
   - Info: Deployment stage change, cost forecast, planned maintenance

4. **Automated Recovery:**
   - Node down → initiate replacement (Block 2)
   - Quorum loss → scale down to 2-node cluster
   - Replication lag >5min → switch to read-only mode
   - Disk >90% → emergency cleanup + alert operator

**Alert Rules (Prometheus):**

```yaml
- alert: NodeDown
  expr: up{job="node-exporter"} == 0 for 2m
  action: replace_instance (Block 2)

- alert: QuorumLoss
  expr: raft_quorum_count < 2 for 1m
  action: failover_to_secondary (A6)

- alert: ReplicationLagCritical
  expr: repl_lag_seconds > 300 for 1m
  action: scale_down_to_2_nodes

- alert: DiskFull
  expr: node_filesystem_avail_bytes / node_filesystem_size_bytes < 0.1 for 5m
  action: trigger_gc (A3)
```

**Integration Points:**
- Grafana dashboards (A8 Block 4: 5 dashboards)
- Prometheus (metrics collection)
- AlertManager (deduplication, routing)
- SNS (notifications to teams)
- A6 recovery actions (automated remediation)
- A8 management API (runbook execution)

**Test Coverage:**
- ✅ Metric scrape success (Prometheus healthy)
- ✅ Alert rule evaluation (correct thresholds)
- ✅ AlertManager deduplication (no duplicate alerts)
- ✅ Runbook execution (recovery script success)
- ✅ End-to-end: metric spike → alert → recovery action
- ✅ False positive handling (alert suppression windows)
- ✅ Multi-alert coordination (quorum loss handling)

**Success Criteria:**
- ✅ All metrics scraping successfully (0% errors)
- ✅ Alert evaluation latency <10s
- ✅ Recovery action latency <30s
- ✅ No false positives (>95% correctness)
- ✅ Runbook success rate >95%
- ✅ Dashboard load time <3s
- ✅ MTTR (Mean Time To Recovery) <5 min

---

### Block 5: GitOps & Declarative Infrastructure (3-4 days)

**Objective:** Infrastructure-as-code with declarative state management

**Deliverables:**

| Component | Purpose | LOC | Tests |
|-----------|---------|-----|-------|
| `infra/` directory structure | Git-based config | 500 | 4 |
| `infra/cluster.yaml` | Declarative cluster topology | 200 | 2 |
| `infra/rollout.yaml` | Deployment stages & percentages | 150 | 2 |
| `tools/cfs-gitops-controller.sh` | Reconciliation loop | 300 | 5 |
| `tools/cfs-config-validator.sh` | Schema validation | 200 | 4 |
| Rust tests | GitOps + Terraform reconciliation | 300+ LOC | 6 |

**GitOps Concepts:**

1. **Declarative State:**
   - `infra/cluster.yaml` → desired topology
   - `infra/rollout.yaml` → deployment stages
   - `infra/monitoring.yaml` → alert rules
   - `infra/secrets.yaml` (encrypted) → credentials

2. **Reconciliation Loop:**
   - Every 5 minutes: read `infra/*.yaml`
   - Compare desired vs actual (Terraform state)
   - Detect drift (manual changes)
   - Apply changes via Terraform or alert operator

3. **Version Control:**
   - All infrastructure changes via git commits
   - Pull requests for review before apply
   - Git history = audit trail
   - Rollback = git revert

4. **Multi-Environment:**
   - `infra/dev/` — local testing (1 node)
   - `infra/staging/` — canary cluster (3 nodes)
   - `infra/prod/` — production (10+ nodes, multi-region)

**Workflow:**

```
Developer:
  git clone claudefs
  cd infra/dev
  vim cluster.yaml (increase storage nodes)
  git commit -m "[A11] Scale cluster to 6 storage nodes"
  git push

CI/CD:
  GitHub Actions: terraform plan (shows diff)
  Manual approval gate
  terraform apply (reconciles desired state)
  Verify cluster converged (all nodes up, metrics flowing)
  Post success to GitHub
```

**Integration with Terraform:**

- Terraform remote state in S3 (tfstate bucket)
- YAML → Terraform variables (generated)
- GitOps controller → terraform apply/destroy
- Backup state before apply (snapshots)

**Test Coverage:**
- ✅ YAML schema validation (cluster.yaml)
- ✅ Terraform variable generation from YAML
- ✅ Drift detection (manual vs GitOps changes)
- ✅ Concurrent reconciliation (idempotent)
- ✅ Rollback via git revert
- ✅ Multi-environment isolation
- ✅ Secrets encryption (sealed-secrets or AWS Secrets Manager)

**Success Criteria:**
- ✅ All cluster changes go through git
- ✅ Audit trail: git log shows all changes
- ✅ Rollback time <2 min
- ✅ Drift detection latency <5 min
- ✅ Zero unintended infrastructure changes
- ✅ Multi-region cluster support (via YAML)

---

## Phase 5 Timeline & Dependencies

### Critical Path

```
Week 1: Block 1 (Terraform provisioning)
  ├─ Days 1-2: Core Terraform modules + networking
  ├─ Day 3: Testing + AWS account setup
  └─ Day 4: Integration with tools/cfs-dev

Week 2: Block 2 (Preemptible management) + Block 3 (CI/CD)
  ├─ Days 1-2: Spot lifecycle + CI/CD workflows (parallel)
  ├─ Day 3: Integration testing
  └─ Day 4: Documentation + handoff

Week 3: Block 4 (Monitoring) + Block 5 (GitOps)
  ├─ Days 1-3: Monitoring integration (depends on A8 Block 4)
  ├─ Day 3-4: GitOps implementation (parallel)
  └─ Final: End-to-end validation

Week 4: Phase 5 completion + Phase 6 planning
```

### Dependencies

| Block | Depends On | Status | Note |
|-------|-----------|--------|------|
| Block 1 | AWS account, Terraform installed | ✅ Ready | No blockers |
| Block 2 | Block 1, EC2 IAM permissions | ✅ Ready | Terraform outputs |
| Block 3 | GitHub Actions, S3 bucket | ✅ Ready | (existing) |
| Block 4 | A8 Block 4 (dashboards), Prometheus | 🟡 In Progress | A8 planning complete |
| Block 5 | Block 1-3 (foundation) | ✅ Ready | Builds on others |

### Parallel Opportunities

- Blocks 1, 2, 3 can start simultaneously (mostly independent)
- Block 4 can start after Block 1 (depends on node infrastructure)
- Block 5 can start once Block 1 is complete

---

## Success Criteria: Phase 5

### Deliverables ✅

- [ ] Terraform automation fully operational (all 10 nodes, <5 min provision)
- [ ] Preemptible instance lifecycle management (zero manual ops)
- [ ] GitHub Actions CI/CD (60 min full pipeline, all tests passing)
- [ ] Monitoring integration complete (metrics flowing, alerts firing)
- [ ] GitOps enabled (all infra changes via git)

### Quality Metrics ✅

- [ ] 50+ new Rust tests (600+ LOC), all passing
- [ ] 1,500+ lines of Terraform code (production-ready)
- [ ] 6 new GitHub Actions workflows (500 LOC YAML)
- [ ] 5 CloudWatch dashboards (via A8 integration)
- [ ] 100% code coverage for CI/CD orchestration

### Operational Goals ✅

- [ ] Cluster provision/destroy: fully automated
- [ ] Cost per test run: tracked in CloudWatch
- [ ] Deployment automation: canary → 10% → 50% → 100%
- [ ] Monitoring latency: <10s (metric → alert)
- [ ] Recovery latency: <5 min (alert → action)
- [ ] MTTR (Mean Time To Recovery): <5 min for any failure

### Documentation ✅

- [ ] Terraform README (150+ lines) — how to provision, customize, debug
- [ ] Preemptible management guide (200+ lines) — spot lifecycle, cost optimization
- [ ] CI/CD runbook (300+ lines) — workflow triggers, manual gates, debugging
- [ ] Monitoring runbook (250+ lines) — alert thresholds, remediation, escalation
- [ ] GitOps guide (200+ lines) — declarative state, multi-environment, rollback

---

## Resource Allocation

### Time Estimate: 15-20 engineer-hours

| Block | Hours | Notes |
|-------|-------|-------|
| Block 1 | 4-5 | Terraform modules, networking, testing |
| Block 2 | 3-4 | Spot pricing logic, lifecycle management |
| Block 3 | 4-5 | 6 workflow files, secret management, artifact handling |
| Block 4 | 3-4 | Integration with A8, alert rules, recovery hooks |
| Block 5 | 2-3 | YAML schema, reconciliation logic, rollback testing |
| **Total** | **16-21** | ~3-4 weeks of work |

### Cost Estimate (AWS)

| Resource | Daily | Phase Duration | Total |
|----------|-------|-----------------|-------|
| EC2 orchestrator | $10 | 28 days | $280 |
| 9 preemptible nodes | $16 | 20 days (periodic) | $320 |
| Terraform state (S3) | $0.02 | 28 days | $0.50 |
| **Total** | **~$26** | **Phase 5** | **~$600** |

---

## Risk Mitigation

### Risks

1. **Spot interruption race conditions**
   - Mitigation: Graceful drain window (2 min), replacement launch in parallel
   - Test: Simulate interruptions, verify recovery

2. **Terraform state corruption**
   - Mitigation: State backups before every apply, remote state lock
   - Test: Backup restore procedure

3. **CI/CD secret leakage**
   - Mitigation: OIDC federation, no long-lived keys, secret scanning
   - Test: Audit logs for unauthorized access

4. **Alert storm (too many alerts)**
   - Mitigation: AlertManager deduplication, grouped notifications
   - Test: Simulate mass failures, verify grouping

5. **GitOps drift detection misses manual changes**
   - Mitigation: Dedicated reconciliation loop, human-in-the-loop review
   - Test: Drift injection tests, manual verification

---

## Next Steps (Implementation)

### Week 1 Tasks

1. **Task 1.1:** Set up AWS account & Terraform backend
   - S3 bucket + DynamoDB lock table
   - IAM roles for Terraform execution
   - Estimated: 1 hour

2. **Task 1.2:** Write Terraform modules (orchestrator, nodes, networking)
   - Estimated: 2-3 hours
   - Reference: AWS Terraform examples, existing security groups

3. **Task 1.3:** Integration testing + documentation
   - Estimated: 1-2 hours
   - Create README, example commands

### Week 2 Tasks

4. **Task 2.1:** Preemptible instance manager
   - Spot pricing queries, lifecycle handling
   - Estimated: 2-3 hours

5. **Task 3.1:** GitHub Actions workflows
   - Build, test, security, deploy, cost-report
   - Estimated: 2-3 hours

### Week 3 Tasks

6. **Task 4.1:** Monitoring integration
   - Prometheus rules, AlertManager config, runbooks
   - Estimated: 2-3 hours
   - Depends on: A8 Block 4 completion

7. **Task 5.1:** GitOps controller
   - YAML parsing, reconciliation loop, validation
   - Estimated: 1-2 hours

---

## References

- [Phase 4 Final Status](A11-PHASE4-BLOCK5-SESSION9-STATUS.md)
- [Terraform Best Practices](https://www.terraform.io/docs/language/modules/develop/best-practices.html)
- [GitHub Actions Security](https://docs.github.com/en/actions/security-guides)
- [Prometheus Alerting](https://prometheus.io/docs/alerting/latest/overview/)
- [CNCF GitOps Best Practices](https://opengitops.dev/)

---

**Document created:** 2026-04-18 Session 10
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING — Ready for implementation approval
