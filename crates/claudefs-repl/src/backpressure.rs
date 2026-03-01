//! Adaptive backpressure control for replication senders.
//!
//! Controls when to slow down the replication sender based on observed lag and error signals.

use std::collections::HashMap;

/// Level of backpressure to apply to the replication sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackpressureLevel {
    /// No backpressure; send at full speed.
    None,
    /// Mild slowdown: introduce small delay (e.g., 5ms).
    Mild,
    /// Moderate slowdown: delay 50ms per batch.
    Moderate,
    /// Severe: delay 500ms per batch.
    Severe,
    /// Halt: stop sending until explicitly cleared.
    Halt,
}

impl BackpressureLevel {
    /// Returns the suggested delay in milliseconds for this level.
    pub fn suggested_delay_ms(&self) -> u64 {
        match self {
            Self::None => 0,
            Self::Mild => 5,
            Self::Moderate => 50,
            Self::Severe => 500,
            Self::Halt => u64::MAX,
        }
    }

    /// Returns true if sending should be halted entirely.
    pub fn is_halted(&self) -> bool {
        matches!(self, Self::Halt)
    }

    /// Returns true if any backpressure is applied.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Configuration for backpressure thresholds.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Queue depth (pending entries) that triggers Mild backpressure.
    pub mild_queue_depth: u64,
    /// Queue depth for Moderate.
    pub moderate_queue_depth: u64,
    /// Queue depth for Severe.
    pub severe_queue_depth: u64,
    /// Queue depth for Halt.
    pub halt_queue_depth: u64,
    /// Consecutive error count that triggers Moderate backpressure.
    pub error_count_moderate: u32,
    /// Consecutive error count for Severe backpressure.
    pub error_count_severe: u32,
    /// Consecutive error count for Halt backpressure.
    pub error_count_halt: u32,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            mild_queue_depth: 1_000,
            moderate_queue_depth: 10_000,
            severe_queue_depth: 100_000,
            halt_queue_depth: 1_000_000,
            error_count_moderate: 3,
            error_count_severe: 10,
            error_count_halt: 20,
        }
    }
}

/// Tracks backpressure state for one remote site.
pub struct BackpressureController {
    config: BackpressureConfig,
    /// Current queue depth (entries waiting to be sent).
    queue_depth: u64,
    /// Consecutive send errors since last success.
    consecutive_errors: u32,
    /// Whether backpressure is manually overridden (forced halt).
    force_halt: bool,
    /// Last computed level (cached for reporting).
    last_level: BackpressureLevel,
}

impl BackpressureController {
    /// Create a new controller with the given config.
    pub fn new(config: BackpressureConfig) -> Self {
        Self {
            config,
            queue_depth: 0,
            consecutive_errors: 0,
            force_halt: false,
            last_level: BackpressureLevel::None,
        }
    }

    /// Update the observed queue depth.
    pub fn set_queue_depth(&mut self, depth: u64) {
        self.queue_depth = depth;
    }

    /// Record a successful send (resets consecutive_errors).
    pub fn record_success(&mut self) {
        self.consecutive_errors = 0;
    }

    /// Record a send error (increments consecutive_errors).
    pub fn record_error(&mut self) {
        self.consecutive_errors += 1;
    }

    /// Force halt regardless of other signals (e.g., admin command).
    pub fn force_halt(&mut self) {
        self.force_halt = true;
    }

    /// Clear the force halt.
    pub fn clear_halt(&mut self) {
        self.force_halt = false;
    }

    /// Compute and return the current backpressure level.
    /// Uses the max of queue-depth-based and error-count-based levels.
    pub fn compute_level(&mut self) -> BackpressureLevel {
        let queue_level = if self.force_halt {
            BackpressureLevel::Halt
        } else if self.queue_depth >= self.config.halt_queue_depth {
            BackpressureLevel::Halt
        } else if self.queue_depth >= self.config.severe_queue_depth {
            BackpressureLevel::Severe
        } else if self.queue_depth >= self.config.moderate_queue_depth {
            BackpressureLevel::Moderate
        } else if self.queue_depth >= self.config.mild_queue_depth {
            BackpressureLevel::Mild
        } else {
            BackpressureLevel::None
        };

        let error_level = if self.force_halt {
            BackpressureLevel::Halt
        } else if self.consecutive_errors >= self.config.error_count_halt {
            BackpressureLevel::Halt
        } else if self.consecutive_errors >= self.config.error_count_severe {
            BackpressureLevel::Severe
        } else if self.consecutive_errors >= self.config.error_count_moderate {
            BackpressureLevel::Moderate
        } else {
            BackpressureLevel::None
        };

        self.last_level = if queue_level > error_level {
            queue_level
        } else {
            error_level
        };

        self.last_level
    }

