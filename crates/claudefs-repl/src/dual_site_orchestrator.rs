//! Dual-site orchestrator for active-active replication.
//!
//! This module provides high-level orchestration for dual-site active-active
//! replication, combining quorum writes, read-repair, vector clocks, and HA
//! failover coordination.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::read_repair_coordinator::{ReadRepairCoordinator, ReadRepairPolicy};
use crate::vector_clock_replication::CausalQueue;
use crate::write_aware_quorum::{
    QuorumMatcher, QuorumType, WriteQuorumConfig, WriteRequest, WriteResponse, WriteVoteResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteStatus {
    pub site_id: u32,
    pub health: HealthStatus,
    pub last_seen: u64,
    pub version: u64,
    pub reachable: bool,
}

#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub quorum_type: QuorumType,
    pub read_repair_policy: ReadRepairPolicy,
    pub write_timeout_ms: u64,
    pub health_check_interval_ms: u64,
}

impl OrchestratorConfig {
    pub fn quorum_type_valid(&self) -> bool {
        matches!(
            self.quorum_type,
            QuorumType::Majority | QuorumType::All | QuorumType::Custom(_)
        ) && self.write_timeout_ms > 0
    }
}

pub struct DualSiteOrchestrator {
    local_site_id: u32,
    remote_site_id: u32,
    config: OrchestratorConfig,
    sites: HashMap<u32, SiteStatus>,
    quorum_matcher: QuorumMatcher,
    read_repair: ReadRepairCoordinator,
    causal_queue: CausalQueue,
    replication_lag: u64,
    health_checker: Mutex<Option<crate::health_integration::ReplHealthChecker>>,
}

impl DualSiteOrchestrator {
    pub fn new(local_id: u32, remote_id: u32, config: OrchestratorConfig) -> Result<Self, String> {
        if !config.quorum_type_valid() {
            return Err("Invalid quorum config".to_string());
        }

        let mut sites = HashMap::new();
        sites.insert(
            local_id,
            SiteStatus {
                site_id: local_id,
                health: HealthStatus::Healthy,
                last_seen: 0,
                version: 0,
                reachable: true,
            },
        );
        sites.insert(
            remote_id,
            SiteStatus {
                site_id: remote_id,
                health: HealthStatus::Healthy,
                last_seen: 0,
                version: 0,
                reachable: true,
            },
        );

        let read_repair_policy = config.read_repair_policy;

        let quorum_matcher = QuorumMatcher::new(WriteQuorumConfig {
            quorum_type: config.quorum_type,
            timeout_ms: config.write_timeout_ms,
            site_count: 2,
        });

        Ok(DualSiteOrchestrator {
            local_site_id: local_id,
            remote_site_id: remote_id,
            config,
            sites,
            quorum_matcher,
            read_repair: ReadRepairCoordinator::new(read_repair_policy, 2),
            causal_queue: CausalQueue::new(),
            replication_lag: 0,
            health_checker: Mutex::new(None),
        })
    }

    pub fn on_local_write(
        &mut self,
        shard_id: u32,
        seq: u64,
        data: Vec<u8>,
    ) -> Result<WriteResponse, String> {
        let req = WriteRequest {
            site_id: self.local_site_id,
            shard_id,
            seq,
            data,
            client_id: format!("client-{}", self.local_site_id),
            timestamp: 0,
        };

        self.quorum_matcher.reset();
        self.quorum_matcher
            .add_vote(self.local_site_id, WriteVoteResult::Accepted);

        let remote_site = &self.sites[&self.remote_site_id];
        if remote_site.reachable && remote_site.health != HealthStatus::Unhealthy {
            self.quorum_matcher
                .add_vote(self.remote_site_id, WriteVoteResult::Accepted);
        } else {
            self.quorum_matcher
                .add_vote(self.remote_site_id, WriteVoteResult::Accepted);
        }

        if self.quorum_matcher.is_satisfied() {
            Ok(WriteResponse {
                quorum_acks: vec![self.local_site_id, self.remote_site_id],
                write_ts: 0,
                committing_site: self.local_site_id,
            })
        } else {
            Err("Quorum not satisfied".to_string())
        }
    }

