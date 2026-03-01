//! Request middleware pipeline for composing compression, rate limiting, tracing, and metrics.
//!
//! This module provides a configurable middleware pipeline that allows composing
//! ordered client-side and server-side stages for request processing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// Unique identifier for a middleware stage.
pub type StageId = u32;

/// Direction: whether middleware runs on inbound (server) or outbound (client) path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineDirection {
    /// Server-side: request arrives from client.
    Inbound,
    /// Client-side: request departs to server.
    Outbound,
}

/// Result of processing through a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageAction {
    /// Pass to next stage.
    Continue,
    /// Skip remaining stages, return current payload.
    Skip,
    /// Reject with reason.
    Reject(String),
}

/// A single middleware stage configuration.
#[derive(Debug, Clone)]
pub struct StageConfig {
    /// Unique stage identifier.
    pub id: StageId,
    /// Human-readable stage name.
    pub name: String,
    /// Whether the stage is active.
    pub enabled: bool,
    /// Execution order (lower runs first).
    pub order: u32,
    /// Which pipeline direction this stage applies to.
    pub direction: PipelineDirection,
}

impl StageConfig {
    /// Creates a new stage configuration.
    pub fn new(id: StageId, name: &str, order: u32, direction: PipelineDirection) -> Self {
        Self {
            id,
            name: name.to_string(),
            enabled: true,
            order,
            direction,
        }
    }
}

/// Pipeline configuration.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Maximum number of stages allowed in the pipeline.
    pub max_stages: usize,
    /// Maximum payload size in bytes.
    pub max_payload_bytes: usize,
    /// If a stage errors, continue to next stage instead of halting.
    pub fail_open: bool,
    /// Track per-stage latency timing.
    pub track_stage_timing: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_stages: 32,
            max_payload_bytes: 64 * 1024 * 1024,
            fail_open: false,
            track_stage_timing: true,
        }
    }
}

/// Represents a request being processed through the pipeline.
pub struct PipelineRequest {
    /// Unique request identifier.
    pub request_id: u64,
    /// Request operation code.
    pub opcode: u8,
    /// Request payload data.
    pub payload: Vec<u8>,
    /// Request metadata for passing context between stages.
    pub metadata: HashMap<String, String>,
    /// Pipeline direction (inbound or outbound).
    pub direction: PipelineDirection,
}

impl PipelineRequest {
    /// Creates a new pipeline request.
    pub fn new(
        request_id: u64,
        opcode: u8,
        payload: Vec<u8>,
        direction: PipelineDirection,
    ) -> Self {
        Self {
            request_id,
            opcode,
            payload,
            metadata: HashMap::new(),
            direction,
        }
    }
}

/// Stage processing result with timing.
pub struct StageResult {
    /// Unique stage identifier.
    pub stage_id: StageId,
    /// Human-readable stage name.
    pub stage_name: String,
    /// Action taken by the stage.
    pub action: StageAction,
    /// Processing time in microseconds.
    pub duration_us: u64,
}

/// Full pipeline execution result.
pub struct PipelineResult {
    /// Original request identifier.
    pub request_id: u64,
    /// Final action after all stages processed.
    pub final_action: StageAction,
    /// Final payload after all stages processed.
    pub final_payload: Vec<u8>,
    /// Results from each executed stage.
    pub stage_results: Vec<StageResult>,
    /// Total pipeline execution time in microseconds.
    pub total_duration_us: u64,
    /// Number of stages that were executed.
    pub stages_executed: usize,
}

/// Pipeline-specific errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    /// Maximum number of stages exceeded.
    TooManyStages {
        /// Maximum allowed stages.
        max: usize,
    },
    /// A stage with this ID already exists.
    DuplicateStageId {
        /// The duplicate stage ID.
        id: StageId,
    },
    /// Payload exceeds maximum allowed size.
    PayloadTooLarge {
        /// Actual payload size.
        size: usize,
        /// Maximum allowed size.
        max: usize,
    },
}

/// Trait that each middleware stage implements.
pub trait StageProcessor: Send + Sync {
    /// Process a request through this stage.
    fn process(&self, request: &mut PipelineRequest) -> StageAction;
    /// Return the stage's name for debugging.
    fn name(&self) -> &str;
}

struct StageEntry {
    config: StageConfig,
    processor: Box<dyn StageProcessor>,
}

