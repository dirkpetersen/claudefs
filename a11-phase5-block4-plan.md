# A11: Phase 5 Block 4 — Monitoring Integration — PLANNING DOCUMENT

**Date:** 2026-04-18 Session 15 (Planning)
**Agent:** A11 Infrastructure & CI
**Model:** Haiku (planning) → minimax-m2p5 (implementation)
**Phase:** 5 | **Block:** 4
**Target:** Comprehensive monitoring integration with Prometheus, alerting, dashboards, and cost tracking
**Status:** 🟡 PLANNING IN PROGRESS

---

## Executive Summary

Phase 5 Block 4 integrates Prometheus monitoring into the ClaudeFS infrastructure, providing:

1. **Metrics Collection:** Prometheus scrape targets for test cluster nodes (storage, metadata, clients)
2. **Alert Rules:** Automated detection of infrastructure failures, cost overruns, performance regressions
3. **Dashboards:** Grafana dashboards for cluster health, CI/CD metrics, cost visualization
4. **Cost Tracking:** Automated cost attribution per component (storage nodes, clients, CI runners)
5. **Performance Baselines:** Baseline metrics for FIO, POSIX test suites, latency profiles
6. **Integration Tests:** Comprehensive Rust tests validating Prometheus scraping, alerting, dashboard provisioning

**Target Outcome:**
- ✅ Prometheus server configuration (200 LOC) with alert rules
- ✅ 3-4 Grafana dashboards (500+ LOC JSON) covering infrastructure, CI/CD, costs
- ✅ Alert rule validation & notification hooks (200 LOC)
- ✅ 15-20 Rust integration tests (600-800 LOC)
- ✅ Cost tracking aggregation script (100 LOC shell)
- ✅ All tests passing, Prometheus scraping operational

---

## Current State Assessment

### Monitoring Gaps

**Critical Missing Pieces:**
1. No centralized Prometheus scrape configuration for test cluster
2. No alert rules for:
   - Node failure/heartbeat loss
   - Disk space alerts
   - CPU/memory saturation
   - Cost overrun detection
   - CI/CD timeout violations
3. No Grafana dashboards for:
   - Cluster health summary
   - Storage node capacity/performance
   - Client connection status
   - CI/CD pipeline metrics (pass rate, duration, cost per run)
   - Cost trends (daily spend, monthly projection)
4. No cost tracking aggregation (AWS billing API integration)
5. No baseline performance metrics for regression detection

### Existing Infrastructure (From Blocks 1-3)

**Available from Block 1:**
- Terraform-provisioned test cluster (5 storage nodes, 2 clients, 1 conduit, 1 Jepsen controller)
- Security groups, IAM roles, instance profiles
- SSH accessibility via orchestrator

**Available from Block 2:**
- Instance lifecycle management with disruption handling
- Spot pricing optimization and cost tracking per instance
- Systemd services running on all nodes

**Available from Block 3:**
- GitHub Actions workflows with artifact collection
- CI/CD test reporting infrastructure
- Cost metrics from GitHub Actions

### What Block 4 Adds

**On Each Node:**
- Node Exporter (metrics: CPU, memory, disk, network, system state)
- Optional application metrics if ClaudeFS services are running

**Centralized (Orchestrator):**
- Prometheus server (scrape interval: 15s, retention: 30 days)
- Prometheus alert rules with 5m+ evaluation windows
- AlertManager (routes alerts to SNS/Slack/webhook)
- Grafana server (port 3000) with OIDC/GitHub auth disabled for dev

**Metrics to Collect:**
- Node metrics: CPU, memory, disk I/O, network packets/errors
- Cost metrics: Per-instance hourly cost (from spot pricing script)
- CI metrics: Test pass rate, workflow duration, artifact count
- System metrics: Container/process uptime, file descriptor counts

---

## Design: Monitoring Stack Components

### Component 1: Prometheus Configuration

**File:** `tools/prometheus.yml` (150 LOC)

**Scrape Jobs:**
1. `node_exporter` — All cluster nodes (targets auto-discovered from Terraform state)
2. `orchestrator` — Prometheus self-monitoring
3. `alertmanager` — AlertManager self-monitoring

**Alert Rules File:** `tools/prometheus-alerts.yml` (200 LOC)

**Alert Groups:**
1. **Availability Alerts** (5 rules)
   - `NodeExporterDown` — Node exporter unreachable for 2min
   - `PrometheusDown` — Prometheus down for 5min
   - `HighCPUUsage` — >80% for 5min on any node
   - `HighMemoryUsage` — >85% for 5min on any node
   - `LowDiskSpace` — <10% free on any node

