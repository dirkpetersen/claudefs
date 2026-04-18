# A11: Phase 5 Block 4 — Monitoring Integration — Correction Input

**Agent:** A11 Infrastructure & CI
**Task:** Fix the monitoring integration tests - previous generation had syntax errors

**Status:** Most infrastructure files generated successfully (prometheus.yml, alertmanager.yml, dashboards all valid). Only the Rust test file had issues and was deleted.

**What Worked ✅**
- `tools/prometheus.yml` (11K) — Valid YAML
- `tools/prometheus-alerts.yml` (15K) — Valid YAML with 13 alert rules
- `tools/alertmanager.yml` (8.4K) — Valid YAML
- `tools/cfs-cost-aggregator.sh` (9.5K) — Valid bash script with execute permission
- `tools/grafana-dashboard-infrastructure.json` (8.9K) — Valid JSON
- `tools/grafana-dashboard-cicd-metrics.json` (7.2K) — Valid JSON
- `tools/grafana-dashboard-cost-analysis.json` (7.9K) — Valid JSON
- `tools/grafana-dashboard-storage-performance.json` (7.3K) — Valid JSON

**What Failed ✗**
- `crates/claudefs-tests/src/ci_monitoring_integration_tests.rs` — Syntax error at line 120 (unexpected closing brace)
  - Error: Improper nesting of test functions in module
  - Issue: Line 76 had `#[test]` followed by line 77 `fn test_alert_cost_threshold_correct()` but wasn't properly closed before new test
  - File was deleted by OpenCode, needs regeneration

**Task:** Regenerate ONLY the Rust test file with proper module and test structure

---

## Required: `crates/claudefs-tests/src/ci_monitoring_integration_tests.rs`

**Context:**
- The test file must validate all the successfully-generated infrastructure files
- Add the dependency `serde_yaml = "0.9"` to `crates/claudefs-tests/Cargo.toml` (already done)
- All tests use `std::fs`, `serde_yaml`, `serde_json` for parsing
- Tests do NOT need to run (marked `#[ignore]` if they require AWS access)

**File Structure:**
```rust
//! A11 Phase 5 Block 4: Monitoring Infrastructure Tests
//!
//! Validates all monitoring components are correctly configured:
//! - Prometheus server (prometheus.yml)
//! - Prometheus alert rules (prometheus-alerts.yml)
//! - AlertManager configuration (alertmanager.yml)
//! - Grafana dashboards (4 JSON files)
//! - Cost tracking script (cfs-cost-aggregator.sh)

use std::fs;
use std::path::{Path, PathBuf};

// Helper functions
fn get_tools_dir() -> PathBuf { ... }
fn load_yaml_file(path: &str) -> Result<serde_yaml::Value, String> { ... }
fn load_json_file(path: &str) -> Result<serde_json::Value, String> { ... }

#[cfg(test)]
mod prometheus_configuration { ... }

#[cfg(test)]
mod alert_rule_validation { ... }

#[cfg(test)]
mod grafana_dashboards { ... }

#[cfg(test)]
mod alertmanager_config { ... }

#[cfg(test)]
mod cost_tracking { ... }
```

**Module 1: prometheus_configuration (4 tests)**

Test 1: `test_prometheus_config_valid_yaml`
- Load `tools/prometheus.yml`
- Assert parses as valid YAML
- Assert contains `global` section with `scrape_interval`
- Assert contains `scrape_configs` array with >= 1 job

Test 2: `test_prometheus_scrape_targets_configured`
- Load `prometheus.yml`
- Verify scrape jobs: `prometheus`, `node-exporter`, `alertmanager`
- For each job, assert has valid config: `static_configs` or `file_sd_configs`

Test 3: `test_prometheus_alert_rules_file_valid`
- Load `tools/prometheus-alerts.yml`
- Assert parses as valid YAML
- Assert contains `groups` array
- Assert at least 1 `groups[0]["rules"]` present

Test 4: `test_prometheus_retention_policy_valid`
- Load `prometheus.yml`
- Extract `global.retention`
- Parse as duration (e.g., "30d")
- Assert >= 7 days, <= 90 days

**Module 2: alert_rule_validation (5 tests)**

Test 5: `test_alert_rules_count`
- Load `prometheus-alerts.yml`
- Count all `alert` entries across all groups
- Assert count == 13 (8 concrete + 5 placeholders)

Test 6: `test_alert_rules_have_required_fields`
- Load rules file
- For each alert rule, assert:
  - `alert` field present (string, not empty)
  - `expr` field present (PromQL expression)
  - `for` field present (duration)
  - `annotations` object present with `description`, `summary`
  - `labels` object present with `severity`

Test 7: `test_alert_cost_thresholds_correct`
- Load rules
- Find alerts: `DailySpendExceeded80Percent`, `DailySpendExceeded100Percent`, `MonthlyProjectionExceeded`
- Verify each exists and has correct threshold in expr:
  - First: `> 80`
  - Second: `> 100`
  - Third: `> 3000`

