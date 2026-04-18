# A11: Phase 5 Block 5 — GitOps Orchestration — Comprehensive Plan

**Date:** 2026-04-18 Session 16+
**Agent:** A11 Infrastructure & CI
**Phase:** 5 (Final Block)
**Target:** 10-15 tests, 700+ LOC, 2-3 days implementation

---

## Mission Statement

Implement GitOps orchestration for ClaudeFS test cluster: declarative infrastructure-as-code (IaC), automated cluster deployment from git commits, self-healing based on monitoring signals, and automated rollbacks on critical failures. Enable developers to trigger cluster upgrades and configuration changes via git operations without manual SSH or kubectl commands.

---

## Scope: 5 Deliverables

### D1: GitOps Controller (Rust Tests + Documentation)
**Purpose:** Continuous reconciliation between git HEAD and running cluster state.

**Deliverables:**
- Git-based declarative cluster config format (YAML)
- Controller logic (polling + webhook support)
- Deployment pipeline (Terraform → Kubernetes → ClaudeFS)
- Rollback automation on cluster health degradation
- 3-5 integration tests

**Acceptance Criteria:**
- Controller reads cluster state from git
- Detects drift (running state != git HEAD)
- Applies changes via Terraform
- Tests validate config parsing, drift detection, reconciliation

**Files:**
- `tools/cfs-gitops-controller.sh` (500+ LOC shell script)
- `crates/claudefs-tests/src/gitops_orchestration_tests.rs` (3-5 tests, 150+ LOC)
- `docs/gitops-guide.md` (operator guide)

---

### D2: Declarative Cluster Configuration
**Purpose:** GitOps source of truth — all cluster configuration versioned in git.

**Deliverables:**
- Cluster manifest: `infrastructure/cluster.yaml`
  - Cluster name, version, region
  - Node count, instance types (storage/client/conduit/jepsen)
  - DNS, network, security groups
  - Monitoring (Prometheus scrape configs)
  - Alert rules (Prometheus alert rules)

- Component manifests: `infrastructure/components/`
  - `prometheus.yaml` (declarative scrape config)
  - `alertmanager.yaml` (declarative routing)
  - `dashboards.yaml` (declarative Grafana provisioning)
  - `ingress.yaml` (access rules for Prometheus/Grafana)

- Patch strategy:
  - `infrastructure/patches/` for environment-specific overrides
  - `infrastructure/kustomization.yaml` for Kustomize integration

**Acceptance Criteria:**
- All cluster state lives in infrastructure/ directory
- No manual changes to cluster outside git
- Config parseable YAML, validated against schema
- Supports versioning: cluster can run on multiple schema versions during migration

**Files:**
- `infrastructure/cluster.yaml` (150+ LOC)
- `infrastructure/components/*.yaml` (400+ LOC combined)
- `infrastructure/patches/` (100+ LOC)
- `infrastructure/kustomization.yaml` (50+ LOC)

**Tests:**
- Schema validation (YAML parses correctly)
- Completeness checks (all required fields present)
- Consistency checks (no conflicting configurations)

---

### D3: Automated Drift Detection & Reconciliation
**Purpose:** Detect when running cluster diverges from git config, automatically reconcile.

**Deliverables:**
- Drift detection script: `tools/cfs-drift-detector.sh`
  - Polls AWS EC2 and Terraform state
  - Compares to git HEAD cluster config
  - Categorizes drift: infrastructure (tags, instance type), software (versions, configs), monitoring (alert rules)
  - Reports drift with severity (critical, warning, info)

- Reconciliation logic:
  - **Infrastructure drift:** `terraform apply` to match git state
  - **Software drift:** Redeploy affected components (Prometheus, Grafana, ClaudeFS nodes)
  - **Config drift:** Hot-reload where possible (Prometheus alert rules via API), restart where necessary

- Automation:
  - Cron job every 15 minutes (detect drift)
  - Auto-reconcile for low-risk changes (config hotloads)
  - Require manual approval for high-risk changes (infrastructure, node replacement)

**Acceptance Criteria:**
- Drift detection runs every 15 min
- Reports to Prometheus metrics: `claudefs_drift_detected`, `claudefs_drift_reconciled`
- Auto-reconciliation captures in logs with traceability
- Test verifies detection of 5 drift types

**Files:**
- `tools/cfs-drift-detector.sh` (350+ LOC)
- Cron entry in `tools/cfs-agent-launcher.sh`
- Tests (2-3 tests)

