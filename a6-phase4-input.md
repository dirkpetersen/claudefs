# A6: ClaudeFS Replication — Phase 4: Fanout, Health, and Conflict Reporting

You are implementing Phase 4 of the `claudefs-repl` crate for ClaudeFS.

## Already Implemented (Phase 1-3, 199 tests, 9 modules)

- `error.rs`, `journal.rs`, `wal.rs`, `topology.rs` — foundation
- `conduit.rs` — in-process gRPC-style conduit
- `sync.rs` — ConflictDetector (LWW), BatchCompactor, ReplicationSync
- `uidmap.rs` — UID/GID translation
- `engine.rs` — ReplicationEngine coordinator
- `checkpoint.rs` — Point-in-time snapshots

## Task: Phase 4 — Three New Modules

### Module 1: `src/fanout.rs` — Multi-Site Fanout

When a primary site needs to replicate to N replica sites simultaneously, the fanout module manages the parallel dispatch of journal batches to all remote conduits.

```rust
/// Result of a fanout send to one remote site.
pub struct FanoutResult {
    pub site_id: u64,
    pub success: bool,
    pub entries_sent: usize,
    pub error: Option<String>,
    pub latency_us: u64,  // microseconds to send
}

/// Summary of a fanout operation across all sites.
pub struct FanoutSummary {
    pub batch_seq: u64,
    pub total_sites: usize,
    pub successful_sites: usize,
    pub failed_sites: usize,
    pub results: Vec<FanoutResult>,
}

impl FanoutSummary {
    pub fn all_succeeded(&self) -> bool
    pub fn any_failed(&self) -> bool
    pub fn failure_rate(&self) -> f64  // 0.0 to 1.0
    pub fn successful_site_ids(&self) -> Vec<u64>
    pub fn failed_site_ids(&self) -> Vec<u64>
}

/// Fans out a journal batch to multiple remote conduits in parallel.
pub struct FanoutSender {
    local_site_id: u64,
    conduits: Arc<tokio::sync::RwLock<HashMap<u64, Conduit>>>,  // site_id -> conduit
}

impl FanoutSender {
    /// Create a new fanout sender.
    pub fn new(local_site_id: u64) -> Self

    /// Register a conduit for a remote site.
    pub async fn add_conduit(&self, site_id: u64, conduit: Conduit)

    /// Remove a conduit.
    pub async fn remove_conduit(&self, site_id: u64) -> bool

    /// Returns the number of registered conduits.
    pub async fn conduit_count(&self) -> usize

    /// Fanout a batch to all registered conduits in parallel.
    /// Waits for all sends to complete (success or error).
    pub async fn fanout(&self, batch: EntryBatch) -> FanoutSummary

    /// Fanout to a specific subset of sites.
    pub async fn fanout_to(&self, batch: EntryBatch, site_ids: &[u64]) -> FanoutSummary

    /// List registered site IDs.
    pub async fn site_ids(&self) -> Vec<u64>
}
```

Include at least **20 tests** for:
- fanout to 0 sites (empty summary)
- fanout to 1 site
- fanout to 3 sites (parallel)
- FanoutSummary methods: all_succeeded, any_failed, failure_rate
- successful_site_ids / failed_site_ids
- add_conduit / remove_conduit
- conduit_count
- fanout_to subset
- fanout with empty entries
- batch_seq propagated to summary

### Module 2: `src/health.rs` — Replication Health Monitoring

Monitors the health of each replication link and overall replication status.

