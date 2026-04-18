# A5: FUSE Client — Phase 38: Advanced Configuration & Multi-Node Integration

**Status:** 🟡 **PHASE 38 PLANNING** — Ready for implementation
**Date:** 2026-04-18
**Target:** Advanced configuration management, observability, multi-node integration, performance tuning
**Baseline:** Phase 37 complete with 1175+ tests
**Expected Result:** +110 tests → 1285+ total

---

## Overview

Phase 38 builds on Phase 37's production-ready foundation (otel tracing, QoS, WORM, quota, distributed sessions) by adding:

1. **Configuration Management** — Hot-reload QoS/WORM/quota policies without restart
2. **Observability & Dashboarding** — Grafana dashboards + OpenTelemetry Jaeger integration
3. **Multi-Node Integration** — Session failover, distributed policy enforcement, consistency verification
4. **Performance & Stress Testing** — Tracing overhead, token bucket performance, session lookup under load
5. **Failure Modes & Edge Cases** — Network partitions, clock skew, boundary conditions, policy conflicts

---

## Architecture Context

### Phase 37 Modules (Foundation)

All Phase 37 modules depend on configuration at runtime. Phase 38 adds the configuration layer:

- **otel_tracing_integration.rs** — Needs configurable sampling, endpoint settings
- **qos_client_bridge.rs** — Needs per-tenant QoS policy hot-reload
- **worm_enforcement.rs** — Needs configurable retention policies
- **quota_client_tracker.rs** — Needs per-tenant quota limit adjustments
- **distributed_session_manager.rs** — Needs session failover policy configuration

### Design Decisions

**D1: Configuration Source**
- Primary: A2 Metadata service (via A4 transport)
- Local cache: In-memory DashMap with TTL (3 minutes)
- Fallback: Last-known-good config if metadata unreachable
- Format: Protocol Buffers (prost) for wire format, JSON for debugging

**D2: Policy Update Delivery**
- Push-based: A2 publishes config changes via A4 RPC
- Pull-based: 30-second poll as fallback
- Epoch number to detect stale caches
- Atomic swap with no downtime (lock-free read path)

**D3: Observability**
- OpenTelemetry trace export to Jaeger (optional, configurable)
- Prometheus metrics for all 5 Phase 37 modules
- Grafana dashboards for ops visibility
- Structured logging with correlation IDs

**D4: Multi-Node Consistency**
- LWW (Last-Write-Wins) for policy conflicts
- Quorum reads for critical state (session failover decisions)
- Logical clocks for causality when system clock unreliable
- Periodic full-state sync via A2

**D5: Performance Target**
- Configuration update latency p99 < 100ms
- Token bucket throughput > 500K tokens/sec per tenant
- Session lookup p99 < 50μs (in-memory, no I/O)
- Tracing overhead at 10% sampling: < 5% CPU
- Quota enforcement: < 1μs per operation

---

## Implementation Blocks

### Block 1: Configuration Management Framework (25 tests)

**Objective:** Dynamic policy updates without restart

**New Modules:**

1. **config_manager.rs** (~600 lines)
   - PolicyEpoch: versioned config with generation counter
   - ConfigCache: TTL-based local cache with auto-refresh
   - UpdateListener: async subscriber for config changes
   - MergeStrategy: union/replace/priority-based policy merging
   - Handles: QoS, WORM, Quota, Tracing, Session configs
   - Tests (6):
     - test_config_loading_from_metadata
     - test_config_cache_expiration
     - test_config_merge_strategies
     - test_atomic_config_swap
     - test_policy_update_listener
     - test_fallback_on_metadata_unavailable

2. **config_validator.rs** (~400 lines)
   - Validate policy limits (min/max rates, retention periods)
   - Check for conflicts (e.g., conflicting WORM holds)
   - Quota consistency (total < storage capacity)
   - Policy versioning
   - Tests (4):
     - test_qos_rate_limits
     - test_worm_retention_validity
     - test_quota_consistency
     - test_policy_conflict_detection

3. **config_http_api.rs** (~350 lines)
   - REST endpoints for policy inspection/update (Axum)
   - POST /config/qos/{tenant_id} — update QoS policy
   - POST /config/worm/{path} — update WORM retention
   - POST /config/quota/{user_id} — update quota
   - GET /config/current — dump current config
   - DELETE /config/override/{policy_id} — revert override
   - Tests (6):
     - test_qos_policy_update_api
     - test_worm_retention_api
     - test_quota_update_api
     - test_config_dump_api
     - test_invalid_policy_rejection
     - test_auth_required_on_config_endpoints

