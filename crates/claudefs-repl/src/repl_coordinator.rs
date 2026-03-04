//! Multi-site coordination state machine.
//!
//! Coordinates the overall replication state across all connected sites.
//! Maintains a high-level view of which sites are in sync, which are lagging,
//! and which need catch-up or snapshot.

use std::collections::HashMap;

/// Decision made by the coordinator for a given site.
#[derive(Debug, Clone, PartialEq)]
pub enum CoordinatorDecision {
    /// Site is in sync; continue streaming.
    ContinueStreaming,
    /// Site is lagging; request expedited catch-up.
    TriggerCatchup {
        /// The site to catch up.
        site_id: u64,
        /// Sequence number to catch up from.
        from_seq: u64,
    },
    /// Site is too far behind; needs full snapshot.
    TriggerSnapshot {
        /// The site needing a snapshot.
        site_id: u64,
    },
    /// Site is unreachable; enter failover mode.
    EnterFailover {
        /// The site that is unreachable.
        site_id: u64,
    },
    /// Nothing to do right now.
    Idle,
}

/// Configuration for the coordinator.
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// Entries behind before triggering catchup.
    pub lag_threshold_catchup: u64,
    /// Entries behind before requiring snapshot.
    pub lag_threshold_snapshot: u64,
    /// ms with no heartbeat before "unreachable".
    pub unreachable_threshold_ms: u64,
    /// How often to run coordinator logic.
    pub check_interval_ms: u64,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            lag_threshold_catchup: 10_000,
            lag_threshold_snapshot: 1_000_000,
            unreachable_threshold_ms: 30_000,
            check_interval_ms: 5_000,
        }
    }
}

/// Per-site view maintained by the coordinator.
#[derive(Debug, Clone)]
pub struct SiteCoordinatorView {
    /// Site identifier.
    pub site_id: u64,
    /// Unix ms of last heartbeat.
    pub last_heartbeat_ms: u64,
    /// Our latest seq.
    pub local_seq: u64,
    /// Remote's latest seq (from heartbeat).
    pub remote_seq: u64,
    /// Computed lag.
    pub lag_entries: u64,
    /// Current coordination state.
    pub state: SiteCoordState,
}

/// State of a site from the coordinator's perspective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteCoordState {
    /// Site is in sync.
    Synced,
    /// Site is lagging but within catchup threshold.
    LagWarning,
    /// Site needs catchup.
    Catchup,
    /// Site needs full snapshot.
    SnapshotNeeded,
    /// Site is unreachable.
    Unreachable,
    /// Site is recovering.
    Recovering,
}

/// Statistics for coordinator decisions.
#[derive(Debug, Default, Clone)]
pub struct CoordinatorStats {
    /// Number of catchup triggers.
    pub catchup_triggers: u64,
    /// Number of snapshot triggers.
    pub snapshot_triggers: u64,
    /// Number of failover triggers.
    pub failover_triggers: u64,
    /// Number of check cycles run.
    pub check_cycles: u64,
}

/// The multi-site coordinator.
#[derive(Debug)]
pub struct ReplicationCoordinator {
    config: CoordinatorConfig,
    local_site_id: u64,
    sites: HashMap<u64, SiteCoordinatorView>,
    stats: CoordinatorStats,
}

impl ReplicationCoordinator {
    /// Create a new coordinator.
    pub fn new(local_site_id: u64, config: CoordinatorConfig) -> Self {
        Self {
            config,
            local_site_id,
            sites: HashMap::new(),
            stats: CoordinatorStats::default(),
        }
    }

    /// Register a new site.
    pub fn add_site(&mut self, site_id: u64) {
        self.sites.insert(
            site_id,
            SiteCoordinatorView {
                site_id,
                last_heartbeat_ms: 0,
                local_seq: 0,
                remote_seq: 0,
                lag_entries: 0,
                state: SiteCoordState::Recovering,
            },
        );
    }

    /// Unregister a site.
    pub fn remove_site(&mut self, site_id: u64) {
        self.sites.remove(&site_id);
    }

