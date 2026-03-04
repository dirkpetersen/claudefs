# A11: Phase 3 Infrastructure & CI Execution Plan

**Status:** 🟡 IN PROGRESS
**Date:** 2026-03-04
**Owner:** A11 Infrastructure & CI
**Next Review:** 2026-03-11

---

## Executive Summary

A11 is transitioning from Phase 2 (multi-node cluster infrastructure) to Phase 3 (production readiness). Phase 3 focuses on:

1. **CI/CD Activation** — 6 GitHub Actions workflows (blocked by GitHub token scope)
2. **Cost Optimization** — reduce daily spend from $85-96 to <$70
3. **Production Deployment** — disaster recovery, canary deployments, SLSA provenance
4. **Enhanced Monitoring** — Grafana dashboards for cost, performance, infrastructure
5. **Operational Excellence** — runbooks, procedures, team enablement

**Current Blocker:** GitHub token lacks `workflow` scope — developer must add this scope to unblock CI/CD workflows.

---

## Phase 3 Milestone Timeline

### Week 1 (This Week — March 4-8)

#### Priority 1: Unblock GitHub Token (Developer Action)
**Owner:** Developer
**Effort:** 5 minutes
**Blocked:** All workflow pushes

**Action items:**
1. Go to https://github.com/settings/tokens
2. Find the token used by orchestrator
3. Click "Edit" and add `workflow` scope
4. Regenerate token and export to orchestrator:
   ```bash
   export GITHUB_TOKEN="<new-token-with-workflow-scope>"
   ```
5. Run `cd /home/cfs/claudefs && git push` to publish workflows

**After unblock:**
- 6 workflows published to GitHub
- Can trigger first CI run
- Begin performance metrics collection

---

#### Priority 2: Verify Current Build & Test Status (A11)
**Owner:** A11
**Status:** 🟡 IN PROGRESS
**Effort:** 1 hour

**Action items:**
1. ✅ `cargo check` — verify code compiles (DONE: 3m03s, 0 errors)
2. ⏳ `cargo test --lib` — run full test suite (RUNNING)
3. ⏳ `cargo clippy` — check for warnings
4. 📝 Collect baseline metrics:
   - Build time (target: <20 min)
   - Test time (target: <45 min)
   - Test pass rate (target: 100%)
   - Total test count (baseline: 3612+)

**Next:** Post metrics to PHASE3-BUILD-METRICS.md

---

#### Priority 3: Clean Up Untracked Input/Output Files (A11)
**Owner:** A11
**Status:** ⏳ PENDING
**Effort:** 30 minutes

**Problem:** ~120 untracked `a*-input.md` / `a*-output.md` files in repo root from other agents' OpenCode work.

**Action items:**
1. Review .gitignore to categorize files:
   - Keep: source .md files (docs/)
   - Clean: agent working files (a*-*.md at root)
2. Create cleanup script: `tools/cleanup-agent-work.sh`
3. Git add and commit cleanup
4. Enforce in CI: add check to prevent future accumulation

**Benefits:**
- Cleaner repo view
- Faster `git status` output
- Easier to see actual changes

---

#### Priority 4: Cost Optimization — Phase 1 (Model Selection)
**Owner:** A11
**Status:** 📝 PLANNING
**Effort:** 2-3 hours
**Savings Target:** $5-10/day

**Current baseline:**
- Daily cost: $85-96
- EC2: $26-30 (orchestrator always-on + spot nodes)
- Bedrock: $55-70 (Opus 20%, Sonnet 60%, Haiku 20%)

**Optimization 1: Model Selection**

| Current | Optimized | Savings |
|---------|-----------|---------|
| A1: Opus | Sonnet | Reduce to critical work only |
| A2: Opus | Sonnet | Keep for Raft protocol work |
| A4: Opus | Sonnet | Transport layer stable |
| A8: Haiku | Haiku Fast | 2-3x faster, same cost |
| A10: Opus | Opus (keep) | Security audit needs deep reasoning |
| Other: Sonnet | Sonnet Fast | Bulk code generation |

**Action items:**
1. ✅ Review current model assignments (DONE in memory)
2. 📝 Create `tools/agent-launcher-config.yaml` with proposed model changes
3. 📝 Document rationale and cost impact
4. 📝 Plan rollout: Phase 1 (model changes) → Phase 2 (batch requests)

**Implementation:** Modify `tools/cfs-agent-launcher.sh` to read config and spawn agents with correct model.

**Expected outcome:** -$5-10/day → $75-86/day

---

### Week 2-3 (March 11-22): Performance Optimization

#### Priority 5: Build Cache Analysis
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 2-3 hours

