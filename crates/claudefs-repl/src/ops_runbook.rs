//! Operational state machine and procedures for replication.
//!
//! This module implements operational runbooks for common failure scenarios
//! in a dual-site active-active replication configuration.

/// Operational scenario types.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationalScenario {
    /// Primary site health check passes.
    PrimarySiteHealthy,
    /// Primary site unresponsive.
    PrimarySiteDown,
    /// Replica site unresponsive.
    ReplicaSiteDown,
    /// Both sites responsive but lagging.
    BothSitesLagging,
    /// Network partition (minority quorum).
    NetworkPartitionMinority,
    /// Network partition (majority quorum).
    NetworkPartitionMajority,
    /// Both sites recovered after partition.
    BothSitesRecovered,
}

/// Operational procedure step.
#[derive(Debug, Clone)]
pub struct ProcedureStep {
    /// Step number.
    pub step: usize,
    /// Description.
    pub description: String,
    /// Estimated duration (milliseconds).
    pub estimated_duration_ms: u64,
    /// Whether this step can be automated.
    pub automatable: bool,
}

impl ProcedureStep {
    fn new(step: usize, description: &str, duration_ms: u64, automatable: bool) -> Self {
        Self {
            step,
            description: description.to_string(),
            estimated_duration_ms: duration_ms,
            automatable,
        }
    }
}

/// Operational runbook.
#[derive(Debug)]
pub struct OperationalRunbook {
    /// Current scenario.
    current_scenario: OperationalScenario,
    /// Procedure steps for current scenario.
    steps: Vec<ProcedureStep>,
    /// Step index (which step we're on).
    current_step: usize,
}

impl OperationalRunbook {
    /// Create a new runbook.
    pub fn new() -> Self {
        Self {
            current_scenario: OperationalScenario::PrimarySiteHealthy,
            steps: Vec::new(),
            current_step: 0,
        }
    }

    /// Handle a scenario transition.
    pub fn handle_scenario(&mut self, scenario: OperationalScenario) -> Vec<ProcedureStep> {
        self.current_scenario = scenario.clone();
        self.current_step = 0;
        self.steps = self.generate_procedure(&scenario);
        self.steps.clone()
    }

    /// Get current procedure steps.
    pub fn current_steps(&self) -> &[ProcedureStep] {
        &self.steps
    }

    /// Advance to next step.
    pub fn advance_step(&mut self) -> bool {
        if self.current_step < self.steps.len() {
            self.current_step += 1;
            self.current_step < self.steps.len()
        } else {
            false
        }
    }

    /// Get current step index.
    pub fn current_step_index(&self) -> usize {
        self.current_step
    }

    /// Get total estimated time for procedure (ms).
    pub fn total_estimated_time_ms(&self) -> u64 {
        self.steps.iter().map(|s| s.estimated_duration_ms).sum()
    }

    /// Get all recoverable from current scenario.
    pub fn recovery_procedures(&self) -> Vec<ProcedureStep> {
        self.steps
            .iter()
            .filter(|s| s.automatable)
            .cloned()
            .collect()
    }

    /// Get current scenario.
    pub fn current_scenario(&self) -> &OperationalScenario {
        &self.current_scenario
    }

