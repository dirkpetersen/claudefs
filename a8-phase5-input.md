# A8 Management — Phase 5: Tracing, SLA, QoS, Webhooks, Node Scaling

You are implementing Phase 5 of the `claudefs-mgmt` crate for the ClaudeFS distributed filesystem.

## Context

ClaudeFS is a distributed POSIX filesystem in Rust. The management crate (`claudefs-mgmt`) already has 17 modules (337 tests) covering:
- Phase 1: config, metrics, api, analytics, cli
- Phase 2: indexer, scraper, alerting, quota, grafana
- Phase 3: drain, tiering, snapshot, health
- Phase 4: capacity, events, rbac, migration

## Existing Patterns (follow exactly)

```rust
// Error types: thiserror
use thiserror::Error;
#[derive(Debug, Error)]
pub enum FooError {
    #[error("description: {0}")]
    Variant(String),
}

// Serialization: serde
use serde::{Deserialize, Serialize};

// Async: tokio + Arc/RwLock
use tokio::sync::RwLock;
use std::sync::Arc;

// No external HTTP clients, no OpenTelemetry SDK imports
// All structs are self-contained, pure Rust logic
// Tests are #[cfg(test)] modules inside each file
```

## Task

Implement 5 new modules in `crates/claudefs-mgmt/src/`. Each must be a complete, standalone `.rs` file with all necessary imports, types, logic, and tests. Do NOT modify existing files — only produce new file contents.

---

### Module 1: `tracing_otel.rs`

OpenTelemetry-compatible distributed tracing integration for ClaudeFS.

```rust
// Implement without the opentelemetry SDK — use lightweight in-process types
// that match OTLP span wire format conceptually

pub struct SpanContext {
    pub trace_id: u128,       // 16 bytes
    pub span_id: u64,         // 8 bytes
    pub parent_span_id: Option<u64>,
    pub trace_flags: u8,      // 0x01 = sampled
    pub is_remote: bool,
}

pub enum SpanStatus { Unset, Ok, Error(String) }

pub struct SpanAttribute { pub key: String, pub value: AttributeValue }
pub enum AttributeValue { String(String), Int(i64), Float(f64), Bool(bool) }

pub struct Span {
    pub context: SpanContext,
    pub operation_name: String,
    pub service_name: String,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub attributes: Vec<SpanAttribute>,
    pub events: Vec<SpanEvent>,
    pub status: SpanStatus,
}

pub struct SpanEvent {
    pub name: String,
    pub time_ns: u64,
    pub attributes: Vec<SpanAttribute>,
}

pub struct SpanBuilder {
    // Builder pattern for constructing a Span
    // Methods: operation(name), service(name), parent(SpanContext),
    //          attribute(key, value), start(time_ns), finish(time_ns, status)
}

pub struct TraceBuffer {
    // Ring buffer of completed spans, capacity-bounded
    // Methods: push(Span), drain() -> Vec<Span>, len(), is_full()
    // capacity: usize, spans: VecDeque<Span>
}

pub struct SamplingDecision { pub sampled: bool, pub reason: &'static str }

pub struct RateSampler {
    // Samples 1-in-N traces
    // Methods: new(rate: u32), should_sample(trace_id: u128) -> SamplingDecision
    // rate=1 means 100%, rate=100 means 1%
}

pub struct TracePropagator {
    // W3C TraceContext format: traceparent header
    // Methods:
    //   inject(ctx: &SpanContext) -> String  — produces "00-{trace_id}-{span_id}-{flags}"
    //   extract(header: &str) -> Option<SpanContext>  — parses W3C traceparent
}

pub struct TraceExportBatch {
    pub spans: Vec<Span>,
    pub exported_at_ns: u64,
    pub service_name: String,
}

pub struct SpanStats {
    pub total_spans: u64,
    pub sampled_spans: u64,
    pub dropped_spans: u64,   // due to buffer full
    pub error_spans: u64,
}

pub struct TracingManager {
    // Manages sampling + buffering + export
    // Fields: sampler: RateSampler, buffer: TraceBuffer, stats: SpanStats
    // Methods:
    //   new(sample_rate: u32, buffer_capacity: usize) -> Self
    //   record(span: Span)  — samples, then pushes to buffer or increments dropped
    //   flush() -> TraceExportBatch  — drain buffer into batch
    //   stats() -> &SpanStats
    //   reset_stats()
}
```

