//! FUSE prefetch engine and health monitoring security tests.
//!
//! Part of A10 Phase 17: FUSE prefetch & health monitoring security audit

use claudefs_fuse::health::{
    ComponentHealth, HealthChecker, HealthReport, HealthStatus, HealthThresholds,
};
use claudefs_fuse::prefetch::{PrefetchConfig, PrefetchEngine, PrefetchEntry, PrefetchStats};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> PrefetchEngine {
        PrefetchEngine::default()
    }

    fn make_engine_with_config(
        window_size: usize,
        block_size: u64,
        max_inflight: usize,
        detection_threshold: u32,
    ) -> PrefetchEngine {
        PrefetchEngine::new(PrefetchConfig {
            window_size,
            block_size,
            max_inflight,
            detection_threshold,
        })
    }

    // Category 1: Prefetch Sequential Detection (5 tests)

    #[test]
    fn test_prefetch_single_access_not_sequential() {
        let mut engine = make_engine();
        engine.record_access(1, 0, 512);
        assert!(
            !engine.is_sequential(1),
            "FINDING-FUSE-HEALTH-01: single access doesn't trigger prefetch"
        );
    }

    #[test]
    fn test_prefetch_sequential_detected() {
        let mut engine = make_engine_with_config(8, 65536, 4, 2);
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        assert!(
            engine.is_sequential(1),
            "Two sequential accesses should be detected"
        );
    }

    #[test]
    fn test_prefetch_large_gap_resets() {
        let mut engine = make_engine();
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        assert!(engine.is_sequential(1));

        engine.record_access(1, 200000, 512);
        assert!(
            !engine.is_sequential(1),
            "FINDING-FUSE-HEALTH-02: large gap correctly resets pattern detection"
        );
    }

    #[test]
    fn test_prefetch_independent_inodes() {
        let mut engine = make_engine();

        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);

        engine.record_access(2, 100, 512);

        assert!(engine.is_sequential(1), "inode 1 should be sequential");
        assert!(!engine.is_sequential(2), "inode 2 should not be sequential");
    }

    #[test]
    fn test_prefetch_config_defaults() {
        let config = PrefetchConfig::default();
        assert_eq!(config.window_size, 8, "window_size should be 8");
        assert_eq!(config.block_size, 65536, "block_size should be 65536");
        assert_eq!(config.max_inflight, 4, "max_inflight should be 4");
        assert_eq!(
            config.detection_threshold, 2,
            "detection_threshold should be 2"
        );
    }

    // Category 2: Prefetch Cache & Eviction (5 tests)

    #[test]
    fn test_prefetch_store_and_serve() {
        let mut engine = make_engine();
        let data = vec![0u8; 4096];
        engine.store_prefetch(1, 0, data.clone());

        let result = engine.try_serve(1, 0, 4096);
        assert!(result.is_some(), "Should find cached data");
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn test_prefetch_try_serve_miss() {
        let engine = make_engine();
        let result = engine.try_serve(1, 0, 512);
        assert!(result.is_none(), "Should return None for non-cached offset");
    }

    #[test]
    fn test_prefetch_sub_block_serve() {
        let mut engine = make_engine();
        let data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        engine.store_prefetch(1, 0, data);

        let result = engine.try_serve(1, 100, 200);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 200);
        assert_eq!(result[0], 100);
        assert_eq!(result[199], 43);
    }

    #[test]
    fn test_prefetch_evict_removes_inode() {
        let mut engine = make_engine();
        engine.store_prefetch(1, 0, vec![1u8; 4096]);
        engine.store_prefetch(2, 0, vec![2u8; 4096]);

        engine.evict(1);

        assert!(
            engine.try_serve(1, 0, 4096).is_none(),
            "inode 1 should be evicted"
        );
        assert!(
            engine.try_serve(2, 0, 4096).is_some(),
            "FINDING-FUSE-HEALTH-03: eviction is inode-scoped, doesn't affect others"
        );
    }

    #[test]
    fn test_prefetch_stats() {
        let mut engine = make_engine();

        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);

        engine.record_access(2, 0, 512);
        engine.record_access(2, 512, 512);

        engine.record_access(3, 100, 512);

        engine.store_prefetch(1, 0, vec![1u8; 4096]);
        engine.store_prefetch(2, 4096, vec![2u8; 4096]);

        let stats = engine.stats();
        assert_eq!(stats.entries_cached, 2, "entries_cached should be 2");
        assert_eq!(stats.inodes_tracked, 3, "inodes_tracked should be 3");
        assert_eq!(stats.sequential_inodes, 2, "sequential_inodes should be 2");
    }

    // Category 3: Prefetch List Generation (5 tests)

    #[test]
    fn test_prefetch_list_empty_for_non_sequential() {
        let mut engine = make_engine();
        engine.record_access(1, 100, 512);

        let list = engine.compute_prefetch_list(1, 100);
        assert!(list.is_empty(), "Non-sequential should return empty list");
    }

    #[test]
    fn test_prefetch_list_block_aligned() {
        let mut engine = make_engine_with_config(8, 4096, 4, 2);
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);

        let list = engine.compute_prefetch_list(1, 512);

        for (_, offset) in &list {
            assert_eq!(
                *offset % 4096,
                0,
                "Offset {} should be block-aligned",
                offset
            );
        }
    }

    #[test]
    fn test_prefetch_list_excludes_cached() {
        let mut engine = make_engine_with_config(8, 4096, 4, 2);
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);
        engine.record_access(1, 1024, 512);

        engine.store_prefetch(1, 4096, vec![1u8; 4096]);

        let list = engine.compute_prefetch_list(1, 1024);

        assert!(
            !list.iter().any(|(_, o)| *o == 4096),
            "Should exclude cached block"
        );
    }

    #[test]
    fn test_prefetch_list_respects_max_inflight() {
        let mut engine = make_engine_with_config(8, 4096, 2, 2);
        engine.record_access(1, 0, 512);
        engine.record_access(1, 512, 512);

        let list = engine.compute_prefetch_list(1, 512);

        assert!(list.len() <= 2, "Should not exceed max_inflight");
    }

    #[test]
    fn test_prefetch_record_returns_block_aligned() {
        let mut engine = make_engine_with_config(8, 4096, 4, 2);

        let block_offset = engine.record_access(1, 5000, 512);

        assert_eq!(block_offset, 4096, "Should return block-aligned offset");
    }

    // Category 4: Health Monitoring (5 tests)

    #[test]
    fn test_health_status_variants() {
        let healthy = HealthStatus::Healthy;
        assert!(healthy.is_healthy(), "Healthy.is_healthy() should be true");
        assert!(healthy.reason().is_none(), "Healthy reason should be None");

        let degraded = HealthStatus::Degraded {
            reason: "test".into(),
        };
        assert!(
            degraded.is_degraded(),
            "Degraded.is_degraded() should be true"
        );
        assert!(
            degraded.reason().is_some(),
            "Degraded reason should be Some"
        );

        let unhealthy = HealthStatus::Unhealthy {
            reason: "test".into(),
        };
        assert!(
            unhealthy.is_unhealthy(),
            "Unhealthy.is_unhealthy() should be true"
        );
        assert!(
            unhealthy.reason().is_some(),
            "Unhealthy reason should be Some"
        );
    }

    #[test]
    fn test_health_report_all_healthy() {
        let components = vec![
            ComponentHealth::healthy("transport"),
            ComponentHealth::healthy("cache"),
            ComponentHealth::healthy("errors"),
        ];
        let report = HealthReport::new(components);

        assert!(report.overall.is_healthy(), "Overall should be healthy");
        assert_eq!(report.healthy_count(), 3, "healthy_count should be 3");
        assert_eq!(report.degraded_count(), 0, "degraded_count should be 0");
    }

    #[test]
    fn test_health_report_worst_wins() {
        let components = vec![
            ComponentHealth::healthy("transport"),
            ComponentHealth::degraded("cache", "slow"),
            ComponentHealth::unhealthy("errors", "high rate"),
        ];
        let report = HealthReport::new(components);

        assert!(
            report.overall.is_unhealthy(),
            "FINDING-FUSE-HEALTH-04: worst-status aggregation ensures conservative reporting"
        );

        let components2 = vec![
            ComponentHealth::healthy("transport"),
            ComponentHealth::degraded("cache", "slow"),
        ];
        let report2 = HealthReport::new(components2);

        assert!(
            report2.overall.is_degraded(),
            "Degraded should win over Healthy"
        );
    }

    #[test]
    fn test_health_checker_transport() {
        let checker = HealthChecker::with_defaults();

        let connected = checker.check_transport(true);
        assert!(connected.status.is_healthy(), "Connected should be healthy");

        let disconnected = checker.check_transport(false);
        assert!(
            disconnected.status.is_unhealthy(),
            "Disconnected should be unhealthy"
        );
    }

    #[test]
    fn test_health_checker_cache() {
        let checker = HealthChecker::with_defaults();

        let healthy_cache = checker.check_cache(90, 10);
        assert!(
            healthy_cache.status.is_healthy(),
            "90% hit should be healthy"
        );

        let unhealthy_cache = checker.check_cache(5, 100);
        assert!(
            unhealthy_cache.status.is_unhealthy(),
            "<10% hit should be unhealthy"
        );

        let empty_cache = checker.check_cache(0, 0);
        assert!(empty_cache.status.is_healthy(), "No ops should be healthy");
    }

    // Category 5: Health Thresholds & Edge Cases (5 tests)

    #[test]
    fn test_health_thresholds_default() {
        let thresholds = HealthThresholds::default();

        assert!((thresholds.cache_hit_rate_degraded - 0.5).abs() < f64::EPSILON);
        assert!((thresholds.cache_hit_rate_unhealthy - 0.1).abs() < f64::EPSILON);
        assert!((thresholds.error_rate_degraded - 0.01).abs() < f64::EPSILON);
        assert!((thresholds.error_rate_unhealthy - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_health_checker_errors() {
        let checker = HealthChecker::with_defaults();

        let no_errors = checker.check_errors(0, 100);
        assert!(no_errors.status.is_healthy(), "0% error should be healthy");

        let degraded_errors = checker.check_errors(5, 100);
        assert!(
            degraded_errors.status.is_degraded(),
            "FINDING-FUSE-HEALTH-05: 5% error rate should be degraded (>1%)"
        );

        let unhealthy_errors = checker.check_errors(50, 100);
        assert!(
            unhealthy_errors.status.is_unhealthy(),
            "50% error rate should be unhealthy (>10%)"
        );
    }

    #[test]
    fn test_health_report_component_lookup() {
        let components = vec![
            ComponentHealth::healthy("transport"),
            ComponentHealth::healthy("cache"),
            ComponentHealth::healthy("errors"),
        ];
        let report = HealthReport::new(components);

        assert!(
            report.component("transport").is_some(),
            "Should find transport component"
        );
        assert!(
            report.component("nonexistent").is_none(),
            "Should not find nonexistent component"
        );
    }

    #[test]
    fn test_health_checker_count() {
        let mut checker = HealthChecker::with_defaults();

        checker.build_report(vec![ComponentHealth::healthy("test")]);
        checker.build_report(vec![ComponentHealth::healthy("test")]);
        checker.build_report(vec![ComponentHealth::healthy("test")]);

        assert_eq!(checker.check_count(), 3, "check_count should be 3");
    }

    #[test]
    fn test_health_report_empty_components() {
        let report = HealthReport::new(vec![]);

        assert!(
            report.overall.is_healthy(),
            "No components should be healthy"
        );
        assert_eq!(report.healthy_count(), 0, "healthy_count should be 0");
    }
}
