# A11: Phase 5 Block 4 — Monitoring Integration Implementation

**Agent:** A11 Infrastructure & CI
**Phase:** 5 | **Block:** 4
**Task:** Generate Prometheus config, Grafana dashboards, AlertManager config, and comprehensive integration tests
**Model:** minimax-m2p5
**Output Format:** Infrastructure config files + Rust test module

---

## Context

ClaudeFS is a distributed file system implemented by 11 parallel AI agents. Agent A11 owns infrastructure and CI/CD. Previous blocks delivered:
- Block 1: Terraform infrastructure validation (36 tests)
- Block 2: Preemptible spot instance lifecycle management (17 tests)
- Block 3: GitHub Actions CI/CD hardening with composite actions (12 tests)

Block 4 integrates Prometheus monitoring, Grafana dashboards, cost tracking, and alerting into the ClaudeFS test infrastructure. The test cluster runs on AWS EC2 (5 storage nodes, 2 clients, 1 metadata conduit, 1 Jepsen controller, 1 orchestrator) with preemptible instances.

---

## Deliverables Required

### 1. Prometheus Configuration (`tools/prometheus.yml`)

**Requirements:**
- Comment-heavy for future operators
- Scrape interval: 15s
- Evaluation interval: 15s
- Retention: 30 days (avoid excessive disk usage)
- Scrape jobs:
  1. `prometheus` — Prometheus self-monitoring (localhost:9090)
  2. `node-exporter` — All cluster nodes (file SD from Terraform outputs or static IPs with comments)
  3. `alertmanager` — AlertManager self-monitoring (localhost:9093)
- Global labels: `cluster: "cfs-test"`, `environment: "development"`
- Remote storage: Optional comment about future integration with long-term storage

**Format:** Standard Prometheus YAML with extensive comments explaining each section

**Estimated LOC:** 150-180

### 2. Prometheus Alert Rules (`tools/prometheus-alerts.yml`)

**Alert Rules (8 total across 4 groups):**

**Group 1: Availability (5 rules)**
- `NodeExporterDown` (severity: critical)
  - Condition: `up{job="node-exporter"} == 0` for 2m
  - Annotations: "Node {{ $labels.instance }} is unreachable"

- `PrometheusDown` (severity: warning)
  - Condition: `up{job="prometheus"} == 0` for 5m
  - Annotations: "Prometheus server is unreachable"

- `HighCPUUsage` (severity: warning)
  - Condition: `100 - (avg(rate(node_cpu_seconds_total{mode="idle"}[5m])) by (instance)) * 100 > 80` for 5m
  - Annotations: "CPU usage on {{ $labels.instance }} is {{ $value | humanizePercentage }}"

- `HighMemoryUsage` (severity: warning)
  - Condition: `(1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) * 100 > 85` for 5m
  - Annotations: "Memory usage on {{ $labels.instance }} is {{ $value | humanizePercentage }}"

- `LowDiskSpace` (severity: warning)
  - Condition: `(node_filesystem_avail_bytes{fstype!="tmpfs"} / node_filesystem_size_bytes) * 100 < 10` for 10m
  - Annotations: "Disk space low on {{ $labels.instance }} ({{ $value | humanizePercentage }} free)"

**Group 2: Cost Alerts (3 rules)**
- `DailySpendExceeded80Percent` (severity: warning)
  - Placeholder metric: `aws_daily_spend_usd > 80`
  - Annotations: "Daily AWS spend is ${{ $value }}, approaching limit"

- `DailySpendExceeded100Percent` (severity: critical)
  - Placeholder metric: `aws_daily_spend_usd > 100`
  - Annotations: "Daily AWS spend exceeded $100 limit: ${{ $value }}"

- `MonthlyProjectionExceeded` (severity: warning)
  - Placeholder metric: `aws_monthly_projection_usd > 3000`
  - Annotations: "Monthly projection exceeds $3000: ${{ $value }}"

**Group 3: CI/CD Alerts (3 rules)**
- `CIJobTimeoutFrequent` (severity: warning)
  - Placeholder metric: `ci_job_timeout_ratio_1h > 0.2`
  - Annotations: "CI jobs timing out frequently ({{ $value | humanizePercentage }})"

- `TestPassRateLow` (severity: warning)
  - Placeholder metric: `ci_test_pass_rate_last_10 < 0.9`
  - Annotations: "Test pass rate is low: {{ $value | humanizePercentage }}"

- `WorkflowDurationRegression` (severity: info)
  - Placeholder metric: `ci_workflow_duration_vs_baseline > 1.3`
  - Annotations: "Workflow duration {{ $value | humanizeDuration }} slower than baseline"