4. **config_storage.rs** (~300 lines)
   - Persist config to A2 metadata with versioning
   - Audit trail (who changed what, when)
   - Rollback capability
   - Tests (3):
     - test_config_persistence_to_metadata
     - test_config_audit_trail
     - test_config_rollback

5. **config_replication.rs** (~250 lines)
   - Replicate config changes across cluster nodes
   - Cross-site replication via A6 journal
   - Tests (4):
     - test_config_broadcast_to_nodes
     - test_cross_site_config_replication
     - test_latecomers_receive_full_config
     - test_config_consistency_after_failover

6. **config_snapshot.rs** (~250 lines)
   - Snapshot configuration for auditing/debugging
   - Point-in-time config recovery
   - Tests (2):
     - test_config_snapshot_creation
     - test_config_snapshot_restore

**Key Integration Points:**
- A2: PolicyEpoch stored in metadata KV (key: `/config/fuse/{version}`)
- A4: ConfigUpdateListener receives RPC notifications
- A8: HTTP API exposed on `/config` endpoint

**Test Count:** 25 tests

---

### Block 2: Observability & Dashboarding (25 tests)

**Objective:** Production visibility for all Phase 37 + 38 modules

**New Modules:**

1. **otel_integration_advanced.rs** (~400 lines)
   - Jaeger exporter (gRPC or HTTP)
   - TraceId/SpanId injection into all operations
   - Span events for major lifecycle milestones
   - Baggage for cross-service correlation
   - Tests (6):
     - test_jaeger_export_format
     - test_traceid_propagation
     - test_span_event_recording
     - test_baggage_injection
     - test_jaeger_connectivity_retry
     - test_disable_tracing_on_high_cpu

2. **metrics_aggregator.rs** (~350 lines)
   - Unified metrics from all 5 Phase 37 modules
   - Per-tenant, per-workload-class aggregation
   - Prometheus text format export
   - Tests (5):
     - test_metrics_export_format
     - test_per_tenant_metrics
     - test_per_workload_metrics
     - test_metric_cardinality_bounds
     - test_metrics_scrape_latency

3. **dashboard_fuse_advanced.json** (~500 lines)
   - Grafana dashboard for Phase 37 + 38
   - Panels for otel traces, QoS distribution, WORM holds, quota usage, session count
   - Variables for tenant filtering, time range selection
   - Tests (2):
     - test_dashboard_json_validity
     - test_dashboard_panel_queries

4. **dashboard_configuration.json** (~350 lines)
   - Grafana dashboard for config management
   - Policy version history, config audit trail, rollback points
   - Tests (2):
     - test_config_dashboard_validity
     - test_config_dashboard_queries

5. **alerting_rules_fuse.yaml** (~200 lines)
   - Alert rules for latency, error rate, quota exhaustion
   - Rules for tracing overhead, token bucket starvation
   - Tests (3):
     - test_alerts_yaml_validity
     - test_alert_expression_syntax
     - test_alert_threshold_reasonableness

6. **health_check_advanced.rs** (~250 lines)
   - Comprehensive health checks for Phase 37/38
   - Config consistency checks, session failover readiness
   - Tests (4):
     - test_config_health_check
     - test_session_failover_health
     - test_quota_enforcement_health
     - test_otel_connectivity_health

**Test Count:** 22 tests

---

### Block 3: Multi-Node Integration (25 tests)

**Objective:** Session failover, distributed policy enforcement, consistency

**New Modules:**

1. **session_failover_coordinator.rs** (~500 lines)
   - Graceful session migration on node failure
   - FD-to-inode mapping transfer to replica
   - Lease renewal on replica
   - Client reconnect with session continuity
   - Tests (7):
     - test_primary_node_failure_detection
     - test_session_migration_to_replica
     - test_fd_mapping_transfer
     - test_client_reconnect_after_failover
     - test_lease_renewal_on_replica
     - test_concurrent_failovers
     - test_failover_with_outstanding_requests

