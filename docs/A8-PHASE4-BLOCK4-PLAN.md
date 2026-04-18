# A8 Phase 4 Block 4: Advanced Dashboarding & Operational Tooling

**Date:** 2026-04-18
**Agent:** A8 Management
**Status:** 📋 PLANNING
**Session:** 1 (Phase 4 Block 4 Planning & Implementation)

---

## Executive Summary

Phase 4 Block 4 builds on the recovery infrastructure (Block 3) to provide advanced dashboarding, operational monitoring enhancements, and CLI tooling for administrators and operators. This phase focuses on making the management layer production-ready with comprehensive visibility into cluster health, performance, costs, and recovery actions.

**Key Deliverables:**
1. **Advanced Grafana dashboards** — Health overview, performance trends, capacity planning
2. **Enhanced Web UI** — Real-time cluster status, alert management, resource visualization
3. **CLI enhancements** — Operational commands for health checks, recovery actions, diagnostics
4. **Cost dashboard integration** — Display cost metrics alongside operational metrics
5. **Alert aggregation & management** — Unified alert interface
6. **Operational tests** — Automated tests for dashboard accuracy and CLI behavior

**Success Criteria:**
- ✅ 5+ new Grafana dashboards (health, performance, capacity, cost, alerts)
- ✅ Web UI enhancements with real-time cluster status
- ✅ CLI operational commands (health, diagnostics, recovery, capacity)
- ✅ Alert aggregation and management system
- ✅ 20+ new operational tests (UI, CLI, dashboard)
- ✅ All 1017 existing tests still passing

**Estimated Duration:** 8-10 hours (2-3 days)
**Owner:** A8 Management
**Depends On:** Block 3 (✅ Complete), A11 metrics infrastructure (✅ Block 4 complete)

---

## Current State

### What Exists (Block 3 & Earlier)
- ✅ Recovery actions framework (recovery_actions.rs, 20 tests)
- ✅ Backup rotation manager (backup_rotation.rs, 8 tests)
- ✅ Graceful shutdown manager (graceful_shutdown.rs, 10 tests)
- ✅ Health aggregator (health.rs, callbacks integrated)
- ✅ Prometheus metrics exporter (metrics.rs, collectors)
- ✅ DuckDB indexer (indexer.rs, Parquet schema)
- ✅ Basic Grafana dashboards (grafana.rs)
- ✅ CLI framework (cli.rs)
- ✅ Query gateway (query_gateway.rs, DuckDB SQL interface)
- ✅ 1017 total tests passing

### What's Missing (Block 4)

**Dashboarding Gaps:**
- ❌ Health overview dashboard (cluster-wide, node-level)
- ❌ Performance dashboard (IOPS, latency, throughput trends)
- ❌ Capacity planning dashboard (storage utilization forecast)
- ❌ Cost dashboard (EC2, Bedrock, S3 breakdown) — requires A11 metrics
- ❌ Alert dashboard (active alerts, alert history, silencing)

**Web UI Gaps:**
- ❌ Real-time cluster status page
- ❌ Alert management interface
- ❌ Recovery action history viewer
- ❌ Cost breakdown visualization
- ❌ Performance trends chart
- ❌ Responsive design for mobile operators

**CLI Gaps:**
- ❌ `cfs mgmt health` — Cluster health summary
- ❌ `cfs mgmt diagnostics` — Detailed system diagnostics
- ❌ `cfs mgmt recovery show` — List recovery actions
- ❌ `cfs mgmt recovery execute` — Manually trigger recovery actions
- ❌ `cfs mgmt capacity` — Storage capacity forecast
- ❌ `cfs mgmt alerts` — Alert summary and management
- ❌ `cfs mgmt dashboard` — Quick dashboard URL commands

**Testing Gaps:**
- ❌ Dashboard rendering tests (JSON validation)
- ❌ CLI integration tests
- ❌ Web UI endpoint tests
- ❌ Alert aggregation tests

---

## Phase 4 Block 4: Implementation Plan

### Task 1: Advanced Grafana Dashboards (2.5 hours)

**Files:** `src/grafana.rs` (enhanced), 5 new dashboard JSON files

**Deliverables:**

1. **Health Overview Dashboard** (grafana/health-overview.json)
   - Cluster status summary (green/yellow/red)
   - Node status grid (all nodes with health indicators)
   - Recovery action status (active, recent, history)
   - Backup status (last backup, backup history)
   - Replication lag metrics
   - Last hour health events timeline

2. **Performance Dashboard** (grafana/performance.json)
   - IOPS per node (write, read, total)
   - Latency percentiles (p50, p95, p99)
   - Throughput trends (MB/s, files/s)
   - Cache hit rate by service
   - Query latency distribution
   - I/O queue depth trends

