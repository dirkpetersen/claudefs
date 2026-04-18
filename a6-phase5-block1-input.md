# A6 Phase 5 Block 1: Replication Metrics Integration & Prometheus Export

**Date:** 2026-04-18
**Target:** 22-26 tests, ~450 lines of safe Rust code
**Models:** minimax-m2p5 (primary), glm-5 (fallback)

---

## Context & Architecture

ClaudeFS is a distributed POSIX file system with 8 crates in a Cargo workspace. Agent A6 owns `claudefs-repl` (cross-site replication). Phase 4 completed active-active HA with 982 tests (85% production ready). Phase 5 Block 1 adds Prometheus metrics export for operational visibility.

**Reference commits:**
- `db8d052`: A6 Phase 4 completion (982 tests, all passing)
- `f4bad83`: A6 Phase 5 planning (docs/A6-PHASE5-PLAN.md created)

**Key documents:**
- `docs/decisions.md` (D3: Replication vs EC, D6: Flash layer, D9: Single binary)
- `docs/A6-PHASE4-COMPLETION.md` (Phase 4 modules overview)
- `docs/A6-PHASE5-PLAN.md` (Phase 5 full plan)

---

## Task: Implement Replication Metrics Integration

### Goal

Export replication subsystem metrics to Prometheus format for monitoring and operational visibility. Metrics track:
- Quorum write latency (histogram: p50, p95, p99)
- Replication lag per-site (gauge)
- Split-brain event count (counter)
- Read-repair actions triggered vs successful (counters)
- Site connectivity status (gauge)
- Health check results with lag-based alerting

### Requirements

#### 1. New Module: `repl_metrics_exporter.rs` (~200 lines)

**Purpose:** Comprehensive metrics collector and exporter.

**Types:**
```
struct ReplMetricsExporter {
    quorum_write_latency_micros: Histogram,      // histogram in microseconds
    split_brain_events_total: Counter,
    split_brain_resolution_time_secs: Gauge,
    repair_actions_triggered_total: Counter,
    repair_actions_successful_total: Counter,
    replication_lag_secs: HashMap<SiteId, Gauge>,
    connected_sites_count: Gauge,
    quorum_writes_total: Counter,
    quorum_writes_failed_total: Counter,
    local_writes_total: Counter,
    remote_writes_received_total: Counter,
}

struct HistogramBucket {
    le: f64,  // less-or-equal boundary in microseconds
    count: u64,
}

impl Histogram {
    fn new(name: &str, buckets: Vec<f64>) -> Self
    fn record(&mut self, value_micros: u64)
    fn to_prometheus(&self) -> Vec<String>  // returns lines for p50, p95, p99
}
```

**Methods:**
```
impl ReplMetricsExporter {
    fn new() -> Self
    fn record_quorum_write(&mut self, latency_micros: u64)
    fn record_split_brain_event(&mut self)
    fn record_split_brain_resolved(&mut self, resolution_time_secs: f64)
    fn record_repair_action_triggered(&mut self)
    fn record_repair_action_successful(&mut self)
    fn update_replication_lag(&mut self, site_id: SiteId, lag_secs: f64)
    fn set_connected_sites(&mut self, count: usize)
    fn increment_quorum_writes(&mut self)
    fn increment_quorum_write_failures(&mut self)
    fn increment_local_writes(&mut self)
    fn increment_remote_writes(&mut self)
    fn export_prometheus(&self) -> String  // returns full Prometheus format
    fn get_current_lag(&self, site_id: SiteId) -> Option<f64>
    fn get_current_split_brain_status(&self) -> bool  // true if any ongoing
}
```

**Integration:**
- Thread-safe (use Arc<Mutex<>> or atomic operations)
- No unsafe code
- Serializable (Display trait for Prometheus format)

**Example Prometheus output:**
```
# HELP claudefs_repl_quorum_write_latency_micros Quorum write latency histogram
# TYPE claudefs_repl_quorum_write_latency_micros histogram
claudefs_repl_quorum_write_latency_micros_bucket{le="100"} 42
claudefs_repl_quorum_write_latency_micros_bucket{le="500"} 98
claudefs_repl_quorum_write_latency_micros_bucket{le="1000"} 150
claudefs_repl_quorum_write_latency_micros_bucket{le="+Inf"} 200

# HELP claudefs_repl_replication_lag_secs Replication lag in seconds
# TYPE claudefs_repl_replication_lag_secs gauge
claudefs_repl_replication_lag_secs{site_id="us-west-2"} 2.5
claudefs_repl_replication_lag_secs{site_id="eu-central-1"} 1.8

# HELP claudefs_repl_split_brain_events_total Total split-brain events
# TYPE claudefs_repl_split_brain_events_total counter
claudefs_repl_split_brain_events_total 3

# HELP claudefs_repl_repair_actions_triggered_total Total repair actions triggered
# TYPE claudefs_repl_repair_actions_triggered_total counter
claudefs_repl_repair_actions_triggered_total 45
```

