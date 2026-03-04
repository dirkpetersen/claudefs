//! Transport pipeline/congestion/circuit breaker security tests.
//!
//! Part of A10 Phase 14: Transport pipeline security audit

#[cfg(test)]
mod tests {
    use claudefs_transport::circuitbreaker::{
        CircuitBreaker as TransCircuitBreaker, CircuitBreakerConfig as TransCBConfig,
        CircuitState as TransCircuitState, DEFAULT_FAILURE_THRESHOLD, DEFAULT_OPEN_DURATION_MS,
        DEFAULT_SUCCESS_THRESHOLD,
    };
    use claudefs_transport::congestion::{
        CongestionAlgorithm, CongestionConfig, CongestionState, CongestionWindow,
    };
    use claudefs_transport::pipeline::{
        HeaderStage, PassthroughStage, Pipeline, PipelineConfig, PipelineDirection, PipelineError,
        PipelineRequest, RejectStage, StageAction, StageConfig, StageId,
    };

    fn make_congestion_window() -> CongestionWindow {
        CongestionWindow::new(CongestionConfig::default())
    }

    fn make_circuit_breaker() -> TransCircuitBreaker {
        TransCircuitBreaker::default()
    }

    fn make_circuit_breaker_with_open_duration(open_duration_ms: u64) -> TransCircuitBreaker {
        TransCircuitBreaker::new(TransCBConfig {
            open_duration: std::time::Duration::from_millis(open_duration_ms),
            ..Default::default()
        })
    }

    fn make_pipeline() -> Pipeline {
        Pipeline::new(PipelineConfig::default())
    }

    fn make_pipeline_with_max_stages(max: usize) -> Pipeline {
        Pipeline::new(PipelineConfig {
            max_stages: max,
            ..Default::default()
        })
    }

    // ============================================================================
    // Category 1: Congestion Window Control (5 tests)
    // ============================================================================

    #[test]
    fn test_congestion_initial_slow_start() {
        let mut window = make_congestion_window();

        assert_eq!(*window.state(), CongestionState::SlowStart);
        assert_eq!(window.window_size(), 0);

        // After first send, window should be initialized to initial_window
        window.on_send(1000);
        assert_eq!(
            window.window_size(),
            CongestionConfig::default().initial_window
        );
    }

    #[test]
    fn test_congestion_window_growth_on_ack() {
        let mut window = make_congestion_window();

        window.on_send(1000);
        let window_before = window.window_size();

        window.on_ack(1000, 10000);

        let window_after = window.window_size();
        assert!(window_after > window_before);

        window.on_send(1000);
        window.on_send(1000);
        window.on_ack(2000, 10000);

        let window_final = window.window_size();
        assert!(window_final > window_after);
    }

    #[test]
    fn test_congestion_loss_reduces_window() {
        let mut window = make_congestion_window();

        window.on_send(10000);
        window.on_ack(10000, 1000);

        let window_before_loss = window.window_size();

        window.on_loss(5000);

        let window_after_loss = window.window_size();
        assert!(window_after_loss < window_before_loss);

        let state = window.state();
        assert!(
            matches!(
                state,
                CongestionState::Recovery | CongestionState::CongestionAvoidance
            ),
            "Expected Recovery or CongestionAvoidance after loss, got {:?}",
            state
        );
    }

    #[test]
    fn test_congestion_min_window_floor() {
        let mut window = make_congestion_window();

        let min_window = CongestionConfig::default().min_window;

        for _ in 0..20 {
            window.on_send(10000);
            window.on_ack(10000, 1000);
            window.on_loss(10000);
        }

        // FINDING-TRANS-PIPE-01: Min window prevents complete stall
        // Verify window never goes below min_window
        assert!(window.window_size() >= min_window);
    }

    #[test]
    fn test_congestion_stats_tracking() {
        let mut window = make_congestion_window();

        window.on_send(1000);
        window.on_ack(800, 1000);

        let stats = window.stats();
        assert_eq!(stats.total_sent, 1000);
        assert_eq!(stats.total_acked, 800);

        window.on_loss(150);

        let stats = window.stats();
        assert_eq!(stats.total_lost, 150);
        assert_eq!(stats.loss_events, 1);
    }

    // ============================================================================
    // Category 2: Transport Circuit Breaker (5 tests)
    // ============================================================================

    #[test]
    fn test_trans_circuit_breaker_defaults() {
        let breaker = make_circuit_breaker();

        assert_eq!(breaker.state(), TransCircuitState::Closed);
        assert_eq!(breaker.failure_count(), 0);
        assert!(breaker.can_execute());
    }