3. **Capacity Planning Dashboard** (grafana/capacity.json)
   - Current flash utilization (per node, cluster average)
   - S3 tiered capacity (bytes in flash vs S3)
   - Storage forecast (7-day, 30-day projections)
   - Ingest rate (bytes/sec, trending)
   - Eviction rate (triggered by capacity pressure)
   - Quota utilization by tenant (if multi-tenant)

4. **Cost Dashboard** (grafana/cost.json) — requires A11 metrics
   - Daily cost breakdown (EC2, Bedrock, S3, Data Transfer)
   - Monthly forecast
   - Cost per instance type (i4i.2xlarge, c7a.xlarge, etc.)
   - Spot vs on-demand savings
   - Cost trend (7-day moving average)

5. **Alert Dashboard** (grafana/alerts.json)
   - Active alerts (count by severity)
   - Alert timeline (last 24 hours)
   - Alert history (acknowledge, resolve)
   - Recovery action correlation (which alerts triggered which actions)
   - Alert rules status

**Implementation Approach:**
- Extend `src/grafana.rs` to generate JSON for each dashboard
- Use Grafana SimpleJSON data source for dynamic queries
- Link dashboards with drill-down capabilities
- Add templating variables ($datasource, $node, $service)

**Tests:**
- `test_health_dashboard_json_valid` — Validate JSON structure
- `test_performance_dashboard_json_valid` — Validate JSON structure
- `test_capacity_dashboard_json_valid` — Validate JSON structure
- `test_cost_dashboard_json_valid` — Validate JSON structure
- `test_alert_dashboard_json_valid` — Validate JSON structure

---

### Task 2: Web UI Enhancements (2 hours)

**Files:** `src/web_api.rs` (enhanced), React components (TypeScript/React)

**Deliverables:**

1. **Real-Time Cluster Status Page**
   - Cluster overview: node count, capacity, health status
   - Node grid: individual node status, role, uptime
   - Last 24-hour events: timeline view
   - Quick actions: restart node, trigger recovery, view diagnostics

2. **Alert Management Interface**
   - Active alerts list (sortable, filterable)
   - Alert details (triggered at, severity, affected service)
   - Actions: acknowledge, silence, resolve, create ticket
   - Alert history (resolved, silenced, auto-resolved)

3. **Recovery Action History**
   - Recent actions (last 24 hours)
   - Action details (type, status, duration, errors)
   - Execution timeline
   - Audit trail (who triggered, from where)

4. **Cost Visualization**
   - Daily cost breakdown (pie chart)
   - Monthly trend (line chart)
   - Cost per node type (bar chart)
   - Forecast (next 7/30 days)

5. **Performance Trends**
   - IOPS trend (last 24 hours, 7 days, 30 days)
   - Latency distribution (histogram)
   - Throughput trend (MB/s)
   - Cache hit rate

**Implementation Approach:**
- Use existing Axum web_api.rs as REST backend
- Add new endpoints for cluster status, alerts, recovery actions, costs
- Create React components for dashboard views
- Use Recharts for visualization (IOPS, latency, cost trends)
- Add WebSocket support for real-time updates (optional for Phase 4, Phase 5)

**Tests:**
- `test_cluster_status_endpoint` — GET /api/cluster/status
- `test_alerts_endpoint` — GET /api/alerts
- `test_recovery_actions_endpoint` — GET /api/recovery/actions
- `test_cost_breakdown_endpoint` — GET /api/cost/breakdown
- `test_performance_metrics_endpoint` — GET /api/performance/metrics

---

### Task 3: CLI Operational Commands (2 hours)

**File:** `src/cli.rs` (extended with new subcommands)

**Deliverables:**

New subcommands (integrated into existing CLI):

1. **`cfs mgmt health`**
   - Summary of cluster health (green/yellow/red)
   - List of problematic nodes
   - Recent health events
   - Recovery actions triggered in last hour
   - Options: `--verbose`, `--json`, `--watch` (continuous monitoring)

2. **`cfs mgmt diagnostics`**
   - System diagnostics snapshot
   - Cluster state (nodes, replicas, Raft status)
   - Storage capacity (flash, S3, eviction stats)
   - Network connectivity matrix
   - Performance metrics (avg IOPS, latency)
   - Last 10 events
   - Export to JSON/HTML

3. **`cfs mgmt recovery show`**
   - List recent recovery actions (last 24 hours)
   - Details: timestamp, type, status, duration, affected nodes
   - Filter options: `--type`, `--node`, `--status`
   - Export: `--json`, `--csv`

4. **`cfs mgmt recovery execute`**
   - Manually trigger recovery action (requires confirmation)
   - Actions: ReduceWorkerThreads, ShrinkMemoryCaches, EvictColdData, RestartComponent, etc.
   - Options: `--node`, `--service`, `--dry-run`
   - Audit logging (who, when, what)