**Group 4: Infrastructure Alerts (2 rules)**
- `SpotInterruptionRate` (severity: warning)
  - Placeholder metric: `spot_interruption_count_1h / node_count > 0.1`
  - Annotations: "Spot interruption rate is {{ $value | humanizePercentage }} in last hour"

- `NotEnoughSpotCapacity` (severity: critical)
  - Placeholder metric: `spot_launch_failure_consecutive > 3`
  - Annotations: "Failed to launch spot replacement nodes {{ $value }} times consecutively"

**Format:** Prometheus recording rules and alert rules YAML with detailed descriptions

**Estimated LOC:** 200-250

### 3. Grafana Dashboards (4 dashboards as JSON files)

Generate 4 dashboard JSON files:

#### Dashboard 1: `tools/grafana-dashboard-infrastructure.json`
**Purpose:** Cluster health overview
**Panels:**
- Title: "Infrastructure Health"
- Grid layout with 4 rows:
  - Row 1: CPU usage heatmap (all nodes)
  - Row 2: Memory usage heatmap (all nodes)
  - Row 3: Disk usage (all nodes)
  - Row 4: Network I/O (all nodes)
  - Row 5: Alert status table (all active alerts)
  - Row 6: Node uptime list
- Refresh: 30s
- Time range: Last 1 hour (with past 24h available)

**Estimated LOC:** 200-250

#### Dashboard 2: `tools/grafana-dashboard-cicd-metrics.json`
**Purpose:** CI/CD pipeline performance
**Panels:**
- Title: "CI/CD Metrics"
- Grid layout:
  - Row 1: Test pass rate (line chart, 7 days)
  - Row 2: Workflow duration distribution (bar chart, last 20 runs)
  - Row 3: Cost per run (scatter plot, trend line)
  - Row 4: Test count by module (pie chart)
  - Row 5: Artifact count/size (time series)
- Refresh: 5m
- Time range: Last 7 days

**Estimated LOC:** 200-250

#### Dashboard 3: `tools/grafana-dashboard-cost-analysis.json`
**Purpose:** Cost tracking and trends
**Panels:**
- Title: "Cost Analysis"
- Grid layout:
  - Row 1: Daily spend (bar chart, last 30 days)
  - Row 2: Monthly projection (gauge, 0-5000 scale)
  - Row 3: Spot savings (counter, shows %)
  - Row 4: Cost breakdown by component (pie: storage nodes, clients, CI)
  - Row 5: Hourly cost trend (line, last 24h)
- Refresh: 60m (expensive data source)
- Time range: Last 30 days

**Estimated LOC:** 200-250

#### Dashboard 4: `tools/grafana-dashboard-storage-performance.json`
**Purpose:** Storage node performance (for when ClaudeFS services running)
**Panels:**
- Title: "Storage Performance (Placeholder)"
- Grid layout:
  - Row 1: Disk I/O throughput (prepared, no metrics yet)
  - Row 2: Disk I/O latency (prepared, no metrics yet)
  - Row 3: Network bandwidth utilization (prepared, no metrics yet)
  - Row 4: FIO benchmark results (prepared, no metrics yet)
- Note: All panels have placeholder queries that will work once ClaudeFS services emit metrics
- Refresh: 15s (for live benchmarks)
- Time range: Last 1 hour

**Estimated LOC:** 150-200

**Total Dashboards:** 800-950 LOC JSON

### 4. AlertManager Configuration (`tools/alertmanager.yml`)

**Requirements:**
- Route alert groups by severity and type
- Critical alerts → SNS topic: `arn:aws:sns:us-west-2:ACCOUNT:cfs-alerts-critical`
- Warning alerts → CloudWatch Logs: `/aws/cfs-alerts/warnings`
- Info alerts → GitHub Issues webhook (optional, placeholder)
- Grouping: By cluster + alert type (1m window, 10s initial delay)
- Receiver configurations for SNS, CloudWatch, webhook
- Inhibition rules: (optional, basic setup)

**Format:** Prometheus AlertManager YAML with comments

**Estimated LOC:** 80-120

### 5. Cost Aggregation Script (`tools/cfs-cost-aggregator.sh`)

**Requirements:**
- Query AWS Cost Explorer API (requires CLI v2)
- Extract daily cost (sum across all services)
- Write TSV file: `/var/lib/cfs-metrics/cost.tsv` with format:
  ```
  timestamp  service  cost_usd
  2026-04-18 storage  25.50
  2026-04-18 compute  12.30
  2026-04-18 network  2.15
  ```
- Handle authentication via IAM role on orchestrator instance
- Run daily via cron at 00:30 UTC (after billing finalization)
- Include error handling and logging to syslog
- Optional: Create Prometheus textfile exporter config for this data

**Format:** Bash shell script with extensive comments

**Estimated LOC:** 100-120