2. **policy_consistency_verifier.rs** (~400 lines)
   - Verify QoS, WORM, quota policies consistent across nodes
   - Detect and resolve conflicts (LWW)
   - Audit trails for policy divergence
   - Tests (6):
     - test_policy_consistency_across_nodes
     - test_lww_conflict_resolution
     - test_quorum_policy_read
     - test_policy_divergence_detection
     - test_policy_audit_trail
     - test_consistency_after_network_partition

3. **multi_mount_session_coordinator.rs** (~400 lines)
   - Coordinate sessions across multiple FUSE mounts
   - FD scope isolation per mount
   - Cross-mount FD translation
   - Tests (6):
     - test_multi_mount_session_tracking
     - test_fd_isolation_per_mount
     - test_cross_mount_fd_translation
     - test_multi_mount_failover
     - test_session_scope_enforcement
     - test_multi_mount_consistency

4. **quota_consistency_enforcer.rs** (~350 lines)
   - Enforce per-user/group quotas across cluster
   - Pre-check before write (quorum read)
   - Grace period handling across nodes
   - Tests (4):
     - test_quota_quorum_read
     - test_grace_period_consistency
     - test_quota_enforcement_across_nodes
     - test_quota_limit_convergence

5. **distributed_tracing_correlator.rs** (~300 lines)
   - Correlate traces across A5 (FUSE), A4 (Transport), A2 (Metadata)
   - Baggage propagation through RPC
   - Cross-node latency attribution
   - Tests (4):
     - test_traceid_propagation_across_nodes
     - test_rpc_span_linking
     - test_cross_node_latency_attribution
     - test_trace_correlation_validation

6. **multi_site_replication_coordinator.rs** (~300 lines)
   - Coordinate session state across sites (A6 replication)
   - Site failover with session recovery
   - Tests (2):
     - test_site_failover_session_recovery
     - test_cross_site_consistency

**Test Count:** 25 tests

---

### Block 4: Performance & Stress Testing (20 tests)

**Objective:** Latency, throughput, scalability under load

**New Modules:**

1. **perf_tracing_overhead.rs** (~300 lines)
   - Benchmark otel tracing at various sampling rates (1%, 10%, 50%, 100%)
   - CPU/memory overhead measurement
   - Span creation/export latency
   - Tests (4):
     - test_trace_overhead_1_percent_sampling
     - test_trace_overhead_10_percent_sampling
     - test_trace_overhead_50_percent_sampling
     - test_trace_overhead_100_percent_sampling

2. **perf_qos_token_bucket.rs** (~350 lines)
   - Token bucket throughput at various rates
   - Burst handling performance
   - Per-tenant isolation overhead
   - Tests (4):
     - test_token_bucket_throughput_sequential
     - test_token_bucket_burst_handling
     - test_multi_tenant_qos_fairness
     - test_qos_overhead_under_contention

3. **perf_session_lookup.rs** (~300 lines)
   - Session lookup latency at 1K, 10K, 100K sessions
   - Concurrent lookups (100-1000 threads)
   - LRU eviction performance
   - Tests (3):
     - test_session_lookup_latency_scaling
     - test_concurrent_session_lookups
     - test_session_lru_eviction_overhead

4. **perf_quota_enforcement.rs** (~250 lines)
   - Pre-check latency (quorum read vs local)
   - Concurrent quota increments (fairness)
   - Grace period traversal
   - Tests (3):
     - test_quota_precheck_latency
     - test_concurrent_quota_increments
     - test_grace_period_overhead

5. **perf_config_updates.rs** (~250 lines)
   - Config update latency (end-to-end)
   - Concurrent config reads during update
   - Cache refresh overhead
   - Tests (3):
     - test_config_update_latency_p99
     - test_concurrent_reads_during_config_update
     - test_config_cache_refresh_overhead

6. **perf_multi_node.rs** (~200 lines)
   - Cross-node RPC latency
   - Session failover latency
   - Policy consistency check latency
   - Tests (3):
     - test_cross_node_rpc_latency
     - test_failover_completion_latency
     - test_policy_consistency_check_latency

**Test Count:** 20 tests (no new modules, only benchmark tests)

---

### Block 5: Failure Modes & Edge Cases (15 tests)

**Objective:** Resilience to edge cases and failure scenarios

**New Modules:**