5. **`cfs mgmt capacity`**
   - Storage capacity forecast (7-day, 30-day)
   - Current utilization (flash, S3, eviction rate)
   - Recommendations (add nodes, increase S3 budget, reduce quota)
   - Ingest rate analysis

6. **`cfs mgmt alerts`**
   - List active alerts (sorted by severity)
   - Acknowledge/silence alerts
   - Alert statistics (by severity, by service)
   - Export: `--json`, `--csv`

7. **`cfs mgmt dashboard`**
   - Print Grafana dashboard URLs
   - `--health`, `--performance`, `--capacity`, `--cost`, `--alerts` flags
   - Generate shareable links (with time range)

**Implementation Approach:**
- Extend existing cli.rs with subcommand handlers
- Leverage existing metrics collectors, health aggregators, recovery executors
- Use `structopt` or `clap` for argument parsing
- Format output with tables (for human-readable), JSON (for automation)
- Add `--watch` flag for continuous monitoring (polling every 2-5 seconds)

**Tests:**
- `test_health_command_basic` — cfs mgmt health
- `test_health_command_verbose` — cfs mgmt health --verbose
- `test_health_command_json` — cfs mgmt health --json
- `test_diagnostics_command` — cfs mgmt diagnostics
- `test_recovery_show_command` — cfs mgmt recovery show
- `test_recovery_execute_command_dry_run` — cfs mgmt recovery execute --dry-run
- `test_capacity_command` — cfs mgmt capacity
- `test_alerts_command` — cfs mgmt alerts
- `test_dashboard_command` — cfs mgmt dashboard --health

---

### Task 4: Alert Aggregation & Management (1.5 hours)

**Files:** New `src/alert_manager.rs`

**Deliverables:**

1. **AlertManager struct**
   - In-memory alert registry (Arc<DashMap<AlertId, Alert>>)
   - Alert state transitions: Active → Acknowledged → Resolved
   - Persistence to DuckDB (alert_history table)
   - Webhook triggers on state changes (if configured)

2. **Alert types**
   - Infrastructure alerts (node down, replication lag >60s, disk full >90%)
   - Performance alerts (latency >100ms, IOPS drop >30%, cache hit <50%)
   - Capacity alerts (flash >80%, eviction rate >10%)
   - Cost alerts (daily cost >$25, forecast >$100)
   - Recovery alerts (action triggered, action failed)

3. **Alert actions**
   - `acknowledge()` — Mute alert, log acknowledgment time
   - `silence(duration)` — Temporarily suppress notifications
   - `resolve()` — Mark as resolved
   - `assign_to(user)` — Assign to operator
   - `create_ticket()` — Link to external ticketing system

4. **Alert queries**
   - `active_alerts()` — List unresolved alerts
   - `recent_alerts(duration)` — Last N hours/days
   - `alert_by_severity()` — Count by CRITICAL, WARN, INFO
   - `alert_by_service()` — Group by affected component
   - `correlation(alert)` — Find related alerts

**Tests:**
- `test_alert_manager_new` — Create AlertManager
- `test_alert_new` — Create alert, verify timestamp
- `test_alert_acknowledge` — Acknowledge and verify state
- `test_alert_silence` — Silence for duration
- `test_alert_resolve` — Mark resolved
- `test_active_alerts_query` — Query unresolved alerts
- `test_alert_by_severity_count` — Group and count by severity
- `test_alert_persistence_to_duckdb` — Persist and retrieve

---

### Task 5: Operational Tests (1.5 hours)

**File:** New `src/operational_tests.rs`

**Deliverables:**

1. **Dashboard Tests** (5 tests)
   - All dashboard JSON files valid
   - Dashboard metrics exist in Prometheus
   - Dashboard time ranges work correctly
   - Dashboard drill-down links valid

2. **Web UI Tests** (5 tests)
   - Cluster status endpoint returns valid JSON
   - Alert endpoint returns proper structure
   - Recovery actions endpoint returns proper structure
   - Cost endpoint returns proper structure
   - Performance metrics endpoint returns proper structure

3. **CLI Tests** (8 tests)
   - Health command returns exit code 0
   - Health command --json returns valid JSON
   - Diagnostics command completes in <5 seconds
   - Recovery show command returns sorted list
   - Capacity command forecasts are reasonable
   - Alerts command returns active alerts
   - Dashboard command returns proper URLs
   - Recovery execute --dry-run doesn't modify state

4. **Alert Manager Tests** (5 tests)
   - Alert acknowledgment persists
   - Alert silence duration enforced
   - Alert resolution works
   - Active alerts query accurate
   - Alert correlation finds related alerts

**Implementation Approach:**
- Create integration test suite that exercises all new features
- Mock external dependencies (Prometheus, Grafana, DuckDB)
- Test both happy paths and error cases
- Verify state persistence and consistency