### 6. Comprehensive Integration Tests (`crates/claudefs-tests/src/ci_monitoring_integration_tests.rs`)

**Module Structure:**

```rust
#[cfg(test)]
mod ci_monitoring_integration_tests {
    use std::fs;
    use std::path::Path;

    // Test groups as submodules or functions

    mod prometheus_configuration {
        // 4 tests
    }

    mod alert_rule_validation {
        // 5 tests
    }

    mod grafana_dashboards {
        // 4 tests
    }

    mod alertmanager_config {
        // 3 tests
    }

    mod cost_tracking {
        // 3 tests
    }
}
```

**Test 1: Prometheus Config Parsing** (Group: prometheus_configuration)
- Name: `test_prometheus_config_valid_yaml`
- Load `tools/prometheus.yml`
- Verify parses as valid YAML (no errors)
- Assert: At least one scrape_config present
- Assert: `global.retention` >= 7 days

**Test 2: Prometheus Scrape Targets** (Group: prometheus_configuration)
- Name: `test_prometheus_scrape_targets_defined`
- Verify scrape jobs: `prometheus`, `node-exporter`, `alertmanager`
- Assert: Each job has valid config (scheme, port, path)
- Assert: Node exporter job has >= 1 static target or file_sd config

**Test 3: Prometheus Alert Rules Syntax** (Group: prometheus_configuration)
- Name: `test_prometheus_alert_rules_syntax`
- Load `tools/prometheus-alerts.yml`
- Verify parses as valid YAML
- Assert: At least 1 alert_rules group
- Assert: All alerts have name, expr, for, annotations, labels

**Test 4: Prometheus Retention Policy** (Group: prometheus_configuration)
- Name: `test_prometheus_retention_policy_reasonable`
- Parse `prometheus.yml`
- Extract `global.retention` value
- Assert: >= 7 days and <= 90 days (reasonable for dev cluster)

**Test 5: Alert Rule Thresholds** (Group: alert_rule_validation)
- Name: `test_alert_cost_threshold_correct`
- Parse `prometheus-alerts.yml`
- Extract alerts: DailySpendExceeded80Percent, DailySpendExceeded100Percent, MonthlyProjectionExceeded
- Assert: Threshold values are 80, 100, 3000 respectively

**Test 6: Alert Rule CPU Threshold** (Group: alert_rule_validation)
- Name: `test_alert_cpu_high_threshold_correct`
- Extract HighCPUUsage alert
- Assert: Threshold > 80 and < 95
- Assert: Duration >= 5m

**Test 7: Alert Rule Memory Threshold** (Group: alert_rule_validation)
- Name: `test_alert_memory_high_threshold_correct`
- Extract HighMemoryUsage alert
- Assert: Threshold > 80 and < 95

**Test 8: Alert Rule Structure** (Group: alert_rule_validation)
- Name: `test_alert_rules_have_annotations`
- Verify all 13 alerts have:
  - `description` annotation (explaining the alert)
  - `summary` annotation (short summary)
  - `severity` label (critical/warning/info)

**Test 9: Alert Rule Count** (Group: alert_rule_validation)
- Name: `test_alert_rules_count`
- Assert: Exactly 13 alert rules defined (8 concrete + 5 placeholders)

**Test 10: Grafana Dashboard Structure - Infrastructure** (Group: grafana_dashboards)
- Name: `test_grafana_dashboard_infrastructure_valid_json`
- Load `grafana-dashboard-infrastructure.json`
- Verify parses as valid JSON
- Assert: title field = "Infrastructure Health"
- Assert: At least 5 panels present

**Test 11: Grafana Dashboard Structure - CI/CD** (Group: grafana_dashboards)
- Name: `test_grafana_dashboard_cicd_valid_json`
- Load `grafana-dashboard-cicd-metrics.json`
- Verify parses as valid JSON
- Assert: title field = "CI/CD Metrics"
- Assert: At least 5 panels present

**Test 12: Grafana Dashboard Structure - Cost** (Group: grafana_dashboards)
- Name: `test_grafana_dashboard_cost_valid_json`
- Load `grafana-dashboard-cost-analysis.json`
- Verify parses as valid JSON
- Assert: title field = "Cost Analysis"
- Assert: At least 4 panels present

**Test 13: Grafana Dashboard Structure - Storage** (Group: grafana_dashboards)
- Name: `test_grafana_dashboard_storage_valid_json`
- Load `grafana-dashboard-storage-performance.json`
- Verify parses as valid JSON
- Assert: title field = "Storage Performance"
- Assert: Panel count matches expected (all placeholders)

**Test 14: AlertManager Config Syntax** (Group: alertmanager_config)
- Name: `test_alertmanager_config_valid_yaml`
- Load `tools/alertmanager.yml`
- Verify parses as valid YAML
- Assert: At least 1 route defined
- Assert: At least 1 receiver defined

