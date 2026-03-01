use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum MaintenanceMode {
    Active,
    Inactive,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpgradePhase {
    Idle,
    Preparing,
    Draining(String),
    Upgrading(String),
    Verifying(String),
    Complete,
    RolledBack,
}

#[derive(Debug, Clone)]
pub struct MaintenanceWindow {
    pub start_ms: u64,
    pub end_ms: u64,
    pub reason: String,
}

impl MaintenanceWindow {
    pub fn is_active(&self, now_ms: u64) -> bool {
        now_ms >= self.start_ms && now_ms < self.end_ms
    }

    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

#[derive(Debug, Error)]
pub enum MaintenanceError {
    #[error("Invalid transition: {0}")]
    InvalidTransition(String),
    #[error("Not in maintenance: {0}")]
    NotInMaintenance(String),
    #[error("Window expired: {0}")]
    WindowExpired(String),
}

#[derive(Debug, Clone)]
pub struct UpgradeCoordinator {
    target_version: String,
    current_phase: Arc<Mutex<UpgradePhase>>,
    upgraded_nodes: Arc<Mutex<Vec<String>>>,
}

impl UpgradeCoordinator {
    pub fn new(target_version: &str) -> Self {
        Self {
            target_version: target_version.to_string(),
            current_phase: Arc::new(Mutex::new(UpgradePhase::Idle)),
            upgraded_nodes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start_preparing(&self) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Idle => {
                *phase = UpgradePhase::Preparing;
                Ok(())
            }
            _ => Err(MaintenanceError::InvalidTransition(format!(
                "Cannot start preparing from {:?}",
                *phase
            ))),
        }
    }

    pub fn drain_node(&self, node_id: &str) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Preparing => {
                *phase = UpgradePhase::Draining(node_id.to_string());
                Ok(())
            }
            _ => Err(MaintenanceError::InvalidTransition(format!(
                "Cannot drain node from {:?}",
                *phase
            ))),
        }
    }

    pub fn upgrade_node(&self, node_id: &str) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Draining(_) => {
                *phase = UpgradePhase::Upgrading(node_id.to_string());
                drop(phase);
                let mut nodes = self.upgraded_nodes.lock().unwrap();
                nodes.push(node_id.to_string());
                Ok(())
            }
            _ => Err(MaintenanceError::InvalidTransition(format!(
                "Cannot upgrade node from {:?}",
                *phase
            ))),
        }
    }

    pub fn verify_node(&self, node_id: &str) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Upgrading(_) => {
                *phase = UpgradePhase::Verifying(node_id.to_string());
                Ok(())
            }
            _ => Err(MaintenanceError::InvalidTransition(format!(
                "Cannot verify node from {:?}",
                *phase
            ))),
        }
    }

    pub fn complete_node(&self) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Verifying(_) => {
                *phase = UpgradePhase::Complete;
                Ok(())
            }
            _ => Err(MaintenanceError::InvalidTransition(format!(
                "Cannot complete from {:?}",
                *phase
            ))),
        }
    }

    pub fn rollback(&self) -> Result<(), MaintenanceError> {
        let mut phase = self.current_phase.lock().unwrap();
        match *phase {
            UpgradePhase::Idle | UpgradePhase::Complete => Err(
                MaintenanceError::InvalidTransition(format!("Cannot rollback from {:?}", *phase)),
            ),
            _ => {
                *phase = UpgradePhase::RolledBack;
                Ok(())
            }
        }
    }

    pub fn phase(&self) -> UpgradePhase {
        self.current_phase.lock().unwrap().clone()
    }

    pub fn upgraded_count(&self) -> usize {
        self.upgraded_nodes.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_window_is_active_true_when_now_between_start_end() {
        let window = MaintenanceWindow {
            start_ms: 1000,
            end_ms: 2000,
            reason: "test".to_string(),
        };
        assert!(window.is_active(1500));
    }

    #[test]
    fn maintenance_window_is_active_false_before_start() {
        let window = MaintenanceWindow {
            start_ms: 1000,
            end_ms: 2000,
            reason: "test".to_string(),
        };
        assert!(!window.is_active(500));
    }

    #[test]
    fn maintenance_window_is_active_false_after_end() {
        let window = MaintenanceWindow {
            start_ms: 1000,
            end_ms: 2000,
            reason: "test".to_string(),
        };
        assert!(!window.is_active(2500));
    }

    #[test]
    fn maintenance_window_duration_ms_correct() {
        let window = MaintenanceWindow {
            start_ms: 1000,
            end_ms: 2000,
            reason: "test".to_string(),
        };
        assert_eq!(window.duration_ms(), 1000);
    }

    #[test]
    fn new_coordinator_starts_in_idle_phase() {
        let coord = UpgradeCoordinator::new("1.0.0");
        assert_eq!(coord.phase(), UpgradePhase::Idle);
    }

    #[test]
    fn start_preparing_transitions_to_preparing() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Preparing);
    }

    #[test]
    fn start_preparing_fails_if_already_preparing() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        let result = coord.start_preparing();
        assert!(result.is_err());
    }

    #[test]
    fn drain_node_transitions_to_draining() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Draining("node1".to_string()));
    }

    #[test]
    fn drain_node_fails_if_not_preparing() {
        let coord = UpgradeCoordinator::new("1.0.0");
        let result = coord.drain_node("node1");
        assert!(result.is_err());
    }

    #[test]
    fn upgrade_node_transitions_to_upgrading() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Upgrading("node1".to_string()));
    }

    #[test]
    fn upgrade_node_increments_upgraded_count() {
        let coord = UpgradeCoordinator::new("1.0.0");
        assert_eq!(coord.upgraded_count(), 0);
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        assert_eq!(coord.upgraded_count(), 1);
    }

    #[test]
    fn upgrade_node_fails_if_not_draining() {
        let coord = UpgradeCoordinator::new("1.0.0");
        let result = coord.upgrade_node("node1");
        assert!(result.is_err());
    }

    #[test]
    fn verify_node_transitions_to_verifying() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        coord.verify_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Verifying("node1".to_string()));
    }

    #[test]
    fn complete_node_transitions_to_complete() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        coord.verify_node("node1").unwrap();
        coord.complete_node().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Complete);
    }

    #[test]
    fn rollback_from_preparing_rolled_back() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.rollback().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::RolledBack);
    }

    #[test]
    fn rollback_from_draining_rolled_back() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.rollback().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::RolledBack);
    }

    #[test]
    fn rollback_from_upgrading_rolled_back() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        coord.rollback().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::RolledBack);
    }

    #[test]
    fn rollback_from_complete_returns_error() {
        let coord = UpgradeCoordinator::new("1.0.0");
        coord.start_preparing().unwrap();
        coord.drain_node("node1").unwrap();
        coord.upgrade_node("node1").unwrap();
        coord.verify_node("node1").unwrap();
        coord.complete_node().unwrap();
        let result = coord.rollback();
        assert!(result.is_err());
    }

    #[test]
    fn full_happy_path() {
        let coord = UpgradeCoordinator::new("1.0.0");
        assert_eq!(coord.phase(), UpgradePhase::Idle);

        coord.start_preparing().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Preparing);

        coord.drain_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Draining("node1".to_string()));

        coord.upgrade_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Upgrading("node1".to_string()));

        coord.verify_node("node1").unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Verifying("node1".to_string()));

        coord.complete_node().unwrap();
        assert_eq!(coord.phase(), UpgradePhase::Complete);
    }

    #[test]
    fn upgraded_count_is_0_initially() {
        let coord = UpgradeCoordinator::new("1.0.0");
        assert_eq!(coord.upgraded_count(), 0);
    }
}