    /// Update site heartbeat and remote sequence.
    pub fn update_heartbeat(&mut self, site_id: u64, remote_seq: u64, now_ms: u64) {
        let lag_threshold_catchup = self.config.lag_threshold_catchup;
        let lag_threshold_snapshot = self.config.lag_threshold_snapshot;
        let unreachable_threshold_ms = self.config.unreachable_threshold_ms;

        if let Some(view) = self.sites.get_mut(&site_id) {
            view.last_heartbeat_ms = now_ms;
            view.remote_seq = remote_seq;
            view.lag_entries = view.local_seq.saturating_sub(view.remote_seq);
            view.state = compute_state_static(
                view.lag_entries,
                0,
                lag_threshold_catchup,
                lag_threshold_snapshot,
                unreachable_threshold_ms,
            );
        }
    }

    /// Update our local sequence.
    pub fn update_local_seq(&mut self, seq: u64) {
        let lag_threshold_catchup = self.config.lag_threshold_catchup;
        let lag_threshold_snapshot = self.config.lag_threshold_snapshot;
        let unreachable_threshold_ms = self.config.unreachable_threshold_ms;

        self.local_site_id = seq;
        for view in self.sites.values_mut() {
            view.local_seq = seq;
            view.lag_entries = view.local_seq.saturating_sub(view.remote_seq);
            view.state = compute_state_static(
                view.lag_entries,
                0,
                lag_threshold_catchup,
                lag_threshold_snapshot,
                unreachable_threshold_ms,
            );
        }
    }

    /// Run coordinator logic, return decisions for all sites.
    pub fn check_all(&mut self, now_ms: u64) -> Vec<CoordinatorDecision> {
        self.stats.check_cycles += 1;
        let mut decisions = Vec::new();

        let lag_threshold_catchup = self.config.lag_threshold_catchup;
        let lag_threshold_snapshot = self.config.lag_threshold_snapshot;
        let unreachable_threshold_ms = self.config.unreachable_threshold_ms;

        for (site_id, view) in self.sites.iter_mut() {
            let heartbeat_age = now_ms.saturating_sub(view.last_heartbeat_ms);
            view.state = compute_state_static(
                view.lag_entries,
                heartbeat_age,
                lag_threshold_catchup,
                lag_threshold_snapshot,
                unreachable_threshold_ms,
            );

            let decision = match view.state {
                SiteCoordState::Synced => CoordinatorDecision::ContinueStreaming,
                SiteCoordState::LagWarning => CoordinatorDecision::ContinueStreaming,
                SiteCoordState::Catchup => {
                    self.stats.catchup_triggers += 1;
                    CoordinatorDecision::TriggerCatchup {
                        site_id: *site_id,
                        from_seq: view.remote_seq,
                    }
                }
                SiteCoordState::SnapshotNeeded => {
                    self.stats.snapshot_triggers += 1;
                    CoordinatorDecision::TriggerSnapshot { site_id: *site_id }
                }
                SiteCoordState::Unreachable => {
                    self.stats.failover_triggers += 1;
                    CoordinatorDecision::EnterFailover { site_id: *site_id }
                }
                SiteCoordState::Recovering => CoordinatorDecision::Idle,
            };

            decisions.push(decision);
        }

        decisions
    }

    /// Get view of a specific site.
    pub fn site_view(&self, site_id: u64) -> Option<&SiteCoordinatorView> {
        self.sites.get(&site_id)
    }

    /// Get coordinator statistics.
    pub fn stats(&self) -> &CoordinatorStats {
        &self.stats
    }

    /// Get configuration.
    pub fn config(&self) -> &CoordinatorConfig {
        &self.config
    }

    /// Get number of tracked sites.
    pub fn site_count(&self) -> usize {
        self.sites.len()
    }
}