#### 2. New Module: `health_integration.rs` (~150 lines)

**Purpose:** Health check endpoint for deployment orchestration.

**Types:**
```
struct ReplHealthChecker {
    exporter: Arc<ReplMetricsExporter>,
    lag_warn_threshold_secs: f64,     // 60 seconds
    lag_critical_threshold_secs: f64, // 300 seconds
}

#[derive(Debug, Clone, Serialize)]
struct ReplHealthStatus {
    status: HealthStatus,  // "healthy" | "degraded" | "unhealthy"
    lag_secs: HashMap<SiteId, f64>,
    split_brain_detected: bool,
    connected_sites: usize,
    message: String,
}

#[derive(Debug, Serialize)]
enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}
```

**Methods:**
```
impl ReplHealthChecker {
    fn new(exporter: Arc<ReplMetricsExporter>) -> Self
    fn check_health(&self) -> ReplHealthStatus {
        // Returns Healthy if:
        //   - All connected sites
        //   - All lag < warn_threshold
        //   - No split-brain
        // Returns Degraded if:
        //   - Some lag >= warn_threshold but < critical_threshold
        //   - One site disconnected
        // Returns Unhealthy if:
        //   - Any lag >= critical_threshold
        //   - All sites disconnected
        //   - Split-brain detected
    }
    fn to_http_response(&self) -> (u16, String) {
        // Returns (200, json) for Healthy/Degraded
        // Returns (503, json) for Unhealthy
    }
    fn set_lag_thresholds(&mut self, warn: f64, critical: f64)
    fn get_status_json(&self) -> String  // serde_json serialization
}
```

**JSON response example:**
```json
{
  "status": "healthy",
  "lag_secs": {
    "us-west-2": 2.5,
    "eu-central-1": 1.8
  },
  "split_brain_detected": false,
  "connected_sites": 2,
  "message": "Replication healthy, all sites in sync"
}
```

#### 3. Enhancement: Integrate with `engine.rs` (existing)

**Changes:**
- Add `metrics_exporter: Arc<ReplMetricsExporter>` field to `ReplicationEngine` struct
- Call `metrics_exporter.record_quorum_write(latency)` in quorum write path
- Call `metrics_exporter.update_replication_lag(site_id, lag)` in lag calculation
- Call `metrics_exporter.record_split_brain_event()` when split-brain detected
- Call `metrics_exporter.record_repair_action_*()` in read-repair path

#### 4. Enhancement: Integrate with `dual_site_orchestrator.rs` (existing)

**Changes:**
- Add `metrics_exporter: Arc<ReplMetricsExporter>` field
- Wire metrics calls into orchestrator methods
- Export health status from orchestrator

#### 5. Enhancement: `lib.rs` - Module exports

**Add to `pub mod` declarations:**
```rust
pub mod repl_metrics_exporter;
pub mod health_integration;

pub use repl_metrics_exporter::ReplMetricsExporter;
pub use health_integration::{ReplHealthChecker, ReplHealthStatus};
```

---

## Tests (22-26 tests total)

### File: `crates/claudefs-repl/src/repl_metrics_exporter.rs` (bottom of file)

**Test module (18-20 tests):**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Histogram tests (5 tests)
    #[test]
    fn test_histogram_creation() { ... }
    #[test]
    fn test_histogram_record_single_value() { ... }
    #[test]
    fn test_histogram_record_multiple_values() { ... }
    #[test]
    fn test_histogram_percentiles_calculation() { ... }
    #[test]
    fn test_histogram_prometheus_format() { ... }

    // Counter & Gauge tests (4 tests)
    #[test]
    fn test_counter_increment() { ... }
    #[test]
    fn test_gauge_update_and_read() { ... }
    #[test]
    fn test_multiple_site_lag_tracking() { ... }
    #[test]
    fn test_concurrent_metric_updates() { ... }

    // Integration tests (4 tests)
    #[test]
    fn test_split_brain_counter() { ... }
    #[test]
    fn test_repair_action_tracking() { ... }
    #[test]
    fn test_exporter_full_prometheus_output() { ... }
    #[test]
    fn test_exporter_clone_and_thread_safety() { ... }
}
```

### File: `crates/claudefs-repl/src/health_integration.rs` (bottom of file)

**Test module (4-6 tests):**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_healthy_status() { ... }

    #[test]
    fn test_health_checker_degraded_on_lag() { ... }

    #[test]
    fn test_health_checker_unhealthy_on_split_brain() { ... }

    #[test]
    fn test_health_check_http_response_codes() { ... }

    #[test]
    fn test_health_status_json_serialization() { ... }
}
```

---

## Implementation Notes

### Design Patterns

1. **Thread-Safety:**
   - Use `Arc<RwLock<>>` for shared state
   - Or use atomic operations where applicable
   - No `unsafe` code

2. **Histogram Implementation:**
   - Pre-defined buckets: [100, 500, 1000, 5000, 10000, 50000, 100000, +Inf] microseconds
   - Use `std::sync::atomic::AtomicU64` for lock-free updates (convert f64 bits to u64)

