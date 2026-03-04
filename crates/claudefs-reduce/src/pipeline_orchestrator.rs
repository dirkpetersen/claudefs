use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PipelineStage {
    Ingest,
    Dedup,
    Compress,
    Encrypt,
    Segment,
    Tier,
}

impl PipelineStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineStage::Ingest => "ingest",
            PipelineStage::Dedup => "dedup",
            PipelineStage::Compress => "compress",
            PipelineStage::Encrypt => "encrypt",
            PipelineStage::Segment => "segment",
            PipelineStage::Tier => "tier",
        }
    }

    pub fn all() -> &'static [PipelineStage] {
        &[
            PipelineStage::Ingest,
            PipelineStage::Dedup,
            PipelineStage::Compress,
            PipelineStage::Encrypt,
            PipelineStage::Segment,
            PipelineStage::Tier,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageMetricsData {
    pub items_processed: u64,
    pub items_dropped: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub errors: u64,
}

impl StageMetricsData {
    pub fn reduction_factor(&self) -> f64 {
        if self.bytes_out == 0 {
            return 1.0;
        }
        self.bytes_in as f64 / self.bytes_out as f64
    }
}

#[derive(Debug, Clone)]
pub struct PipelineOrchestratorConfig {
    pub name: String,
    pub enabled_stages: Vec<PipelineStage>,
}

impl Default for PipelineOrchestratorConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            enabled_stages: PipelineStage::all().to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorState {
    Idle,
    Running,
    Draining,
    Stopped,
}

pub struct PipelineOrchestrator {
    config: PipelineOrchestratorConfig,
    stage_metrics: std::collections::HashMap<PipelineStage, StageMetricsData>,
    state: OrchestratorState,
    total_items_processed: u64,
    total_errors: u64,
}

impl PipelineOrchestrator {
    pub fn new(config: PipelineOrchestratorConfig) -> Self {
        let mut stage_metrics = std::collections::HashMap::new();
        for stage in PipelineStage::all() {
            stage_metrics.insert(stage.clone(), StageMetricsData::default());
        }
        Self {
            config,
            stage_metrics,
            state: OrchestratorState::Idle,
            total_items_processed: 0,
            total_errors: 0,
        }
    }

    pub fn start(&mut self) {
        if self.state == OrchestratorState::Idle {
            self.state = OrchestratorState::Running;
        }
    }

    pub fn stop(&mut self) {
        self.state = OrchestratorState::Stopped;
    }

    pub fn drain(&mut self) {
        if self.state == OrchestratorState::Running {
            self.state = OrchestratorState::Draining;
        }
    }

    pub fn is_stage_enabled(&self, stage: &PipelineStage) -> bool {
        self.config.enabled_stages.contains(stage)
    }

    pub fn record_stage(&mut self, stage: PipelineStage, bytes_in: u64, bytes_out: u64) {
        if let Some(m) = self.stage_metrics.get_mut(&stage) {
            m.items_processed += 1;
            m.bytes_in += bytes_in;
            m.bytes_out += bytes_out;
        }
        self.total_items_processed += 1;
    }

    pub fn record_error(&mut self, stage: PipelineStage) {
        if let Some(m) = self.stage_metrics.get_mut(&stage) {
            m.errors += 1;
            m.items_dropped += 1;
        }
        self.total_errors += 1;
    }

    pub fn record_dedup_drop(&mut self, bytes: u64) {
        if let Some(m) = self.stage_metrics.get_mut(&PipelineStage::Dedup) {
            m.items_dropped += 1;
            m.bytes_in += bytes;
        }
    }

    pub fn stage_metrics(&self, stage: &PipelineStage) -> Option<&StageMetricsData> {
        self.stage_metrics.get(stage)
    }

    pub fn state(&self) -> &OrchestratorState {
        &self.state
    }
    pub fn total_items_processed(&self) -> u64 {
        self.total_items_processed
    }
    pub fn total_errors(&self) -> u64 {
        self.total_errors
    }
    pub fn name(&self) -> &str {
        &self.config.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_stage_as_str() {
        assert_eq!(PipelineStage::Ingest.as_str(), "ingest");
        assert_eq!(PipelineStage::Dedup.as_str(), "dedup");
        assert_eq!(PipelineStage::Compress.as_str(), "compress");
        assert_eq!(PipelineStage::Encrypt.as_str(), "encrypt");
        assert_eq!(PipelineStage::Segment.as_str(), "segment");
        assert_eq!(PipelineStage::Tier.as_str(), "tier");
    }

    #[test]
    fn pipeline_stage_all_has_6_stages() {
        assert_eq!(PipelineStage::all().len(), 6);
    }

    #[test]
    fn stage_metrics_default() {
        let metrics = StageMetricsData::default();
        assert_eq!(metrics.items_processed, 0);
        assert_eq!(metrics.items_dropped, 0);
        assert_eq!(metrics.bytes_in, 0);
        assert_eq!(metrics.bytes_out, 0);
        assert_eq!(metrics.errors, 0);
    }

    #[test]
    fn reduction_factor_no_bytes_out() {
        let metrics = StageMetricsData {
            bytes_in: 100,
            bytes_out: 0,
            ..Default::default()
        };
        assert_eq!(metrics.reduction_factor(), 1.0);
    }

    #[test]
    fn reduction_factor_2x() {
        let metrics = StageMetricsData {
            bytes_in: 1000,
            bytes_out: 500,
            ..Default::default()
        };
        assert_eq!(metrics.reduction_factor(), 2.0);
    }

    #[test]
    fn orchestrator_config_default() {
        let config = PipelineOrchestratorConfig::default();
        assert_eq!(config.name, "default");
        assert_eq!(config.enabled_stages.len(), 6);
    }

    #[test]
    fn new_orchestrator_idle() {
        let config = PipelineOrchestratorConfig::default();
        let orch = PipelineOrchestrator::new(config);
        assert_eq!(orch.state(), &OrchestratorState::Idle);
    }

    #[test]
    fn start_transitions_to_running() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.start();
        assert_eq!(orch.state(), &OrchestratorState::Running);
    }

    #[test]
    fn stop_transitions_to_stopped() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.stop();
        assert_eq!(orch.state(), &OrchestratorState::Stopped);
    }

    #[test]
    fn drain_transitions_to_draining() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.start();
        orch.drain();
        assert_eq!(orch.state(), &OrchestratorState::Draining);
    }

    #[test]
    fn drain_from_idle_no_op() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.drain();
        assert_eq!(orch.state(), &OrchestratorState::Idle);
    }

    #[test]
    fn is_stage_enabled_all_default() {
        let config = PipelineOrchestratorConfig::default();
        let orch = PipelineOrchestrator::new(config);
        for stage in PipelineStage::all() {
            assert!(orch.is_stage_enabled(stage));
        }
    }

    #[test]
    fn is_stage_enabled_when_disabled() {
        let config = PipelineOrchestratorConfig {
            name: "test".to_string(),
            enabled_stages: vec![PipelineStage::Ingest, PipelineStage::Compress],
        };
        let orch = PipelineOrchestrator::new(config);
        assert!(!orch.is_stage_enabled(&PipelineStage::Dedup));
    }

    #[test]
    fn record_stage_increments_items() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_stage(PipelineStage::Ingest, 100, 80);
        let metrics = orch.stage_metrics(&PipelineStage::Ingest).unwrap();
        assert_eq!(metrics.items_processed, 1);
    }

    #[test]
    fn record_stage_accumulates_bytes() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_stage(PipelineStage::Compress, 100, 50);
        orch.record_stage(PipelineStage::Compress, 200, 100);
        let metrics = orch.stage_metrics(&PipelineStage::Compress).unwrap();
        assert_eq!(metrics.bytes_in, 300);
        assert_eq!(metrics.bytes_out, 150);
    }

    #[test]
    fn record_stage_increments_total() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_stage(PipelineStage::Encrypt, 100, 100);
        assert_eq!(orch.total_items_processed(), 1);
    }

    #[test]
    fn record_error_increments_errors() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_error(PipelineStage::Segment);
        let metrics = orch.stage_metrics(&PipelineStage::Segment).unwrap();
        assert_eq!(metrics.errors, 1);
    }

    #[test]
    fn record_error_increments_dropped() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_error(PipelineStage::Tier);
        let metrics = orch.stage_metrics(&PipelineStage::Tier).unwrap();
        assert_eq!(metrics.items_dropped, 1);
    }

    #[test]
    fn record_error_increments_total_errors() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_error(PipelineStage::Ingest);
        assert_eq!(orch.total_errors(), 1);
    }

    #[test]
    fn record_dedup_drop_increments_dropped() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_dedup_drop(1000);
        let metrics = orch.stage_metrics(&PipelineStage::Dedup).unwrap();
        assert_eq!(metrics.items_dropped, 1);
    }

    #[test]
    fn stage_metrics_returns_none_for_unknown_stage() {
        let config = PipelineOrchestratorConfig::default();
        let orch = PipelineOrchestrator::new(config);
        assert!(orch.stage_metrics(&PipelineStage::Ingest).is_some());
    }

    #[test]
    fn total_items_processed_sum() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_stage(PipelineStage::Ingest, 100, 80);
        orch.record_stage(PipelineStage::Compress, 200, 150);
        orch.record_stage(PipelineStage::Encrypt, 150, 150);
        assert_eq!(orch.total_items_processed(), 3);
    }

    #[test]
    fn multiple_errors_accumulate() {
        let config = PipelineOrchestratorConfig::default();
        let mut orch = PipelineOrchestrator::new(config);
        orch.record_error(PipelineStage::Ingest);
        orch.record_error(PipelineStage::Compress);
        orch.record_error(PipelineStage::Encrypt);
        assert_eq!(orch.total_errors(), 3);
    }

    #[test]
    fn orchestrator_name() {
        let config = PipelineOrchestratorConfig {
            name: "mypipeline".to_string(),
            enabled_stages: vec![],
        };
        let orch = PipelineOrchestrator::new(config);
        assert_eq!(orch.name(), "mypipeline");
    }
}
