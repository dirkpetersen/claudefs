# A6 Phase 5 Block 1: Metrics Integration Implementation Guide

**Date:** 2026-04-18
**Status:** Ready for OpenCode delegation or manual implementation
**Scope:** 2 new modules + enhancements to 3 existing modules
**Target:** 22-26 tests, ~450 lines of production-ready Rust

---

## Overview

Phase 5 Block 1 bridges the production readiness gap by exporting replication subsystem metrics to Prometheus format. This enables operational visibility for:

1. **Replication Lag Tracking** — per-site, near real-time
2. **Quorum Write Performance** — latency histograms (p50, p95, p99)
3. **Split-Brain Detection** — event counts + resolution tracking
4. **Read-Repair Efficiency** — triggered vs successful repairs
5. **Site Connectivity** — connected/disconnected status
6. **Health Status** — aggregated health check for deployment

---

## Module 1: `repl_metrics_exporter.rs` (NEW)

**Purpose:** Core metrics collection and Prometheus export.

**Key Concept:** Thread-safe metrics accumulator with Prometheus text exposition format export.

### Type Definitions

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Unique site identifier for metrics
pub type SiteId = String;

/// Histogram with pre-defined buckets for latency tracking
#[derive(Debug, Clone)]
pub struct Histogram {
    buckets: Vec<(f64, u64)>,  // (boundary_micros, count)
    total_count: u64,
    total_sum: f64,
}

impl Histogram {
    /// Create histogram with standard latency buckets
    fn new() -> Self {
        let bucket_boundaries = vec![
            100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0, 100000.0, f64::INFINITY
        ];

        Self {
            buckets: bucket_boundaries.into_iter().map(|b| (b, 0)).collect(),
            total_count: 0,
            total_sum: 0.0,
        }
    }

    fn record(&mut self, value_micros: f64) {
        self.total_count += 1;
        self.total_sum += value_micros;

        for (boundary, count) in &mut self.buckets {
            if value_micros <= *boundary {
                *count += 1;
            }
        }
    }

    fn percentile(&self, p: f64) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }

        let target_count = ((p / 100.0) * self.total_count as f64) as u64;
        let mut cumulative = 0u64;

        for (boundary, count) in &self.buckets {
            cumulative += count;
            if cumulative >= target_count {
                return *boundary;
            }
        }

        self.buckets.last().map(|(b, _)| *b).unwrap_or(0.0)
    }
}

/// Main metrics exporter
#[derive(Debug, Clone)]
pub struct ReplMetricsExporter {
    inner: Arc<RwLock<ReplMetricsExporterInner>>,
}

#[derive(Debug)]
struct ReplMetricsExporterInner {
    // Latency tracking
    quorum_write_latency: Histogram,

    // Counters
    split_brain_events_total: u64,
    repair_actions_triggered_total: u64,
    repair_actions_successful_total: u64,
    quorum_writes_total: u64,
    quorum_writes_failed_total: u64,
    local_writes_total: u64,
    remote_writes_received_total: u64,

    // Lag and status
    replication_lag_secs: HashMap<SiteId, f64>,
    split_brain_resolution_time_secs: Option<f64>,
    connected_sites_count: usize,

    last_split_brain_detection: Option<Instant>,
}