```rust
/// Health status of a single replication link.
pub enum LinkHealth {
    /// Replication is current (lag within acceptable bounds).
    Healthy,
    /// Lag is growing but not critical.
    Degraded { lag_entries: u64, lag_ms: Option<u64> },
    /// Conduit is disconnected or not sending.
    Disconnected,
    /// Lag exceeds critical threshold.
    Critical { lag_entries: u64 },
}

/// Health report for one remote site's replication link.
pub struct LinkHealthReport {
    pub site_id: u64,
    pub site_name: String,
    pub health: LinkHealth,
    pub last_successful_batch_us: Option<u64>,
    pub entries_behind: u64,
    pub consecutive_errors: u32,
}

/// Overall replication cluster health.
pub enum ClusterHealth {
    /// All links are healthy.
    Healthy,
    /// Some links are degraded but majority are healthy.
    Degraded,
    /// Majority of links are down or critical.
    Critical,
    /// No remote sites configured.
    NotConfigured,
}

/// Thresholds for health determination.
pub struct HealthThresholds {
    /// Entry lag before a link is considered Degraded.
    pub degraded_lag_entries: u64,
    /// Entry lag before a link is considered Critical.
    pub critical_lag_entries: u64,
    /// Consecutive errors before marking Disconnected.
    pub disconnected_errors: u32,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            degraded_lag_entries: 1000,
            critical_lag_entries: 100_000,
            disconnected_errors: 5,
        }
    }
}

/// Computes and tracks replication health across all sites.
pub struct ReplicationHealthMonitor {
    thresholds: HealthThresholds,
    // per-site state: HashMap<site_id, (consecutive_errors, last_batch_us, entries_behind)>
    site_state: HashMap<u64, SiteHealthState>,
}

// Internal state per site (not pub)
struct SiteHealthState {
    consecutive_errors: u32,
    last_successful_batch_us: Option<u64>,
    entries_behind: u64,
    site_name: String,
}

impl ReplicationHealthMonitor {
    pub fn new(thresholds: HealthThresholds) -> Self

    /// Register a site for monitoring.
    pub fn register_site(&mut self, site_id: u64, site_name: String)

    /// Record a successful batch sent/received for a site.
    pub fn record_success(&mut self, site_id: u64, entries_behind: u64, timestamp_us: u64)

    /// Record a send/receive error for a site.
    pub fn record_error(&mut self, site_id: u64)

    /// Get the health report for a specific site.
    pub fn site_health(&self, site_id: u64) -> Option<LinkHealthReport>

    /// Get health reports for all registered sites.
    pub fn all_site_health(&self) -> Vec<LinkHealthReport>

    /// Get the overall cluster health.
    pub fn cluster_health(&self) -> ClusterHealth

    /// Reset error count and state for a site (after reconnect).
    pub fn reset_site(&mut self, site_id: u64)

    /// Remove a site from monitoring.
    pub fn remove_site(&mut self, site_id: u64)
}
```

Include at least **20 tests** for:
- Empty monitor (NotConfigured cluster health)
- Register site, record success → Healthy
- Record errors → Degraded → Disconnected
- Large lag → Critical
- cluster_health with mixed states
- reset_site clears errors
- remove_site
- all_site_health returns all
- Default thresholds values
- LinkHealthReport fields

### Module 3: `src/report.rs` — Conflict and Replication Reports

Generates human-readable reports about replication status for admin consumption.