    /// Get the last computed level without recomputing.
    pub fn current_level(&self) -> BackpressureLevel {
        self.last_level
    }

    /// Get the suggested delay in milliseconds (delegates to level).
    pub fn suggested_delay_ms(&mut self) -> u64 {
        self.compute_level().suggested_delay_ms()
    }

    /// Returns true if sending is halted.
    pub fn is_halted(&mut self) -> bool {
        self.compute_level().is_halted()
    }

    /// Get current queue depth.
    pub fn queue_depth(&self) -> u64 {
        self.queue_depth
    }

    /// Get consecutive error count.
    pub fn consecutive_errors(&self) -> u32 {
        self.consecutive_errors
    }
}

/// Manages backpressure controllers for multiple remote sites.
pub struct BackpressureManager {
    per_site: HashMap<u64, BackpressureController>,
    default_config: BackpressureConfig,
}

impl BackpressureManager {
    /// Create a new manager with the given default config.
    pub fn new(default_config: BackpressureConfig) -> Self {
        Self {
            per_site: HashMap::new(),
            default_config,
        }
    }

    /// Register a site with the default config.
    pub fn register_site(&mut self, site_id: u64) {
        self.per_site
            .entry(site_id)
            .or_insert_with(|| BackpressureController::new(self.default_config.clone()));
    }

    /// Register a site with a specific config.
    pub fn register_site_with_config(&mut self, site_id: u64, config: BackpressureConfig) {
        self.per_site
            .entry(site_id)
            .or_insert_with(|| BackpressureController::new(config));
    }

    /// Get the current level for a site (None if site not registered).
    pub fn level(&mut self, site_id: u64) -> Option<BackpressureLevel> {
        self.per_site.get_mut(&site_id).map(|c| c.compute_level())
    }

    /// Record a success for a site.
    pub fn record_success(&mut self, site_id: u64) {
        if let Some(controller) = self.per_site.get_mut(&site_id) {
            controller.record_success();
        }
    }

    /// Record an error for a site.
    pub fn record_error(&mut self, site_id: u64) {
        if let Some(controller) = self.per_site.get_mut(&site_id) {
            controller.record_error();
        }
    }

    /// Update queue depth for a site.
    pub fn set_queue_depth(&mut self, site_id: u64, depth: u64) {
        if let Some(controller) = self.per_site.get_mut(&site_id) {
            controller.set_queue_depth(depth);
        }
    }

    /// Force halt for a site.
    pub fn force_halt(&mut self, site_id: u64) {
        if let Some(controller) = self.per_site.get_mut(&site_id) {
            controller.force_halt();
        }
    }

    /// Clear halt for a site.
    pub fn clear_halt(&mut self, site_id: u64) {
        if let Some(controller) = self.per_site.get_mut(&site_id) {
            controller.clear_halt();
        }
    }

    /// Get all sites that are currently halted.
    pub fn halted_sites(&mut self) -> Vec<u64> {
        let mut halted = Vec::new();
        for (site_id, controller) in &mut self.per_site {
            if controller.compute_level().is_halted() {
                halted.push(*site_id);
            }
        }
        halted
    }