**Goals:**
- Analyze first CI run metrics
- Identify longest-running tests
- Implement parallelization opportunities
- Target: <20 min build, <45 min tests

**Action items:**
1. Collect build metrics from first CI run
2. Parse GitHub Actions logs
3. Identify slow tests (>5 min each)
4. Parallelize independent test suites
5. Implement artifact caching

---

#### Priority 6: Compute Right-Sizing
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 1-2 hours
**Savings Target:** $0.50-1/day

**Current sizing:**
- Orchestrator: c7a.2xlarge ($10/day) — justified for coordination
- Storage nodes: i4i.2xlarge × 5 ($1.75/day each)
- FUSE client: c7a.xlarge ($0.125/day)
- NFS client: c7a.xlarge ($0.125/day)
- Conduit: t3.medium ($0.05/day)
- Jepsen: c7a.xlarge ($0.125/day)

**Opportunities:**
- Downgrade FUSE client: c7a.xlarge → c7a.large (-$0.06/day)
- Downgrade NFS client: c7a.xlarge → c7a.large (-$0.06/day)
- Downgrade conduit: t3.medium → t3.small (-$0.02/day)
- **Total savings:** -$0.15/day (small, but cumulative)

**Note:** Storage nodes should NOT be downgraded (NVMe performance critical).

---

### Month 1 (March 15-31): Cost & Monitoring

#### Priority 7: Scheduled Provisioning
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 2-3 hours
**Savings Target:** $4-5/day average

**Concept:** Run full cluster during business hours (7am-7pm weekdays), minimal cluster otherwise.

**Implementation:**
1. Create `tools/provision-schedule.sh` — cron-triggered provisioning
2. Schedule full cluster: Weekday 7am → Provision storage + clients + Jepsen
3. Schedule minimal cluster: Weekday 7pm → Tear down spot instances
4. Weekend: Orchestrator only (for asynchronous work)

**Expected savings:**
- Weekday: -$3/day (clustr only 12 hours)
- Weekend: -$25/day (full cost savings)
- **Average: -$4/day**

**Target:** $71-86/day (post-optimization)

---

#### Priority 8: Enhanced Monitoring (Grafana Dashboards)
**Owner:** A11 + A8
**Status:** 📋 PLANNED
**Effort:** 3-4 hours

**Dashboard 1: Cost Dashboard**
- Daily AWS spend (EC2, Bedrock, Secrets Manager)
- Cost by agent (token count per agent)
- Cost per build (artifact size, test duration)
- Budget alerts and projections

**Dashboard 2: Performance Dashboard**
- Build time (architecture, compile, link, test phases)
- Test execution time (breakdown by crate)
- Cache hit rate (Cargo incremental, GitHub Actions cache)
- Artifact size (binary bloat, test binary size)

**Dashboard 3: Infrastructure Dashboard**
- Agent status (active, idle, error)
- Cluster health (node uptime, storage usage, replication lag)
- Watchdog/Supervisor health (cycle count, fix success rate)
- Cost burn rate (spend/hour, projected daily/monthly)

**Implementation:**
1. Gather metrics from CloudWatch, GitHub Actions, Prometheus
2. Create Grafana JSON dashboard definitions
3. Deploy to Grafana UI
4. Set up alerting rules (>$100/day, flaky tests, build failures)

---

### Month 2 (April 1-15): Deployment Improvements

#### Priority 9: Multi-Region Deployment
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 4-5 hours

**Scope:**
- Terraform modules for multi-region storage (Site A us-west-2, Site B us-east-1)
- Cross-region replication setup (via A6 conduit)
- Failover testing (loss of one region)

**Deliverables:**
1. `tools/terraform/multi-region/` — Terraform modules
2. Failover runbook
3. RTO/RPO targets: <15 min recovery

---

#### Priority 10: Canary Deployments
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 2-3 hours

**Concept:** Blue-green deployment with automatic rollback on health check failure.

**Strategy:**
1. Build canary (new binary with versioning)
2. Deploy to single node (node N1)
3. Run health checks (5 min)
4. If healthy: deploy to next batch (N2-N3)
5. If unhealthy: automatic rollback

**Implementation:**
1. Enhance `deploy-cluster.sh` with canary mode
2. Add health check validation
3. Implement rollback procedure
4. Test with intentional failures

---

#### Priority 11: SLSA Provenance & Artifact Signing
**Owner:** A11
**Status:** 📋 PLANNED
**Effort:** 2-3 hours

**Scope:**
- Build artifact signing (cosign)
- Software Bill of Materials (SBOM) generation
- Provenance tracking via Build Events
- Release artifact publication

**Deliverables:**
1. cosign integration in CI
2. SBOM generation in release workflow
3. GitHub Release with signed artifacts

