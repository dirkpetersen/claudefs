//! Replication infrastructure security tests: audit, uidmap, backpressure, lag monitor.
//!
//! Part of A10 Phase 12: Replication infrastructure security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::backpressure::{
        BackpressureConfig, BackpressureController, BackpressureLevel, BackpressureManager,
    };
    use claudefs_repl::conflict_resolver::SiteId;
    use claudefs_repl::lag_monitor::{LagMonitor, LagSla, LagStats, LagStatus};
    use claudefs_repl::repl_audit::{AuditEvent, AuditEventKind, AuditFilter, AuditLog};
    use claudefs_repl::uidmap::{GidMapping, UidMapper, UidMapping};

    fn ts(n: u64) -> u64 {
        n * 1_000_000_000
    }

    fn site_id(n: u64) -> SiteId {
        SiteId(n)
    }

    // Category 1: Audit Trail Security (6 tests)

    #[test]
    fn test_audit_log_record_and_count() {
        let mut log = AuditLog::new();
        assert_eq!(log.event_count(), 0);

        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(1),
            "test1",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            site_id(1),
            ts(2),
            "test2",
            None,
        );
        log.record(
            AuditEventKind::ConflictDetected,
            site_id(1),
            ts(3),
            "test3",
            None,
        );

        assert_eq!(log.event_count(), 3);
    }

    #[test]
    fn test_audit_log_query_by_kind() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(1),
            "started1",
            None,
        );
        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(2),
            "started2",
            None,
        );
        log.record(
            AuditEventKind::ConflictDetected,
            site_id(1),
            ts(3),
            "conflict1",
            None,
        );

        let filter = AuditFilter {
            kind: Some(AuditEventKind::ConflictDetected),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, AuditEventKind::ConflictDetected);
    }

    #[test]
    fn test_audit_log_query_by_time_range() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(100),
            "event100",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            site_id(1),
            ts(200),
            "event200",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            site_id(1),
            ts(300),
            "event300",
            None,
        );

        let filter = AuditFilter {
            since_ns: Some(ts(150)),
            until_ns: Some(ts(250)),
            ..Default::default()
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp_ns, ts(200));
    }

    #[test]
    fn test_audit_log_events_for_site() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(1),
            "site1_event1",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            site_id(2),
            ts(2),
            "site2_event1",
            None,
        );
        log.record(
            AuditEventKind::ConflictDetected,
            site_id(1),
            ts(3),
            "site1_event2",
            None,
        );

        let site1_events = log.events_for_site(site_id(1));
        assert_eq!(site1_events.len(), 2);
    }

    #[test]
    fn test_audit_log_latest_n() {
        let mut log = AuditLog::new();
        for i in 0..10 {
            log.record(
                AuditEventKind::ReplicationStarted,
                site_id(1),
                ts(i),
                format!("event_{}", i),
                None,
            );
        }

        let results = log.latest_n(3);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].details, "event_7");
        assert_eq!(results[1].details, "event_8");
        assert_eq!(results[2].details, "event_9");
    }

    #[test]
    fn test_audit_log_clear_before() {
        let mut log = AuditLog::new();
        log.record(
            AuditEventKind::ReplicationStarted,
            site_id(1),
            ts(100),
            "event100",
            None,
        );
        log.record(
            AuditEventKind::ReplicationCompleted,
            site_id(1),
            ts(200),
            "event200",
            None,
        );
        log.record(
            AuditEventKind::ReplicationFailed,
            site_id(1),
            ts(300),
            "event300",
            None,
        );

        log.clear_before(ts(250));
        assert_eq!(log.event_count(), 1);
        let events = log.latest_n(1);
        assert_eq!(events[0].timestamp_ns, ts(300));
    }

    // Category 2: UID/GID Translation Security (6 tests)

    #[test]
    fn test_uidmap_passthrough_mode() {
        let mapper = UidMapper::passthrough();
        assert!(mapper.is_passthrough());
        assert_eq!(mapper.translate_uid(1, 1000), 1000);
        assert_eq!(mapper.translate_gid(1, 1000), 1000);
    }

    #[test]
    fn test_uidmap_explicit_mapping() {
        let mapper = UidMapper::new(
            vec![UidMapping {
                source_site_id: 1,
                source_uid: 1000,
                dest_uid: 2000,
            }],
            vec![],
        );

        assert_eq!(mapper.translate_uid(1, 1000), 2000);
        assert_eq!(mapper.translate_uid(1, 999), 999);
    }

    #[test]
    fn test_uidmap_gid_mapping() {
        let mapper = UidMapper::new(
            vec![],
            vec![GidMapping {
                source_site_id: 1,
                source_gid: 100,
                dest_gid: 200,
            }],
        );

        assert_eq!(mapper.translate_gid(1, 100), 200);
        assert_eq!(mapper.translate_gid(2, 100), 100);
    }

    #[test]
    fn test_uidmap_add_remove_mapping() {
        let mut mapper = UidMapper::passthrough();

        mapper.add_uid_mapping(UidMapping {
            source_site_id: 1,
            source_uid: 1000,
            dest_uid: 2000,
        });
        assert_eq!(mapper.translate_uid(1, 1000), 2000);

        mapper.remove_uid_mapping(1, 1000);
        assert_eq!(mapper.translate_uid(1, 1000), 1000);
    }

    #[test]
    fn test_uidmap_root_uid_zero() {
        let mapper = UidMapper::new(
            vec![UidMapping {
                source_site_id: 1,
                source_uid: 0,
                dest_uid: 65534,
            }],
            vec![],
        );

        assert_eq!(mapper.translate_uid(1, 0), 65534);
    }

    #[test]
    fn test_uidmap_listing() {
        let mapper = UidMapper::new(
            vec![
                UidMapping {
                    source_site_id: 1,
                    source_uid: 100,
                    dest_uid: 200,
                },
                UidMapping {
                    source_site_id: 1,
                    source_uid: 300,
                    dest_uid: 400,
                },
                UidMapping {
                    source_site_id: 2,
                    source_uid: 500,
                    dest_uid: 600,
                },
            ],
            vec![
                GidMapping {
                    source_site_id: 1,
                    source_gid: 1000,
                    dest_gid: 2000,
                },
                GidMapping {
                    source_site_id: 2,
                    source_gid: 3000,
                    dest_gid: 4000,
                },
            ],
        );

        assert_eq!(mapper.uid_mappings().len(), 3);
        assert_eq!(mapper.gid_mappings().len(), 2);
    }

    // Category 3: Backpressure Throttling (7 tests)

    #[test]
    fn test_backpressure_level_ordering() {
        assert!(BackpressureLevel::None < BackpressureLevel::Mild);
        assert!(BackpressureLevel::Mild < BackpressureLevel::Moderate);
        assert!(BackpressureLevel::Moderate < BackpressureLevel::Severe);
        assert!(BackpressureLevel::Severe < BackpressureLevel::Halt);

        assert!(!BackpressureLevel::None.is_active());
        assert!(BackpressureLevel::Mild.is_active());
        assert!(BackpressureLevel::Moderate.is_active());
        assert!(BackpressureLevel::Severe.is_active());
        assert!(BackpressureLevel::Halt.is_active());

        assert!(!BackpressureLevel::None.is_halted());
        assert!(!BackpressureLevel::Mild.is_halted());
        assert!(!BackpressureLevel::Moderate.is_halted());
        assert!(!BackpressureLevel::Severe.is_halted());
        assert!(BackpressureLevel::Halt.is_halted());
    }

    #[test]
    fn test_backpressure_suggested_delays() {
        assert_eq!(BackpressureLevel::None.suggested_delay_ms(), 0);
        assert_eq!(BackpressureLevel::Mild.suggested_delay_ms(), 5);
        assert_eq!(BackpressureLevel::Moderate.suggested_delay_ms(), 50);
        assert_eq!(BackpressureLevel::Severe.suggested_delay_ms(), 500);
        assert_eq!(BackpressureLevel::Halt.suggested_delay_ms(), u64::MAX);
    }

    #[test]
    fn test_backpressure_controller_queue_depth() {
        let config = BackpressureConfig {
            mild_queue_depth: 1_000,
            moderate_queue_depth: 10_000,
            severe_queue_depth: 100_000,
            halt_queue_depth: 1_000_000,
            ..Default::default()
        };

        let mut controller = BackpressureController::new(config.clone());

        controller.set_queue_depth(500);
        assert_eq!(controller.compute_level(), BackpressureLevel::None);

        controller.set_queue_depth(5000);
        assert_eq!(controller.compute_level(), BackpressureLevel::Mild);

        controller.set_queue_depth(50000);
        assert_eq!(controller.compute_level(), BackpressureLevel::Moderate);

        controller.set_queue_depth(200000);
        assert_eq!(controller.compute_level(), BackpressureLevel::Severe);
    }

    #[test]
    fn test_backpressure_error_escalation() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());

        controller.record_error();
        controller.record_error();
        controller.record_error();
        assert!(controller.compute_level() >= BackpressureLevel::Moderate);

        for _ in 3..10 {
            controller.record_error();
        }
        assert_eq!(controller.compute_level(), BackpressureLevel::Severe);

        controller.record_success();
        assert_eq!(controller.consecutive_errors(), 0);
    }

    #[test]
    fn test_backpressure_force_halt() {
        let mut controller = BackpressureController::new(BackpressureConfig::default());

        controller.force_halt();
        assert!(controller.is_halted());

        controller.clear_halt();
        assert!(!controller.is_halted());
    }

    #[test]
    fn test_backpressure_manager_per_site() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);
        manager.register_site(2);

        manager.set_queue_depth(1, 50000);
        assert_eq!(manager.level(1), Some(BackpressureLevel::Moderate));
        assert_eq!(manager.level(2), Some(BackpressureLevel::None));
    }

    #[test]
    fn test_backpressure_manager_halted_sites() {
        let mut manager = BackpressureManager::new(BackpressureConfig::default());
        manager.register_site(1);
        manager.register_site(2);
        manager.register_site(3);

        manager.force_halt(1);
        manager.force_halt(3);

        let halted = manager.halted_sites();
        assert!(halted.contains(&1));
        assert!(halted.contains(&3));
        assert!(!halted.contains(&2));

        manager.remove_site(1);
        let halted_after = manager.halted_sites();
        assert!(halted_after.contains(&3));
        assert!(!halted_after.contains(&1));
    }

    // Category 4: Lag Monitoring & SLA (6 tests)

    #[test]
    fn test_lag_monitor_ok_status() {
        let mut monitor = LagMonitor::new(LagSla::default());
        let status = monitor.record_sample("site-a".to_string(), 50);
        assert_eq!(status, LagStatus::Ok);
    }

    #[test]
    fn test_lag_monitor_warning_status() {
        let mut monitor = LagMonitor::new(LagSla::default());
        let status = monitor.record_sample("site-a".to_string(), 200);
        assert_eq!(status, LagStatus::Warning { lag_ms: 200 });
    }

    #[test]
    fn test_lag_monitor_critical_status() {
        let mut monitor = LagMonitor::new(LagSla::default());
        let status = monitor.record_sample("site-a".to_string(), 800);
        assert_eq!(status, LagStatus::Critical { lag_ms: 800 });
    }

    #[test]
    fn test_lag_monitor_exceeded_status() {
        let mut monitor = LagMonitor::new(LagSla::default());
        let status = monitor.record_sample("site-a".to_string(), 3000);
        assert_eq!(status, LagStatus::Exceeded { lag_ms: 3000 });
    }

    #[test]
    fn test_lag_monitor_stats_accumulate() {
        let mut monitor = LagMonitor::new(LagSla::default());

        monitor.record_sample("site-a".to_string(), 50);
        monitor.record_sample("site-a".to_string(), 150);
        monitor.record_sample("site-a".to_string(), 250);
        monitor.record_sample("site-a".to_string(), 450);
        monitor.record_sample("site-a".to_string(), 600);

        let stats = monitor.stats();
        assert_eq!(stats.sample_count, 5);

        let expected_avg: f64 = (50.0 + 150.0 + 250.0 + 450.0 + 600.0) / 5.0;
        assert!((stats.avg_lag_ms - expected_avg).abs() < 0.001);

        assert_eq!(stats.max_lag_ms, 600);
        assert_eq!(stats.warning_count, 3);
        assert_eq!(stats.critical_count, 1);
    }

    #[test]
    fn test_lag_monitor_clear_samples() {
        let mut monitor = LagMonitor::new(LagSla::default());

        monitor.record_sample("site-a".to_string(), 50);
        monitor.record_sample("site-b".to_string(), 150);
        monitor.record_sample("site-c".to_string(), 250);

        assert_eq!(monitor.stats().sample_count, 3);

        monitor.clear_samples();

        let stats = monitor.stats();
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.avg_lag_ms, 0.0);
        assert_eq!(stats.max_lag_ms, 0);
    }
}