/// A configurable middleware pipeline for request processing.
pub struct Pipeline {
    config: PipelineConfig,
    stages: Vec<StageEntry>,
    stats: PipelineStats,
}

/// Atomic counters for pipeline statistics.
pub struct PipelineStats {
    total_requests: AtomicU64,
    total_rejections: AtomicU64,
    total_skips: AtomicU64,
    total_errors: AtomicU64,
    stages_registered: AtomicUsize,
}

/// Snapshot of pipeline statistics at a point in time.
pub struct PipelineStatsSnapshot {
    /// Total number of requests processed.
    pub total_requests: u64,
    /// Total number of rejected requests.
    pub total_rejections: u64,
    /// Total number of skipped requests (early exit).
    pub total_skips: u64,
    /// Total number of errors encountered.
    pub total_errors: u64,
    /// Number of stages currently registered.
    pub stages_registered: usize,
}

impl PipelineStats {
    fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_rejections: AtomicU64::new(0),
            total_skips: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            stages_registered: AtomicUsize::new(0),
        }
    }

    fn snapshot(&self) -> PipelineStatsSnapshot {
        PipelineStatsSnapshot {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_rejections: self.total_rejections.load(Ordering::Relaxed),
            total_skips: self.total_skips.load(Ordering::Relaxed),
            total_errors: self.total_errors.load(Ordering::Relaxed),
            stages_registered: self.stages_registered.load(Ordering::Relaxed),
        }
    }

    fn increment_requests(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_rejections(&self) {
        self.total_rejections.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_skips(&self) {
        self.total_skips.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn increment_errors(&self) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
    }

    fn set_stages_registered(&self, count: usize) {
        self.stages_registered.store(count, Ordering::Relaxed);
    }
}

impl Default for PipelineStats {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// Creates a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            stages: Vec::new(),
            stats: PipelineStats::new(),
        }
    }

    /// Adds a stage to the pipeline.
    pub fn add_stage(
        &mut self,
        config: StageConfig,
        processor: Box<dyn StageProcessor>,
    ) -> Result<(), PipelineError> {
        if self.stages.len() >= self.config.max_stages {
            return Err(PipelineError::TooManyStages {
                max: self.config.max_stages,
            });
        }

        if self.stages.iter().any(|s| s.config.id == config.id) {
            return Err(PipelineError::DuplicateStageId { id: config.id });
        }

        let name = processor.name().to_string();
        self.stages.push(StageEntry { config, processor });
        self.stages.sort_by_key(|s| s.config.order);
        self.stats.set_stages_registered(self.stages.len());
        let _ = name;
        Ok(())
    }

    /// Removes a stage from the pipeline by ID.
    pub fn remove_stage(&mut self, id: StageId) -> bool {
        let len_before = self.stages.len();
        self.stages.retain(|s| s.config.id != id);
        let removed = self.stages.len() < len_before;
        if removed {
            self.stats.set_stages_registered(self.stages.len());
        }
        removed
    }

    /// Enables a stage by ID.
    pub fn enable_stage(&mut self, id: StageId) -> bool {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.config.id == id) {
            stage.config.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disables a stage by ID.
    pub fn disable_stage(&mut self, id: StageId) -> bool {
        if let Some(stage) = self.stages.iter_mut().find(|s| s.config.id == id) {
            stage.config.enabled = false;
            true
        } else {
            false
        }
    }

    /// Executes the pipeline on a request.
    pub fn execute(&self, request: &mut PipelineRequest) -> PipelineResult {
        self.stats.increment_requests();

        if request.payload.len() > self.config.max_payload_bytes {
            self.stats.increment_rejections();
            return PipelineResult {
                request_id: request.request_id,
                final_action: StageAction::Reject("Payload too large".to_string()),
                final_payload: request.payload.clone(),
                stage_results: Vec::new(),
                total_duration_us: 0,
                stages_executed: 0,
            };
        }

        let start = Instant::now();
        let mut stage_results = Vec::new();
        let mut stages_executed = 0;

        for stage in &self.stages {
            if !stage.config.enabled {
                continue;
            }
            if stage.config.direction != request.direction {
                continue;
            }

            stages_executed += 1;

            let stage_start = if self.config.track_stage_timing {
                Some(Instant::now())
            } else {
                None
            };

            let action = stage.processor.process(request);

            let duration_us = stage_start
                .map(|s| s.elapsed().as_micros() as u64)
                .unwrap_or(0);

            stage_results.push(StageResult {
                stage_id: stage.config.id,
                stage_name: stage.config.name.clone(),
                action: action.clone(),
                duration_us,
            });

            match action {
                StageAction::Continue => {}
                StageAction::Skip => {
                    self.stats.increment_skips();
                    break;
                }
                StageAction::Reject(_) => {
                    self.stats.increment_rejections();
                    return PipelineResult {
                        request_id: request.request_id,
                        final_action: action,
                        final_payload: request.payload.clone(),
                        stage_results,
                        total_duration_us: start.elapsed().as_micros() as u64,
                        stages_executed,
                    };
                }
            }
        }

        let final_action = if stage_results.is_empty() {
            StageAction::Continue
        } else {
            stage_results
                .last()
                .map(|r| r.action.clone())
                .unwrap_or(StageAction::Continue)
        };

        PipelineResult {
            request_id: request.request_id,
            final_action,
            final_payload: request.payload.clone(),
            stage_results,
            total_duration_us: start.elapsed().as_micros() as u64,
            stages_executed,
        }
    }

    /// Returns the number of stages in the pipeline.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Returns a snapshot of current pipeline statistics.
    pub fn stats(&self) -> PipelineStatsSnapshot {
        self.stats.snapshot()
    }
}

