# A1 Phase 11 Block 4: Production Hardening & Observability — OpenCode Implementation

**Context:** ClaudeFS Storage Engine (A1), Phase 11 Block 4 implementation (final block). All code must pass `cargo build` and `cargo test --release` with zero clippy warnings.

**Target:** Implement 3 modules (edge_cases.rs, observability_enhancements.rs, graceful_degradation.rs) with 20 comprehensive tests totaling ~500 LOC.

---

## Architecture Overview

### System Context
- **Crate:** `crates/claudefs-storage`
- **Related Modules:** `io_scheduler_fairness.rs`, `io_accounting.rs`, `device_health_monitor.rs`
- **Goal:** Production readiness for Phase 3 validation

### Key Constraints
- No unbounded memory allocation
- All I/O paths must handle edge cases gracefully
- Observability overhead <1% on critical paths
- No data loss under any failure scenario

### Dependencies
- `tokio` — async runtime
- `parking_lot` — efficient locks
- `dashmap` — concurrent HashMap
- `tracing` — distributed tracing
- `metrics` (optional) — Prometheus integration
- Standard: `std::sync`, `std::collections`

---

## Module 1: edge_cases.rs (~150 LOC)

**Purpose:** Handle extremely large segments, extreme I/O rates, thermal throttling, partial completions.

### Public API
```rust
pub struct EdgeCaseHandler {
    max_segment_size_bytes: u64,
    max_io_rate_ops_per_sec: u64,
    thermal_threshold: u32, // degrees C
}

impl EdgeCaseHandler {
    pub fn new(
        max_segment_size_bytes: u64,
        max_io_rate_ops_per_sec: u64,
        thermal_threshold: u32,
    ) -> Self { ... }

    /// Handle I/O to very large segment (>1GB)
    pub async fn handle_large_segment_io(
        &self,
        segment_id: SegmentId,
        offset: u64,
        size: u64,
    ) -> Result<(), EdgeCaseError> { ... }

    /// Handle extreme I/O rate (>1M ops/sec)
    pub async fn handle_extreme_io_rate(
        &self,
        pending_ops: u64,
    ) -> Result<RateControlDecision, EdgeCaseError> { ... }

    /// Check thermal status and adjust performance
    pub fn check_thermal_status(
        &self,
    ) -> Result<ThermalStatus, EdgeCaseError> { ... }

    /// Handle partial I/O completion
    pub async fn handle_partial_io_completion(
        &self,
        completed_bytes: u64,
        requested_bytes: u64,
    ) -> Result<PartialIOAction, EdgeCaseError> { ... }

    /// Handle device timeout
    pub async fn handle_device_timeout(
        &self,
        device_id: DeviceId,
        timeout_duration_ms: u64,
    ) -> Result<TimeoutAction, EdgeCaseError> { ... }

    /// Validate I/O coalescing limits
    pub fn validate_io_coalescing_limits(
        &self,
        op_count: usize,
        total_bytes: u64,
    ) -> Result<bool, EdgeCaseError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum EdgeCaseError {
    #[error("segment too large")]
    SegmentTooLarge,
    #[error("I/O rate exceeded")]
    RateExceeded,
    #[error("thermal throttling required")]
    ThermalThrottle,
    #[error("partial I/O: {0} of {1} bytes")]
    PartialIO(u64, u64),
    #[error("device timeout")]
    DeviceTimeout,
    #[error("coalescing limit exceeded")]
    CoalescingLimitExceeded,
}

#[derive(Clone, Copy, Debug)]
pub enum RateControlDecision {
    Accept,
    Backpressure, // Slow down caller
    Reject,       // Too much, refuse I/O
}

#[derive(Clone, Copy, Debug)]
pub enum ThermalStatus {
    Normal,
    Warm,         // <threshold
    Hot,          // ≥threshold
    Throttled,    // Reduce I/O rate
}

#[derive(Clone, Copy, Debug)]
pub enum PartialIOAction {
    Retry,        // Try again later
    Scatter,      // Spread to other devices
    ReturnError,  // Tell caller about partial completion
}

#[derive(Clone, Copy, Debug)]
pub enum TimeoutAction {
    Retry,
    Failover,     // Use replica
    ReturnError,
}
```

### Implementation Details

**Algorithms:**

1. **Large Segment I/O:**
   - Split into chunks (max 64MB per chunk)
   - Process sequentially with progress tracking
   - Checksum after each chunk

2. **Extreme I/O Rate:**
   - Use token bucket: max tokens = 1M ops/sec
   - If pending > capacity: apply backpressure (return Backpressure)
   - Prevent queue runaway

