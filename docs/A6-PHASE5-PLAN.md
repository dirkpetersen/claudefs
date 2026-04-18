# A6: Replication — Phase 5 Plan

**Date:** 2026-04-18
**Agent:** A6 (Replication Service)
**Current Status:** Phase 4 complete (982 tests), 85% production ready
**Target:** Phase 5 production hardening + operational readiness (Phase 5 target: 1100+ tests)

---

## Executive Summary

Phase 4 delivered all core active-active HA components:
- ✅ Write-aware quorum coordination (24 tests)
- ✅ Anti-entropy read repair (24 tests)
- ✅ Vector clock causal consistency (26 tests)
- ✅ Dual-site HA orchestration (30 tests)

Phase 5 focus: **Production hardening & operational readiness**
- Bridge metrics gap: integrate replication metrics with A8 monitoring infrastructure
- Operational validation: cluster-level testing, failover procedures, performance tuning
- Compliance: audit trail integration, SLA enforcement, recovery time validation

**Estimated scope:** 5 blocks, 110-130 new tests, 2-3 weeks

---

## Phase 5 Success Criteria

### Metrics & Monitoring (Block 1)
- [ ] Replication lag exported to Prometheus (per-site, per-operation)
- [ ] Split-brain event counter and resolution time tracked
- [ ] Read-repair action metrics (repairs triggered, success rate)
- [ ] Quorum write latency percentiles (p50, p95, p99)
- [ ] Grafana dashboard displaying all metrics (6+ panels)
- [ ] Health check integration for failover readiness

### Operational Procedures (Block 2)
- [ ] Failover procedure validated with <5s recovery time
- [ ] Split-brain resolution procedure tested under partition
- [ ] Manual recovery procedures documented and tested
- [ ] Performance tuning guide for replication lag targets
- [ ] Runbook for common operational scenarios

### Cluster-Level Testing (Block 3)
- [ ] Multi-node cluster tests (3-5 sites)
- [ ] Network partition scenarios (Jepsen-style)
- [ ] Site failure and recovery cycles
- [ ] Concurrent writes across sites
- [ ] Consistency validation across all failure modes

### Performance Validation (Block 4)
- [ ] Latency benchmarks under load (local + multi-node)
- [ ] Throughput scaling with site count
- [ ] Replication lag under various conditions
- [ ] Recovery time from partial failures
- [ ] Resource usage monitoring (CPU, memory, disk I/O)

### Integration & Production Readiness (Block 5)
- [ ] Full integration with A8 monitoring stack
- [ ] Integration with A2 metadata service (journal source)
- [ ] Integration with A4 transport (RPC endpoints)
- [ ] Integration with A11 infrastructure (cluster provisioning)
- [ ] Production deployment checklist completion

---

## Phase 5 Implementation Blocks

### Block 1: Metrics Integration (22-26 tests, ~1.5 days)

**Goal:** Export all replication metrics to Prometheus for monitoring and alerting.

**Modules to create/enhance:**
1. `repl_metrics_exporter.rs` (NEW, ~200 lines)
   - Prometheus histogram for quorum write latency
   - Counter for split-brain events
   - Gauge for replication lag (per-site)
   - Counter for repair actions triggered/successful
   - Gauge for connected sites

2. `metrics.rs` (EXISTING, ~100 lines)
   - Integrate with A8's `metrics.rs`
   - Export via Prometheus endpoint (`/metrics`)
   - Aggregate multi-site metrics

3. `health_integration.rs` (NEW, ~150 lines)
   - Health check endpoint `/health/replication`
   - Lag thresholds for alerting (warn: >60s, critical: >300s)
   - Failover readiness status
   - Return HTTP 503 if unhealthy

**Integration points:**
- A8 Management: `metrics.rs` + `metrics_collector.rs`
- A11 Infrastructure: Prometheus scrape config
- Grafana: Dashboard for replication metrics

**Tests (22-26 tests):**
- Histogram recording and retrieval (5 tests)
- Counter increments and resets (4 tests)
- Gauge updates for lag tracking (4 tests)
- Multi-site aggregation (3 tests)
- Health check thresholds (3 tests)
- Endpoint exposure and serialization (4 tests)

