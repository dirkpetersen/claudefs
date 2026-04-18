# A6 Phase 5 Block 1: Replication Metrics Integration

## Context

You're implementing Prometheus metrics export for the ClaudeFS replication subsystem. The crate is `claudefs-repl`. 

**SiteId type:** Already defined in topology.rs as `pub type SiteId = u64;`

**Existing patterns to follow:**
- `crates/claudefs-repl/src/metrics.rs` - existing Metric, ReplMetrics pattern with Prometheus formatting
- `crates/claudefs-repl/src/health.rs` - existing HealthStatus, health check patterns
- `crates/claudefs-repl/src/dual_site_orchestrator.rs` - has HealthStatus enum (use this)

## Task 1: Create `repl_metrics_exporter.rs` (~200 lines + 18-20 tests)

### Types to implement:

```rust
/// Histogram bucket for latency tracking
#[derive(Debug, Clone)]
pub struct Histogram {
    name: String,
    help: String,
    buckets: Vec<f64>,  // le boundaries: [100, 500, 1000, 5000, 10000, 50000, 100000, +Inf]
    counts: Vec<std::sync::atomic::AtomicU64>,
    sum: std::sync::atomic::AtomicU64,
    count: std::sync::atomic::AtomicU64,
}

impl Histogram {
    pub fn new(name: &str, help: &str, buckets: Vec<f64>) -> Self
    pub fn record(&self, value_micros: u64)
    pub fn to_prometheus(&self) -> Vec<String>  // returns HELP, TYPE, bucket lines
}
```

```rust
/// Atomic counter for thread-safe incrementing
#[derive(Debug)]
pub struct Counter {
    name: String,
    help: String,
    value: std::sync::atomic::AtomicU64,
}

impl Counter {
    pub fn new(name: &str, help: &str) -> Self
    pub fn increment(&self)
    pub fn get(&self) -> u64
}
```

```rust
/// Atomic gauge for thread-safe updates
#[derive(Debug)]
pub struct Gauge {
    name: String,
    help: String,
    value: std::sync::atomic::AtomicU64,  // store f64 bits
}

impl Gauge {
    pub fn new(name: &str, help: &str) -> Self
    pub fn set(&self, value: f64)
    pub fn get(&self) -> f64
}
```

```rust
/// Main metrics exporter - thread-safe via Arc
pub struct ReplMetricsExporter {
    quorum_write_latency_micros: Arc<Histogram>,
    split_brain_events_total: Arc<Counter>,
    split_brain_resolution_time_secs: Arc<Gauge>,
    repair_actions_triggered_total: Arc<Counter>,
    repair_actions_successful_total: Arc<Counter>,
    replication_lag_secs: std::sync::Mutex<std::collections::HashMap<SiteId, Arc<Gauge>>>,
    connected_sites_count: Arc<Gauge>,
    quorum_writes_total: Arc<Counter>,
    quorum_writes_failed_total: Arc<Counter>,
    local_writes_total: Arc<Counter>,
    remote_writes_received_total: Arc<Counter>,
}
```

### Methods:

```rust
impl ReplMetricsExporter {
    pub fn new() -> Self
    pub fn record_quorum_write(&self, latency_micros: u64)
    pub fn record_split_brain_event(&self)
    pub fn record_split_brain_resolved(&self, resolution_time_secs: f64)
    pub fn record_repair_action_triggered(&self)
    pub fn record_repair_action_successful(&self)
    pub fn update_replication_lag(&self, site_id: SiteId, lag_secs: f64)
    pub fn set_connected_sites(&self, count: usize)
    pub fn increment_quorum_writes(&self)
    pub fn increment_quorum_write_failures(&self)
    pub fn increment_local_writes(&self)
    pub fn increment_remote_writes(&self)
    pub fn export_prometheus(&self) -> String  // full Prometheus format
    pub fn get_current_lag(&self, site_id: SiteId) -> Option<f64>
    pub fn get_current_split_brain_status(&self) -> bool
}
```

### Implementation notes:
- Use `std::sync::atomic` for lock-free counters/gauges
- Histogram buckets: [100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0, 100000.0, f64::MAX]
- Convert f64 to u64 bits for atomic storage in gauge
- Use Arc everywhere for thread-safety
- Prometheus format: HELP line, TYPE line, metric lines with labels