1. **failure_network_partition.rs** (~300 lines)
   - Network partition between mounts and metadata
   - Policy conflicts from divergent updates
   - Session state inconsistency detection
   - Tests (3):
     - test_network_partition_fallback_to_cached_config
     - test_conflict_resolution_after_partition_heals
     - test_session_consistency_after_partition

2. **failure_clock_skew.rs** (~250 lines)
   - Clock skew detection (wall-clock vs logical clock)
   - WORM hold validation with skewed clocks
   - Quota renewal with time-based grace periods
   - Tests (3):
     - test_clock_skew_detection
     - test_worm_hold_validity_with_skew
     - test_quota_grace_period_with_skew

3. **failure_quota_boundary.rs** (~200 lines)
   - Quota exactly at limit (allow/deny edge case)
   - Quota refresh race conditions
   - Overflow handling
   - Tests (3):
     - test_quota_exactly_at_limit
     - test_quota_refresh_race
     - test_quota_overflow_handling

4. **failure_concurrent_policy_updates.rs** (~250 lines)
   - Concurrent updates to same policy
   - Policy merge conflicts
   - Rollback during concurrent updates
   - Tests (3):
     - test_concurrent_qos_updates
     - test_concurrent_worm_updates
     - test_policy_merge_conflict_resolution

5. **failure_session_state_loss.rs** (~200 lines)
   - Session state loss on replica crash
   - In-flight operation handling
   - Lease revocation
   - Tests (3):
     - test_session_recovery_after_state_loss
     - test_inflight_operation_cleanup
     - test_lease_revocation_after_state_loss

**Test Count:** 15 tests

---

## Test Execution Plan

### Test Phases

1. **Phase 38A: Unit Tests (Blocks 1-5)**
   - Blocks 1-2: Configuration, Observability → 47 tests
   - Blocks 3-4: Multi-Node, Performance → 45 tests
   - Block 5: Failure Modes → 15 tests
   - **Subtotal:** 107 tests

2. **Phase 38B: Integration Tests**
   - Config + QoS interaction tests → 3 tests
   - Config + WORM interaction tests → 2 tests
   - Config + Quota interaction tests → 2 tests
   - Session failover end-to-end → 3 tests
   - **Subtotal:** 10 tests

3. **Phase 38C: Multi-Node Cluster Tests** (requires A11 cluster)
   - Failover with 3-node cluster → 5 tests
   - Cross-site replication (A6 integration) → 3 tests
   - **Subtotal:** 8 tests (pending cluster)

### Test Infrastructure

- **proptest** for property-based testing (e.g., config merge strategies)
- **mock_transport.rs** for simulating RPC failures
- **simulation.rs** for node failures and network partitions
- **Jepsen-style** testing for LWW conflict resolution

---

## Deliverables

### Code

| File | Size | Purpose |
|------|------|---------|
| `config_manager.rs` | 600 | Policy versioning, caching, merging |
| `config_validator.rs` | 400 | Policy validation |
| `config_http_api.rs` | 350 | REST endpoints |
| `config_storage.rs` | 300 | A2 persistence |
| `config_replication.rs` | 250 | Cluster broadcast |
| `config_snapshot.rs` | 250 | Config snapshots |
| `otel_integration_advanced.rs` | 400 | Jaeger export |
| `metrics_aggregator.rs` | 350 | Unified metrics |
| `session_failover_coordinator.rs` | 500 | Session migration |
| `policy_consistency_verifier.rs` | 400 | Consistency checks |
| `multi_mount_session_coordinator.rs` | 400 | Multi-mount sessions |
| `quota_consistency_enforcer.rs` | 350 | Distributed quotas |
| `distributed_tracing_correlator.rs` | 300 | Trace correlation |
| `multi_site_replication_coordinator.rs` | 300 | Cross-site state |
| `perf_*.rs` (6 modules) | 1350 | Performance benchmarks |
| `failure_*.rs` (5 modules) | 1200 | Failure scenarios |
| **Total Rust** | ~8000 | Core implementation |

### Configuration & Dashboards

- `dashboard_fuse_advanced.json` — Grafana dashboard for Phase 37/38
- `dashboard_configuration.json` — Config management dashboard
- `alerting_rules_fuse.yaml` — Alert rules

### Documentation

- Phase 38 Plan (this file)
- Per-module design docs (inline)
- Operational runbook for config management
- Grafana dashboard guide