**Tests (20+):**
- SpanContext creation with trace/span IDs
- SpanBuilder builds valid spans with attributes and events
- TraceBuffer: push up to capacity, is_full, drain clears buffer
- TraceBuffer: push beyond capacity drops oldest (or drops new), verify count stays bounded
- RateSampler: rate=1 samples everything, rate=100 samples ~1%
- RateSampler: rate=1 always sampled, rate=0 or very high rate
- TracePropagator: inject produces valid W3C traceparent string format
- TracePropagator: extract parses valid traceparent header
- TracePropagator: extract returns None for invalid/malformed input
- TracingManager: record updates stats, buffer fills, flush drains
- TracingManager: dropped_spans increments when buffer full
- SpanStatus Display/Debug works
- AttributeValue: all variants serialize with serde

---

### Module 2: `sla.rs`

Performance SLA tracking with p50/p95/p99 latency percentile computation.

```rust
pub enum SlaMetricKind {
    ReadLatencyUs,
    WriteLatencyUs,
    MetadataLatencyUs,
    ThroughputMBps,
    Iops,
    AvailabilityPercent,
}

pub struct SlaTarget {
    pub kind: SlaMetricKind,
    pub p50_threshold: f64,
    pub p95_threshold: f64,
    pub p99_threshold: f64,
    pub description: String,
}

pub struct LatencySample {
    pub value_us: u64,
    pub timestamp: u64,
}

pub struct PercentileResult {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub sample_count: usize,
}

pub fn compute_percentiles(samples: &[u64]) -> Option<PercentileResult>;
// Returns None if samples is empty
// Uses sorting approach (not HDR histogram, keep it simple)

pub enum SlaViolation {
    P50Exceeded { actual: f64, threshold: f64 },
    P95Exceeded { actual: f64, threshold: f64 },
    P99Exceeded { actual: f64, threshold: f64 },
}

pub struct SlaCheckResult {
    pub target: SlaMetricKind,
    pub percentiles: PercentileResult,
    pub violations: Vec<SlaViolation>,
    pub compliant: bool,
    pub checked_at: u64,
}

pub struct SlaWindow {
    // Sliding window of latency samples
    // max_samples: usize (ring buffer, drops oldest)
    // Methods: push(value_us, timestamp), compute() -> Option<PercentileResult>,
    //          len(), clear(), oldest_timestamp(), newest_timestamp()
}

pub struct SlaTarget { ... } // as above

pub struct SlaChecker {
    // Checks measurements against SLA targets
    // Methods: new() -> Self
    //          add_target(target: SlaTarget)
    //          check(kind: &SlaMetricKind, window: &SlaWindow) -> SlaCheckResult
    //          check_all(windows: &HashMap<SlaMetricKind, SlaWindow>) -> Vec<SlaCheckResult>
}

pub struct SlaReport {
    pub generated_at: u64,
    pub cluster_id: String,
    pub checks: Vec<SlaCheckResult>,
    pub compliant_count: usize,
    pub violation_count: usize,
    pub overall_compliant: bool,
}

impl SlaReport {
    pub fn new(cluster_id: String, checks: Vec<SlaCheckResult>) -> Self;
    pub fn summary_line(&self) -> String; // e.g. "3/4 SLAs met, 1 violation"
}
```

**Tests (20+):**
- compute_percentiles: empty returns None
- compute_percentiles: single element returns all same value
- compute_percentiles: known sorted slice matches expected percentiles
- compute_percentiles: unsorted slice same result as sorted
- SlaWindow: push samples, len grows, max_samples bounds ring
- SlaWindow: oldest/newest timestamps track correctly
- SlaWindow: clear resets to empty
- SlaChecker: no violation when all percentiles under threshold
- SlaChecker: p95 violation when p95 > threshold
- SlaChecker: p99 violation fires correctly
- SlaReport: compliant_count and violation_count computed correctly
- SlaReport: overall_compliant false when any violation
- SlaReport: summary_line format correct
- LatencySample: serde round-trip