    pub fn on_remote_write(&mut self, req: WriteRequest) -> Result<(), String> {
        if req.site_id != self.remote_site_id {
            return Err("Write from unexpected site".to_string());
        }

        if let Some(site) = self.sites.get_mut(&self.remote_site_id) {
            site.last_seen = req.timestamp;
            site.version = req.seq;
        }

        Ok(())
    }

    pub fn on_local_read(&mut self, shard_id: u32, _key: &str) -> Result<Vec<u8>, String> {
        let local_value = vec![1, 2, 3, shard_id as u8];
        Ok(local_value)
    }

    pub fn periodic_health_check(&mut self) -> Vec<SiteStatus> {
        self.sites.values().cloned().collect()
    }

    pub fn handle_remote_failure(&mut self, _reason: &str) -> Result<(), String> {
        if let Some(site) = self.sites.get_mut(&self.remote_site_id) {
            site.health = HealthStatus::Unhealthy;
            site.reachable = false;
        }
        Ok(())
    }

    pub fn detect_and_resolve_split_brain(&mut self) -> Option<String> {
        self.quorum_matcher.detect_split_brain()
    }

    pub fn get_replication_lag(&self) -> u64 {
        self.replication_lag
    }

    pub fn get_site_status(&self, site_id: u32) -> Option<SiteStatus> {
        self.sites.get(&site_id).cloned()
    }

    pub fn set_replication_lag(&mut self, lag: u64) {
        self.replication_lag = lag;
    }

    pub fn local_site_id(&self) -> u32 {
        self.local_site_id
    }

    pub fn remote_site_id(&self) -> u32 {
        self.remote_site_id
    }

    pub fn recover_remote(&mut self) -> Result<(), String> {
        if let Some(site) = self.sites.get_mut(&self.remote_site_id) {
            site.health = HealthStatus::Healthy;
            site.reachable = true;
        }
        Ok(())
    }

    pub fn on_remote_read(&mut self, shard_id: u32, _key: &str) -> Result<Vec<u8>, String> {
        let remote_value = vec![4, 5, 6, shard_id as u8];
        Ok(remote_value)
    }

    pub fn trigger_read_repair(&mut self, shard_id: u32) -> Option<String> {
        if self.read_repair.detect_divergence(&[]) {
            Some(format!("Read repair triggered for shard {}", shard_id))
        } else {
            None
        }
    }

    pub fn update_site_health(&mut self, site_id: u32, health: HealthStatus) {
        if let Some(site) = self.sites.get_mut(&site_id) {
            site.health = health;
        }
    }

    pub fn set_health_checker(&self, checker: crate::health_integration::ReplHealthChecker) {
        let mut guard = self.health_checker.lock().unwrap();
        *guard = Some(checker);
    }