**Test 15: AlertManager Routes Complete** (Group: alertmanager_config)
- Name: `test_alertmanager_routes_cover_all_severity_levels`
- Parse alertmanager.yml
- Verify routes for: critical, warning, info
- Assert: Each route has valid receiver

**Test 16: AlertManager SNS Target** (Group: alertmanager_config)
- Name: `test_alertmanager_sns_receiver_configured`
- Parse config
- Assert: SNS receiver present with ARN format: `arn:aws:sns:us-west-2:*:cfs-alerts-critical`

**Test 17: Cost Aggregator Script Exists** (Group: cost_tracking)
- Name: `test_cost_aggregator_script_exists`
- Assert: File `tools/cfs-cost-aggregator.sh` exists
- Assert: File has execute permission (+x)
- Assert: File contains bash shebang

**Test 18: Cost Aggregator Script Syntax** (Group: cost_tracking)
- Name: `test_cost_aggregator_script_valid_bash`
- Run: `bash -n tools/cfs-cost-aggregator.sh`
- Assert: No syntax errors

**Test 19: Cost Tracking Cron Expression** (Group: cost_tracking)
- Name: `test_cost_aggregator_daily_cron_valid`
- Parse systemd timer or crontab format in script comments
- Assert: Expression evaluates to valid time (00:30 UTC)

**Test 20: Monitoring Infrastructure Dependencies** (Group: cost_tracking)
- Name: `test_monitoring_infrastructure_completeness`
- Assert: All required files exist:
  - `tools/prometheus.yml`
  - `tools/prometheus-alerts.yml`
  - `tools/alertmanager.yml`
  - `tools/cfs-cost-aggregator.sh`
  - `tools/grafana-dashboard-*.json` (4 files)

**Total Tests:** 20
**Estimated LOC:** 700-900 (with proper test structure, documentation, and error handling)

---

## Implementation Notes

### Configuration File Locations (Terraform will provision these)
- Prometheus config: `/etc/prometheus/prometheus.yml` on orchestrator
- Alert rules: `/etc/prometheus/alerts.yml` on orchestrator
- AlertManager config: `/etc/alertmanager/alertmanager.yml` on orchestrator
- Cost aggregator: `/usr/local/bin/cfs-cost-aggregator.sh` on orchestrator
- Grafana dashboards: Provisioned via Grafana API (reference JSON files in `tools/`)

### Prometheus Metric Naming
- Use `cfs_` prefix for custom metrics (future)
- Node metrics: standard `node_*` (from Node Exporter)
- Cost metrics: `aws_*` (from cost aggregator)
- CI metrics: `ci_*` (from GitHub Actions + workflow parsing)

### Test Coverage
- All config files: YAML/JSON syntax validation
- All alert rules: Threshold correctness, annotation completeness
- All dashboards: JSON structure, panel presence
- All scripts: Bash syntax, file permissions, dependency presence

### Error Handling in Tests
- File not found → Descriptive assertion message
- YAML parse errors → Show exact error line
- JSON parse errors → Show exact error location
- Missing fields → List all expected vs actual fields

### Rust Test Style
- Use `Result<(), String>` for error reporting
- Use `assert_eq!`, `assert!` with descriptive messages
- Use helper functions to reduce duplication:
  - `load_yaml_file(path: &str) -> Result<...>`
  - `load_json_file(path: &str) -> Result<...>`
  - `get_resource_path(filename: &str) -> String`

---

## Success Criteria

✅ All 20 tests passing (100% pass rate)
✅ All YAML/JSON files syntactically valid
✅ All threshold values correctly set
✅ All 4 Grafana dashboards provisioned
✅ Prometheus alerts firing correctly
✅ Cost tracking producing valid output
✅ Zero warnings or clippy issues in generated code

---

## Output Format

Generate the following files in the output:

1. `tools/prometheus.yml` — Prometheus configuration
2. `tools/prometheus-alerts.yml` — Alert rules
3. `tools/alertmanager.yml` — AlertManager configuration
4. `tools/cfs-cost-aggregator.sh` — Cost tracking script
5. `tools/grafana-dashboard-infrastructure.json` — Dashboard 1
6. `tools/grafana-dashboard-cicd-metrics.json` — Dashboard 2
7. `tools/grafana-dashboard-cost-analysis.json` — Dashboard 3
8. `tools/grafana-dashboard-storage-performance.json` — Dashboard 4
9. `crates/claudefs-tests/src/ci_monitoring_integration_tests.rs` — Comprehensive tests

Include inline comments explaining:
- Purpose of each section
- Why specific thresholds were chosen
- How operators should modify for different environments
- Integration points with Terraform/AWS

---

## End of Input Specification