---

## Dependencies & Integration Points

### A4 Transport
- **Uses:** RPC for config updates, multi-node coordination
- **Provides:** ConfigUpdateListener subscribes to RPC notifications

### A2 Metadata
- **Uses:** PolicyEpoch KV storage, audit trail
- **Integration:** Key format `/config/fuse/{epoch}`

### A6 Replication
- **Uses:** Cross-site policy replication
- **Via:** A6 journal replication mechanism

### A8 Management
- **Uses:** HTTP API for policy updates
- **Provides:** /config endpoint in management API

### A9 Testing
- **Uses:** Multi-node cluster for failover tests
- **Depends:** A11 provisioned cluster

### A11 Infrastructure
- **Uses:** Provisioned 3-5 node cluster for multi-node tests
- **Depends:** Phase 38 Block 3+ pending A11 cluster availability

---

## Success Criteria

✅ **Block 1 (Config Management):** All 25 tests passing, zero config update failures in stress test
✅ **Block 2 (Observability):** All 22 tests passing, dashboards query in < 1s
✅ **Block 3 (Multi-Node):** All 25 tests passing, failover latency < 500ms
✅ **Block 4 (Performance):** All 20 tests passing, no regressions vs Phase 37 baselines
✅ **Block 5 (Failure Modes):** All 15 tests passing, 100% state consistency after failures

**Phase 38 Total:** 110 tests → 1285+ total (from 1175 baseline)

---

## Implementation Timeline

| Phase | Duration | Focus |
|-------|----------|-------|
| **Phase 38A** | Sessions 2-3 | Blocks 1-2 (Config + Observability) → 47 tests |
| **Phase 38B** | Sessions 3-4 | Blocks 3-4 (Multi-Node + Performance) → 70 tests |
| **Phase 38C** | Session 5+ | Block 5 (Failures) + Integration → 25 tests (cluster-dependent) |

---

## Coordination with Other Agents

### A9: Test & Validation
- Provides multi-node cluster setup for failover tests
- Validates config consistency via Jepsen-style testing
- Runs POSIX suites with config hot-reload

### A11: Infrastructure & CI
- Provisions 3-5 node test cluster for Phase 38B+
- Monitors config management performance

### A3: Data Reduction
- No direct dependency; Phase 38 is orthogonal to dedup/tiering

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Config update storms | Rate limiting, epoch-based deduplication |
| Network partition divergence | LWW + quorum reads for critical decisions |
| Session state loss | Periodic full-state sync to A2 |
| High tracing overhead | Configurable sampling (default 10%) |
| Quota limit races | Pre-check before write + grace period |

---

## Next Steps

1. **Review & Approve Plan** — Stakeholder feedback on Block structure
2. **OpenCode Implementation** — Delegate Block 1-2 to OpenCode (minimax-m2p5)
3. **Unit Test Development** — Implement 107 unit tests in parallel
4. **Integration Testing** — Post-cluster tests when A11 cluster ready
5. **Documentation** — Operational runbooks, tuning guides

---

## Appendix: Configuration Schema

### QoS Policy (per tenant)

```protobuf
message QoSPolicy {
  string tenant_id = 1;
  uint64 read_bps_limit = 2;      // bytes/sec
  uint64 write_bps_limit = 3;
  uint64 iops_limit = 4;
  uint32 burst_duration_ms = 5;
  uint64 epoch = 6;               // version number
  string last_updated_by = 7;     // user ID
  int64 updated_at_ns = 8;        // wall-clock time
}
```

### WORM Policy (per path prefix)

```protobuf
message WORMPolicy {
  string path_prefix = 1;
  uint64 retention_days = 2;      // or INFINITE
  repeated LegalHold legal_holds = 3;
  uint64 epoch = 4;
  string last_updated_by = 5;
  int64 updated_at_ns = 6;
}
```

### Quota Policy (per user/group)

```protobuf
message QuotaPolicy {
  string user_or_group_id = 1;
  uint64 bytes_hard_limit = 2;
  uint64 bytes_soft_limit = 3;
  uint64 grace_period_days = 4;
  uint64 inodes_limit = 5;
  uint64 epoch = 6;
  string last_updated_by = 7;
  int64 updated_at_ns = 8;
}
```

---

**Phase 38 Plan: Ready for implementation 🚀**