2. **Cost Alerts** (3 rules)
   - `DailySpendExceeded80Percent` — >$80 today
   - `DailySpendExceeded100Percent` — >$100 today
   - `MonthlyProjectionExceeded` — Projected monthly > $3000

3. **CI/CD Alerts** (3 rules)
   - `CIJobTimeoutFrequent` — >20% of jobs timing out in 1h
   - `TestPassRateLow` — <90% pass rate (last 10 jobs)
   - `WorkflowDurationRegression` — 30% slower than baseline

4. **Infrastructure Alerts** (2 rules)
   - `SpotInterruptionRate` — >10% nodes interrupted in 1h
   - `NotEnoughSpotCapacity` — Failed to launch replacement within 5min

### Component 2: Grafana Dashboards

**Dashboard 1: Infrastructure Health** (200 LOC JSON)
- Grid of nodes with CPU, memory, disk, network status
- Alert status panel
- Node list with uptime

**Dashboard 2: Storage Performance** (150 LOC JSON)
- Disk I/O throughput/latency (if ClaudeFS running)
- Network bandwidth utilization
- FIO benchmark results (if available)

**Dashboard 3: CI/CD Metrics** (150 LOC JSON)
- Workflow pass rate (line chart over 7 days)
- Workflow duration distribution
- Cost per run trend
- Test count by crate/module

**Dashboard 4: Cost Analysis** (100 LOC JSON)
- Daily spend (bar chart)
- Monthly projection (gauge)
- Spot savings (vs on-demand)
- Cost breakdown: storage / clients / CI runners

### Component 3: AlertManager & Notification

**File:** `tools/alertmanager.yml` (50 LOC)

**Routing Rules:**
- Critical alerts (availability, cost overrun) → SNS topic `cfs-alerts-critical`
- Warning alerts (high utilization) → CloudWatch Logs only
- Info alerts (performance regression) → GitHub Issues (via webhook)

**Webhook Target:** GitHub API to auto-create issue on regression

### Component 4: Cost Tracking Aggregation

**File:** `tools/cfs-cost-aggregator.sh` (100 LOC)

**Functionality:**
- Query AWS Cost Explorer API (daily costs, breakdown by service)
- Write time-series data to file: `/var/lib/cfs-metrics/cost.tsv`
- Prometheus Node Exporter can textfile-export this as metrics

**Schedule:** Daily cron job at 00:30 UTC (after daily billing finalization)

---

## Implementation Plan

### Phase 4 Block 4 Implementation Steps

**Step 1: Prometheus Installation & Configuration** (1h)
- Write `tools/prometheus.yml` with scrape jobs
- Write `tools/prometheus-alerts.yml` with alert rules (8 rules total)
- Terraform: Deploy Prometheus container on orchestrator
- Terraform: Expose port 9090 internally
- SSH to orchestrator: verify Prometheus running, checking scrape targets

**Step 2: Node Exporter Deployment** (1h)
- Terraform: Add `node-exporter` systemd service to all cluster nodes
- Verify metrics available at `http://node:9100/metrics`
- Prometheus should auto-discover targets (via file_sd_configs from Terraform outputs)

**Step 3: Grafana Dashboards** (2h)
- Create 4 dashboards (Infrastructure, Storage, CI/CD, Cost) as JSON files
- Deploy via Terraform or manual provisioning API calls
- Create test fixtures for dashboard structure validation

**Step 4: AlertManager Integration** (1h)
- Deploy AlertManager container on orchestrator
- Configure routing rules
- Setup SNS topic integration (send alerts to `cfs-alerts-critical`)
- Test alert firing (synthetic alerts via amtool)

**Step 5: Cost Tracking** (1h)
- Write cost aggregation script
- Setup cron job to run daily
- Export metrics via Node Exporter textfile module

**Step 6: Integration Tests** (2h)
- Write 15-20 Rust tests covering:
  - Prometheus scrape target validation (all nodes reachable)
  - Alert rule parsing and validation (no YAML errors)
  - Dashboard JSON structure (all panels present)
  - Alerting threshold correctness (cost, CPU, memory)
  - Notification routing validation
  - Metrics aggregation accuracy

---

## Test Specifications

### Test Module: `crates/claudefs-tests/src/ci_monitoring_integration_tests.rs` (600 LOC)

**Test Groups:**

1. **Prometheus Configuration Tests** (4 tests)
   - `test_prometheus_config_valid_yaml` — Parse prometheus.yml without errors
   - `test_prometheus_alert_rules_syntax` — Parse alert rules without errors
   - `test_prometheus_scrape_targets_defined` — At least 1 scrape job configured
   - `test_prometheus_retention_policy_reasonable` — Retention >= 7 days