    #[test]
    fn test_trans_circuit_breaker_opens_on_failures() {
        let breaker = make_circuit_breaker();

        for _ in 0..4 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), TransCircuitState::Closed);

        breaker.record_failure();

        assert_eq!(breaker.state(), TransCircuitState::Open);
        assert!(!breaker.can_execute());
    }

    #[test]
    fn test_trans_circuit_breaker_half_open() {
        let mut config = TransCBConfig::default();
        config.open_duration = std::time::Duration::from_millis(50);
        let breaker = TransCircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), TransCircuitState::Open);

        std::thread::sleep(std::time::Duration::from_millis(60));

        let can_exec = breaker.can_execute();

        // FINDING-TRANS-PIPE-02: Half-open state transition
        // Document half-open behavior after open_duration elapses
        if can_exec {
            let state = breaker.state();
            println!(
                "FINDING-TRANS-PIPE-02: Circuit transitioned to {:?} after duration",
                state
            );
        }

        assert!(can_exec);
    }

    #[test]
    fn test_trans_circuit_breaker_reset() {
        let breaker = make_circuit_breaker();

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), TransCircuitState::Open);

        breaker.reset();

        assert_eq!(breaker.state(), TransCircuitState::Closed);
        assert_eq!(breaker.failure_count(), 0);
        assert!(breaker.can_execute());
    }

    #[test]
    fn test_trans_circuit_breaker_success_recovers() {
        let mut config = TransCBConfig::default();
        config.open_duration = std::time::Duration::from_millis(50);
        let breaker = TransCircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), TransCircuitState::Open);

        std::thread::sleep(std::time::Duration::from_millis(60));
        breaker.can_execute();

        for _ in 0..3 {
            breaker.record_success();
        }

        assert_eq!(breaker.state(), TransCircuitState::Closed);
    }

    // ============================================================================
    // Category 3: Pipeline Stage Management (5 tests)
    // ============================================================================

    #[test]
    fn test_pipeline_add_passthrough_stage() {
        let mut pipeline = make_pipeline();

        let result = pipeline.add_stage(
            StageConfig::new(1, "passthrough", 0, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        assert!(result.is_ok());
        assert_eq!(pipeline.stage_count(), 1);

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Continue));
    }

    #[test]
    fn test_pipeline_reject_stage() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "reject", 0, PipelineDirection::Outbound),
                Box::new(RejectStage::new(42)),
            )
            .unwrap();

        // Request with matching opcode should be rejected
        let mut request = PipelineRequest::new(1, 42, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);
        assert!(matches!(result.final_action, StageAction::Reject(_)));

        // Request with different opcode should continue
        let mut request = PipelineRequest::new(2, 1, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);
        assert!(matches!(result.final_action, StageAction::Continue));
    }

    #[test]
    fn test_pipeline_max_stages_limit() {
        let mut pipeline = make_pipeline_with_max_stages(2);

        pipeline
            .add_stage(
                StageConfig::new(1, "stage1", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "stage2", 1, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let result = pipeline.add_stage(
            StageConfig::new(3, "stage3", 2, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        // FINDING-TRANS-PIPE-03: Stage limit prevents unbounded pipeline growth
        // Verify TooManyStages error when limit exceeded
        assert!(matches!(
            result,
            Err(PipelineError::TooManyStages { max: 2 })
        ));
    }

    #[test]
    fn test_pipeline_duplicate_stage_id() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "stage1", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        let result = pipeline.add_stage(
            StageConfig::new(1, "stage2", 1, PipelineDirection::Outbound),
            Box::new(PassthroughStage),
        );

        assert!(matches!(
            result,
            Err(PipelineError::DuplicateStageId { id: 1 })
        ));
    }

    #[test]
    fn test_pipeline_enable_disable() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "stage1", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        pipeline.disable_stage(1);

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.stages_executed, 0);

        pipeline.enable_stage(1);

        let mut request = PipelineRequest::new(2, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.stages_executed, 1);
    }

    // ============================================================================
    // Category 4: Pipeline Execution & Stats (5 tests)
    // ============================================================================

    #[test]
    fn test_pipeline_execution_order() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(10, "stage10", 10, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![10])),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(5, "stage5", 5, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![5])),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(20, "stage20", 20, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(vec![20])),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![0], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        // Stages should execute in order: 5, 10, 20 (by order field)
        assert_eq!(result.stage_results.len(), 3);
        assert_eq!(result.stage_results[0].stage_id, 5);
        assert_eq!(result.stage_results[1].stage_id, 10);
        assert_eq!(result.stage_results[2].stage_id, 20);
    }

    #[test]
    fn test_pipeline_header_stage() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "header", 0, PipelineDirection::Outbound),
                Box::new(HeaderStage::new(b"PREFIX".to_vec())),
            )
            .unwrap();

        let mut request =
            PipelineRequest::new(1, 10, b"data".to_vec(), PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.final_payload, b"PREFIXdata".to_vec());
    }

    #[test]
    fn test_pipeline_stats_tracking() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "passthrough", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "reject", 1, PipelineDirection::Outbound),
                Box::new(RejectStage::new(99)),
            )
            .unwrap();

        // 2 pass-through requests
        let mut _req1 = PipelineRequest::new(1, 10, vec![1], PipelineDirection::Outbound);
        pipeline.execute(&mut _req1);

        let mut _req2 = PipelineRequest::new(2, 20, vec![2], PipelineDirection::Outbound);
        pipeline.execute(&mut _req2);

        // 1 rejected request
        let mut _req3 = PipelineRequest::new(3, 99, vec![3], PipelineDirection::Outbound);
        pipeline.execute(&mut _req3);

        let stats = pipeline.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_rejections, 1);
    }

    #[test]
    fn test_pipeline_remove_stage() {
        let mut pipeline = make_pipeline();

        pipeline
            .add_stage(
                StageConfig::new(1, "stage1", 0, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        pipeline
            .add_stage(
                StageConfig::new(2, "stage2", 1, PipelineDirection::Outbound),
                Box::new(PassthroughStage),
            )
            .unwrap();

        assert_eq!(pipeline.stage_count(), 2);

        pipeline.remove_stage(1);

        assert_eq!(pipeline.stage_count(), 1);

        let mut request = PipelineRequest::new(1, 10, vec![1], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert_eq!(result.stages_executed, 1);
        assert_eq!(result.stage_results[0].stage_id, 2);
    }

    #[test]
    fn test_pipeline_request_metadata() {
        let mut pipeline = make_pipeline();

        struct MetadataReaderStage;

        impl claudefs_transport::pipeline::StageProcessor for MetadataReaderStage {
            fn process(&self, request: &mut PipelineRequest) -> StageAction {
                if let Some(value) = request.metadata.get("test_key") {
                    if value == "test_value" {
                        request
                            .metadata
                            .insert("found".to_string(), "true".to_string());
                    }
                }
                StageAction::Continue
            }

            fn name(&self) -> &str {
                "metadata_reader"
            }
        }

        pipeline
            .add_stage(
                StageConfig::new(1, "metadata_reader", 0, PipelineDirection::Outbound),
                Box::new(MetadataReaderStage),
            )
            .unwrap();

        let mut request = PipelineRequest::new(1, 10, vec![1], PipelineDirection::Outbound);
        request
            .metadata
            .insert("test_key".to_string(), "test_value".to_string());

        pipeline.execute(&mut request);

        assert_eq!(request.metadata.get("found"), Some(&"true".to_string()));
    }

    // ============================================================================
    // Category 5: Config Defaults & Edge Cases (5 tests)
    // ============================================================================

    #[test]
    fn test_congestion_config_defaults() {
        let config = CongestionConfig::default();

        assert_eq!(config.initial_window, 65536);
        assert_eq!(config.min_window, 4096);
        assert_eq!(config.max_window, 16 * 1024 * 1024);
        assert!(config.aimd_decrease_factor > 0.0 && config.aimd_decrease_factor < 1.0);
    }

    #[test]
    fn test_pipeline_config_defaults() {
        let config = PipelineConfig::default();

        assert!(config.max_stages > 0);
        assert!(config.max_payload_bytes > 0);

        // FINDING-TRANS-PIPE-04: Document fail_open and track_stage_timing defaults
        println!(
            "FINDING-TRANS-PIPE-04: PipelineConfig defaults: fail_open={}, track_stage_timing={}",
            config.fail_open, config.track_stage_timing
        );
    }

    #[test]
    fn test_trans_cb_config_defaults() {
        assert_eq!(DEFAULT_FAILURE_THRESHOLD, 5);
        assert_eq!(DEFAULT_SUCCESS_THRESHOLD, 3);
        assert_eq!(DEFAULT_OPEN_DURATION_MS, 30000);
    }

    #[test]
    fn test_pipeline_empty_execute() {
        let pipeline = make_pipeline();

        let mut request = PipelineRequest::new(1, 10, vec![1, 2, 3], PipelineDirection::Outbound);
        let result = pipeline.execute(&mut request);

        assert!(matches!(result.final_action, StageAction::Continue));
        assert_eq!(result.final_payload, vec![1, 2, 3]);
        assert_eq!(result.stages_executed, 0);
    }

    #[test]
    fn test_congestion_can_send() {
        let mut window = make_congestion_window();

        window.on_send(1000);

        assert!(window.can_send(1000));

        window.on_send(65536);

        assert!(!window.can_send(1));
        assert_eq!(window.available_window(), 0);
    }
}