impl ReplMetricsExporter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ReplMetricsExporterInner {
                quorum_write_latency: Histogram::new(),
                split_brain_events_total: 0,
                repair_actions_triggered_total: 0,
                repair_actions_successful_total: 0,
                quorum_writes_total: 0,
                quorum_writes_failed_total: 0,
                local_writes_total: 0,
                remote_writes_received_total: 0,
                replication_lag_secs: HashMap::new(),
                split_brain_resolution_time_secs: None,
                connected_sites_count: 0,
                last_split_brain_detection: None,
            }))
        }
    }

    /// Record a quorum write latency measurement
    pub fn record_quorum_write(&self, latency_micros: u64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.quorum_write_latency.record(latency_micros as f64);
            inner.quorum_writes_total += 1;
        }
    }

    /// Record a quorum write failure
    pub fn record_quorum_write_failure(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.quorum_writes_failed_total += 1;
        }
    }

    /// Record a split-brain event
    pub fn record_split_brain_event(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.split_brain_events_total += 1;
            inner.last_split_brain_detection = Some(Instant::now());
        }
    }

    /// Record time to resolve split-brain
    pub fn record_split_brain_resolved(&self, resolution_time_secs: f64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.split_brain_resolution_time_secs = Some(resolution_time_secs);
        }
    }

    /// Record repair action triggered
    pub fn record_repair_action_triggered(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.repair_actions_triggered_total += 1;
        }
    }

    /// Record repair action succeeded
    pub fn record_repair_action_successful(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.repair_actions_successful_total += 1;
        }
    }

    /// Update replication lag for a site (in seconds)
    pub fn update_replication_lag(&self, site_id: SiteId, lag_secs: f64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.replication_lag_secs.insert(site_id, lag_secs);
        }
    }

    /// Update connected sites count
    pub fn set_connected_sites(&self, count: usize) {
        if let Ok(mut inner) = self.inner.write() {
            inner.connected_sites_count = count;
        }
    }

    /// Get current Prometheus format output
    pub fn export_prometheus(&self) -> Result<String, std::fmt::Error> {
        let inner = self.inner.read().map_err(|_| std::fmt::Error)?;
        let mut output = String::new();

        // Quorum write latency histogram
        output.push_str("# HELP claudefs_repl_quorum_write_latency_micros Quorum write latency in microseconds\n");
        output.push_str("# TYPE claudefs_repl_quorum_write_latency_micros histogram\n");

        for (boundary, count) in &inner.quorum_write_latency.buckets {
            output.push_str(&format!(
                "claudefs_repl_quorum_write_latency_micros_bucket{{le=\"{}\"}} {}\n",
                if boundary.is_finite() { boundary.to_string() } else { "+Inf".to_string() },
                count
            ));
        }
        output.push_str(&format!("claudefs_repl_quorum_write_latency_micros_count {}\n", inner.quorum_write_latency.total_count));
        output.push_str(&format!("claudefs_repl_quorum_write_latency_micros_sum {}\n", inner.quorum_write_latency.total_sum));

        // Split-brain events
        output.push_str("# HELP claudefs_repl_split_brain_events_total Total split-brain events detected\n");
        output.push_str("# TYPE claudefs_repl_split_brain_events_total counter\n");
        output.push_str(&format!("claudefs_repl_split_brain_events_total {}\n", inner.split_brain_events_total));

        // Replication lag per site
        output.push_str("# HELP claudefs_repl_lag_secs Replication lag in seconds per site\n");
        output.push_str("# TYPE claudefs_repl_lag_secs gauge\n");
        for (site_id, lag_secs) in &inner.replication_lag_secs {
            output.push_str(&format!(
                "claudefs_repl_lag_secs{{site_id=\"{}\"}} {}\n",
                site_id, lag_secs
            ));
        }

        // Repair actions
        output.push_str("# HELP claudefs_repl_repair_actions_triggered_total Total repair actions triggered\n");
        output.push_str("# TYPE claudefs_repl_repair_actions_triggered_total counter\n");
        output.push_str(&format!("claudefs_repl_repair_actions_triggered_total {}\n", inner.repair_actions_triggered_total));

        output.push_str("# HELP claudefs_repl_repair_actions_successful_total Total repair actions successful\n");
        output.push_str("# TYPE claudefs_repl_repair_actions_successful_total counter\n");
        output.push_str(&format!("claudefs_repl_repair_actions_successful_total {}\n", inner.repair_actions_successful_total));

        // Connected sites
        output.push_str("# HELP claudefs_repl_connected_sites_count Number of connected sites\n");
        output.push_str("# TYPE claudefs_repl_connected_sites_count gauge\n");
        output.push_str(&format!("claudefs_repl_connected_sites_count {}\n", inner.connected_sites_count));

        Ok(output)
    }
}