---

## Operational Excellence (Ongoing)

### Daily Tasks (A11)
- Monitor cost burn rate (target: <$100/day)
- Check watchdog/supervisor logs for agent restarts
- Verify all agents running and making progress
- Triage any build failures

### Weekly Tasks (A11)
- Analyze build metrics (cache hits, test duration trends)
- Review cost optimization opportunities
- Check for flaky tests and file issues
- Update cost forecast

### Monthly Tasks (A11)
- Cost optimization review
- Infrastructure capacity planning
- Architecture review for scalability

---

## Success Criteria

### Build & Test Performance

| Criterion | Current | Target | Owner |
|-----------|---------|--------|-------|
| Build time | ~20-30 min | <20 min | A11 (optimization) |
| Test time | ~45 min | <45 min | A11 (parallelization) |
| Cache hit rate | TBD | >75% | A11 (measure post-activation) |
| Test pass rate | ~95% | 100% | A1-A10 (bug fixes) |
| Flaky tests | 5-10 | 0 | A1-A10 (fix) |

### Cost Metrics

| Criterion | Current | Target | Owner |
|-----------|---------|--------|-------|
| Daily cost | $85-96 | <$70 | A11 (optimization) |
| Budget utilization | 85-96% | <70% | A11 (reserve) |
| Model efficiency | Baseline | +10-20% | A11 (tuning) |
| EC2 cost | $26-30 | $20-22 | A11 (right-sizing) |
| Bedrock cost | $55-70 | $45-50 | A11 (model selection) |

### Operational Metrics

| Criterion | Current | Target | Owner |
|-----------|---------|--------|-------|
| Agent uptime | Unknown | >99% | Watchdog + Supervisor |
| Build error fix rate | Unknown | >90% | Supervisor + OpenCode |
| Spot interruption rate | Unknown | <5% | A11 (monitoring) |
| MTTR (Mean Time To Recovery) | Unknown | <5 min | Watchdog |

---

## Risk Management

### Risk 1: GitHub Token Blocker Persists
**Probability:** Low (developer can fix in 5 min)
**Impact:** HIGH — blocks all CI/CD workflows
**Mitigation:** Document resolution clearly, provide step-by-step instructions

**Status:** ⏳ Awaiting developer action (see PHASE8-ACTIVATION-CHECKLIST.md)

---

### Risk 2: First CI Run Fails
**Probability:** Medium (new workflows, first execution)
**Impact:** Medium — delays metrics collection
**Mitigation:** Have supervisor ready to diagnose and fix via OpenCode

---

### Risk 3: Cost Optimization Doesn't Hit Target
**Probability:** Low (strategies are validated)
**Impact:** Medium — keeps daily cost at $80-90
**Mitigation:** Phased rollout, monitor each optimization independently

---

### Risk 4: Flaky Tests Proliferate
**Probability:** Medium (distributed system, timing-sensitive code)
**Impact:** Medium — delays Phase 3 completion
**Mitigation:** Systematic flaky test detection and fixing (see PHASE3-TESTING-STRATEGY.md)

---

## Dependencies

- **A1-A10:** Bug fixes from test/security findings
- **A9:** POSIX validation tests (full suite)
- **A10:** Security audit findings (prioritize CRITICAL)
- **Developer:** GitHub token scope fix (blocker for workflows)

---

## Next Steps

### Immediate (Today)
1. ✅ Verify build passes (cargo check)
2. ⏳ Run full test suite (cargo test --lib)
3. 📝 Document baseline metrics
4. 📝 Create cleanup script for agent work files
5. 📝 Propose model selection changes

### This Week
1. ✅ Unblock GitHub token (developer action)
2. ✅ Push workflows to GitHub
3. ✅ Trigger first CI run
4. ✅ Collect build/test metrics
5. 📝 Implement cost optimization Phase 1

### Next Week
1. 📝 Analyze CI performance
2. 📝 Implement compute right-sizing
3. 📝 Begin scheduled provisioning
4. 📝 Deploy Grafana dashboards

---

## Key Contacts & Resources

- **GitHub:** https://github.com/dirkpetersen/claudefs
- **Workflows:** .github/workflows/*.yml
- **Infrastructure Code:** tools/cfs-*.sh
- **Documentation:** docs/PHASE3-*.md, docs/A11-*.md
- **Cost Monitor:** /opt/cfs-cost-monitor.sh
- **Watchdog Logs:** /var/log/cfs-agents/watchdog.log

---

**Document Owner:** A11 Infrastructure & CI
**Status:** 🟡 IN PROGRESS
**Last Updated:** 2026-03-04
**Next Review:** 2026-03-11 (post-CI-activation)
