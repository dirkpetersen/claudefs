//! Multi-path transport for network resilience and bandwidth aggregation.
//!
//! This module provides routing across multiple network paths with configurable
//! selection policies, health monitoring, and automatic failover.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathId(u64);

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

impl Default for PathId {
    fn default() -> Self {
        PathId(0)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathSelectionPolicy {
    RoundRobin,
    LowestLatency,
    WeightedRandom,
    Failover,
}

impl Default for PathSelectionPolicy {
    fn default() -> Self {
        PathSelectionPolicy::LowestLatency
    }
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

    fn select_round_robin_static(
        active_paths: &[&PathInfo],
        mut rr_idx: usize,
    ) -> (Option<PathId>, usize) {
        if active_paths.is_empty() {
            return (None, rr_idx);
        }

        let start_idx = rr_idx;
        let len = active_paths.len();

        for _ in 0..len {
            let idx = rr_idx % len;
            rr_idx = (rr_idx + 1) % len;

            if active_paths[idx].state.is_usable() {
                return (Some(active_paths[idx].id), rr_idx);
            }
        }

        for i in 0..len {
            let idx = (start_idx + i) % len;
            if active_paths[idx].state.is_usable() {
                return (Some(active_paths[idx].id), (idx + 1) % len);
            }
        }

        (None, rr_idx)
    }

    fn select_lowest_latency(&self, active_paths: &[&PathInfo]) -> Option<PathId> {
        let mut best: Option<(&PathInfo, u64, u32)> = None;

        for path in active_paths {
            let latency = if path.metrics.latency_us == 0 {
                u64::MAX
            } else {
                path.metrics.latency_us
            };

            match best {
                None => best = Some((path, latency, path.priority)),
                Some((_, best_lat, best_prio)) => {
                    if latency < best_lat || (latency == best_lat && path.priority < best_prio) {
                        best = Some((path, latency, path.priority));
                    }
                }
            }
        }

        best.map(|(p, _, _)| p.id)
    }

    fn select_weighted_random(&self, active_paths: &[&PathInfo]) -> Option<PathId> {
        if active_paths.is_empty() {
            return None;
        }

        let total_weight: u64 = active_paths.iter().map(|p| p.weight as u64).sum();
        if total_weight == 0 {
            return active_paths.first().map(|p| p.id);
        }

        let idx = (self.total_requests % total_weight) as usize;
        let mut sum = 0usize;

        for path in active_paths {
            sum += path.weight as usize;
            if idx < sum {
                return Some(path.id);
            }
        }

        active_paths.last().map(|p| p.id)
    }

    fn select_failover(&self, active_paths: &[&PathInfo]) -> Option<PathId> {
        active_paths.iter().min_by_key(|p| p.priority).map(|p| p.id)
    }

    pub fn record_success(&mut self, id: PathId, latency_us: u64, bytes: u64) {
        if let Some(path) = self.paths.iter_mut().find(|p| p.id == id) {
            let alpha = self.config.latency_ewma_alpha;

            if path.metrics.latency_us == 0 {
                path.metrics.latency_us = latency_us;
            } else {
                path.metrics.latency_us = ((alpha * latency_us as f64)
                    + ((1.0 - alpha) * path.metrics.latency_us as f64))
                    as u64;
            }

            if latency_us < path.metrics.min_latency_us || path.metrics.min_latency_us == 0 {
                path.metrics.min_latency_us = latency_us;
            }

            let latency_diff = if path.metrics.latency_us > latency_us {
                path.metrics.latency_us - latency_us
            } else {
                latency_us - path.metrics.latency_us
            };
            path.metrics.jitter_us =
                ((0.5 * latency_diff as f64) + (0.5 * path.metrics.jitter_us as f64)) as u64;

            path.metrics.bytes_sent += bytes;
            self.consecutive_failures.remove(&id);

            if path.metrics.bytes_sent > 0 {
                let denominator = path.metrics.bytes_sent / 1024 + 1;
                path.metrics.loss_rate = path.metrics.errors as f64 / denominator as f64;
            }

            if path.state == PathState::Degraded || path.state == PathState::Failed {
                let counter = self.consecutive_successes.entry(id).or_insert(0);
                *counter += 1;

                if *counter >= self.config.recovery_threshold {
                    path.state = PathState::Active;
                    *counter = 0;
                    debug!(path = ?path.name, "path recovered to Active");
                }
            }
        }
    }

    pub fn record_failure(&mut self, id: PathId, bytes: u64) {
        if let Some(path) = self.paths.iter_mut().find(|p| p.id == id) {
            path.metrics.errors += 1;
            path.metrics.bytes_sent += bytes;

            let denominator = path.metrics.bytes_sent / 1024 + 1;
            path.metrics.loss_rate = path.metrics.errors as f64 / denominator as f64;

            let counter = self.consecutive_failures.entry(id).or_insert(0);
            *counter += 1;

            if path.state.is_usable() && *counter >= self.config.failure_threshold {
                path.state = PathState::Failed;
                self.failover_events += 1;
                *counter = 0;
                debug!(path = ?path.name, errors = path.metrics.errors, "path marked Failed");
            } else if path.state == PathState::Active
                && path.metrics.loss_rate > self.config.max_loss_rate
            {
                path.state = PathState::Degraded;
                debug!(
                    path = ?path.name,
                    loss_rate = path.metrics.loss_rate,
                    "path marked Degraded"
                );
            }
        }
    }

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

impl Default for MultipathRouter {
    fn default() -> Self {
        Self::new(MultipathConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MultipathConfig::default();
        assert_eq!(config.policy, PathSelectionPolicy::LowestLatency);
        assert_eq!(config.max_paths, 8);
        assert_eq!(config.probe_interval_ms, 1000);
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
        assert!((config.latency_ewma_alpha - 0.2).abs() < 0.001);
        assert!((config.max_loss_rate - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_add_remove_path() {
        let mut router = MultipathRouter::new(MultipathConfig::default());

        let id1 = router.add_path("eth0".to_string(), 100, 1);
        let id2 = router.add_path("eth1".to_string(), 50, 2);

        assert_ne!(id1, id2);
        assert_eq!(router.paths.len(), 2);

        assert!(router.remove_path(id1));
        assert_eq!(router.paths.len(), 1);

        assert!(!router.remove_path(PathId::new(999)));
    }

    #[test]
    fn test_round_robin_selection() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::RoundRobin,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("p1".to_string(), 100, 1);
        let id2 = router.add_path("p2".to_string(), 100, 2);
        let id3 = router.add_path("p3".to_string(), 100, 3);

        let mut selected = Vec::new();
        for _ in 0..6 {
            if let Some(id) = router.select_path() {
                selected.push(id);
            }
        }

        assert_eq!(selected.len(), 6);
        assert_eq!(selected[0], id1);
        assert_eq!(selected[1], id2);
        assert_eq!(selected[2], id3);
        assert_eq!(selected[3], id1);
        assert_eq!(selected[4], id2);
        assert_eq!(selected[5], id3);
    }

    #[test]
    fn test_lowest_latency_selection() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::LowestLatency,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("slow".to_string(), 100, 1);
        let id2 = router.add_path("fast".to_string(), 100, 2);
        let id3 = router.add_path("medium".to_string(), 100, 3);

        router.record_success(id1, 1000, 1024);
        router.record_success(id2, 100, 1024);
        router.record_success(id3, 500, 1024);

        let selected = router.select_path();
        assert_eq!(selected, Some(id2));
    }

    #[test]
    fn test_weighted_selection() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::WeightedRandom,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("high".to_string(), 100, 1);
        let _id2 = router.add_path("low".to_string(), 1, 2);

        let mut high_count = 0;
        let iterations = 1000;

        for _ in 0..iterations {
            if let Some(id) = router.select_path() {
                if id == id1 {
                    high_count += 1;
                }
            }
        }

        let ratio = high_count as f64 / iterations as f64;
        assert!(ratio > 0.9, "high weight path should be selected >90%");
    }

    #[test]
    fn test_failover_selection() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::Failover,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("primary".to_string(), 100, 1);
        let _id2 = router.add_path("backup".to_string(), 50, 2);

        let selected = router.select_path();
        assert_eq!(selected, Some(id1));

        router.mark_failed(id1);

        let selected = router.select_path();
        assert_ne!(selected, Some(id1));
    }

    #[test]
    fn test_record_success_updates_metrics() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.record_success(id, 500, 2048);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.latency_us, 500);
        assert_eq!(info.metrics.bytes_sent, 2048);
    }

    #[test]
    fn test_record_failure_increments_errors() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.record_failure(id, 1024);
        router.record_failure(id, 1024);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.errors, 2);
    }

    #[test]
    fn test_path_state_transitions() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.state, PathState::Active);

        for _ in 0..3 {
            router.record_failure(id, 1024);
        }

        let info = router.path_info(id).unwrap();
        assert_eq!(info.state, PathState::Failed);

        for _ in 0..2 {
            router.record_success(id, 100, 1024);
        }

        let info = router.path_info(id).unwrap();
        assert_eq!(info.state, PathState::Active);
    }

    #[test]
    fn test_mark_failed_and_active() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.mark_failed(id);
        assert_eq!(router.path_info(id).unwrap().state, PathState::Failed);

        router.mark_active(id);
        assert_eq!(router.path_info(id).unwrap().state, PathState::Active);
    }

    #[test]
    fn test_active_paths_filter() {
        let mut router = MultipathRouter::new(Default::default());

        let id1 = router.add_path("active".to_string(), 100, 1);
        let _id2 = router.add_path("failed".to_string(), 100, 2);

        router.mark_failed(id1);

        let active = router.active_paths();
        assert!(!active.contains(&id1));
    }

    #[test]
    fn test_stats_snapshot() {
        let mut router = MultipathRouter::new(Default::default());

        router.add_path("p1".to_string(), 100, 1);
        router.add_path("p2".to_string(), 100, 2);

        let stats = router.stats();

        assert_eq!(stats.total_paths, 2);
        assert_eq!(stats.active_paths, 2);
    }

    #[test]
    fn test_skip_failed_in_round_robin() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::RoundRobin,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("p1".to_string(), 100, 1);
        let id2 = router.add_path("p2".to_string(), 100, 2);

        router.mark_failed(id1);

        let selected = router.select_path();
        assert_eq!(selected, Some(id2));

        let selected = router.select_path();
        assert_eq!(selected, Some(id2));
    }

    #[test]
    fn test_no_active_paths_returns_none() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::RoundRobin,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id = router.add_path("p1".to_string(), 100, 1);
        router.mark_failed(id);

        let selected = router.select_path();
        assert_eq!(selected, None);
    }

    #[test]
    fn test_degraded_detection() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        for _ in 0..100 {
            router.record_failure(id, 1024);
        }

        let info = router.path_info(id).unwrap();
        assert!(info.metrics.loss_rate > 0.05);

        assert!(info.state == PathState::Degraded || info.state == PathState::Failed);
    }

    #[test]
    fn test_path_id_newtype() {
        let id = PathId::new(42);
        assert_eq!(id.as_u64(), 42);
        assert_eq!(u64::from(id), 42);
        assert_eq!(PathId::from(42u64), id);
    }

    #[test]
    fn test_latency_ewma_smoothing() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.record_success(id, 100, 1024);
        assert_eq!(router.path_info(id).unwrap().metrics.latency_us, 100);

        router.record_success(id, 200, 1024);
        let latency = router.path_info(id).unwrap().metrics.latency_us;
        assert!(latency > 100 && latency < 200);
    }

    #[test]
    fn test_jitter_calculation() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.record_success(id, 100, 1024);
        router.record_success(id, 200, 1024);

        let info = router.path_info(id).unwrap();
        assert!(info.metrics.jitter_us > 0);
    }

    #[test]
    fn test_min_latency_tracking() {
        let mut router = MultipathRouter::new(Default::default());

        let id = router.add_path("test".to_string(), 100, 1);

        router.record_success(id, 500, 1024);
        router.record_success(id, 100, 1024);
        router.record_success(id, 300, 1024);

        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.min_latency_us, 100);
    }

    #[test]
    fn test_lowest_latency_tie_breaker_priority() {
        let config = MultipathConfig {
            policy: PathSelectionPolicy::LowestLatency,
            ..Default::default()
        };
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("p1".to_string(), 100, 2);
        let id2 = router.add_path("p2".to_string(), 100, 1);

        router.record_success(id1, 100, 1024);
        router.record_success(id2, 100, 1024);

        let selected = router.select_path();
        assert_eq!(selected, Some(id2));
    }
}