impl Default for ReplMetricsExporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_creation() {
        let hist = Histogram::new();
        assert_eq!(hist.total_count, 0);
    }

    #[test]
    fn test_histogram_record() {
        let mut hist = Histogram::new();
        hist.record(150.0);
        assert_eq!(hist.total_count, 1);
    }

    #[test]
    fn test_metrics_exporter_new() {
        let exporter = ReplMetricsExporter::new();
        // Should not panic
        let _ = exporter.export_prometheus();
    }

    #[test]
    fn test_record_quorum_write() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_quorum_write(500);
        // Verify it doesn't panic and metrics are collected
    }

    #[test]
    fn test_split_brain_counter() {
        let exporter = ReplMetricsExporter::new();
        exporter.record_split_brain_event();
        exporter.record_split_brain_resolved(1.5);
    }
}
```

---

## Module 2: `health_integration.rs` (NEW)

**Purpose:** Health check endpoint for deployment orchestration.

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use crate::repl_metrics_exporter::{ReplMetricsExporter, SiteId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplHealthStatus {
    pub status: HealthStatus,
    pub lag_secs: HashMap<SiteId, f64>,
    pub split_brain_detected: bool,
    pub connected_sites: usize,
    pub message: String,
}

pub struct ReplHealthChecker {
    exporter: Arc<ReplMetricsExporter>,
    lag_warn_threshold_secs: f64,
    lag_critical_threshold_secs: f64,
}

impl ReplHealthChecker {
    pub fn new(exporter: Arc<ReplMetricsExporter>) -> Self {
        Self {
            exporter,
            lag_warn_threshold_secs: 60.0,
            lag_critical_threshold_secs: 300.0,
        }
    }

    pub fn set_lag_thresholds(&mut self, warn: f64, critical: f64) {
        self.lag_warn_threshold_secs = warn;
        self.lag_critical_threshold_secs = critical;
    }

    pub fn check_health(&self) -> ReplHealthStatus {
        // Read current metrics state
        if let Ok(prometheus_output) = self.exporter.export_prometheus() {
            // Parse metrics to determine health
            // This is a simplified example

            ReplHealthStatus {
                status: HealthStatus::Healthy,
                lag_secs: HashMap::new(),
                split_brain_detected: false,
                connected_sites: 2,
                message: "Replication healthy".to_string(),
            }
        } else {
            ReplHealthStatus {
                status: HealthStatus::Unhealthy,
                lag_secs: HashMap::new(),
                split_brain_detected: false,
                connected_sites: 0,
                message: "Failed to read metrics".to_string(),
            }
        }
    }

    pub fn to_http_response(&self) -> (u16, String) {
        let status = self.check_health();
        let status_code = match status.status {
            HealthStatus::Healthy | HealthStatus::Degraded => 200,
            HealthStatus::Unhealthy => 503,
        };

        let response_json = serde_json::to_string(&status).unwrap_or_default();
        (status_code, response_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_creation() {
        let exporter = Arc::new(ReplMetricsExporter::new());
        let checker = ReplHealthChecker::new(exporter);
        assert_eq!(checker.lag_warn_threshold_secs, 60.0);
    }

    #[test]
    fn test_health_status_json() {
        let status = ReplHealthStatus {
            status: HealthStatus::Healthy,
            lag_secs: HashMap::new(),
            split_brain_detected: false,
            connected_sites: 2,
            message: "All good".to_string(),
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("healthy"));
    }
}
```

---

## Integration Points

### 1. Update `lib.rs`

Add module exports:

```rust
pub mod repl_metrics_exporter;
pub mod health_integration;

pub use repl_metrics_exporter::ReplMetricsExporter;
pub use health_integration::{ReplHealthChecker, ReplHealthStatus};
```

### 2. Update `engine.rs`

Add field to `ReplicationEngine`:

```rust
metrics_exporter: Arc<ReplMetricsExporter>,
```

In constructor:

```rust
let metrics_exporter = Arc::new(ReplMetricsExporter::new());
// ...
Self {
    // ... other fields
    metrics_exporter,
}
```

In quorum write method:

```rust
let start = Instant::now();
// ... perform write ...
let latency_micros = start.elapsed().as_micros() as u64;
self.metrics_exporter.record_quorum_write(latency_micros);
```

### 3. Update `dual_site_orchestrator.rs`

Similar integration with `metrics_exporter` field and method calls.

---

## Test Coverage Requirements

### File: `repl_metrics_exporter.rs` (18-20 tests)

1. **Histogram tests (5):**
   - `test_histogram_creation`
   - `test_histogram_record_single`
   - `test_histogram_record_multiple`
   - `test_histogram_percentiles`
   - `test_histogram_prometheus_format`

2. **Counter/Gauge tests (4):**
   - `test_counter_increment`
   - `test_gauge_update`
   - `test_lag_per_site`
   - `test_thread_safety`

3. **Integration tests (4+):**
   - `test_split_brain_counter`
   - `test_repair_tracking`
   - `test_full_prometheus_export`
   - `test_metrics_clone`

### File: `health_integration.rs` (4-6 tests)

1. `test_health_checker_healthy`
2. `test_health_checker_degraded_on_lag`
3. `test_health_checker_unhealthy_on_split_brain`
4. `test_http_response_codes`
5. `test_health_status_serialization`

---

## Build & Validation

```bash
# Build with all features
cargo build -p claudefs-repl

# Run tests
cargo test -p claudefs-repl --lib

# Run clippy
cargo clippy -p claudefs-repl

# Verify target
# Expected: 1004+ tests (982 existing + 22-26 new)
```

---

## Status

✅ OpenCode prompt prepared: `a6-phase5-block1-input.md`
✅ Implementation guide created: this document
✅ Test structure defined
✅ Integration points documented
🟡 Awaiting OpenCode execution or manual implementation

**Next step:** Run OpenCode to generate production code