---

### D4: Self-Healing & Auto-Remediation
**Purpose:** Automatically recover from known failure modes without manual intervention.

**Deliverables:**
- Remediation rules engine: `tools/cfs-remediation-rules.yaml`
  - Rule 1: High CPU on node → scale resources or evict workload
  - Rule 2: High memory pressure → trigger GC/cleanup
  - Rule 3: Spot interruption detected → drain node and rebalance
  - Rule 4: Prometheus down → restart and re-scrape
  - Rule 5: AlertManager receiver misconfigured → reload from config

- Auto-remediation actions:
  - **Scale:** Increase instance type, add nodes
  - **Restart:** Graceful shutdown + re-launch service
  - **Evict:** Move workload to healthier node
  - **Rollback:** Revert last code deployment
  - **Notify:** Escalate to human if auto-fix fails

- Integration with monitoring:
  - Prometheus alerts trigger remediation actions
  - AlertManager webhook → remediation engine
  - Action captured as GitHub Issue for human review

**Acceptance Criteria:**
- Rules engine parses YAML correctly
- Each rule has clear trigger (alert), action, and fallback
- Auto-remediation tested for 3 scenarios
- Failed auto-remediation creates GitHub Issue for investigation

**Files:**
- `tools/cfs-remediation-rules.yaml` (200+ LOC)
- `tools/cfs-remediation-engine.sh` (400+ LOC)
- Tests (2-3 tests)

---

### D5: Automated Rollback on Critical Failures
**Purpose:** Revert to last-known-good cluster state if critical failures detected.

**Deliverables:**
- Checkpoint mechanism:
  - Store cluster config snapshot at each successful deployment
  - Snapshots: git tag `cluster-working-<timestamp>`, S3 backup of Terraform state
  - Keep 5 most recent snapshots (7 days retention)

- Rollback decision logic:
  - Trigger: Critical alert fires for >5 minutes
  - Validation: Run smoke tests (cluster is reachable, basic ops work)
  - If smoke tests fail: Automatic rollback to last snapshot
  - If rollback succeeds: Create GitHub Issue for root cause analysis

- Automated rollback steps:
  1. Mark current deployment as "bad"
  2. Restore Terraform state from last snapshot
  3. `terraform apply` to recreate infrastructure
  4. Redeploy software (Prometheus, Grafana, ClaudeFS services)
  5. Run smoke tests to verify recovery
  6. If recovery successful: Create "Rollback Incident" Issue on GitHub

**Acceptance Criteria:**
- Snapshots created on each successful deployment
- Rollback initiated on critical failure + 5 min timeout
- Smoke tests validate cluster health before + after rollback
- Test verifies rollback recovery from 2 failure scenarios

**Files:**
- `tools/cfs-checkpoint-manager.sh` (250+ LOC)
- `tools/cfs-rollback-engine.sh` (300+ LOC)
- Tests (2 tests)

---

## Implementation Plan

### Phase 1: Planning & Specification (Day 1)
- [x] Create this comprehensive plan
- [ ] Review existing Terraform modules (infrastructure/terraform/)
- [ ] Map current cluster state to YAML config format
- [ ] Define schema for cluster.yaml and components
- [ ] Prepare OpenCode input prompt

**Deliverable:** `a11-phase5-block5-input.md` (600+ lines)

### Phase 2: GitOps Infrastructure (Days 1-2)
- [ ] Generate GitOps controller script (D1)
- [ ] Generate declarative cluster config (D2)
- [ ] Generate drift detector (D3)
- [ ] Generate remediation rules + engine (D4)
- [ ] Generate checkpoint/rollback tools (D5)

**OpenCode Runs:**
- Run 1: `a11-phase5-block5-input.md` → 5 shell scripts + YAML configs (1,500+ LOC)
- Run 2: (if needed) Corrections/refinements

**Deliverable:** `a11-phase5-block5-output.md`

### Phase 3: Rust Integration Tests (Day 2)
- [ ] Create GitOps orchestration test module
  - Test 1: Cluster config parsing (YAML validates)
  - Test 2: Drift detection logic (5 drift types detected)
  - Test 3: Remediation rules engine (rules parse, actions valid)
  - Test 4: Checkpoint creation/management (snapshots stored)
  - Test 5: Rollback scenario (recovery from failure)
  - Test 6-10: End-to-end scenarios (config change → deploy → verify)