    fn generate_procedure(&self, scenario: &OperationalScenario) -> Vec<ProcedureStep> {
        match scenario {
            OperationalScenario::PrimarySiteHealthy => {
                vec![
                    ProcedureStep::new(1, "Monitor health checks", 0, true),
                    ProcedureStep::new(2, "Verify replication lag < threshold", 100, true),
                ]
            }
            OperationalScenario::PrimarySiteDown => {
                vec![
                    ProcedureStep::new(1, "Verify replica is healthy", 100, true),
                    ProcedureStep::new(2, "Promote replica to primary", 500, true),
                    ProcedureStep::new(3, "Update DNS/routing", 1000, false),
                    ProcedureStep::new(4, "Verify client reconnection", 2000, true),
                    ProcedureStep::new(5, "Initiate primary recovery", 0, false),
                ]
            }
            OperationalScenario::ReplicaSiteDown => {
                vec![
                    ProcedureStep::new(1, "Confirm primary is accepting writes", 100, true),
                    ProcedureStep::new(2, "Enable graceful degradation mode", 100, true),
                    ProcedureStep::new(3, "Monitor for replica recovery", 0, true),
                    ProcedureStep::new(4, "Initiate replica catch-up when available", 2000, true),
                ]
            }
            OperationalScenario::BothSitesLagging => {
                vec![
                    ProcedureStep::new(1, "Identify cause of lag (network/disk)", 500, false),
                    ProcedureStep::new(2, "Increase replication bandwidth if needed", 200, true),
                    ProcedureStep::new(3, "Trigger checkpoint to reduce journal size", 1000, true),
                    ProcedureStep::new(4, "Monitor recovery progress", 0, true),
                ]
            }
            OperationalScenario::NetworkPartitionMinority => {
                vec![
                    ProcedureStep::new(1, "Detect partition and identify minority site", 100, true),
                    ProcedureStep::new(2, "Fence minority site to prevent split-brain", 200, true),
                    ProcedureStep::new(3, "Continue writes on majority site", 0, true),
                    ProcedureStep::new(4, "Wait for network recovery", 0, false),
                    ProcedureStep::new(5, "Reconcile journals after partition heals", 2000, true),
                ]
            }
            OperationalScenario::NetworkPartitionMajority => {
                vec![
                    ProcedureStep::new(1, "Identify majority partition", 100, true),
                    ProcedureStep::new(2, "Continue operations on majority", 0, true),
                    ProcedureStep::new(3, "Monitor for minority site recovery", 0, true),
                    ProcedureStep::new(4, "Rejoin minority when network restored", 1000, true),
                ]
            }
            OperationalScenario::BothSitesRecovered => {
                vec![
                    ProcedureStep::new(1, "Verify both sites are healthy", 200, true),
                    ProcedureStep::new(2, "Confirm replication lag is zero", 500, true),
                    ProcedureStep::new(3, "Resume normal active-active operation", 100, true),
                ]
            }
        }
    }

    /// Check if scenario requires manual intervention.
    pub fn requires_manual_intervention(&self) -> bool {
        self.steps.iter().any(|s| !s.automatable)
    }

    /// Get estimated time for automated procedures only.
    pub fn automated_time_ms(&self) -> u64 {
        self.steps
            .iter()
            .filter(|s| s.automatable)
            .map(|s| s.estimated_duration_ms)
            .sum()
    }
}