**Success criteria:**
- All metrics exported to Prometheus format
- Health endpoint returns correct status
- Grafana dashboard displays live metrics
- Alerts fire when lag exceeds thresholds

---

### Block 2: Operational Procedures & Documentation (20-24 tests, ~2 days)

**Goal:** Document and validate operational procedures for production deployment.

**Modules to create/enhance:**
1. `failover_controller.rs` (NEW, ~250 lines)
   - Automated failover trigger logic
   - Health-check-based failover (>3 consecutive failures)
   - Graceful degradation (continue serving single-site)
   - Recovery action when failed site returns

2. `split_brain_resolver.rs` (ENHANCEMENT, ~150 lines)
   - Automated split-brain detection
   - Resolution strategies (LWW, quorum-based, manual)
   - Audit trail for all resolutions
   - Notification to operators

3. `ops_runbook.rs` (NEW, ~200 lines)
   - Common operational scenarios (state machine)
   - Failover procedures with timing
   - Manual recovery steps
   - Performance tuning recommendations

**Documentation files:**
- `docs/REPLICATION-OPERATIONS.md` (300+ lines)
  - Quick start failover procedures
  - Split-brain troubleshooting guide
  - Performance tuning parameters
  - SLA definitions and monitoring
  - Common scenarios and remediation
- `docs/REPLICATION-PROCEDURES.md` (200+ lines)
  - Step-by-step failover process
  - Manual recovery procedures
  - Disaster recovery from dual-site failure
  - Validation checklists

**Tests (20-24 tests):**
- Failover trigger conditions (4 tests)
- Automated failover execution (4 tests)
- Graceful degradation modes (3 tests)
- Split-brain resolution strategies (5 tests)
- Recovery procedures (4 tests)

**Success criteria:**
- Failover completes in <5 seconds
- Zero data loss during failover
- Split-brain detected and reported
- All procedures documented and validated

---

### Block 3: Cluster-Level Testing (26-30 tests, ~2-3 days)

**Goal:** Validate replication across multi-node clusters with failure scenarios.

**Modules to enhance:**
1. `cluster_simulation.rs` (NEW, ~300 lines)
   - Multi-node cluster simulator (3-5 sites)
   - Simulated network delays (configurable)
   - Partition injection framework
   - Node failure/recovery scenarios

2. `jepsen_replication.rs` (NEW, ~200 lines)
   - Jepsen-style distributed tests
   - Network partition scenarios
   - Linearizability checking
   - Consistency verification post-failure

3. `scenario_runner.rs` (NEW, ~150 lines)
   - Orchestrate complex test scenarios
   - Concurrent writes during failures
   - Network partition with minority quorum
   - Dual-site failure and recovery

**Test scenarios (26-30 tests):**
- Basic multi-node replication (3 tests)
  - Write on primary, verify on replica
  - Read from both sites
  - Consistency checks

- Network partitions (6 tests)
  - Partition entire site A (write quorum preserved)
  - Partition entire site B (write quorum lost, fallback to single)
  - Asymmetric partition (A→B OK, B→A timeout)
  - Partition at different times
  - Multiple sequential partitions

- Site failures (6 tests)
  - Primary site node crash
  - Replica site node crash
  - Multiple nodes crash
  - Cascade failures

- Concurrent writes (4 tests)
  - Simultaneous writes to both sites
  - Write storm with consistent ordering
  - Quorum saturation

- Recovery scenarios (4 tests)
  - Failed site returns with lag
  - Catch-up mechanism validation
  - No data loss after recovery
  - Idempotency of repair actions

- Consistency validation (3 tests)
  - Read-your-writes after failover
  - Monotonic read consistency
  - Causal consistency with concurrent ops

**Success criteria:**
- All tests pass on cluster
- <5s failover under all failure modes
- Zero silent data corruption
- Consistency preserved after recovery

---

### Block 4: Performance Validation (20-24 tests, ~1.5 days)