---

### Module 3: `qos.rs`

QoS policy management: priority tiers, bandwidth limits per client/tenant.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum QosPriority {
    Critical = 4,   // real-time workloads
    High = 3,
    Normal = 2,     // default
    Low = 1,
    Background = 0, // GC, scrubbing, backups
}

impl QosPriority {
    pub fn weight(&self) -> u32;  // Critical=100, High=50, Normal=20, Low=5, Background=1
}

pub struct BandwidthLimit {
    pub read_mbps: Option<u64>,    // None = unlimited
    pub write_mbps: Option<u64>,
    pub iops_read: Option<u64>,
    pub iops_write: Option<u64>,
}

impl BandwidthLimit {
    pub fn unlimited() -> Self;
    pub fn read_only_mbps(mbps: u64) -> Self;
    pub fn symmetric_mbps(mbps: u64) -> Self;
    pub fn is_unlimited(&self) -> bool;
}

pub struct QosPolicy {
    pub policy_id: String,
    pub name: String,
    pub priority: QosPriority,
    pub limits: BandwidthLimit,
    pub burst_multiplier: f64,  // e.g. 1.5 = 50% burst above limit
    pub created_at: u64,
}

impl QosPolicy {
    pub fn new(policy_id, name, priority, limits) -> Self;
    pub fn with_burst(mut self, multiplier: f64) -> Self;
    pub fn effective_read_mbps(&self) -> Option<u64>;  // limit * burst
    pub fn effective_write_mbps(&self) -> Option<u64>;
}

pub struct QosAssignment {
    pub subject_id: String,        // tenant ID, client IP, or user
    pub subject_kind: SubjectKind,
    pub policy_id: String,
    pub assigned_at: u64,
}

pub enum SubjectKind { Tenant, ClientIp, User, Group }

pub struct TokenBucket {
    // Simple token bucket for rate limiting
    // capacity: u64, tokens: f64, refill_rate: f64 (tokens/sec)
    // Methods: new(capacity, refill_rate) -> Self
    //          try_consume(n: u64, elapsed_secs: f64) -> bool
    //          fill_level(&self) -> f64  // 0.0..=1.0
    //          reset()
}

pub struct QosRegistry {
    // Manages policies and assignments
    // Methods:
    //   new() -> Self
    //   add_policy(policy: QosPolicy) -> Result<(), QosError>
    //   remove_policy(id: &str) -> Result<(), QosError>
    //   get_policy(id: &str) -> Option<&QosPolicy>
    //   assign(subject_id, subject_kind, policy_id) -> Result<(), QosError>
    //   unassign(subject_id: &str) -> Result<(), QosError>
    //   get_assignment(subject_id: &str) -> Option<&QosAssignment>
    //   effective_policy(subject_id: &str) -> Option<&QosPolicy>
    //   policy_count() -> usize
    //   assignment_count() -> usize
    //   assignments_for_policy(policy_id: &str) -> Vec<&QosAssignment>
}