```rust
use crate::sync::Conflict;
use crate::health::LinkHealthReport;
use crate::checkpoint::ReplicationCheckpoint;

/// Summary of replication conflicts for admin reporting.
pub struct ConflictReport {
    pub site_id: u64,
    pub report_time_us: u64,
    pub conflict_count: usize,
    pub conflicts: Vec<Conflict>,
    pub affected_inodes: Vec<u64>,  // unique inodes with conflicts (sorted)
    pub lww_resolution_count: usize,  // how many were auto-resolved by LWW
}

impl ConflictReport {
    /// Generate a conflict report from a list of conflicts.
    pub fn generate(site_id: u64, conflicts: Vec<Conflict>, report_time_us: u64) -> Self

    /// Returns true if there are any unresolved conflicts requiring admin attention.
    /// (In LWW mode, all conflicts are auto-resolved, but flagged for admin review.)
    pub fn requires_attention(&self) -> bool

    /// Format as a human-readable summary string.
    pub fn summary(&self) -> String
}

/// Full replication status report.
pub struct ReplicationStatusReport {
    pub generated_at_us: u64,
    pub local_site_id: u64,
    pub engine_state: String,  // "Running", "Stopped", etc.
    pub link_health: Vec<LinkHealthReport>,
    pub cluster_health: String,
    pub latest_checkpoint: Option<ReplicationCheckpoint>,
    pub conflict_count: usize,
    pub total_entries_sent: u64,
    pub total_entries_received: u64,
}

impl ReplicationStatusReport {
    /// Create a new status report.
    pub fn new(
        local_site_id: u64,
        generated_at_us: u64,
        engine_state: String,
        link_health: Vec<LinkHealthReport>,
        cluster_health: String,
        latest_checkpoint: Option<ReplicationCheckpoint>,
        conflict_count: usize,
        total_entries_sent: u64,
        total_entries_received: u64,
    ) -> Self

    /// Format as a one-line summary.
    pub fn one_line_summary(&self) -> String

    /// Returns true if the system is in a degraded state requiring attention.
    pub fn is_degraded(&self) -> bool
}

/// Generates replication reports from engine components.
pub struct ReportGenerator {
    site_id: u64,
}

impl ReportGenerator {
    pub fn new(site_id: u64) -> Self

    /// Generate a conflict report from a conflict list.
    pub fn conflict_report(&self, conflicts: Vec<Conflict>, report_time_us: u64) -> ConflictReport

    /// Generate a basic status report (without engine integration).
    pub fn status_report(
        &self,
        generated_at_us: u64,
        engine_state: &str,
        link_health: Vec<LinkHealthReport>,
        cluster_health: &str,
        latest_checkpoint: Option<ReplicationCheckpoint>,
        conflict_count: usize,
        total_sent: u64,
        total_received: u64,
    ) -> ReplicationStatusReport
}
```

Include at least **18 tests** for:
- ConflictReport generation with 0 conflicts
- ConflictReport with multiple conflicts
- affected_inodes is sorted and deduplicated
- requires_attention() returns true when conflicts exist
- summary() returns non-empty string
- ReplicationStatusReport creation
- one_line_summary() returns non-empty string
- is_degraded() when cluster_health is "Degraded"
- ReportGenerator::conflict_report
- ReportGenerator::status_report

### Updated `src/lib.rs`

```rust
pub mod checkpoint;
pub mod conduit;
pub mod engine;
pub mod error;
pub mod fanout;
pub mod health;
pub mod journal;
pub mod report;
pub mod sync;
pub mod topology;
pub mod uidmap;
pub mod wal;
```

### Implementation Requirements

1. **Only use existing Cargo.toml deps** — no new crates.

2. **All pub types derive Debug, Clone** where possible.
   - `FanoutResult`, `FanoutSummary` derive `Debug, Clone`
   - `LinkHealth`, `LinkHealthReport`, `ClusterHealth`, `HealthThresholds` derive `Debug, Clone`
   - `ConflictReport`, `ReplicationStatusReport` derive `Debug, Clone`

3. **Imports in fanout.rs**:
   ```rust
   use crate::conduit::{Conduit, EntryBatch};
   use std::collections::HashMap;
   use std::sync::Arc;
   use tokio::time::Instant;
   ```

4. **Imports in health.rs**: No crate imports needed — standalone module.

5. **Imports in report.rs**:
   ```rust
   use crate::sync::Conflict;
   use crate::health::{LinkHealthReport, ClusterHealth};
   use crate::checkpoint::ReplicationCheckpoint;
   ```

6. **Async tests** use `#[tokio::test]`.
   Non-async modules (health.rs, report.rs) can use plain `#[test]`.

7. **All tests pass** `cargo test -p claudefs-repl`.

8. **Zero clippy warnings** `cargo clippy -p claudefs-repl -- -D warnings`.

### Output Format

```rust
// File: crates/claudefs-repl/src/fanout.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/health.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/report.rs
<complete file>
```

```rust
// File: crates/claudefs-repl/src/lib.rs
<complete updated lib.rs>
```