**Goal:** Establish performance baselines and validate latency/throughput SLAs.

**Modules to create:**
1. `bench_replication.rs` (NEW, ~250 lines)
   - Latency benchmarks for quorum writes
   - Throughput scaling tests
   - Lag measurement under load
   - Resource utilization tracking

2. `performance_tracker_repl.rs` (NEW, ~150 lines)
   - Per-operation latency tracking
   - Histogram generation for p50, p95, p99
   - Throughput aggregation
   - Baseline comparison

**Benchmarks (20-24 tests):**
- Single-site latency (3 tests)
  - Local write latency (p50, p95, p99)
  - Local read latency
  - Metadata-only operations

- Multi-site latency (4 tests)
  - Quorum write latency (2 of 2, majority)
  - Failover latency (from primary to replica)
  - Cross-site read latency

- Throughput scaling (4 tests)
  - Operations/sec with 2 sites
  - Operations/sec with 5 sites
  - Concurrent client scaling
  - Batch write throughput

- Lag metrics (4 tests)
  - Lag under normal load
  - Lag during network congestion
  - Lag recovery time after pause
  - Lag stability (no jitter)

- Resource usage (4 tests)
  - CPU utilization (target <50% per core)
  - Memory growth over time (no leaks)
  - Disk I/O patterns
  - Network bandwidth efficiency

**Success criteria:**
- Quorum write latency <100ms p99
- Failover <5s recovery time
- Throughput >10K ops/sec on single site
- Replication lag <60s under normal load
- No resource leaks in 24h soak test

---

### Block 5: Integration & Production Readiness (18-22 tests, ~1.5 days)

**Goal:** Full integration with other subsystems and production deployment readiness.

**Integration points:**
1. A8 Management integration
   - Metrics endpoint scraping
   - Health check aggregation
   - Alerting on replication failures

2. A2 Metadata integration
   - Journal source implementation
   - Cursor tracking and recovery
   - Snapshot transfer protocol

3. A4 Transport integration
   - RPC endpoint registration
   - Connection pool management
   - Circuit breaker for failed sites

4. A11 Infrastructure integration
   - Cluster provisioning automation
   - Monitoring dashboard deployment
   - Log aggregation setup

**Modules to create/enhance:**
1. `production_readiness.rs` (NEW, ~100 lines)
   - Pre-flight checks (all sites reachable)
   - Configuration validation
   - Deployment checklist
   - Rollback procedures

2. `integration_tests.rs` (NEW, ~200 lines)
   - Full stack integration tests
   - A2+A6 metadata replication
   - A4+A6 transport reliability
   - A8+A6 monitoring integration

**Tests (18-22 tests):**
- Pre-flight validation (3 tests)
  - Site connectivity checks
  - Configuration validation
  - Resource availability

- Full stack integration (5 tests)
  - End-to-end write through all layers
  - Replication end-to-end
  - Metrics export end-to-end
  - Health checks end-to-end
  - Failover with all subsystems

- Monitoring integration (4 tests)
  - Metrics scraped by Prometheus
  - Alerts triggered correctly
  - Grafana dashboard functional
  - Health API responds correctly

- Deployment procedures (3 tests)
  - Blue-green deployment
  - Canary deployment with 1 site
  - Rollback procedures

- Production scenarios (3 tests)
  - Sustained load (8h)
  - Node scaling (add/remove sites)
  - Kernel upgrade (graceful restart)

**Success criteria:**
- 100% integration tests passing
- Production checklist completed
- Deployment procedures validated
- Ready for deployment to production cluster

---

## Implementation Schedule

### Session Plan (3 weeks total)

**Week 1 (Session 1):**
- Block 1: Metrics Integration (Days 1-2)
- Block 2: Operational Procedures (Days 3-4)
- Total: ~3 days, 48 tests

**Week 2 (Session 2):**
- Block 3: Cluster-Level Testing (Days 5-7)
- Total: ~3 days, 28 tests