Test 8: `test_alert_cpu_memory_thresholds_correct`
- Load rules
- Find `HighCPUUsage` alert
- Extract threshold from expr, verify > 80 and < 95
- Find `HighMemoryUsage` alert
- Extract threshold, verify > 80 and < 95

Test 9: `test_alert_rule_severity_labels`
- Load rules
- Verify each rule has `labels.severity` in [critical, warning, info]
- Assert critical alerts: NodeExporterDown, DailySpendExceeded100Percent, NotEnoughSpotCapacity
- Assert warning alerts: PrometheusDown, HighCPU/Memory, Low Disk, etc.

**Module 3: grafana_dashboards (4 tests)**

Test 10: `test_grafana_dashboard_infrastructure_valid`
- Load `tools/grafana-dashboard-infrastructure.json`
- Assert valid JSON
- Assert `title` == "Infrastructure Health"
- Assert `panels` array with >= 5 panels
- Assert panels have types: heatmap (CPU/memory/disk), table (alerts), stat (uptime)

Test 11: `test_grafana_dashboard_cicd_valid`
- Load `tools/grafana-dashboard-cicd-metrics.json`
- Assert valid JSON
- Assert `title` == "CI/CD Metrics"
- Assert `panels` array with >= 5 panels
- Assert panel titles include "Pass Rate", "Duration", "Cost"

Test 12: `test_grafana_dashboard_cost_valid`
- Load `tools/grafana-dashboard-cost-analysis.json`
- Assert valid JSON
- Assert `title` == "Cost Analysis"
- Assert `panels` array with >= 4 panels
- Assert panel titles include "Daily Spend", "Monthly Projection", "Spot Savings"

Test 13: `test_grafana_dashboard_storage_valid`
- Load `tools/grafana-dashboard-storage-performance.json`
- Assert valid JSON
- Assert `title` == "Storage Performance"
- Assert `panels` array with >= 4 panels (all placeholder queries)

**Module 4: alertmanager_config (3 tests)**

Test 14: `test_alertmanager_config_valid_yaml`
- Load `tools/alertmanager.yml`
- Assert valid YAML
- Assert contains `global` section
- Assert contains `route` section
- Assert contains `receivers` array with >= 1 receiver

Test 15: `test_alertmanager_routes_complete`
- Load config
- Extract route tree
- Verify at least 3 routes with `group_by`, `group_wait`, `group_interval`
- Verify severities covered: critical, warning, info

Test 16: `test_alertmanager_receivers_configured`
- Load config
- Verify `receivers` array has entries
- Verify at least one receiver with SNS webhook or similar
- Extract SNS topic ARN if present, verify format `arn:aws:sns:us-west-2:*:cfs-alerts*`

**Module 5: cost_tracking (3 tests)**

Test 17: `test_cost_aggregator_script_exists`
- Assert file exists: `tools/cfs-cost-aggregator.sh`
- Assert file is readable
- Assert file has execute permission (+x)

Test 18: `test_cost_aggregator_script_valid_bash`
- Read `tools/cfs-cost-aggregator.sh`
- Assert contains shebang `#!/bin/bash`
- Assert contains function definitions or logic
- (Technically validated at shell syntax level already, but assert non-empty and has key sections)

Test 19: `test_cost_aggregator_cron_valid`
- Read script
- Extract cron expression or schedule info from comments
- Verify format is valid (e.g., "0 0 * * *" or daily schedule)
- Assert scheduled for once daily

Test 20: `test_monitoring_files_complete`
- Assert all required files exist with correct sizes:
  - `prometheus.yml` — > 5KB
  - `prometheus-alerts.yml` — > 10KB
  - `alertmanager.yml` — > 5KB
  - `cfs-cost-aggregator.sh` — > 5KB
  - 4 x `grafana-dashboard-*.json` — each > 5KB

---

## Implementation Notes

**Key Points:**
- Use proper module nesting: each `mod X { ... }` contains only tests for that group
- All tests should be `#[test]` functions
- Use `#[allow(dead_code)]` for helper functions not used by all tests
- Error messages should be descriptive (e.g., "Prometheus retention must be >= 7 days, got 3d")
- Handle optional fields gracefully (e.g., some YAML keys might not exist)
- Keep each test focused on ONE validation (single responsibility)

**Error Handling:**
- Use `assert!()`, `assert_eq!()` with descriptive messages
- Use `.expect()` or `.map_err()` with clear context
- Don't use `.unwrap()` without context

**Testing Philosophy:**
- These are infrastructure validation tests (not business logic)
- They verify configuration is correct, not that it works in production
- All should compile and pass without external dependencies
- Use only std + serde_yaml + serde_json

---

## Success Criteria

✅ File compiles without errors
✅ File generates no clippy warnings
✅ All 20 tests defined
✅ No syntax errors or brace mismatches
✅ Proper module structure (5 modules, 4 tests per module on average)
✅ Each test has clear assertions

---

## End of Correction Input