**Deliverable:** `crates/claudefs-tests/src/gitops_orchestration_tests.rs` (400+ LOC, 10 tests)

### Phase 4: Validation & Deployment (Day 3)
- [ ] Validate YAML configs: syntax, schema, completeness
- [ ] Validate shell scripts: bash syntax, executability, error handling
- [ ] Build and test: `cargo build -p claudefs-tests`
- [ ] Manual validation: Parse cluster.yaml, simulate drift detection, verify rules engine
- [ ] Create validation script: `tools/validate-gitops-config.sh`

**Acceptance:** Zero build errors, zero warnings, all tests passing

### Phase 5: Documentation & Commit (Day 3)
- [ ] Update CHANGELOG with comprehensive summary
- [ ] Create operator guide: `docs/gitops-setup.md`
- [ ] Create developer guide: `docs/gitops-usage.md` (how to trigger deployments via git)
- [ ] Stage and commit all changes
- [ ] Push to GitHub main branch

**Commits:**
1. `[A11] Phase 5 Block 5: GitOps Orchestration — 10-15 tests, 700+ LOC`
2. `[A11] Update CHANGELOG — Phase 5 Complete (All 5 Blocks, 95+ Tests)`

---

## Detailed Implementation Spec

### GitOps Configuration Format

**Example: `infrastructure/cluster.yaml`**
```yaml
apiVersion: claudefs.io/v1alpha1
kind: Cluster
metadata:
  name: cfs-dev-cluster
  region: us-west-2
  environment: development
spec:
  version: 2026.04  # ClaudeFS version to deploy

  nodes:
    storage:
      count: 5
      instance_type: i4i.2xlarge
      volume_size_gb: 500
      tags:
        role: storage

    clients:
      count: 2
      instance_type: c7a.xlarge
      tags:
        role: client

    conduit:
      count: 1
      instance_type: t3.medium
      tags:
        role: conduit

    jepsen:
      count: 1
      instance_type: c7a.xlarge
      tags:
        role: jepsen

  monitoring:
    prometheus:
      enabled: true
      scrape_interval_seconds: 15
      retention_days: 30

    alertmanager:
      enabled: true

    grafana:
      enabled: true
      admin_password_secret: grafana-admin-pwd  # from AWS Secrets Manager
```

### Drift Detection Categories

1. **Infrastructure Drift:**
   - Instance count mismatch
   - Instance type changes
   - Volume size changes
   - Security group changes
   - Tag mismatches

2. **Software Drift:**
   - ClaudeFS version running != declared version
   - Prometheus version mismatch
   - Grafana version mismatch
   - Configuration file hashes differ

3. **Config Drift:**
   - Prometheus scrape_configs changed
   - Alert rules modified outside git
   - Grafana dashboard config changed

4. **Monitoring Drift:**
   - Alert rule firing patterns changed
   - Metrics collection gaps

5. **Deployment Drift:**
   - Node was manually SSH'd and modified
   - Service wasn't restarted on config change

### Remediation Rules Example

```yaml
# File: tools/cfs-remediation-rules.yaml
rules:
  - name: "High CPU on Node"
    trigger: "node_cpu_usage > 85 for 5m"
    actions:
      - type: scale
        target: affected_instance
        action: increase_instance_type
      - type: evict
        workload: highest_cpu_consumer
        destination: healthier_node
    fallback: notify_admin

  - name: "Spot Interruption"
    trigger: "spot_interruption_notice_detected"
    actions:
      - type: drain
        node: interrupted_instance
        timeout_seconds: 300
      - type: rebalance
        strategy: consistent_hash
      - type: provision_replacement
        instance_type: same_as_interrupted
    fallback: failover_to_backup_node

  - name: "Prometheus Down"
    trigger: "alert_prometheus_down"
    actions:
      - type: restart
        service: prometheus
        graceful_timeout: 30
        max_retries: 3
      - type: verify
        check: prometheus_responding
    fallback: escalate_to_human
```

---

## Test Strategy

### Test Module: `gitops_orchestration_tests.rs`