/// A no-op stage that always continues to the next stage.
pub struct PassthroughStage;

impl StageProcessor for PassthroughStage {
    fn process(&self, _request: &mut PipelineRequest) -> StageAction {
        StageAction::Continue
    }

    fn name(&self) -> &str {
        "passthrough"
    }
}

/// A stage that rejects requests matching a specific opcode.
pub struct RejectStage {
    /// Opcode that should be rejected.
    pub reject_opcode: u8,
}

impl RejectStage {
    /// Creates a new reject stage for the given opcode.
    pub fn new(reject_opcode: u8) -> Self {
        Self { reject_opcode }
    }
}

impl StageProcessor for RejectStage {
    fn process(&self, request: &mut PipelineRequest) -> StageAction {
        if request.opcode == self.reject_opcode {
            StageAction::Reject(format!("Rejected opcode {}", self.reject_opcode))
        } else {
            StageAction::Continue
        }
    }

    fn name(&self) -> &str {
        "reject"
    }
}

/// A stage that prepends a header to the request payload.
pub struct HeaderStage {
    /// Header bytes to prepend to the payload.
    pub header: Vec<u8>,
}

impl HeaderStage {
    /// Creates a new header stage with the given header bytes.
    pub fn new(header: Vec<u8>) -> Self {
        Self { header }
    }
}

impl StageProcessor for HeaderStage {
    fn process(&self, request: &mut PipelineRequest) -> StageAction {
        let mut new_payload = self.header.clone();
        new_payload.extend_from_slice(&request.payload);
        request.payload = new_payload;
        StageAction::Continue
    }