2. **Alert Rule Validation Tests** (5 tests)
   - `test_alert_node_down_threshold` — Evaluate correctly (>2min threshold)
   - `test_alert_cost_threshold` — Evaluate correctly ($80, $100, $3k projection)
   - `test_alert_cpu_high_threshold` — Evaluate correctly (>80% for >5min)
   - `test_alert_spot_interruption_rate` — Rules present and valid
   - `test_alert_rule_group_structure` — All rules in valid groups

3. **Grafana Dashboard Tests** (4 tests)
   - `test_dashboard_infrastructure_health_structure` — All required panels (CPU, memory, disk, alerts)
   - `test_dashboard_cicd_metrics_structure` — Pass rate, duration, cost panels
   - `test_dashboard_cost_analysis_structure` — Daily spend, monthly projection, breakdown
   - `test_dashboard_panel_references_valid_metrics` — Metric names exist or have sensible defaults

4. **AlertManager Configuration Tests** (3 tests)
   - `test_alertmanager_config_valid_yaml` — Parse alertmanager.yml without errors
   - `test_alertmanager_routing_rules_complete` — All alert names have routes
   - `test_alertmanager_notification_targets_reachable` — SNS topic ARN correct

5. **Cost Tracking Tests** (3 tests)
   - `test_cost_aggregator_script_exists_and_executable` — File exists with +x permission
   - `test_cost_aggregator_produces_valid_metrics_file` — Output format parseable as TSV
   - `test_cost_aggregator_daily_schedule_valid` — Cron expression syntactically correct

---

## Dependencies & Prerequisites

**From Previous Blocks:**
- ✅ Phase 5 Block 1 (Terraform infrastructure) — Test cluster provisioned
- ✅ Phase 5 Block 2 (Spot instance lifecycle) — Cost tracking available
- ✅ Phase 5 Block 3 (CI/CD hardening) — Workflow metrics available
- ✅ Docs/agents.md (monitoring requirements) — Understood
- ✅ Docs/management.md (Prometheus, DuckDB, Grafana vision) — Reference

**External Dependencies:**
- Prometheus binary (will be installed via Terraform)
- Grafana binary (will be installed via Terraform)
- AlertManager binary (will be installed via Terraform)
- AWS CloudWatch Logs (existing SNS topic)

---

## Success Criteria

- ✅ All 15-20 tests passing (100% pass rate)
- ✅ Zero YAML parsing errors in Prometheus/AlertManager configs
- ✅ Prometheus scrape targets showing green in web UI
- ✅ At least 3/4 dashboards provisioned and queryable
- ✅ Alert rules firing correctly (can verify via synthetic targets)
- ✅ Cost tracking aggregation producing valid output
- ✅ No manual intervention needed for ongoing operation

---

## Rollout & Risk Mitigation

**Staging:**
- Deploy to orchestrator instance only (no impact to test cluster)
- Prometheus retention limited to 30 days (conserves disk space)
- AlertManager initially routes only to CloudWatch Logs (safer than email)

**Validation:**
- Prometheus web UI accessible at http://orchestrator:9090
- Grafana dashboards accessible at http://orchestrator:3000 (no auth required for dev)
- Test suite validates all configs before Terraform apply

**Cleanup:**
- All monitoring containers can be stopped/removed without affecting test cluster
- Monitoring data persisted to EBS volume (survives container restart)
- No agents modified, no changes to core ClaudeFS components

---

## Timeline Estimate

**Total Implementation Time: 3-4 hours**

1. Prometheus config + deployment: 1h
2. Node Exporter rollout: 1h
3. Grafana dashboards: 1.5h
4. AlertManager + cost tracking: 1h
5. Tests + validation: 1h
6. Fix any issues + final commit: 0.5h

**OpenCode Involvement:**
- All YAML configuration generation: 30 min
- All Rust test generation: 30 min
- Total OpenCode time: 1h

---

## Next Steps After Block 4

**Phase 5 Block 5 (GitOps Orchestration)** will:
- Use Prometheus metrics for automated remediation (e.g., relaunch failed nodes)
- Use GitHub Actions status from Block 3 to trigger automatic retries
- Implement ArgoCD or Flux for automatic deployment rollbacks on failures
- Create deployment pipeline: commit → build → test → Prometheus health check → auto-deploy

---

## Appendix: Monitoring Roadmap (Future)

### Phase 6 Enhancements
- Distributed tracing (Jaeger/Tempo)
- Custom ClaudeFS metrics (if services implemented)
- E2E latency profiling
- Performance regression tracking

### Enterprise Features (Post-MVP)
- Multi-tenant dashboards (isolated by customer)
- SLA compliance dashboards
- Capacity planning trends
- Budget forecasting

---

**End of Phase 5 Block 4 Planning Document**