3. **Lag Tracking:**
   - HashMap<SiteId, (lag_secs, last_updated_ts)>
   - Automatic cleanup of stale entries (not updated > 5 minutes)

4. **Prometheus Format:**
   - Follow Prometheus text exposition format exactly
   - HELP + TYPE lines, then metric lines
   - Labels in quoted strings: `{site_id="us-west-2"}`

### Error Handling

- Use `anyhow::Result<T>` for recoverable errors
- Use `thiserror::Error` for library errors
- Never panic; return errors gracefully

### Testing Philosophy

- Property-based tests for histogram percentile accuracy
- Mock-based tests for concurrent updates
- JSON serialization roundtrip tests
- Prometheus format validation tests

---

## Existing Code to Reference

**Similar patterns in the codebase:**

1. `crates/claudefs-mgmt/src/metrics.rs` — Gauge, Counter implementations
2. `crates/claudefs-repl/src/metrics.rs` — Basic Prometheus formatting
3. `crates/claudefs-mgmt/src/health.rs` — Health check endpoint pattern
4. `crates/claudefs-repl/src/dual_site_orchestrator.rs` — HA orchestration context

---

## Deliverables

### Files to create:
1. ✅ `crates/claudefs-repl/src/repl_metrics_exporter.rs` (~200 lines, 18-20 tests)
2. ✅ `crates/claudefs-repl/src/health_integration.rs` (~150 lines, 4-6 tests)

### Files to enhance:
3. ✅ `crates/claudefs-repl/src/lib.rs` — Add module exports
4. ✅ `crates/claudefs-repl/src/engine.rs` — Wire metrics calls
5. ✅ `crates/claudefs-repl/src/dual_site_orchestrator.rs` — Wire metrics calls

### Total:
- ~450 lines of new/enhanced code
- 22-26 new tests
- 0 unsafe code required
- All dependencies within workspace (serde, tokio, tracing already available)

---

## Success Criteria

✅ All 22-26 tests passing
✅ No clippy errors (crate-level only: missing_docs)
✅ Prometheus format output validated
✅ Health check responses correct (200 for healthy, 503 for unhealthy)
✅ Thread-safe metric updates
✅ JSON serialization working
✅ Code integrated into engine and orchestrator
✅ Modules exported in lib.rs
✅ Documentation comments on all public types/methods

---

## Build & Test Commands

```bash
# Build the repl crate
cargo build -p claudefs-repl

# Run tests
cargo test -p claudefs-repl --lib repl_metrics_exporter
cargo test -p claudefs-repl --lib health_integration

# Lint
cargo clippy -p claudefs-repl

# Full test suite (all A6 tests must pass)
cargo test -p claudefs-repl --lib
# Expected: 982+ tests passing (922 existing + 22-26 new)
```

---

## Reference & Context

**Architecture decisions (docs/decisions.md):**
- D3: Replication vs EC — 2x journal replication for durability
- D6: Flash layer full — cache mode eviction and tiering
- D9: Single binary — all subsystems as Tokio tasks

**Agent responsibilities (docs/agents.md):**
- A6 (Replication): Cross-site journal replication, cloud conduit
- A8 (Management): Prometheus exporter, Parquet indexer, DuckDB gateway
- A11 (Infrastructure): Terraform, test cluster, CI/CD

**Phase 5 plan (docs/A6-PHASE5-PLAN.md):**
- Block 1: Metrics Integration (22-26 tests, 1.5 days)
- Block 2-5: Operational procedures, cluster testing, performance, integration

---

## Questions for OpenCode

If anything is unclear, please ask before implementing. Key clarifications:

1. Should histogram use pre-allocated bucketing or dynamic per-call?
2. Should lag tracking auto-expire old entries or keep forever?
3. Should health status be a simple enum or trait-based?
4. Should Prometheus export format validation be in tests or production code?

---

## Timeline

**Estimated:** 1.5 days (6-8 hours)
- 2h: Design + module skeleton
- 2h: Core implementation (metrics_exporter, health_integration)
- 1h: Integration (wire into engine, orchestrator)
- 1h: Tests (22-26 total)
- 1h: Bug fixes, clippy cleanup, validation

**Commit strategy:**
```
[A6] Phase 5 Block 1: Metrics Integration — Prometheus Export (22-26 tests)
```

---

## Output Format

Please provide:

1. **Code for each file:**
   - `repl_metrics_exporter.rs` (full implementation)
   - `health_integration.rs` (full implementation)
   - Partial code for `engine.rs` enhancements (show insertion points)
   - Partial code for `dual_site_orchestrator.rs` enhancements
   - Partial code for `lib.rs` exports

2. **Test code:**
   - Full test module for both new files

3. **Integration guidance:**
   - Specific lines to modify in existing files
   - Usage examples

4. **Documentation:**
   - Module-level doc comments
   - Examples in public type documentation

---

## End of Specification

This is a complete specification for OpenCode. All required context, examples, patterns, and success criteria are included. Proceed with implementation.

