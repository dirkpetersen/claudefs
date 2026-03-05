Add missing Rust doc comments to the multipath.rs module for the claudefs-transport crate.

The file has #![warn(missing_docs)] enabled at the crate level, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does
- Output the COMPLETE file with ALL the added doc comments

Missing docs needed for:
- PathId struct and its methods (new, as_u64) and impl From blocks
- PathState variants (Active, Degraded, Failed, Draining)
- PathMetrics struct and all its fields (latency_us, min_latency_us, jitter_us, loss_rate, bandwidth_bps, bytes_sent, bytes_received, errors, last_probe_us)
- PathInfo struct and all its fields (id, name, state, metrics, weight, priority)
- PathSelectionPolicy variants (RoundRobin, LowestLatency, WeightedRandom, Failover)
- MultipathConfig struct and all its fields (policy, max_paths, probe_interval_ms, failure_threshold, recovery_threshold, latency_ewma_alpha, max_loss_rate)
- MultipathStats struct and all its fields (total_paths, active_paths, failed_paths, total_requests, failover_events, paths)
- MultipathError variants (PathNotFound, MaxPathsExceeded, NoAvailablePaths)
- MultipathRouter struct and all its methods (new, add_path, remove_path, select_path, record_success, record_failure, mark_failed, mark_active, active_paths, path_info, stats)

Here is the current file content:

