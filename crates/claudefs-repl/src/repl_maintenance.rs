use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum MaintenanceState {
    Active,
    Paused,
    CatchingUp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MaintenanceWindow {
    start_hour: u8,
    end_hour: u8,
    days_of_week: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MaintenanceStats {
    windows_entered: u64,
    windows_exited: u64,
    pause_duration_ms: u64,
    catchup_duration_ms: u64,
}

struct MaintenanceCoordinator {
    state: MaintenanceState,
    window: Option<MaintenanceWindow>,
    stats: MaintenanceStats,
}

impl MaintenanceCoordinator {
    pub fn new() -> Self {
        Self {
            state: MaintenanceState::Active,
            window: None,
            stats: MaintenanceStats::default(),
        }
    }

    pub fn set_window(&mut self, w: MaintenanceWindow) {
        self.window = Some(w);
    }

    pub fn enter_maintenance(&mut self) -> bool {
        if self.state == MaintenanceState::Active {
            self.state = MaintenanceState::Paused;
            self.stats.windows_entered += 1;
            info!(
                "Entered maintenance mode, windows_entered: {}",
                self.stats.windows_entered
            );
            return true;
        }
        false
    }

    pub fn exit_maintenance(&mut self) -> bool {
        if self.state == MaintenanceState::Paused {
            self.state = MaintenanceState::CatchingUp;
            self.stats.windows_exited += 1;
            info!(
                "Exited maintenance mode, windows_exited: {}",
                self.stats.windows_exited
            );
            return true;
        }
        false
    }

    pub fn complete_catchup(&mut self) {
        if self.state == MaintenanceState::CatchingUp {
            self.state = MaintenanceState::Active;
            info!("Catchup complete, returned to active state");
        }
    }

    pub fn state(&self) -> &MaintenanceState {
        &self.state
    }

    pub fn stats(&self) -> &MaintenanceStats {
        &self.stats
    }

    pub fn is_in_maintenance(&self) -> bool {
        matches!(
            self.state,
            MaintenanceState::Paused | MaintenanceState::CatchingUp
        )
    }
}

impl Default for MaintenanceCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_coordinator_is_active() {
        let coord = MaintenanceCoordinator::new();
        assert_eq!(coord.state(), &MaintenanceState::Active);
    }

    #[test]
    fn test_new_coordinator_has_no_window() {
        let coord = MaintenanceCoordinator::new();
        assert!(coord.window.is_none());
    }

    #[test]
    fn test_new_coordinator_stats_default() {
        let coord = MaintenanceCoordinator::new();
        let stats = coord.stats();
        assert_eq!(stats.windows_entered, 0);
        assert_eq!(stats.windows_exited, 0);
    }

    #[test]
    fn test_set_window() {
        let mut coord = MaintenanceCoordinator::new();
        let window = MaintenanceWindow {
            start_hour: 2,
            end_hour: 5,
            days_of_week: vec![1, 2, 3, 4, 5],
        };
        coord.set_window(window.clone());
        assert_eq!(coord.window, Some(window));
    }

    #[test]
    fn test_enter_maintenance_from_active() {
        let mut coord = MaintenanceCoordinator::new();
        let result = coord.enter_maintenance();
        assert!(result);
        assert_eq!(coord.state(), &MaintenanceState::Paused);
    }

    #[test]
    fn test_enter_maintenance_increments_counter() {
        let mut coord = MaintenanceCoordinator::new();
        coord.enter_maintenance();
        assert_eq!(coord.stats().windows_entered, 1);
        coord.state = MaintenanceState::Active;
        coord.enter_maintenance();
        assert_eq!(coord.stats().windows_entered, 2);
    }

    #[test]
    fn test_enter_maintenance_from_paused() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::Paused;
        let result = coord.enter_maintenance();
        assert!(!result);
        assert_eq!(coord.state(), &MaintenanceState::Paused);
    }

    #[test]
    fn test_enter_maintenance_from_catching_up() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::CatchingUp;
        let result = coord.enter_maintenance();
        assert!(!result);
    }

    #[test]
    fn test_exit_maintenance_from_paused() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::Paused;
        let result = coord.exit_maintenance();
        assert!(result);
        assert_eq!(coord.state(), &MaintenanceState::CatchingUp);
    }

    #[test]
    fn test_exit_maintenance_increments_exit_counter() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::Paused;
        coord.exit_maintenance();
        assert_eq!(coord.stats().windows_exited, 1);
    }

    #[test]
    fn test_exit_maintenance_from_active() {
        let mut coord = MaintenanceCoordinator::new();
        let result = coord.exit_maintenance();
        assert!(!result);
        assert_eq!(coord.state(), &MaintenanceState::Active);
    }

    #[test]
    fn test_exit_maintenance_from_catching_up() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::CatchingUp;
        let result = coord.exit_maintenance();
        assert!(!result);
    }

    #[test]
    fn test_complete_catchup_from_catching_up() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::CatchingUp;
        coord.complete_catchup();
        assert_eq!(coord.state(), &MaintenanceState::Active);
    }

    #[test]
    fn test_complete_catchup_from_active() {
        let mut coord = MaintenanceCoordinator::new();
        coord.complete_catchup();
        assert_eq!(coord.state(), &MaintenanceState::Active);
    }

    #[test]
    fn test_complete_catchup_from_paused() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::Paused;
        coord.complete_catchup();
        assert_eq!(coord.state(), &MaintenanceState::Paused);
    }

    #[test]
    fn test_is_in_maintenance_when_active() {
        let coord = MaintenanceCoordinator::new();
        assert!(!coord.is_in_maintenance());
    }

    #[test]
    fn test_is_in_maintenance_when_paused() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::Paused;
        assert!(coord.is_in_maintenance());
    }

    #[test]
    fn test_is_in_maintenance_when_catching_up() {
        let mut coord = MaintenanceCoordinator::new();
        coord.state = MaintenanceState::CatchingUp;
        assert!(coord.is_in_maintenance());
    }

    #[test]
    fn test_maintenance_window_fields() {
        let window = MaintenanceWindow {
            start_hour: 22,
            end_hour: 6,
            days_of_week: vec![0, 6],
        };
        assert_eq!(window.start_hour, 22);
        assert_eq!(window.end_hour, 6);
        assert_eq!(window.days_of_week, vec![0, 6]);
    }

    #[test]
    fn test_maintenance_stats_default() {
        let stats = MaintenanceStats::default();
        assert_eq!(stats.windows_entered, 0);
        assert_eq!(stats.windows_exited, 0);
        assert_eq!(stats.pause_duration_ms, 0);
        assert_eq!(stats.catchup_duration_ms, 0);
    }

    #[test]
    fn test_full_maintenance_cycle() {
        let mut coord = MaintenanceCoordinator::new();

        assert_eq!(coord.state(), &MaintenanceState::Active);
        assert!(!coord.is_in_maintenance());

        assert!(coord.enter_maintenance());
        assert_eq!(coord.state(), &MaintenanceState::Paused);
        assert!(coord.is_in_maintenance());

        assert!(coord.exit_maintenance());
        assert_eq!(coord.state(), &MaintenanceState::CatchingUp);
        assert!(coord.is_in_maintenance());

        coord.complete_catchup();
        assert_eq!(coord.state(), &MaintenanceState::Active);
        assert!(!coord.is_in_maintenance());

        assert_eq!(coord.stats().windows_entered, 1);
        assert_eq!(coord.stats().windows_exited, 1);
    }

    #[test]
    fn test_multiple_maintenance_cycles() {
        let mut coord = MaintenanceCoordinator::new();

        for i in 1..=5 {
            coord.enter_maintenance();
            coord.exit_maintenance();
            coord.complete_catchup();
        }

        assert_eq!(coord.stats().windows_entered, 5);
        assert_eq!(coord.stats().windows_exited, 5);
    }

    #[test]
    fn test_clone_maintenance_state() {
        let state = MaintenanceState::Active;
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_clone_maintenance_window() {
        let window = MaintenanceWindow {
            start_hour: 1,
            end_hour: 4,
            days_of_week: vec![1, 2, 3],
        };
        let cloned = window.clone();
        assert_eq!(window.start_hour, cloned.start_hour);
        assert_eq!(window.end_hour, cloned.end_hour);
        assert_eq!(window.days_of_week, cloned.days_of_week);
    }

    #[test]
    fn test_clone_maintenance_stats() {
        let mut stats = MaintenanceStats::default();
        stats.windows_entered = 10;
        stats.windows_exited = 5;
        stats.pause_duration_ms = 1000;
        stats.catchup_duration_ms = 2000;
        let cloned = stats.clone();
        assert_eq!(stats.windows_entered, cloned.windows_entered);
        assert_eq!(stats.windows_exited, cloned.windows_exited);
        assert_eq!(stats.pause_duration_ms, cloned.pause_duration_ms);
        assert_eq!(stats.catchup_duration_ms, cloned.catchup_duration_ms);
    }
}
