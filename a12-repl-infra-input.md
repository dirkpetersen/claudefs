# Task: Write repl_infra_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-repl` crate focusing on audit trail integrity, UID/GID translation security, backpressure throttling, and replication lag monitoring.

## File location
`crates/claudefs-security/src/repl_infra_security_tests.rs`

## Module structure
```rust
//! Replication infrastructure security tests: audit, uidmap, backpressure, lag monitor.
//!
//! Part of A10 Phase 12: Replication infrastructure security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_repl::repl_audit::{AuditEventKind, AuditEvent, AuditFilter, AuditLog};
use claudefs_repl::uidmap::{UidMapping, GidMapping, UidMapper};
use claudefs_repl::backpressure::{
    BackpressureLevel, BackpressureConfig, BackpressureController, BackpressureManager,
};
use claudefs_repl::lag_monitor::{LagSla, LagStatus, LagSample, LagStats, LagMonitor};
use claudefs_repl::conflict_resolver::SiteId;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests. For SiteId, try `SiteId(1)` or `SiteId::new(1)` or just `1_u64` depending on how it's defined.

## Existing tests to AVOID duplicating
- `repl_security_tests.rs`: journal CRC, batch auth, conflict resolution, tls policy
- `repl_phase2_security_tests.rs`: journal source, sliding window, catchup
- `repl_deep_security_tests_v2.rs`: sliding window attacks, split-brain, active-active, checkpoint

DO NOT duplicate these. Focus on audit trail, UID mapping, backpressure, lag monitoring.

## Test categories (25 tests total)

### Category 1: Audit Trail Security (6 tests)

1. **test_audit_log_record_and_count** — Create AuditLog::new(). Record 3 events with different kinds. Verify event_count() == 3.

2. **test_audit_log_query_by_kind** — Create log. Record 2 ReplicationStarted and 1 ConflictDetected. Create AuditFilter with kind=Some(ConflictDetected). Call query(). Verify 1 result.

3. **test_audit_log_query_by_time_range** — Create log. Record events at timestamps 100, 200, 300. Filter since_ns=150, until_ns=250. Verify only timestamp 200 returned.

4. **test_audit_log_events_for_site** — Create log. Record events for site 1 and site 2. Call events_for_site(1). Verify only site 1 events returned.

5. **test_audit_log_latest_n** — Create log. Record 10 events. Call latest_n(3). Verify 3 events returned and they are the most recent.

6. **test_audit_log_clear_before** — Create log. Record events at timestamps 100, 200, 300. Call clear_before(250). Verify event_count() == 1 (only timestamp 300 remains).

### Category 2: UID/GID Translation Security (6 tests)

7. **test_uidmap_passthrough_mode** — Create UidMapper::passthrough(). Verify is_passthrough() returns true. Call translate_uid(site=1, uid=1000). Verify returns 1000 unchanged.

8. **test_uidmap_explicit_mapping** — Create UidMapper with mapping: site 1, uid 1000 → uid 2000. Call translate_uid(1, 1000). Verify returns 2000. Call translate_uid(1, 999). Verify returns 999 (unmapped = passthrough).

9. **test_uidmap_gid_mapping** — Create mapper with GidMapping: site 1, gid 100 → gid 200. Verify translate_gid(1, 100) == 200. Verify translate_gid(2, 100) == 100 (different site, no mapping).

10. **test_uidmap_add_remove_mapping** — Create empty mapper. Add UID mapping. Verify translation works. Remove mapping. Verify uid now passes through.

11. **test_uidmap_root_uid_zero** — Create mapper with mapping: site 1, uid 0 → uid 65534. Verify translate_uid(1, 0) returns 65534 (root mapped to nfsnobody). (FINDING: root UID translation prevents privilege escalation across sites).

12. **test_uidmap_listing** — Create mapper with 3 UID mappings and 2 GID mappings. Call uid_mappings(). Verify 3 returned. Call gid_mappings(). Verify 2 returned.

### Category 3: Backpressure Throttling (7 tests)

13. **test_backpressure_level_ordering** — Verify BackpressureLevel ordering: None < Mild < Moderate < Severe < Halt. Verify is_active() returns false for None and true for all others. Verify is_halted() returns true only for Halt.

14. **test_backpressure_suggested_delays** — Verify suggested_delay_ms: None=0, Mild=5, Moderate=50, Severe=500. Verify Halt returns a very large value.

15. **test_backpressure_controller_queue_depth** — Create BackpressureController with default config. Set queue_depth to 500 (below mild=1000). Compute level. Verify None. Set to 5000 (between mild and moderate). Verify Mild. Set to 50000. Verify Moderate. Set to 200000. Verify Severe.

16. **test_backpressure_error_escalation** — Create controller. Record 3 consecutive errors. Compute level. Verify at least Moderate (error_count_moderate=3). Record more errors to reach severe. Verify Severe. Call record_success() to reset error counter.

17. **test_backpressure_force_halt** — Create controller. Force halt. Verify is_halted() returns true. Clear halt. Verify is_halted() returns false.

18. **test_backpressure_manager_per_site** — Create BackpressureManager. Register sites 1 and 2. Set queue_depth 50000 for site 1 (Moderate). Verify site 2 still at None. Verify level(1) returns Some(Moderate) and level(2) returns Some(None).

19. **test_backpressure_manager_halted_sites** — Create manager. Register 3 sites. Force halt on site 1 and 3. Call halted_sites(). Verify returns [1, 3] (or contains both). Remove site 1. Verify halted_sites() only returns [3].

### Category 4: Lag Monitoring & SLA (6 tests)

20. **test_lag_monitor_ok_status** — Create LagMonitor with default SLA (warn=100ms, critical=500ms, max=2000ms). Record sample with lag_ms=50. Verify returns LagStatus::Ok.

21. **test_lag_monitor_warning_status** — Record sample with lag_ms=200 (above warn=100, below critical=500). Verify returns LagStatus::Warning { lag_ms: 200 }.

22. **test_lag_monitor_critical_status** — Record sample with lag_ms=800 (above critical=500, below max=2000). Verify returns LagStatus::Critical { lag_ms: 800 }.

23. **test_lag_monitor_exceeded_status** — Record sample with lag_ms=3000 (above max=2000). Verify returns LagStatus::Exceeded { lag_ms: 3000 }. (FINDING: exceeded SLA should trigger alert).

24. **test_lag_monitor_stats_accumulate** — Record 5 samples with varying lag values. Verify stats().sample_count == 5. Verify avg_lag_ms is correct. Verify max_lag_ms is the maximum value. Verify warning_count and critical_count match expected.

25. **test_lag_monitor_clear_samples** — Record 3 samples. Verify stats().sample_count == 3. Call clear_samples(). Verify stats reflect cleared state.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark findings with `// FINDING-REPL-INFRA-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For SiteId: try SiteId(1) first, then SiteId::new(1), then just pass 1_u64 as the site_id parameter
- For AuditLog: AuditLog::new()
- For UidMapper: UidMapper::passthrough() or UidMapper::new(uid_maps, gid_maps)
- For BackpressureController: BackpressureController::new(BackpressureConfig::default())
- For LagMonitor: LagMonitor::new(LagSla::default())

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