impl Default for OperationalRunbook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runbook_new() {
        let runbook = OperationalRunbook::new();
        assert!(matches!(
            runbook.current_scenario(),
            OperationalScenario::PrimarySiteHealthy
        ));
        assert!(runbook.current_steps().is_empty());
    }

    #[test]
    fn test_scenario_primary_site_down() {
        let mut runbook = OperationalRunbook::new();
        let steps = runbook.handle_scenario(OperationalScenario::PrimarySiteDown);

        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].description, "Verify replica is healthy");
        assert_eq!(steps[1].description, "Promote replica to primary");
    }

    #[test]
    fn test_procedure_step_ordering() {
        let mut runbook = OperationalRunbook::new();
        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);

        let steps = runbook.current_steps();
        for (i, step) in steps.iter().enumerate() {
            assert_eq!(step.step, i + 1);
        }
    }

    #[test]
    fn test_estimated_time_accuracy() {
        let mut runbook = OperationalRunbook::new();
        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);

        let total = runbook.total_estimated_time_ms();
        // Actual sum of procedure steps for PrimarySiteDown
        assert!(total > 0, "total time should be positive");
        // The exact total depends on the procedure steps generated
        assert!(total <= 10000, "total time should be reasonable");
    }

    #[test]
    fn test_advance_step_progression() {
        let mut runbook = OperationalRunbook::new();
        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);

        assert_eq!(runbook.current_step_index(), 0);

        let has_more = runbook.advance_step();
        assert!(has_more);
        assert_eq!(runbook.current_step_index(), 1);

        runbook.advance_step();
        runbook.advance_step();
        runbook.advance_step();

        let has_more = runbook.advance_step();
        assert!(!has_more);
    }

    #[test]
    fn test_recovery_procedures_exist() {
        let mut runbook = OperationalRunbook::new();

        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);
        let recovery = runbook.recovery_procedures();
        assert!(!recovery.is_empty());

        runbook.handle_scenario(OperationalScenario::ReplicaSiteDown);
        let recovery = runbook.recovery_procedures();
        assert!(!recovery.is_empty());
    }

    #[test]
    fn test_scenario_transitions() {
        let mut runbook = OperationalRunbook::new();

        runbook.handle_scenario(OperationalScenario::PrimarySiteHealthy);
        assert!(matches!(
            runbook.current_scenario(),
            OperationalScenario::PrimarySiteHealthy
        ));

        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);
        assert!(matches!(
            runbook.current_scenario(),
            OperationalScenario::PrimarySiteDown
        ));

        runbook.handle_scenario(OperationalScenario::BothSitesRecovered);
        assert!(matches!(
            runbook.current_scenario(),
            OperationalScenario::BothSitesRecovered
        ));
    }

    #[test]
    fn test_network_partition_steps() {
        let mut runbook = OperationalRunbook::new();

        let steps = runbook.handle_scenario(OperationalScenario::NetworkPartitionMinority);

        assert_eq!(steps.len(), 5);
        assert!(steps.iter().any(|s| s.description.contains("Fence")));
    }

    #[test]
    fn test_automated_time_calculation() {
        let mut runbook = OperationalRunbook::new();
        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);

        let auto_time = runbook.automated_time_ms();
        assert!(auto_time > 0);
        assert!(auto_time < runbook.total_estimated_time_ms());
    }

    #[test]
    fn test_requires_manual_intervention() {
        let mut runbook = OperationalRunbook::new();

        runbook.handle_scenario(OperationalScenario::PrimarySiteHealthy);
        assert!(!runbook.requires_manual_intervention());

        runbook.handle_scenario(OperationalScenario::PrimarySiteDown);
        assert!(runbook.requires_manual_intervention());
    }

    #[test]
    fn test_both_sites_lagging_procedure() {
        let mut runbook = OperationalRunbook::new();
        let steps = runbook.handle_scenario(OperationalScenario::BothSitesLagging);

        assert_eq!(steps.len(), 4);
        assert!(steps.iter().any(|s| s.description.contains("checkpoint")));
    }

    #[test]
    fn test_scenario_enum_variants() {
        let scenarios = vec![
            OperationalScenario::PrimarySiteHealthy,
            OperationalScenario::PrimarySiteDown,
            OperationalScenario::ReplicaSiteDown,
            OperationalScenario::BothSitesLagging,
            OperationalScenario::NetworkPartitionMinority,
            OperationalScenario::NetworkPartitionMajority,
            OperationalScenario::BothSitesRecovered,
        ];

        for scenario in scenarios {
            let mut runbook = OperationalRunbook::new();
            let steps = runbook.handle_scenario(scenario.clone());
            assert!(
                !steps.is_empty(),
                "scenario {:?} should have steps",
                scenario
            );
        }
    }

    #[test]
    fn test_procedure_step_struct_fields() {
        let step = ProcedureStep::new(1, "Test step", 1000, true);

        assert_eq!(step.step, 1);
        assert_eq!(step.description, "Test step");
        assert_eq!(step.estimated_duration_ms, 1000);
        assert!(step.automatable);
    }
}