**Week 3 (Session 3):**
- Block 4: Performance Validation (Days 8-9)
- Block 5: Integration & Production Readiness (Days 10-11)
- Total: ~4 days, 40 tests
- Validation, commit, CHANGELOG update

---

## Dependencies & Blockers

### Hard dependencies:
1. **A8 Management:** metrics.rs, metrics_collector.rs (for Prometheus integration)
   - Status: ✅ Available
   - Required for: Block 1, Block 5

2. **A2 Metadata:** journal_source trait definition
   - Status: ✅ Available
   - Required for: Block 5 integration

3. **A4 Transport:** RPC endpoint registration
   - Status: ✅ Available
   - Required for: Block 5 integration

4. **A11 Infrastructure:** Multi-node test cluster provisioning
   - Status: ✅ In progress (Phase 4 Block 5)
   - Required for: Block 3, Block 4, Block 5

### Soft dependencies:
- A9 Test validation (Jepsen framework)
- A10 Security audit (of failover procedures)

---

## Expected Results

### Test Coverage
- Current: 982 tests (Phase 4)
- Phase 5 target: 1,100+ tests
- Blocks: 110-130 new tests
- Total LOC: ~1,800 (new modules + enhancements)

### Deliverables
1. ✅ `repl_metrics_exporter.rs` — Prometheus metrics
2. ✅ `failover_controller.rs` — Automated failover
3. ✅ `split_brain_resolver.rs` — Conflict resolution
4. ✅ `cluster_simulation.rs` — Multi-node testing
5. ✅ `jepsen_replication.rs` — Partition tolerance tests
6. ✅ `bench_replication.rs` — Performance benchmarks
7. ✅ `production_readiness.rs` — Pre-flight checks
8. ✅ `integration_tests.rs` — Full stack tests
9. ✅ Docs: REPLICATION-OPERATIONS.md (operational guide)
10. ✅ Docs: REPLICATION-PROCEDURES.md (step-by-step procedures)

### Production Readiness
- Current: 85% (Phase 4)
- Target: 100% (Phase 5)
- Gap closed by: Metrics + Cluster testing + Integration

### Success Metrics
- All tests passing (1,100+)
- Zero clippy errors in repl crate
- Failover <5s with zero data loss
- Replication lag <60s under load
- Production deployment checklist 100% complete

---

## Commit Strategy

Each block completion = 1-2 commits:

```
[A6] Phase 5 Block 1: Metrics Integration — Prometheus Export (22 tests)
[A6] Phase 5 Block 2: Operational Procedures — Failover & Documentation (20 tests)
[A6] Phase 5 Block 3: Cluster-Level Testing — Multi-Site Validation (28 tests)
[A6] Phase 5 Block 4: Performance Validation — Latency & Throughput (22 tests)
[A6] Phase 5 Block 5: Integration & Production Readiness (20 tests)
[A6] Phase 5 Complete: Production-Ready Replication (1,100+ tests total)
```

---

## Risk Mitigation

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|-----------|
| A11 cluster delayed | Medium | High | Start local cluster simulation (Block 3) in parallel |
| Performance regression | Medium | Medium | Establish Phase 4 baselines immediately, compare continuously |
| Failover timing exceeds SLA | Low | High | Prototype failover independently, validate repeatedly |
| Split-brain not detected | Low | Critical | Comprehensive partition testing in Block 3 |
| Integration issues with A2/A4 | Medium | Medium | Early integration tests, communicate with owners |

---

## Reference

- **Phase 4 Completion:** docs/A6-PHASE4-COMPLETION.md
- **Architecture:** docs/decisions.md (D3, D6, D9 on replication)
- **Metadata Service:** docs/metadata.md
- **Transport Layer:** docs/transport.md
- **Management:** docs/agents.md (A8 specification)

---

## Next Steps

1. ✅ Review this plan with dev team
2. ✅ Confirm dependency availability (A8, A2, A4, A11)
3. ✅ Start Block 1: Metrics Integration
4. ✅ Commit plan to `docs/A6-PHASE5-PLAN.md`
5. ⏭️ Begin implementation (OpenCode + Haiku)