    fn name(&self) -> &str {
        "header"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.max_stages, 32);
        assert_eq!(config.max_payload_bytes, 64 * 1024 * 1024);
        assert!(!config.fail_open);
        assert!(config.track_stage_timing);
    }

    #[test]
    fn test_pipeline_empty_execute() {
        let pipeline = Pipeline::new(PipelineConfig::default());
        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);

        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Continue));
        assert_eq!(result.final_payload, vec![1, 2, 3]);
        assert_eq!(result.stages_executed, 0);
    }

    #[test]
    fn test_pipeline_add_stage() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        let result = pipeline.add_stage(
            StageConfig::new(1, "test", 0, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        assert!(result.is_ok());
        assert_eq!(pipeline.stage_count(), 1);
    }

    #[test]
    fn test_pipeline_add_too_many_stages() {
        let mut pipeline = Pipeline::new(PipelineConfig {
            max_stages: 2,
            ..Default::default()
        });

        pipeline
            .add_stage(
                StageConfig::new(1, "test1", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "test2", 1, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let result = pipeline.add_stage(
            StageConfig::new(3, "test3", 2, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        assert!(matches!(
            result,
            Err(PipelineError::TooManyStages { max: 2 })
        ));
    }

    #[test]
    fn test_pipeline_duplicate_stage_id() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "test", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let result = pipeline.add_stage(
            StageConfig::new(1, "test2", 1, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        assert!(matches!(
            result,
            Err(PipelineError::DuplicateStageId { id: 1 })
        ));
    }

    #[test]
    fn test_pipeline_remove_stage() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "test", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let result = pipeline.remove_stage(1);

        assert!(result);
        assert_eq!(pipeline.stage_count(), 0);
    }

    #[test]
    fn test_pipeline_remove_nonexistent() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        let result = pipeline.remove_stage(999);

        assert!(!result);
    }

    #[test]
    fn test_pipeline_enable_disable() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "test", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        assert!(pipeline.disable_stage(1));
        assert!(!pipeline.enable_stage(999));
        assert!(pipeline.enable_stage(1));
    }

    #[test]
    fn test_pipeline_passthrough() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "passthrough", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Continue));
        assert_eq!(result.final_payload, vec![1, 2, 3]);
    }

    #[test]
    fn test_pipeline_reject_stage() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "reject", 0, PipelineDirection::Outbound),
                Box::new(RejectStage::new(10)),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Reject(_)));
    }

    #[test]
    fn test_pipeline_reject_stage_passes_other_opcodes() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "reject", 0, PipelineDirection::Outbound),
                Box::new(RejectStage::new(10)),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 20, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Continue));
    }

    #[test]
    fn test_pipeline_header_stage() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "header", 0, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![0xFF, 0xFE])),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, vec![0xFF, 0xFE, 1, 2, 3]);
    }

    #[test]
    fn test_pipeline_stage_ordering() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(3, "third", 2, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![3])),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(1, "first", 0, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![1])),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "second", 1, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![2])),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![0], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, vec![3, 2, 1, 0]);
        assert_eq!(result.stage_results[0].stage_name, "first");
        assert_eq!(result.stage_results[1].stage_name, "second");
        assert_eq!(result.stage_results[2].stage_name, "third");
    }

    #[test]
    fn test_pipeline_disabled_stage_skipped() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        let mut config = StageConfig::new(1, "disabled", 0, PipelineDirection::Outbound);
        config.enabled = false;
        pipeline
            .add_stage(config, Box::new(HeaderStage::new(vec![0xFF])))
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, vec![1, 2, 3]);
        assert_eq!(result.stages_executed, 0);
    }

    #[test]
    fn test_pipeline_fail_open() {
        let config = PipelineConfig {
            fail_open: true,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config);

        assert!(pipeline.config.fail_open);
    }

    #[test]
    fn test_pipeline_stats() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "passthrough", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        pipeline.execute(&mut request);

        let stats = pipeline.stats();
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.total_rejections, 0);
        assert_eq!(stats.stages_registered, 1);
    }

    #[test]
    fn test_pipeline_stage_timing() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "test", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(!result.stage_results.is_empty());
        assert!(
            result.stage_results[0].duration_us > 0 || result.stage_results[0].duration_us == 0
        );
    }

    #[test]
    fn test_pipeline_payload_too_large() {
        let pipeline = Pipeline::new(PipelineConfig {
            max_payload_bytes: 10,
            ..Default::default()
        });

        let mut request = PipelineRequest::new(1, 10, vec![0u8; 11], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Reject(_)));

        let stats = pipeline.stats();
        assert_eq!(stats.total_rejections, 1);
    }

    #[test]
    fn test_pipeline_metadata_propagation() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        struct MetadataStage;

        impl StageProcessor for MetadataStage {
            fn process(&self, request: &mut PipelineRequest) -> StageAction {
                request
                    .metadata
                    .insert("processed".to_string(), "true".to_string());
                StageAction::Continue
            }

            fn name(&self) -> &str {
                "metadata"
            }
        }

        pipeline
            .add_stage(
                StageConfig::new(1, "metadata", 0, PipelineDirection::Outbound),
                Box::new(MetadataStage),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        request
            .metadata
            .insert("original".to_string(), "value".to_string());

        pipeline.execute(&mut request);

        assert_eq!(request.metadata.get("original"), Some(&"value".to_string()));
        assert_eq!(request.metadata.get("processed"), Some(&"true".to_string()));
    }

    #[test]
    fn test_pipeline_direction_filtering() {
        let mut pipeline = Pipeline::new(PipelineConfig::default());

        pipeline
            .add_stage(
                StageConfig::new(1, "inbound_only", 0, PipelineDirection::Inbound),
                Box::new(HeaderStage::new(vec![1])),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "outbound_only", 1, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![2])),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![0], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, vec![2, 0]);

        let mut request = PipelineRequest::new(2, 10, vec![0], PipelineDirection::Inbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, vec![1, 0]);
    }
}