pub enum QosError {
    PolicyNotFound(String),
    PolicyAlreadyExists(String),
    AssignmentNotFound(String),
}
```

**Tests (20+):**
- QosPriority: ordering Critical > High > Normal > Low > Background
- QosPriority: weight() returns correct values
- BandwidthLimit: unlimited() has all None fields
- BandwidthLimit: symmetric_mbps sets both read and write
- BandwidthLimit: is_unlimited() true only when all None
- QosPolicy: burst multiplier computes effective limits correctly
- QosPolicy: effective_read_mbps None when no limit (unlimited)
- TokenBucket: new bucket starts full
- TokenBucket: try_consume succeeds when tokens available
- TokenBucket: try_consume fails when insufficient tokens
- TokenBucket: refill adds tokens proportional to elapsed_secs
- TokenBucket: fill_level returns 0.0 when empty, 1.0 when full
- QosRegistry: add/get/remove policy round-trip
- QosRegistry: add duplicate policy returns PolicyAlreadyExists
- QosRegistry: assign/unassign round-trip
- QosRegistry: get_assignment returns None after unassign
- QosRegistry: effective_policy returns None for unknown subject
- QosRegistry: effective_policy returns policy after assignment
- QosRegistry: assignments_for_policy returns correct set
- QosRegistry: remove policy when assignments exist — still removes

---

### Module 4: `webhook.rs`

HTTP webhook dispatcher for outbound delivery of filesystem events.

```rust
pub enum WebhookEvent {
    FileCreated { path: String, size: u64, owner: u32 },
    FileDeleted { path: String },
    FileModified { path: String, new_size: u64 },
    DirectoryCreated { path: String },
    DirectoryDeleted { path: String },
    NodeJoined { node_id: String, node_addr: String },
    NodeDeparted { node_id: String },
    SlaViolation { metric: String, actual: f64, threshold: f64 },
    QuotaExceeded { tenant_id: String, used_bytes: u64, quota_bytes: u64 },
    SnapshotCreated { snapshot_id: String, source_path: String },
    ReplicationLag { site_id: String, lag_ms: u64 },
}

pub struct WebhookPayload {
    pub event_id: String,
    pub event_type: String,  // snake_case name of the variant
    pub cluster_id: String,
    pub timestamp: u64,
    pub event: WebhookEvent,
}

impl WebhookPayload {
    pub fn new(cluster_id: String, event: WebhookEvent) -> Self;
    pub fn event_type(&self) -> &str;
    pub fn to_json_body(&self) -> String;  // serde_json::to_string, fallback to "{}" if error
}

pub struct WebhookEndpoint {
    pub endpoint_id: String,
    pub url: String,
    pub secret: Option<String>,   // HMAC-SHA256 signing key
    pub event_filter: Vec<String>, // empty = all events, else whitelist of event_type strings
    pub created_at: u64,
    pub active: bool,
}

impl WebhookEndpoint {
    pub fn new(endpoint_id, url) -> Self;
    pub fn with_secret(mut self, secret: String) -> Self;
    pub fn with_filter(mut self, events: Vec<String>) -> Self;
    pub fn matches(&self, event_type: &str) -> bool;  // always true if filter empty
    pub fn compute_signature(&self, body: &str) -> Option<String>;
    // HMAC-SHA256 of body using self.secret, hex-encoded
    // If no secret, returns None
    // Use: sha2 not available — use simple XOR-based mock HMAC for tests
    // Actually: implement a simple HMAC using std only — XOR of all bytes of key with body bytes
    // OR just store the signature as: format!("sha256={}", hex_encode(xor_hash(key, body)))
    // where xor_hash = iterate over body bytes XOR'd with cycling key bytes, accumulate u64
}

pub struct DeliveryAttempt {
    pub attempt_number: u32,
    pub delivered_at: u64,
    pub success: bool,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
}

pub struct DeliveryRecord {
    pub event_id: String,
    pub endpoint_id: String,
    pub payload: WebhookPayload,
    pub attempts: Vec<DeliveryAttempt>,
    pub final_success: bool,
}

impl DeliveryRecord {
    pub fn new(endpoint_id: String, payload: WebhookPayload) -> Self;
    pub fn add_attempt(&mut self, attempt: DeliveryAttempt);
    pub fn attempt_count(&self) -> usize;
    pub fn last_attempt(&self) -> Option<&DeliveryAttempt>;
}

pub struct WebhookRegistry {
    // Manages endpoints and delivery records
    // Methods:
    //   new() -> Self
    //   register(endpoint: WebhookEndpoint) -> Result<(), WebhookError>
    //   unregister(endpoint_id: &str) -> Result<(), WebhookError>
    //   get_endpoint(id: &str) -> Option<&WebhookEndpoint>
    //   active_endpoints() -> Vec<&WebhookEndpoint>
    //   endpoints_for_event(event_type: &str) -> Vec<&WebhookEndpoint>
    //   record_delivery(&mut self, record: DeliveryRecord)
    //   delivery_history(endpoint_id: &str) -> Vec<&DeliveryRecord>
    //   endpoint_count() -> usize
    //   success_rate(endpoint_id: &str) -> f64  // 0.0..=1.0
}