```rust
//! Multi-path transport for network resilience and bandwidth aggregation.
//!
//! This module provides routing across multiple network paths with configurable
//! selection policies, health monitoring, and automatic failover.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathId(#[allow(dead_code)] u64);

#[allow(clippy::derivable_impls)]
impl Default for PathId {
    fn default() -> Self {
        PathId(0)
    }
}

impl PathId {
    pub fn new(id: u64) -> Self {
        PathId(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<u64> for PathId {
    fn from(id: u64) -> Self {
        PathId(id)
    }
}

impl From<PathId> for u64 {
    fn from(id: PathId) -> Self {
        id.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathState {
    Active,
    Degraded,
    Failed,
    Draining,
}

impl PathState {
    fn is_usable(&self) -> bool {
        matches!(self, PathState::Active | PathState::Degraded)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathMetrics {
    pub latency_us: u64,
    pub min_latency_us: u64,
    pub jitter_us: u64,
    pub loss_rate: f64,
    pub bandwidth_bps: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors: u64,
    pub last_probe_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathInfo {
    pub id: PathId,
    pub name: String,
    pub state: PathState,
    pub metrics: PathMetrics,
    pub weight: u32,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PathSelectionPolicy {
    #[default]
    RoundRobin,
    LowestLatency,
    WeightedRandom,
    Failover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipathConfig {
    pub policy: PathSelectionPolicy,
    pub max_paths: usize,
    pub probe_interval_ms: u64,
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub latency_ewma_alpha: f64,
    pub max_loss_rate: f64,
}

impl Default for MultipathConfig {
    fn default() -> Self {
        MultipathConfig {
            policy: PathSelectionPolicy::LowestLatency,
            max_paths: 8,
            probe_interval_ms: 1000,
            failure_threshold: 3,
            recovery_threshold: 2,
            latency_ewma_alpha: 0.2,
            max_loss_rate: 0.05,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipathStats {
    pub total_paths: usize,
    pub active_paths: usize,
    pub failed_paths: usize,
    pub total_requests: u64,
    pub failover_events: u64,
    pub paths: Vec<PathInfo>,
}

#[derive(Debug, Error)]
pub enum MultipathError {
    #[error("path not found: {0:?}")]
    PathNotFound(PathId),
    #[error("max paths exceeded: max={0}")]
    MaxPathsExceeded(usize),
    #[error("no available paths")]
    NoAvailablePaths,
}

pub struct MultipathRouter {
    config: MultipathConfig,
    paths: Vec<PathInfo>,
    round_robin_idx: usize,
    total_requests: u64,
    failover_events: u64,
    next_path_id: u64,
    consecutive_successes: std::collections::HashMap<PathId, u32>,
    consecutive_failures: std::collections::HashMap<PathId, u32>,
}

impl MultipathRouter {
    pub fn new(config: MultipathConfig) -> Self {
        MultipathRouter {
            config,
            paths: Vec::new(),
            round_robin_idx: 0,
            total_requests: 0,
            failover_events: 0,
            next_path_id: 1,
            consecutive_successes: std::collections::HashMap::new(),
            consecutive_failures: std::collections::HashMap::new(),
        }
    }

    pub fn add_path(&mut self, name: String, weight: u32, priority: u32) -> PathId {
        let id = PathId(self.next_path_id);
        self.next_path_id += 1;

        let path_info = PathInfo {
            id,
            name,
            state: PathState::Active,
            metrics: PathMetrics::default(),
            weight,
            priority,
        };

        self.paths.push(path_info);
        id
    }

    pub fn remove_path(&mut self, id: PathId) -> bool {
        if let Some(pos) = self.paths.iter().position(|p| p.id == id) {
            self.paths.remove(pos);
            self.consecutive_successes.remove(&id);
            self.consecutive_failures.remove(&id);
            true
        } else {
            false
        }
    }

    pub fn select_path(&mut self) -> Option<PathId> {
        self.total_requests += 1;

        let (policy, rr_idx) = (self.config.policy, self.round_robin_idx);

        let active_paths: Vec<&PathInfo> =
            self.paths.iter().filter(|p| p.state.is_usable()).collect();

        if active_paths.is_empty() {
            return None;
        }

        match policy {
            PathSelectionPolicy::RoundRobin => {
                let (result, new_idx) = Self::select_round_robin_static(&active_paths, rr_idx);
                self.round_robin_idx = new_idx;
                result
            }
            PathSelectionPolicy::LowestLatency => self.select_lowest_latency(&active_paths),
            PathSelectionPolicy::WeightedRandom => self.select_weighted_random(&active_paths),
            PathSelectionPolicy::Failover => self.select_failover(&active_paths),
        }
    }

    pub fn record_success(&mut self, id: PathId, latency_us: u64, bytes: u64) { /* ... */ }

    pub fn record_failure(&mut self, id: PathId, bytes: u64) { /* ... */ }

    pub fn mark_failed(&mut self, id: PathId) {
        if let Some(path) = self.paths.iter_mut().find(|p| p.id == id) {
            path.state = PathState::Failed;
            self.failover_events += 1;
        }
    }

    pub fn mark_active(&mut self, id: PathId) {
        if let Some(path) = self.paths.iter_mut().find(|p| p.id == id) {
            path.state = PathState::Active;
            self.consecutive_successes.remove(&id);
        }
    }

    pub fn active_paths(&self) -> Vec<PathId> {
        self.paths
            .iter()
            .filter(|p| p.state == PathState::Active)
            .map(|p| p.id)
            .collect()
    }

    pub fn path_info(&self, id: PathId) -> Option<&PathInfo> {
        self.paths.iter().find(|p| p.id == id)
    }

    pub fn stats(&self) -> MultipathStats {
        let active_count = self
            .paths
            .iter()
            .filter(|p| p.state == PathState::Active)
            .count();
        let failed_count = self
            .paths
            .iter()
            .filter(|p| p.state == PathState::Failed)
            .count();

        MultipathStats {
            total_paths: self.paths.len(),
            active_paths: active_count,
            failed_paths: failed_count,
            total_requests: self.total_requests,
            failover_events: self.failover_events,
            paths: self.paths.clone(),
        }
    }
}
```

Output the COMPLETE updated multipath.rs file (use the ORIGINAL file content from the codebase, not the abbreviated version above) with all missing doc comments added. Add docs to PathId, PathState, PathMetrics, PathInfo, PathSelectionPolicy, MultipathConfig, MultipathStats, MultipathError, and MultipathRouter and all their public fields, variants, and methods.

Output ONLY the Rust source code, no markdown fences.