**Group 1: Configuration Parsing** (2 tests)
- `test_cluster_config_valid_yaml` — Parse infrastructure/cluster.yaml
- `test_components_config_valid` — Parse all infrastructure/components/*.yaml

**Group 2: Drift Detection** (3 tests)
- `test_drift_detector_detects_count_change` — Instance count mismatch
- `test_drift_detector_detects_config_change` — Config file hash mismatch
- `test_drift_detector_categorizes_drift_severity` — Severity classification

**Group 3: Remediation Rules** (2 tests)
- `test_remediation_rules_parse` — YAML parse + validation
- `test_remediation_rules_execution_logic` — Trigger + action matching

**Group 4: Checkpoint & Rollback** (2 tests)
- `test_checkpoint_creation_and_storage` — Snapshot creation, tagging
- `test_rollback_scenario_recovery` — Simulate failure + rollback + verification

**Group 5: End-to-End Scenarios** (1-3 tests)
- `test_gitops_config_change_deployment` — Push config change → verify deployment
- `test_gitops_drift_auto_reconciliation` — Detect drift → auto-fix
- `test_gitops_critical_failure_rollback` — Critical alert → auto-rollback

**Total: 10-13 tests, 400+ LOC**

---

## Success Criteria

### Code Quality
- ✅ All Rust compiles cleanly: `cargo build -p claudefs-tests`
- ✅ Zero new warnings from clippy
- ✅ All shell scripts pass shellcheck
- ✅ All YAML configs pass yamllint
- ✅ All JSON configs pass jq validation

### Functionality
- ✅ GitOps controller reads cluster config from git
- ✅ Drift detector runs every 15 min, reports to Prometheus
- ✅ Remediation rules engine executes actions on alert triggers
- ✅ Checkpoint system creates snapshots at each successful deployment
- ✅ Rollback mechanism recovers from critical failures
- ✅ 10+ integration tests all passing

### Documentation
- ✅ Operator guide explains how to deploy and configure
- ✅ Developer guide explains how to trigger deployments via git
- ✅ CHANGELOG updated with comprehensive summary
- ✅ Comments in code explain complex logic

### Git & CI/CD
- ✅ All changes staged and committed
- ✅ Commit messages follow format: `[A11] <scope> — <summary>`
- ✅ Pushed to GitHub main branch
- ✅ CI/CD pipeline passes (tests, security scans, artifact builds)

---

## Effort Estimation

| Phase | Task | Time | Notes |
|-------|------|------|-------|
| 1 | Planning + review existing Terraform | 2-3h | Understanding current infra |
| 2a | First OpenCode run (scripts + configs) | 45-60m | GitOps controller, configs, scripts |
| 2b | Review + validation of first output | 30m | Check syntax, completeness |
| 2c | Correction run (if needed) | 30-45m | Fix any syntax/logic issues |
| 3 | Rust test generation + build | 45-60m | 10-13 tests, validation |
| 4 | Manual validation + operator review | 1h | Dry-run scenarios, edge cases |
| 5 | Documentation + commits + push | 30-45m | CHANGELOG, guides, final push |
| **Total** | | **6-9 hours** | 1-2 developer days |

---

## Risk Mitigation

### Risk 1: Over-complexity in GitOps Logic
**Mitigation:** Start simple (drift detection), add auto-remediation incrementally. Focus on high-confidence scenarios first.

### Risk 2: False Positive Drift Detections
**Mitigation:** Configure grace period (5 min) before triggering auto-remediation. Log all detections for review.

### Risk 3: Rollback Failures
**Mitigation:** Smoke tests before and after rollback. If rollback fails, escalate to human (create GitHub Issue, don't auto-retry).

### Risk 4: Git Config Corruption
**Mitigation:** Schema validation on every git push. Terraform plan always runs before apply. Use git revert, not reset --hard, for recovery.

---

## Phase 5 Completion Status (After Block 5)

**Target Completion:**
- Block 1: ✅ Terraform infrastructure (36 tests)
- Block 2: ✅ Preemptible instances (17 tests)
- Block 3: ✅ CI/CD hardening (12 tests)
- Block 4: ✅ Monitoring integration (20 tests)
- Block 5: ⏳ GitOps orchestration (10-15 tests)

**Total: 95-100 tests, 10K+ LOC infrastructure + testing**

---

## Next Steps After Phase 5

### Phase 6: Advanced Cluster Operations
- Multi-cluster orchestration (cross-site failover)
- Advanced disaster recovery (S3-based backups)
- Performance optimization (auto-scaling, predictive provisioning)

### Integration with Builders (A1-A8)
- Builders deploy code → triggers Git commit → GitOps redeploys cluster
- Monitoring alerts → auto-remediation → GitHub Issues for builders to investigate

---

**Status: PLAN READY FOR OPENCODE GENERATION**

Next: Create detailed OpenCode input prompt in `a11-phase5-block5-input.md`