---

## Testing Strategy

### Unit Tests (per module)
- 60+ new unit tests across all new modules
- Target: 95%+ code coverage
- Test both success and error paths

### Integration Tests
- 20+ integration tests
- Test CLI → API → storage layer
- Test dashboard generation from live metrics
- Test alert aggregation across multiple events

### Manual Testing Checklist
- [ ] Run all 5 Grafana dashboards on test cluster
- [ ] Access Web UI, verify all pages load
- [ ] Run all CLI commands, verify output format
- [ ] Create test alerts, verify aggregation
- [ ] Execute recovery action from CLI
- [ ] Verify alert notifications

### Success Metrics
- ✅ All 1017 existing tests still passing
- ✅ 60+ new tests added
- ✅ Total tests: 1077+
- ✅ Zero ignored tests
- ✅ Build warnings: minimal (only unused imports)

---

## Module Organization

### New Modules
- `src/alert_manager.rs` — Alert aggregation and management

### Enhanced Modules
- `src/grafana.rs` — 5 new dashboard generators
- `src/web_api.rs` — New endpoints for cluster status, alerts, recovery, costs
- `src/cli.rs` — 7 new subcommands
- `src/operational_tests.rs` — Operational integration tests

### Existing Modules (unchanged)
- `src/recovery_actions.rs` — No changes (used by new alert manager)
- `src/health.rs` — No changes (used by new UI/CLI)
- `src/metrics.rs` — No changes (used by new dashboards)
- `src/query_gateway.rs` — No changes (used by new UI/CLI)

---

## Implementation Timeline

| Task | Duration | Blockers | Dependencies |
|------|----------|----------|--------------|
| Task 1: Grafana Dashboards | 2.5h | None | A11 Block 4 metrics (✅ ready) |
| Task 2: Web UI Enhancements | 2h | Task 1 JSON | React setup |
| Task 3: CLI Commands | 2h | None | Existing cli.rs |
| Task 4: Alert Manager | 1.5h | None | DuckDB (existing) |
| Task 5: Operational Tests | 1.5h | Tasks 1-4 | All modules |
| **Total** | **10h** | — | — |

### Execution Order
1. Start with Task 1 (Grafana dashboards) — independent, highest visibility
2. Parallel: Task 3 (CLI commands) — independent of dashboards
3. Task 4 (Alert manager) — used by Task 2 and Task 5
4. Task 2 (Web UI) — depends on Task 1 dashboards, Task 4 alerts
5. Task 5 (Tests) — final integration testing, depends on all tasks

---

## Success Criteria

### Functional
- ✅ All 5 Grafana dashboards render correctly
- ✅ Web UI cluster status page shows real-time data
- ✅ All 7 CLI commands execute without errors
- ✅ Alert manager aggregates and tracks alerts correctly
- ✅ All 60+ new tests pass

### Non-Functional
- ✅ Dashboard queries complete <1 second
- ✅ Web UI endpoints respond <100ms
- ✅ CLI commands complete <5 seconds
- ✅ Alert manager latency <100ms
- ✅ No new warnings or errors in build

### Test Coverage
- ✅ 1077+ total tests passing (1017 existing + 60+ new)
- ✅ 0 failing tests
- ✅ 0 ignored tests (DuckDB tests from Block 3 remain ignored)
- ✅ 95%+ code coverage on new modules

---

## Notes

- **Grafana dashboard JSONs:** Will be stored as static files in `src/grafana/` and generated at startup
- **React components:** Optional for Phase 4; focus on REST API first, then UI can be added incrementally
- **WebSocket support:** Deferred to Phase 5 (real-time updates)
- **External integrations:** PagerDuty, Slack, Opsgenie support deferred to Phase 5
- **Mobile UI:** Responsive design basic for Phase 4, enhanced for Phase 5

---

## Dependencies & Blockers

### Internal Dependencies
- ✅ Block 3 recovery framework (complete)
- ✅ A11 metrics infrastructure (Block 4 complete)
- ✅ DuckDB indexer (existing)

### External Dependencies
- ✅ Grafana instance (assumed running per A11 setup)
- ✅ Prometheus scraping (assumed configured per A11)
- ⚠️ React/TypeScript (for Web UI, optional for Phase 4)

### Potential Blockers
- None identified. All dependencies are satisfied.

---

## References

- Previous block: `docs/A8-PHASE4-BLOCK3-PLAN.md` (Recovery actions)
- A11 Block 4: Production deployment pipeline (✅ complete)
- A11 Block 5: Cost monitoring (ongoing, provides metrics for Task 1)
- CLAUDE.md: Agent roles, model selection
- agents.md: A8 scope (Prometheus, DuckDB, Web UI, CLI)