pub enum WebhookError {
    EndpointNotFound(String),
    DuplicateEndpoint(String),
    InvalidUrl(String),
}
```

**Tests (20+):**
- WebhookPayload: event_type() returns snake_case string for each variant
- WebhookPayload: to_json_body() produces valid JSON (parse back)
- WebhookEndpoint: matches() true when filter empty
- WebhookEndpoint: matches() true for matching event type in filter
- WebhookEndpoint: matches() false for non-matching event type
- WebhookEndpoint: compute_signature() returns None when no secret
- WebhookEndpoint: compute_signature() returns Some with secret
- WebhookEndpoint: same key+body always produces same signature
- WebhookEndpoint: different body produces different signature
- DeliveryRecord: add_attempt() grows attempts vec
- DeliveryRecord: last_attempt() returns last added
- WebhookRegistry: register/unregister round-trip
- WebhookRegistry: duplicate registration returns DuplicateEndpoint
- WebhookRegistry: active_endpoints() only returns active=true endpoints
- WebhookRegistry: endpoints_for_event filters correctly
- WebhookRegistry: record_delivery + delivery_history round-trip
- WebhookRegistry: success_rate 1.0 when all successful
- WebhookRegistry: success_rate 0.5 with equal success/failure
- WebhookRegistry: success_rate 0.0 for empty history

---

### Module 5: `node_scaling.rs`

Online node scaling management — add/remove nodes and track rebalancing.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeRole {
    Storage,
    Metadata,
    StorageAndMetadata,
    Gateway,
    Client,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    Joining,         // announced, not yet integrated
    Active,          // fully in cluster, serving I/O
    Draining,        // being removed, migrating data out
    Drained,         // data migration complete, safe to remove
    Failed,          // unreachable
    Decommissioned,  // removed from cluster
}

pub struct NodeSpec {
    pub node_id: String,
    pub address: String,
    pub role: NodeRole,
    pub nvme_capacity_bytes: u64,
    pub ram_bytes: u64,
    pub cpu_cores: u32,
}

pub struct ClusterNode {
    pub spec: NodeSpec,
    pub state: NodeState,
    pub added_at: u64,
    pub state_changed_at: u64,
    pub data_bytes: u64,    // data currently stored on this node
    pub shards: Vec<u32>,   // shard IDs assigned to this node
}

impl ClusterNode {
    pub fn new(spec: NodeSpec, now: u64) -> Self;
    pub fn transition(&mut self, new_state: NodeState, now: u64);
    pub fn is_serving(&self) -> bool;  // Active state
    pub fn fill_percent(&self) -> f64; // data_bytes / nvme_capacity_bytes * 100
}

pub struct RebalanceTask {
    pub task_id: String,
    pub from_node: String,
    pub to_node: String,
    pub shard_id: u32,
    pub bytes_total: u64,
    pub bytes_moved: u64,
    pub started_at: u64,
    pub completed_at: Option<u64>,
}

impl RebalanceTask {
    pub fn new(from: String, to: String, shard_id: u32, bytes_total: u64, now: u64) -> Self;
    pub fn progress_percent(&self) -> f64;
    pub fn is_complete(&self) -> bool;
    pub fn complete(&mut self, now: u64);
}

pub struct ScalingPlan {
    pub plan_id: String,
    pub trigger: ScalingTrigger,
    pub tasks: Vec<RebalanceTask>,
    pub created_at: u64,
    pub estimated_bytes: u64,
    pub completed_tasks: usize,
}

pub enum ScalingTrigger {
    NodeAdded(String),
    NodeRemoved(String),
    Manual,
    CapacityThreshold { threshold_percent: f64 },
}

impl ScalingPlan {
    pub fn new(plan_id: String, trigger: ScalingTrigger, tasks: Vec<RebalanceTask>, now: u64) -> Self;
    pub fn total_tasks(&self) -> usize;
    pub fn progress_percent(&self) -> f64;
    pub fn is_complete(&self) -> bool;
}

pub struct NodeScalingManager {
    // Manages cluster node lifecycle and rebalancing plans
    // Methods:
    //   new() -> Self
    //   add_node(spec: NodeSpec, now: u64) -> Result<(), ScalingError>
    //   remove_node(node_id: &str, now: u64) -> Result<(), ScalingError>
    //   transition_node(node_id: &str, new_state: NodeState, now: u64) -> Result<(), ScalingError>
    //   get_node(node_id: &str) -> Option<&ClusterNode>
    //   active_nodes() -> Vec<&ClusterNode>
    //   node_count() -> usize
    //   active_count() -> usize
    //   add_scaling_plan(plan: ScalingPlan)
    //   get_plan(plan_id: &str) -> Option<&ScalingPlan>
    //   active_plans() -> Vec<&ScalingPlan>
    //   cluster_fill_percent() -> f64  // average fill across active nodes
    //   total_capacity_bytes() -> u64  // sum across all active nodes
    //   total_data_bytes() -> u64
}

pub enum ScalingError {
    NodeAlreadyExists(String),
    NodeNotFound(String),
    InvalidTransition { from: NodeState, to: NodeState },
}
```