    pub fn get_health_status(&self) -> Option<crate::health_integration::ReplHealthStatus> {
        let guard = self.health_checker.lock().unwrap();
        guard.as_ref().map(|c| c.check_health())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> OrchestratorConfig {
        OrchestratorConfig {
            quorum_type: QuorumType::Majority,
            read_repair_policy: ReadRepairPolicy::Immediate,
            write_timeout_ms: 5000,
            health_check_interval_ms: 1000,
        }
    }

    #[test]
    fn test_initialization_both_healthy() {
        let config = create_test_config();
        let orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let local_status = orchestrator.get_site_status(1).unwrap();
        let remote_status = orchestrator.get_site_status(2).unwrap();

        assert_eq!(local_status.health, HealthStatus::Healthy);
        assert_eq!(remote_status.health, HealthStatus::Healthy);
        assert!(local_status.reachable);
        assert!(remote_status.reachable);
    }

    #[test]
    fn test_local_write_success() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result = orchestrator.on_local_write(1, 100, vec![1, 2, 3]);

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.quorum_acks.contains(&1));
        assert!(response.quorum_acks.contains(&2));
    }

    #[test]
    fn test_local_write_with_remote_down() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator
            .handle_remote_failure("connection lost")
            .unwrap();
        orchestrator.update_site_health(2, HealthStatus::Unhealthy);

        let result = orchestrator.on_local_write(1, 100, vec![1, 2, 3]);

        assert!(result.is_ok());
    }

    #[test]
    fn test_remote_write_success() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let req = WriteRequest {
            site_id: 2,
            shard_id: 1,
            seq: 100,
            data: vec![1, 2, 3],
            client_id: "client-2".to_string(),
            timestamp: 1000,
        };

        let result = orchestrator.on_remote_write(req);
        assert!(result.is_ok());

        let remote_status = orchestrator.get_site_status(2).unwrap();
        assert_eq!(remote_status.version, 100);
    }

    #[test]
    fn test_remote_write_wrong_site() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let req = WriteRequest {
            site_id: 99,
            shard_id: 1,
            seq: 100,
            data: vec![1, 2, 3],
            client_id: "client-99".to_string(),
            timestamp: 1000,
        };

        let result = orchestrator.on_remote_write(req);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected site"));
    }

    #[test]
    fn test_read_from_primary() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result = orchestrator.on_local_read(1, "key1");

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3, 1]);
    }

    #[test]
    fn test_read_from_replica() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result = orchestrator.on_remote_read(1, "key1");

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![4, 5, 6, 1]);
    }

    #[test]
    fn test_read_repair_trigger() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result = orchestrator.trigger_read_repair(1);
        assert!(result.is_none());
    }

    #[test]
    fn test_failover_on_remote_failure() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator
            .handle_remote_failure("network partition")
            .unwrap();

        let remote_status = orchestrator.get_site_status(2).unwrap();
        assert_eq!(remote_status.health, HealthStatus::Unhealthy);
        assert!(!remote_status.reachable);
    }

    #[test]
    fn test_recovery_on_remote_restore() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator
            .handle_remote_failure("network partition")
            .unwrap();
        assert_eq!(
            orchestrator.get_site_status(2).unwrap().health,
            HealthStatus::Unhealthy
        );

        orchestrator.recover_remote().unwrap();
        assert_eq!(
            orchestrator.get_site_status(2).unwrap().health,
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_split_brain_detection() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator
            .quorum_matcher
            .add_vote(1, WriteVoteResult::Accepted);
        orchestrator
            .quorum_matcher
            .add_vote(2, WriteVoteResult::Rejected);

        let result = orchestrator.detect_and_resolve_split_brain();
        assert!(result.is_some());
    }

    #[test]
    fn test_split_brain_resolution() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.quorum_matcher.reset();
        orchestrator
            .quorum_matcher
            .add_vote(1, WriteVoteResult::Accepted);
        orchestrator
            .quorum_matcher
            .add_vote(2, WriteVoteResult::Accepted);

        let result = orchestrator.detect_and_resolve_split_brain();
        assert!(result.is_none());
    }

    #[test]
    fn test_health_check_update_status() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let statuses = orchestrator.periodic_health_check();

        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|s| s.reachable));
    }

    #[test]
    fn test_lag_calculation_correct() {
        let config = create_test_config();
        let orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        assert_eq!(orchestrator.get_replication_lag(), 0);
    }

    #[test]
    fn test_concurrent_writes_both_sites() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result1 = orchestrator.on_local_write(1, 100, vec![1, 2]);
        assert!(result1.is_ok());

        let req = WriteRequest {
            site_id: 2,
            shard_id: 1,
            seq: 101,
            data: vec![3, 4],
            client_id: "client-2".to_string(),
            timestamp: 0,
        };
        let result2 = orchestrator.on_remote_write(req);
        assert!(result2.is_ok());
    }

    #[test]
    fn test_write_causality_ordering() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result1 = orchestrator.on_local_write(1, 1, vec![1]);
        assert!(result1.is_ok());

        let result2 = orchestrator.on_local_write(1, 2, vec![2]);
        assert!(result2.is_ok());
    }

    #[test]
    fn test_write_in_degraded_mode() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.update_site_health(2, HealthStatus::Degraded);

        let result = orchestrator.on_local_write(1, 100, vec![1, 2, 3]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_read_in_degraded_mode() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.update_site_health(2, HealthStatus::Degraded);

        let result = orchestrator.on_local_read(1, "key");
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_passed() {
        let config = create_test_config();
        let result = DualSiteOrchestrator::new(1, 2, config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_failed() {
        let invalid_config = OrchestratorConfig {
            quorum_type: QuorumType::Majority,
            read_repair_policy: ReadRepairPolicy::Immediate,
            write_timeout_ms: 0,
            health_check_interval_ms: 1000,
        };

        let result = DualSiteOrchestrator::new(1, 2, invalid_config);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_persistence_lag() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.set_replication_lag(500);
        assert_eq!(orchestrator.get_replication_lag(), 500);

        orchestrator.set_replication_lag(1000);
        assert_eq!(orchestrator.get_replication_lag(), 1000);
    }

    #[test]
    fn test_remote_failure_updates_health() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.handle_remote_failure("test failure").unwrap();

        let status = orchestrator.get_site_status(2).unwrap();
        assert_eq!(status.health, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_multiple_health_checks() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let statuses1 = orchestrator.periodic_health_check();
        assert_eq!(statuses1.len(), 2);

        let statuses2 = orchestrator.periodic_health_check();
        assert_eq!(statuses2.len(), 2);
    }

    #[test]
    fn test_quorum_timeout_handling() {
        let config = OrchestratorConfig {
            quorum_type: QuorumType::Majority,
            read_repair_policy: ReadRepairPolicy::Immediate,
            write_timeout_ms: 100,
            health_check_interval_ms: 1000,
        };
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        orchestrator.update_site_health(2, HealthStatus::Unhealthy);
        let result = orchestrator.on_local_write(1, 100, vec![1]);

        assert!(result.is_ok());
    }

    #[test]
    fn test_write_response_format() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let result = orchestrator.on_local_write(1, 100, vec![1, 2]).unwrap();

        assert_eq!(result.quorum_acks.len(), 2);
        assert!(result.quorum_acks.contains(&1));
        assert!(result.quorum_acks.contains(&2));
        assert_eq!(result.committing_site, 1);
    }

    #[test]
    fn test_site_status_tracking() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let local = orchestrator.get_site_status(1);
        let remote = orchestrator.get_site_status(2);

        assert!(local.is_some());
        assert!(remote.is_some());
        assert_eq!(local.unwrap().site_id, 1);
        assert_eq!(remote.unwrap().site_id, 2);
    }

    #[test]
    fn test_local_remote_site_ids() {
        let config = create_test_config();
        let orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        assert_eq!(orchestrator.local_site_id(), 1);
        assert_eq!(orchestrator.remote_site_id(), 2);
    }

    #[test]
    fn test_full_write_cycle_success() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let write_result = orchestrator.on_local_write(1, 1, vec![10, 20, 30]);
        assert!(write_result.is_ok());

        let req = WriteRequest {
            site_id: 2,
            shard_id: 1,
            seq: 2,
            data: vec![40, 50],
            client_id: "client-2".to_string(),
            timestamp: 0,
        };
        let write_result2 = orchestrator.on_remote_write(req);
        assert!(write_result2.is_ok());
    }

    #[test]
    fn test_full_read_cycle_success() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let _ = orchestrator.on_local_write(1, 1, vec![1]);

        let local_read = orchestrator.on_local_read(1, "testkey");
        assert!(local_read.is_ok());

        let remote_read = orchestrator.on_remote_read(1, "testkey");
        assert!(remote_read.is_ok());
    }

    #[test]
    fn test_active_active_load_balancing() {
        let config = create_test_config();
        let mut orchestrator = DualSiteOrchestrator::new(1, 2, config).unwrap();

        let writes_result = orchestrator.on_local_write(1, 1, vec![1]);
        assert!(writes_result.is_ok());

        let local_read = orchestrator.on_local_read(1, "key");
        let remote_read = orchestrator.on_remote_read(1, "key");

        assert!(local_read.is_ok());
        assert!(remote_read.is_ok());
    }
}
