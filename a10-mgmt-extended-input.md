# Task: Write mgmt_extended_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-mgmt` crate focusing on alerting, cluster bootstrap, config sync, cost tracking, and node health/scaling.

## File location
`crates/claudefs-security/src/mgmt_extended_security_tests.rs`

## Module structure
```rust
//! Extended security tests for claudefs-mgmt: alerting, bootstrap, config sync, cost, health.
//!
//! Part of A10 Phase 10: Management extended security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs)

```rust
use claudefs_mgmt::alerting::{Alert, AlertRule, AlertSeverity, AlertState};
use claudefs_mgmt::cluster_bootstrap::{BootstrapConfig, BootstrapManager, BootstrapState};
use claudefs_mgmt::config_sync::{ConfigEntry, ConfigStore, ConfigVersion};
use claudefs_mgmt::cost_tracker::{BudgetStatus, CostEntry, CostTracker};
use claudefs_mgmt::diagnostics::{CheckBuilder, DiagnosticCheck, DiagnosticReport, DiagnosticsRunner};
use claudefs_mgmt::health::{HealthStatus, NodeHealth};
use claudefs_mgmt::node_scaling::{ClusterNode, NodeRole, NodeSpec, NodeState as ScalingNodeState};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `mgmt_pentest.rs`: API endpoint security, RBAC basic, metrics exposure
- `mgmt_rbac_security_tests.rs`: RBAC mutations, audit trail, compliance, live config, rate limiter

DO NOT duplicate any of these.

## Test categories (25 tests total, 5 per category)

### Category 1: Alerting & Diagnostics (5 tests)

1. **test_alert_rule_evaluate_boundary** — Create AlertRule with threshold=100.0 and GreaterThan comparison. Evaluate with 100.0 (should be false — NOT greater). Evaluate with 100.1 (should be true). Document boundary behavior.

2. **test_alert_rule_nan_handling** — Create AlertRule. Evaluate with f64::NAN. Document whether NaN triggers the alert or is handled safely. (FINDING: NaN comparison always false in IEEE 754).

3. **test_alert_severity_ordering** — Verify AlertSeverity variants have correct ordering: Info < Warning < Critical. Document how severity is compared.

4. **test_diagnostic_report_is_healthy** — Create DiagnosticReport with all passing checks. Verify is_healthy() returns true. Add one failed critical check. Verify is_healthy() returns false.

5. **test_diagnostic_check_builder** — Create CheckBuilder::new("test"). Build a passing check. Verify it has correct name and status. Build a failing check. Verify failure message stored.

### Category 2: Cluster Bootstrap Security (5 tests)

6. **test_bootstrap_empty_cluster_name** — Create BootstrapConfig with empty cluster_name. Call BootstrapManager::new(config).start(). Verify error (empty name validation).

7. **test_bootstrap_invalid_erasure_params** — Create BootstrapConfig with erasure_k=1, erasure_m=0. Start bootstrap. Verify error (k must be >= 2, m must be >= 1).

8. **test_bootstrap_state_transitions** — Create BootstrapManager. Start bootstrap. Verify state is InProgress. Document state machine behavior.

9. **test_bootstrap_empty_nodes** — Create BootstrapConfig with empty nodes list. Start bootstrap. Verify error (no nodes to bootstrap).

10. **test_bootstrap_duplicate_node_registration** — Create BootstrapManager. Register same node_id twice. Document whether duplicate is rejected or idempotent.

### Category 3: Config Sync (5 tests)

11. **test_config_store_put_get_roundtrip** — Create ConfigStore. Put key="test", value="hello", author="admin". Get key="test". Verify value matches and version > 0.

12. **test_config_store_version_increments** — Create ConfigStore. Put 3 entries with different keys. Verify current_version() increments each time (monotonic).

13. **test_config_store_delete** — Create ConfigStore. Put entry. Delete it. Verify get returns None. Verify delete of non-existent key returns false.

14. **test_config_store_entries_since** — Create ConfigStore. Put 5 entries. Call entries_since(version=3). Verify only entries with version > 3 returned.

15. **test_config_store_empty_key** — Create ConfigStore. Put with empty key "". Document whether empty keys are accepted. (FINDING: empty key may cause lookup confusion).

### Category 4: Cost Tracking (5 tests)

16. **test_cost_tracker_total** — Create CostTracker. Record 3 entries with costs 10.0, 20.5, 30.0. Verify total_cost() == 60.5.

17. **test_cost_tracker_budget_exceeded** — Create CostTracker with budget=50.0. Record entries totaling 60.0. Call budget_status(). Verify Exceeded state.

18. **test_cost_tracker_negative_cost** — Create CostTracker. Record entry with negative cost (-5.0). Verify total_cost() correctly reflects the negative. (FINDING: negative costs could reduce apparent spend).

19. **test_cost_tracker_daily_total** — Create CostTracker. Record entries at different timestamps. Call daily_total(). Verify only entries within the day window are counted.

20. **test_cost_budget_status_thresholds** — Create CostTracker with budget=100.0. Record 70.0 (should be Ok). Record more to reach 85.0 (Warning). Record to 96.0 (Critical). Record to 101.0 (Exceeded). Verify each state.

### Category 5: Health & Node Scaling (5 tests)

21. **test_node_health_capacity_percent** — Create NodeHealth. Set capacity_total=1000, capacity_used=800. Verify capacity_percent() == 80.0. (FINDING: check if capacity_total=0 causes division by zero).

22. **test_node_health_capacity_thresholds** — Create NodeHealth. Set capacity_used to 79% of total. Verify is_capacity_warning() false. Set to 81%. Verify warning true. Set to 96%. Verify critical true.

23. **test_node_scaling_state_transitions** — Verify NodeState can_transition_to: Joining→Active (ok), Active→Draining (ok), Drained→Decommissioned (ok). Verify invalid: Drained→Active (rejected), Decommissioned→anything (rejected).

24. **test_node_role_predicates** — Verify NodeRole::Storage.is_storage() true. Verify NodeRole::Metadata.is_metadata() true. Verify cross predicates are false.

25. **test_node_health_stale_detection** — Create NodeHealth at time T. Call is_stale(T+100, threshold=60). Verify stale (100 > 60). Call is_stale(T+50, threshold=60). Verify not stale.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-MGMT-EXT-XX: description`
- If a type is not public, skip and replace with alternative
- DO NOT use async code — all tests synchronous
- Use `assert!`, `assert_eq!`, `matches!`

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