3. **Thermal Handling:**
   - Read device temperature via SMART or kernel interface
   - If approaching threshold: reduce prefetch depth, increase io_depth limits
   - If exceeded: throttle I/O rate, reject new writes

4. **Partial I/O:**
   - NVMe can return partial completions (rare)
   - Retry if transient, scatter to other devices if persistent

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_large_segment_io() {
        // Given: segment 2GB, request 1GB read
        // When: handle_large_segment_io()
        // Then: splits into 16 x 64MB chunks
        //       all checksums match, no corruption
    }

    #[tokio::test]
    async fn test_extreme_io_rates() {
        // Given: 1M pending I/O operations
        // When: handle_extreme_io_rate(1_000_000)
        // Then: returns Backpressure or Reject (not Accept)
        //       prevents runaway queue
    }

    #[tokio::test]
    async fn test_thermal_throttling_graceful() {
        // Given: device temp = 90°C, threshold = 85°C
        // When: check_thermal_status()
        // Then: returns Hot, I/O rate reduced
        //       client I/O still works (not hung)
    }

    #[tokio::test]
    async fn test_partial_io_completion() {
        // Given: write request 100MB, device returns 99MB completed
        // When: handle_partial_io_completion(99, 100)
        // Then: action = Retry or Scatter (not data loss)
    }

    #[tokio::test]
    async fn test_device_timeout_recovery() {
        // Given: NVMe device stops responding for 2s
        // When: timeout detected
        // Then: action = Failover to replica (not hang client)
    }

    #[tokio::test]
    async fn test_io_coalescing_limits() {
        // Given: 10K I/O ops, 1GB total
        // When: validate_io_coalescing_limits(10000, 1GB)
        // Then: returns true (within limits)
        // Given: 1M I/O ops, 1GB total
        // Then: returns false (too many small ops, coalesce needed)
    }
}
```

---

## Module 2: observability_enhancements.rs (~200 LOC)

**Purpose:** Add latency histograms, health dashboarding, trace-based analysis.

### Public API
```rust
pub struct ObservabilityEnhancer {
    latency_histogram: Arc<LatencyHistogram>,
    device_health_exporter: Arc<DeviceHealthExporter>,
    trace_context_tracker: Arc<TraceContextTracker>,
}

impl ObservabilityEnhancer {
    pub fn new() -> Self { ... }

    /// Record operation latency
    pub fn record_operation_latency(
        &self,
        op_type: OperationType,
        latency_us: u64,
    ) { ... }

    /// Get latency percentiles
    pub fn get_latency_percentiles(
        &self,
        op_type: OperationType,
    ) -> Result<LatencyPercentiles, ObsError> { ... }

    /// Export device health for Prometheus
    pub fn export_device_health(
        &self,
    ) -> Result<Vec<HealthMetric>, ObsError> { ... }

    /// Propagate trace context through I/O chain
    pub fn set_trace_context(
        &self,
        trace_id: TraceId,
        span_id: SpanId,
    ) { ... }

    /// Get trace-based latency attribution
    pub fn get_trace_latency_attribution(
        &self,
        trace_id: TraceId,
    ) -> Result<LatencyAttribution, ObsError> { ... }

    /// Record histogram with concurrency
    pub fn record_histogram_concurrent(
        &self,
        op_type: OperationType,
        latencies: Vec<u64>,
    ) { ... }

    /// Measure observability overhead
    pub fn measure_overhead_fast_path(&self) -> f64 { ... }