**Tests (25+):**
- NodeState: transitions Active→Draining valid
- ClusterNode: new() starts in Joining state
- ClusterNode: transition() updates state and timestamp
- ClusterNode: is_serving() true only for Active
- ClusterNode: fill_percent() = 0.0 for empty node
- ClusterNode: fill_percent() = 50.0 for half-full node
- RebalanceTask: progress_percent() = 0 when no bytes moved
- RebalanceTask: progress_percent() = 50.0 when half done
- RebalanceTask: progress_percent() = 100.0 when complete
- RebalanceTask: is_complete() false initially, true after complete()
- ScalingPlan: progress_percent() = 0 initially
- ScalingPlan: is_complete() true when completed_tasks == total_tasks
- NodeScalingManager: add_node adds to registry
- NodeScalingManager: duplicate add returns NodeAlreadyExists
- NodeScalingManager: add/get/remove round-trip
- NodeScalingManager: active_nodes() filters by Active state
- NodeScalingManager: remove_node on missing node returns NotFound
- NodeScalingManager: transition_node updates state
- NodeScalingManager: node_count() and active_count() correct
- NodeScalingManager: total_capacity_bytes() sums active nodes
- NodeScalingManager: cluster_fill_percent() average across active nodes
- NodeScalingManager: add_scaling_plan / get_plan round-trip
- NodeScalingManager: active_plans() only incomplete plans
- ScalingError: Debug formatting works

---

## File Structure Required

Produce exactly these 5 files:

### FILE: crates/claudefs-mgmt/src/tracing_otel.rs
[complete file content]

### FILE: crates/claudefs-mgmt/src/sla.rs
[complete file content]

### FILE: crates/claudefs-mgmt/src/qos.rs
[complete file content]

### FILE: crates/claudefs-mgmt/src/webhook.rs
[complete file content]

### FILE: crates/claudefs-mgmt/src/node_scaling.rs
[complete file content]

## Critical Requirements

1. No external crate imports except: `serde`, `thiserror`, `tokio`, `std`
2. Do NOT use `sha2`, `hmac`, or any crypto crates — use simple std-based mock
3. Every file must compile with `cargo build`
4. Every test must pass with `cargo test`
5. Each file must have at least 20 tests in a `#[cfg(test)]` module
6. Do NOT modify any existing files — only produce the 5 new files
7. Follow the exact struct/method signatures described above
8. Use `serde::{Serialize, Deserialize}` on all public data types
9. All `use serde_json` — use `serde_json` only in webhook.rs for JSON serialization (it is already a dependency via axum in the crate's Cargo.toml)

## Cargo.toml context

The existing Cargo.toml already has these dependencies:
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
axum = "0.8"
```

No new dependencies are needed.