    /// Remove a site.
    pub fn remove_site(&mut self, site_id: u64) {
        self.per_site.remove(&site_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backpressure_level_ordering() {
        assert!(BackpressureLevel::None < BackpressureLevel::Mild);
        assert!(BackpressureLevel::Mild < BackpressureLevel::Moderate);
        assert!(BackpressureLevel::Moderate < BackpressureLevel::Severe);
        assert!(BackpressureLevel::Severe < BackpressureLevel::Halt);
    }

    #[test]
    fn suggested_delay_ms_values() {
        assert_eq!(BackpressureLevel::None.suggested_delay_ms(), 0);
        assert_eq!(BackpressureLevel::Mild.suggested_delay_ms(), 5);
        assert_eq!(BackpressureLevel::Moderate.suggested_delay_ms(), 50);
        assert_eq!(BackpressureLevel::Severe.suggested_delay_ms(), 500);
        assert_eq!(BackpressureLevel::Halt.suggested_delay_ms(), u64::MAX);
    }

    #[test]
    fn is_halted_only_for_halt() {
        assert!(!BackpressureLevel::None.is_halted());
        assert!(!BackpressureLevel::Mild.is_halted());
        assert!(!BackpressureLevel::Moderate.is_halted());
        assert!(!BackpressureLevel::Severe.is_halted());
        assert!(BackpressureLevel::Halt.is_halted());
    }

    #[test]
    fn is_active_for_non_none() {
        assert!(!BackpressureLevel::None.is_active());
        assert!(BackpressureLevel::Mild.is_active());
        assert!(BackpressureLevel::Moderate.is_active());
        assert!(BackpressureLevel::Severe.is_active());
        assert!(BackpressureLevel::Halt.is_active());
    }

    #[test]
    fn controller_default_level_is_none() {
        let controller = BackpressureController::new(BackpressureConfig::default());
        assert_eq!(controller.current_level(), BackpressureLevel::None);
    }

    #[test]
    fn controller_set_queue_depth_mild() {
        let config = BackpressureConfig {
            mild_queue_depth: 500,
            moderate_queue_depth: 5_000,
            severe_queue_depth: 50_000,
            halt_queue_depth: 500_000,
            ..Default::default()
        };
        let mut controller = BackpressureController::new(config);
        controller.set_queue_depth(500);
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Mild);
    }

    #[test]
    fn controller_set_queue_depth_moderate() {
        let config = BackpressureConfig {
            mild_queue_depth: 500,
            moderate_queue_depth: 5_000,
            severe_queue_depth: 50_000,
            halt_queue_depth: 500_000,
            ..Default::default()
        };
        let mut controller = BackpressureController::new(config);
        controller.set_queue_depth(5_000);
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Moderate);
    }

    #[test]
    fn controller_set_queue_depth_severe() {
        let config = BackpressureConfig {
            mild_queue_depth: 500,
            moderate_queue_depth: 5_000,
            severe_queue_depth: 50_000,
            halt_queue_depth: 500_000,
            ..Default::default()
        };
        let mut controller = BackpressureController::new(config);
        controller.set_queue_depth(50_000);
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Severe);
    }

    #[test]
    fn controller_set_queue_depth_halt() {
        let config = BackpressureConfig {
            mild_queue_depth: 500,
            moderate_queue_depth: 5_000,
            severe_queue_depth: 50_000,
            halt_queue_depth: 500_000,
            ..Default::default()
        };
        let mut controller = BackpressureController::new(config);
        controller.set_queue_depth(2_000_000);
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Halt);
    }

    #[test]
    fn controller_error_count_moderate() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        for _ in 0..3 {
            controller.record_error();
        }
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Moderate);
    }

    #[test]
    fn controller_error_count_severe() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        for _ in 0..10 {
            controller.record_error();
        }
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Severe);
    }

    #[test]
    fn controller_error_count_halt() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        for _ in 0..20 {
            controller.record_error();
        }
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Halt);
    }

    #[test]
    fn controller_record_success_resets_errors() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        for _ in 0..10 {
            controller.record_error();
        }
        controller.record_success();
        assert_eq!(controller.consecutive_errors(), 0);
    }

    #[test]
    fn controller_force_halt() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        controller.force_halt();
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Halt);
    }

    #[test]
    fn controller_clear_halt() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());
        controller.force_halt();
        controller.clear_halt();
        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::None);
    }

    #[test]
    fn controller_queue_and_error_max_level() {
        let mut config = BackpressureConfig::default();
        config.mild_queue_depth = 100_000;
        config.moderate_queue_depth = 200_000;
        let mut controller = BackpressureController::new(config);

        controller.set_queue_depth(50_000);
        controller.record_error();
        controller.record_error();
        controller.record_error();

        let level = controller.compute_level();
        assert_eq!(level, BackpressureLevel::Moderate);
    }

    #[test]
    fn manager_register_and_level() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);

        let level = manager.level(1);
        assert_eq!(level, Some(BackpressureLevel::None));
    }

    #[test]
    fn manager_record_success_error() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);

        manager.record_error(1);
        manager.record_error(1);
        let level = manager.level(1);
        assert!(level.is_some());

        manager.record_success(1);
        let controller = manager.per_site.get(&1).unwrap();
        assert_eq!(controller.consecutive_errors(), 0);
    }

    #[test]
    fn manager_halted_sites() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);
        manager.register_site(2);

        manager.force_halt(1);

        let halted = manager.halted_sites();
        assert!(halted.contains(&1));
        assert!(!halted.contains(&2));
    }

    #[test]
    fn manager_remove_site() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);

        manager.remove_site(1);

        let level = manager.level(1);
        assert_eq!(level, None);
    }
}