    /// Configure trace sampling
    pub fn set_trace_sampling_rate(&self, rate: f64) { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum ObsError {
    #[error("histogram not found")]
    HistogramNotFound,
    #[error("insufficient data")]
    InsufficientData,
    #[error("export error")]
    ExportError,
}

#[derive(Clone, Copy, Debug)]
pub enum OperationType {
    Read,
    Write,
    Seek,
    Flush,
}

pub struct LatencyPercentiles {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub p99_9: u64,
}

pub struct HealthMetric {
    pub device_id: DeviceId,
    pub metric_name: String,
    pub value: f64,
}

pub struct LatencyAttribution {
    pub total_latency_us: u64,
    pub device_io_latency_us: u64,
    pub scheduling_latency_us: u64,
    pub other_latency_us: u64,
}

pub struct TraceId(u64);
pub struct SpanId(u64);

struct LatencyHistogram {
    buckets: Arc<DashMap<OperationType, Vec<u64>>>,
}

struct DeviceHealthExporter {
    health_data: Arc<RwLock<Vec<HealthMetric>>>,
}

struct TraceContextTracker {
    trace_map: Arc<DashMap<TraceId, TraceContext>>,
}

struct TraceContext {
    trace_id: TraceId,
    span_id: SpanId,
    start_time: std::time::Instant,
    events: Vec<TraceEvent>,
}

struct TraceEvent {
    timestamp: std::time::Instant,
    event_type: String,
    latency_us: u64,
}
```

### Implementation Details

**Algorithms:**

1. **Latency Histogram:**
   - Store in circular buffer (capped memory)
   - Compute percentiles on-demand (p50, p95, p99, p99.9)
   - Thread-safe concurrent recording

2. **Device Health Export:**
   - Poll device SMART data every 10s
   - Format as Prometheus metrics
   - Include: temp, errors, wear level, etc.

3. **Trace Context:**
   - Attach trace ID to every I/O operation
   - Record: entry time, exit time, device latency, scheduling latency
   - Export JSON for distributed tracing tools

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_latency_histogram_accuracy() {
        // Given: 1000 operations with known latencies
        // When: record latencies, compute percentiles
        // Then: p50, p95, p99 within 1% of expected
    }

    #[tokio::test]
    async fn test_device_health_export() {
        // Given: device health data
        // When: export_device_health()
        // Then: returns Prometheus format metrics
        //       includes temp, errors, wear
    }

    #[tokio::test]
    async fn test_trace_context_propagation() {
        // Given: trace_id = 123, span_id = 456
        // When: set_trace_context(), perform I/O
        // Then: trace_id flows through entire operation
        //       no context loss
    }

    #[tokio::test]
    async fn test_histogram_concurrency() {
        // Given: 100 threads recording latencies concurrently
        // When: record_histogram_concurrent()
        // Then: all 100 threads' data recorded
        //       no data loss, no crashes
    }

    #[tokio::test]
    async fn test_histogram_memory_bounds() {
        // Given: histogram with N buckets
        // When: record 1M latencies
        // Then: memory usage capped (circular buffer)
        //       no unbounded growth
    }

    #[tokio::test]
    async fn test_observability_zero_overhead_fast_path() {
        // Given: observability enabled
        // When: measure_overhead_fast_path()
        // Then: overhead <1% (minimal impact)
    }

    #[tokio::test]
    async fn test_trace_sampling() {
        // Given: sampling_rate = 0.1 (10%)
        // When: record 1000 traces
        // Then: ~100 traces recorded (not all)
        //       reduces overhead
    }

    #[tokio::test]
    async fn test_dashboarding_queries() {
        // Given: Prometheus queries defined
        // When: run queries (mocked Prometheus)
        // Then: return valid results
        //       p99 latency, error rate, etc.
    }
}
```

---

## Module 3: graceful_degradation.rs (~150 LOC)

**Purpose:** Degrade scheduling under load, adapt EC reconstruction, maintain correctness.

### Public API
```rust
pub struct GracefulDegradation {
    degradation_state: Arc<RwLock<DegradationState>>,
    load_monitor: Arc<LoadMonitor>,
    recovery_scheduler: Arc<BackgroundScheduler>,
}

impl GracefulDegradation {
    pub fn new(
        load_monitor: Arc<LoadMonitor>,
        recovery_scheduler: Arc<BackgroundScheduler>,
    ) -> Self { ... }

    /// Degrade scheduling under disk pressure
    pub fn degrade_scheduling(&self, load_percent: u32) -> SchedulingDegradation { ... }

    /// Degrade EC reconstruction if overloaded
    pub fn degrade_ec_reconstruction(&self) -> EcDegradation { ... }

    /// Apply load shedding (prioritize critical I/O)
    pub fn apply_load_shedding(
        &self,
        pending_ops: &[IoOperation],
    ) -> Result<Vec<IoOperation>, DegradationError> { ... }

    /// Recover from degradation
    pub async fn recover_full_capability(&self) -> Result<(), DegradationError> { ... }

    /// Get current degradation visibility
    pub fn get_degradation_status(&self) -> DegradationStatus { ... }

    /// Ensure no data loss during degradation
    pub fn verify_correctness_maintained(&self) -> Result<bool, DegradationError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum DegradationError {
    #[error("recovery failed")]
    RecoveryFailed,
    #[error("correctness check failed")]
    CorrectnessCheckFailed,
    #[error("invalid operation sequence")]
    InvalidSequence,
}

#[derive(Clone, Copy, Debug)]
pub struct SchedulingDegradation {
    pub prefetch_depth_reduction: f64, // 0-1, 0=no reduction
    pub io_depth_limit_reduction: f64,
    pub scheduling_latency_increase: f64, // seconds
}

#[derive(Clone, Copy, Debug)]
pub enum EcDegradation {
    Normal,              // 4+2 EC
    HighLoad,            // 3+2 EC (faster reconstruction)
    CriticalLoad,        // 2+1 EC (minimal compute)
    NoEC,                // Replication only (fallback)
}

pub struct DegradationStatus {
    pub is_degraded: bool,
    pub degradation_level: u32, // 0-3
    pub scheduling_degradation: SchedulingDegradation,
    pub ec_degradation: EcDegradation,
    pub estimated_recovery_time_ms: u64,
}

struct DegradationState {
    current_level: u32,
    last_degraded: std::time::Instant,
    recovery_in_progress: bool,
}

pub struct IoOperation {
    pub op_id: u64,
    pub priority: Priority,
    pub op_type: OperationType,
}

pub enum Priority {
    Critical,
    High,
    Normal,
    Low,
}
```

### Implementation Details

**Algorithms:**

1. **Scheduling Degradation:**
   - At >80% disk utilization: reduce prefetch_depth 20%
   - At >90%: reduce 50%
   - Prevents queue saturation

2. **EC Degradation:**
   - Normal: 4+2 (1.5x overhead)
   - High load: 3+2 (1.67x overhead, faster reconstruction)
   - Critical: 2+1 (fallback, minimal compute)
   - Never: 1+0 (no protection)

3. **Load Shedding:**
   - Prioritize: data writes > metadata writes > prefetch > monitoring
   - Drop: lowest-priority ops if queue >1000

4. **Recovery:**
   - Monitor load continuously
   - Once <70%: begin recovering full capability
   - Incremental: restore prefetch depth 10% per minute
   - Restore EC scheme back to 4+2

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_disk_pressure_degradation() {
        // Given: disk utilization 85%
        // When: degrade_scheduling()
        // Then: prefetch_depth_reduction ≈ 0.2 (20%)
    }

    #[tokio::test]
    async fn test_ec_reconstruction_degradation() {
        // Given: load_percent = 95%
        // When: degrade_ec_reconstruction()
        // Then: returns CriticalLoad (2+1 EC)
    }

    #[tokio::test]
    async fn test_load_shedding() {
        // Given: 1500 pending ops (queue full)
        // When: apply_load_shedding()
        // Then: returns top 1000 by priority
        //       data writes > prefetch > monitoring
    }

    #[tokio::test]
    async fn test_degradation_recovery() {
        // Given: degraded (level=3)
        // When: load drops to 50%, recover_full_capability()
        // Then: incremental restore, back to normal after <10 min
    }

    #[tokio::test]
    async fn test_degradation_visibility() {
        // Given: degradation active
        // When: get_degradation_status()
        // Then: returns current level, estimates
        //       admin can see state
    }

    #[tokio::test]
    async fn test_no_data_loss_in_degradation() {
        // Given: degradation in effect
        // When: data write submitted
        // Then: verify_correctness_maintained() = true
        //       no data loss, no corruption
    }
}
```

---

## Integration Testing

**All 3 modules work together:**

```
EdgeCaseHandler
    ← detects_extreme_conditions
    → triggers_degradation

GracefulDegradation
    → adapts_scheduling
    → adapts_ec_scheme
    → applies_load_shedding

ObservabilityEnhancer
    ← observes_degradation_state
    → exports_metrics
    → enables_debugging
```

---

## Code Style & Conventions

**Async:**
- All I/O via Tokio
- `#[tokio::test]` for async tests

**Error Handling:**
- `#[derive(thiserror::Error)]`
- Propagate via `?`
- Test all error paths

**Concurrency:**
- `Arc<DashMap>` for concurrent updates
- `Arc<RwLock>` for state
- `Arc<AtomicU32>` for counters

**Testing:**
- Deterministic tests (no timing flakiness)
- Property-based with `proptest`
- Concurrency verified with thread spawning

**Logging:**
- Use `tracing` for important events
- Include operation IDs for debugging

---

## Expected Output

**Files to create:**
1. `crates/claudefs-storage/src/edge_cases.rs` (~150 LOC, 6 tests)
2. `crates/claudefs-storage/src/observability_enhancements.rs` (~200 LOC, 8 tests)
3. `crates/claudefs-storage/src/graceful_degradation.rs` (~150 LOC, 6 tests)

**Total:** ~500 LOC, 20 tests

**Validation:**
- `cargo build -p claudefs-storage` succeeds
- `cargo test -p claudefs-storage --lib` passes 20/20 new + 1320+ existing tests
- `cargo clippy` has zero errors
- No regressions

---

## Deliverable Checklist

- [ ] All 3 modules compile
- [ ] All 20 tests pass
- [ ] Zero clippy warnings
- [ ] No regressions
- [ ] Code follows ClaudeFS conventions
- [ ] Error types properly defined
- [ ] Thread safety verified
- [ ] Test coverage >90%
- [ ] Documentation complete
- [ ] Ready for `cargo build && cargo test --release`