## Task 2: Create `health_integration.rs` (~150 lines + 4-6 tests)

### Types:

```rust
use serde::Serialize;

/// Health check status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded => write!(f, "degraded"),
            HealthStatus::Unhealthy => write!(f, "unhealthy"),
        }
    }
}
```

```rust
/// Health status response
#[derive(Debug, Clone, Serialize)]
pub struct ReplHealthStatus {
    pub status: HealthStatus,
    pub lag_secs: std::collections::HashMap<SiteId, f64>,
    pub split_brain_detected: bool,
    pub connected_sites: usize,
    pub message: String,
}
```

```rust
/// Health checker using metrics exporter
pub struct ReplHealthChecker {
    exporter: std::sync::Arc<crate::repl_metrics_exporter::ReplMetricsExporter>,
    lag_warn_threshold_secs: f64,      // default 60.0
    lag_critical_threshold_secs: f64,  // default 300.0
    split_brain_active: std::sync::Mutex<bool>,
}
```

### Methods:

```rust
impl ReplHealthChecker {
    pub fn new(exporter: std::sync::Arc<crate::repl_metrics_exporter::ReplMetricsExporter>) -> Self
    pub fn check_health(&self) -> ReplHealthStatus
    pub fn to_http_response(&self) -> (u16, String)  // (status_code, json_body)
    pub fn set_lag_thresholds(&mut self, warn: f64, critical: f64)
    pub fn get_status_json(&self) -> String  // serde_json
    pub fn mark_split_brain(&self, active: bool)
}
```

### Health logic:
- **Healthy**: all sites connected, all lag < warn_threshold, no split-brain
- **Degraded**: some lag >= warn but < critical, or one site disconnected
- **Unhealthy**: any lag >= critical, all sites disconnected, or split-brain detected

## Task 3: Update `lib.rs`

Add module exports:
```rust
pub mod repl_metrics_exporter;
pub mod health_integration;

pub use repl_metrics_exporter::ReplMetricsExporter;
pub use health_integration::{ReplHealthChecker, ReplHealthStatus, HealthStatus};
```

## Task 4: Update `engine.rs`

Add to `ReplicationEngine` struct:
```rust
pub struct ReplicationEngine {
    // ... existing fields ...
    metrics_exporter: std::sync::Arc<crate::repl_metrics_exporter::ReplMetricsExporter>,
}
```

Update `ReplicationEngine::new()` to create the exporter and add a public getter:
```rust
pub fn metrics_exporter(&self) -> std::sync::Arc<crate::repl_metrics_exporter::ReplMetricsExporter> {
    self.metrics_exporter.clone()
}
```

## Task 5: Update `dual_site_orchestrator.rs`

Add to `DualSiteOrchestrator` struct:
```rust
pub struct DualSiteOrchestrator {
    // ... existing fields ...
    health_checker: std::sync::Mutex<Option<crate::health_integration::ReplHealthChecker>>,
}
```

Add methods:
```rust
pub fn set_health_checker(&self, checker: crate::health_integration::ReplHealthChecker)
pub fn get_health_status(&self) -> Option<crate::health_integration::ReplHealthStatus>
```

## Tests (22-26 total)

### repl_metrics_exporter tests (18-20):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Histogram tests
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

    // Counter & Gauge tests
    #[test]
    fn test_counter_increment() { ... }
    #[test]
    fn test_gauge_update_and_read() { ... }
    #[test]
    fn test_multiple_site_lag_tracking() { ... }
    #[test]
    fn test_concurrent_metric_updates() { ... }

    // Integration tests
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

### health_integration tests (4-6):
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

## Success Criteria

- All tests pass
- No clippy errors
- Prometheus format correct (validate with existing patterns in metrics.rs)
- Thread-safe (Arc<Atomic> pattern)
- No unsafe code
- ~450 lines total

## Important Notes

1. Use `std::sync::atomic` - no tokio specific async needed for metrics
2. Follow existing patterns from metrics.rs for Prometheus formatting
3. Use the HealthStatus enum already in dual_site_orchestrator.rs for consistency
4. SiteId comes from topology.rs: `pub type SiteId = u64;`
5. All public types need doc comments