/// Compute state based on lag and heartbeat staleness (static version).
fn compute_state_static(
    lag: u64,
    heartbeat_age_ms: u64,
    lag_threshold_catchup: u64,
    lag_threshold_snapshot: u64,
    unreachable_threshold_ms: u64,
) -> SiteCoordState {
    if heartbeat_age_ms > unreachable_threshold_ms {
        return SiteCoordState::Unreachable;
    }
    if lag >= lag_threshold_snapshot {
        return SiteCoordState::SnapshotNeeded;
    }
    if lag >= lag_threshold_catchup {
        return SiteCoordState::Catchup;
    }
    if lag > 0 {
        return SiteCoordState::LagWarning;
    }
    SiteCoordState::Synced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> CoordinatorConfig {
        CoordinatorConfig {
            lag_threshold_catchup: 100,
            lag_threshold_snapshot: 1000,
            unreachable_threshold_ms: 5000,
            check_interval_ms: 1000,
        }
    }

    #[test]
    fn test_coordinator_new() {
        let coord = ReplicationCoordinator::new(1, make_test_config());
        assert_eq!(coord.local_site_id, 1);
        assert_eq!(coord.site_count(), 0);
        let default_stats = CoordinatorStats::default();
        assert_eq!(coord.stats(), &default_stats);
    }

    #[test]
    fn test_add_site() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.add_site(3);

        assert_eq!(coord.site_count(), 2);
        assert!(coord.site_view(2).is_some());
        assert!(coord.site_view(3).is_some());
    }

    #[test]
    fn test_remove_site() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.remove_site(2);

        assert_eq!(coord.site_count(), 0);
        assert!(coord.site_view(2).is_none());
    }

    #[test]
    fn test_update_heartbeat() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(500);
        coord.update_heartbeat(2, 450, 10000);

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.remote_seq, 450);
        assert_eq!(view.lag_entries, 50);
        assert_eq!(view.last_heartbeat_ms, 10000);
    }

    #[test]
    fn test_update_local_seq() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_heartbeat(2, 100, 10000);
        coord.update_local_seq(300);

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.local_seq, 300);
        assert_eq!(view.lag_entries, 200);
    }

    #[test]
    fn test_decision_catchup_when_lag_above_threshold() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(500);
        coord.update_heartbeat(2, 100, 10000); // lag = 400

        let decisions = coord.check_all(10000);
        assert_eq!(decisions.len(), 1);
        assert!(matches!(
            decisions[0],
            CoordinatorDecision::TriggerCatchup {
                site_id: 2,
                from_seq: 100
            }
        ));
        assert_eq!(coord.stats().catchup_triggers, 1);
    }

    #[test]
    fn test_decision_snapshot_when_lag_very_high() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(5000);
        coord.update_heartbeat(2, 100, 10000); // lag = 4900 > snapshot threshold

        let decisions = coord.check_all(10000);
        assert_eq!(decisions.len(), 1);
        assert!(matches!(
            decisions[0],
            CoordinatorDecision::TriggerSnapshot { site_id: 2 }
        ));
        assert_eq!(coord.stats().snapshot_triggers, 1);
    }

    #[test]
    fn test_decision_failover_when_heartbeat_stale() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(500);
        coord.update_heartbeat(2, 400, 10000); // lag = 100, OK

        // Now check with stale heartbeat (> 5 seconds old)
        let decisions = coord.check_all(20000);
        assert_eq!(decisions.len(), 1);
        assert!(matches!(
            decisions[0],
            CoordinatorDecision::EnterFailover { site_id: 2 }
        ));
        assert_eq!(coord.stats().failover_triggers, 1);
    }

    #[test]
    fn test_continue_streaming_when_synced() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(500);
        coord.update_heartbeat(2, 500, 10000); // lag = 0, in sync

        let decisions = coord.check_all(10000);
        assert_eq!(decisions.len(), 1);
        assert!(matches!(
            decisions[0],
            CoordinatorDecision::ContinueStreaming
        ));
    }

    #[test]
    fn test_lag_warning_when_lag_small() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(50);
        coord.update_heartbeat(2, 45, 10000); // lag = 5, below catchup threshold

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.state, SiteCoordState::LagWarning);
    }

    #[test]
    fn test_stats_increment() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.add_site(3);

        coord.update_local_seq(500);
        coord.update_heartbeat(2, 100, 10000);
        coord.update_heartbeat(3, 400, 10000);

        coord.check_all(10000);

        let stats = coord.stats();
        assert_eq!(stats.check_cycles, 1);
        assert_eq!(stats.catchup_triggers, 1);
    }

    #[test]
    fn test_multiple_sites_multiple_decisions() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2); // synced
        coord.add_site(3); // needs catchup
        coord.add_site(4); // needs snapshot

        coord.update_local_seq(5000);
        coord.update_heartbeat(2, 5000, 10000);
        coord.update_heartbeat(3, 100, 10000);
        coord.update_heartbeat(4, 10, 10000);

        let decisions = coord.check_all(10000);
        assert_eq!(decisions.len(), 3);

        let mut found_catchup = false;
        let mut found_snapshot = false;
        let mut found_continue = false;

        for d in &decisions {
            match d {
                CoordinatorDecision::ContinueStreaming => found_continue = true,
                CoordinatorDecision::TriggerCatchup { .. } => found_catchup = true,
                CoordinatorDecision::TriggerSnapshot { .. } => found_snapshot = true,
                _ => {}
            }
        }

        assert!(found_continue);
        assert!(found_catchup);
        assert!(found_snapshot);
    }

    #[test]
    fn test_site_coord_state_transitions() {
        let config = make_test_config();

        // LagWarning
        let state = compute_state_static(
            50,
            0,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::LagWarning);

        // Catchup
        let state = compute_state_static(
            150,
            0,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::Catchup);

        // SnapshotNeeded
        let state = compute_state_static(
            1500,
            0,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::SnapshotNeeded);

        // Unreachable (stale heartbeat)
        let state = compute_state_static(
            0,
            10000,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::Unreachable);

        // Synced
        let state = compute_state_static(
            0,
            0,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::Synced);
    }

    #[test]
    fn test_default_config_values() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.lag_threshold_catchup, 10_000);
        assert_eq!(config.lag_threshold_snapshot, 1_000_000);
        assert_eq!(config.unreachable_threshold_ms, 30_000);
        assert_eq!(config.check_interval_ms, 5_000);
    }

    #[test]
    fn test_coordinator_config_accessor() {
        let config = make_test_config();
        let coord = ReplicationCoordinator::new(1, config.clone());
        assert_eq!(
            coord.config().lag_threshold_catchup,
            config.lag_threshold_catchup
        );
    }

    #[test]
    fn test_view_fields() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);
        coord.update_local_seq(1000);
        coord.update_heartbeat(2, 800, 5000);

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.site_id, 2);
        assert_eq!(view.local_seq, 1000);
        assert_eq!(view.remote_seq, 800);
        assert_eq!(view.lag_entries, 200);
        assert_eq!(view.last_heartbeat_ms, 5000);
    }

    #[test]
    fn test_empty_check_all() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        let decisions = coord.check_all(10000);
        assert!(decisions.is_empty());
        assert_eq!(coord.stats().check_cycles, 1);
    }

    #[test]
    fn test_check_all_updates_state() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);

        // Initially recovering
        let view = coord.site_view(2).unwrap();
        assert_eq!(view.state, SiteCoordState::Recovering);

        // After heartbeat, should be synced
        coord.update_local_seq(100);
        coord.update_heartbeat(2, 100, 10000);
        coord.check_all(10000);

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.state, SiteCoordState::Synced);
    }

    #[test]
    fn test_update_heartbeat_nonexistent_site() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        // Should not panic
        coord.update_heartbeat(999, 100, 10000);
        assert!(coord.site_view(999).is_none());
    }

    #[test]
    fn test_unreachable_overrides_lag() {
        let config = CoordinatorConfig {
            lag_threshold_catchup: 10,
            lag_threshold_snapshot: 100,
            unreachable_threshold_ms: 50,
            check_interval_ms: 100,
        };

        // Even with low lag, if heartbeat is stale, should be unreachable
        let state = compute_state_static(
            5,
            100,
            config.lag_threshold_catchup,
            config.lag_threshold_snapshot,
            config.unreachable_threshold_ms,
        );
        assert_eq!(state, SiteCoordState::Unreachable);
    }

    #[test]
    fn test_saturating_lag_calculation() {
        let mut coord = ReplicationCoordinator::new(1, make_test_config());
        coord.add_site(2);

        // remote_seq > local_seq should not underflow
        coord.update_local_seq(100);
        coord.update_heartbeat(2, 200, 10000);

        let view = coord.site_view(2).unwrap();
        assert_eq!(view.lag_entries, 0); // saturating
    }
}
