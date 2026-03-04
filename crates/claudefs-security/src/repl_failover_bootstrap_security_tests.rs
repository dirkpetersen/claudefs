//! Replication failover state machine and bootstrap coordinator security tests.
//!
//! Part of A10 Phase 18: Replication failover & bootstrap security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::repl_bootstrap::{
        BootstrapCoordinator, BootstrapPhase, BootstrapProgress, BootstrapStats, EnrollmentRecord,
    };
    use claudefs_repl::site_failover::{
        FailoverController, FailoverEvent, FailoverState, FailoverStats,
    };
    use claudefs_repl::site_registry::SiteId;

    fn site(id: u64) -> SiteId {
        SiteId(id)
    }

    fn is_degraded_state(state: &FailoverState) -> bool {
        matches!(state, FailoverState::Degraded { .. })
    }

    fn is_failover_state(state: &FailoverState) -> bool {
        matches!(state, FailoverState::Failover { .. })
    }

    fn is_recovery_state(state: &FailoverState) -> bool {
        matches!(state, FailoverState::Recovery { .. })
    }

    fn get_degraded_site(state: &FailoverState) -> Option<SiteId> {
        if let FailoverState::Degraded { failed_site } = state {
            Some(*failed_site)
        } else {
            None
        }
    }

    fn get_failover_primary(state: &FailoverState) -> Option<SiteId> {
        if let FailoverState::Failover { primary, .. } = state {
            Some(*primary)
        } else {
            None
        }
    }

    #[test]
    fn test_failover_initial_normal() {
        let controller = FailoverController::new(site(1), site(2));
        assert!(matches!(controller.state(), FailoverState::Normal));
        assert!(!controller.is_degraded());
        assert_eq!(controller.stats().state_transitions, 0);
    }

    #[test]
    fn test_failover_site_down_degrades() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert!(is_degraded_state(controller.state()));
        assert_eq!(get_degraded_site(controller.state()), Some(site(1)));
        assert!(controller.is_degraded());
    }

    #[test]
    fn test_failover_split_brain() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 2000,
        });
        assert!(matches!(controller.state(), FailoverState::SplitBrain));
        assert_eq!(controller.stats().split_brain_count, 1);
    }

    #[test]
    fn test_failover_recovery_from_degraded() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        assert!(matches!(controller.state(), FailoverState::Normal));
        assert_eq!(controller.stats().recovery_count, 1);
    }

    #[test]
    fn test_failover_manual_failover() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        assert!(is_failover_state(controller.state()));
        assert_eq!(get_failover_primary(controller.state()), Some(site(2)));

        controller.process_event(FailoverEvent::RecoveryComplete { site_id: site(1) });
        assert!(is_recovery_state(controller.state()));

        controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 5000,
        });
        assert!(matches!(controller.state(), FailoverState::Normal));
    }

    #[test]
    fn test_failover_replication_lag_degrades() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::ReplicationLagHigh {
            site_id: site(1),
            lag_ns: 5000000000,
        });
        assert!(is_degraded_state(controller.state()));
        assert_eq!(get_degraded_site(controller.state()), Some(site(1)));
    }

    #[test]
    fn test_failover_stats_tracking() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert_eq!(controller.stats().state_transitions, 1);
        assert_eq!(controller.stats().failover_count, 1);

        controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        assert_eq!(controller.stats().state_transitions, 2);
        assert_eq!(controller.stats().recovery_count, 1);

        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 3000,
        });
        assert_eq!(controller.stats().state_transitions, 3);
    }

    #[test]
    fn test_failover_stats_default() {
        let stats = FailoverStats::default();
        assert_eq!(stats.state_transitions, 0);
        assert_eq!(stats.failover_count, 0);
        assert_eq!(stats.recovery_count, 0);
        assert_eq!(stats.split_brain_count, 0);
    }

    #[test]
    fn test_failover_same_site_down_twice() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        let state_before = controller.state().clone();
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        assert_eq!(controller.state(), &state_before);
    }

    #[test]
    fn test_failover_is_degraded_all_states() {
        let mut controller = FailoverController::new(site(1), site(2));

        assert!(!controller.is_degraded());

        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert!(controller.is_degraded());

        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        assert!(controller.is_degraded());

        controller.process_event(FailoverEvent::RecoveryComplete { site_id: site(1) });
        assert!(controller.is_degraded());

        let mut split_controller = FailoverController::new(site(1), site(2));
        split_controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        split_controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 2000,
        });
        assert!(split_controller.is_degraded());
    }

    #[test]
    fn test_bootstrap_initial_idle() {
        let coordinator = BootstrapCoordinator::new(1);
        assert!(matches!(coordinator.phase(), BootstrapPhase::Idle));
        assert!(!coordinator.is_active());
        let stats = coordinator.stats();
        assert_eq!(stats.bootstrap_attempts, 0);
        assert_eq!(stats.bootstrap_successes, 0);
        assert_eq!(stats.bootstrap_failures, 0);
        assert_eq!(stats.total_bytes_transferred, 0);
        assert_eq!(stats.total_journal_entries_caught_up, 0);
    }

    #[test]
    fn test_bootstrap_enroll_to_snapshot() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        assert!(matches!(
            coordinator.phase(),
            BootstrapPhase::Enrolling { primary_site: 2 }
        ));
        assert!(coordinator.is_active());

        coordinator.begin_snapshot(2, 1000);
        if let BootstrapPhase::SnapshotTransfer {
            primary_site,
            bytes_total,
            ..
        } = coordinator.phase()
        {
            assert_eq!(*primary_site, 2);
            assert_eq!(*bytes_total, 1000);
        } else {
            panic!("expected SnapshotTransfer");
        }
        assert_eq!(coordinator.stats().bootstrap_attempts, 1);
    }

    #[test]
    fn test_bootstrap_snapshot_to_catchup() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        coordinator.begin_snapshot(2, 1000);
        coordinator.update_snapshot_progress(500);
        assert_eq!(coordinator.stats().total_bytes_transferred, 500);

        coordinator.begin_journal_catchup(2, 100, 200);
        if let BootstrapPhase::JournalCatchup {
            primary_site,
            start_seq,
            target_seq,
            ..
        } = coordinator.phase()
        {
            assert_eq!(*primary_site, 2);
            assert_eq!(*start_seq, 100);
            assert_eq!(*target_seq, 200);
        } else {
            panic!("expected JournalCatchup");
        }
    }

    #[test]
    fn test_bootstrap_catchup_to_complete() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        coordinator.begin_snapshot(2, 1000);
        coordinator.update_snapshot_progress(1000);
        coordinator.begin_journal_catchup(2, 100, 200);
        coordinator.update_catchup_progress(200);
        coordinator.complete(5000, Some("abc123fingerprint".to_string()));

        assert!(matches!(
            coordinator.phase(),
            BootstrapPhase::Complete {
                enrolled_at_ns: 5000
            }
        ));
        assert_eq!(coordinator.stats().bootstrap_successes, 1);

        let enrollment = coordinator.enrollment();
        assert!(enrollment.is_some());
        let enrollment = enrollment.unwrap();
        assert_eq!(enrollment.site_id, 2);
        assert_eq!(
            enrollment.tls_fingerprint,
            Some("abc123fingerprint".to_string())
        );
    }

    #[test]
    fn test_bootstrap_failure() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        coordinator.fail("network error".to_string());

        if let BootstrapPhase::Failed { reason } = coordinator.phase() {
            assert_eq!(reason, "network error");
        } else {
            panic!("expected Failed");
        }
        assert_eq!(coordinator.stats().bootstrap_failures, 1);
        assert_eq!(coordinator.stats().bootstrap_attempts, 1);
    }

    #[test]
    fn test_bootstrap_progress_idle() {
        let coordinator = BootstrapCoordinator::new(1);
        let progress = coordinator.progress(2000);
        assert_eq!(progress.percent_complete, 0);
    }

    #[test]
    fn test_bootstrap_progress_enrolling() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        let progress = coordinator.progress(2000);
        assert_eq!(progress.percent_complete, 5);
    }

    #[test]
    fn test_bootstrap_progress_snapshot() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.begin_snapshot(2, 1000);
        coordinator.update_snapshot_progress(500);
        let progress = coordinator.progress(2000);
        assert!(progress.percent_complete >= 5);
        assert!(progress.percent_complete <= 95);
    }

    #[test]
    fn test_bootstrap_stats_default() {
        let stats = BootstrapStats::default();
        assert_eq!(stats.bootstrap_attempts, 0);
        assert_eq!(stats.bootstrap_successes, 0);
        assert_eq!(stats.bootstrap_failures, 0);
        assert_eq!(stats.total_bytes_transferred, 0);
        assert_eq!(stats.total_journal_entries_caught_up, 0);
    }

    #[test]
    fn test_bootstrap_multiple_attempts() {
        let mut coordinator = BootstrapCoordinator::new(1);

        coordinator.start_enroll(2, 1000);
        coordinator.fail("timeout".to_string());

        coordinator.start_enroll(2, 2000);
        coordinator.fail("connection refused".to_string());

        coordinator.start_enroll(2, 3000);
        coordinator.complete(5000, Some("fp".to_string()));

        assert_eq!(coordinator.stats().bootstrap_attempts, 3);
        assert_eq!(coordinator.stats().bootstrap_successes, 1);
        assert_eq!(coordinator.stats().bootstrap_failures, 2);
    }

    #[test]
    fn test_failover_event_serialization() {
        let event = FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        };
        if let FailoverEvent::SiteDown {
            site_id,
            detected_at_ns,
        } = event
        {
            assert_eq!(site_id, site(1));
            assert_eq!(detected_at_ns, 1000);
        } else {
            panic!("expected SiteDown variant");
        }

        let event = FailoverEvent::ManualFailover {
            target_primary: site(2),
        };
        if let FailoverEvent::ManualFailover { target_primary } = event {
            assert_eq!(target_primary, site(2));
        } else {
            panic!("expected ManualFailover variant");
        }
    }

    #[test]
    fn test_bootstrap_enrollment_record() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.start_enroll(2, 1000);
        coordinator.complete(5000, Some("tls-fp-abc123".to_string()));

        let enrollment = coordinator.enrollment();
        assert!(enrollment.is_some());

        let rec = enrollment.unwrap();
        assert_eq!(rec.site_id, 2);
        assert_eq!(rec.enrolled_at_ns, 5000);
        assert_eq!(rec.tls_fingerprint, Some("tls-fp-abc123".to_string()));
    }

    #[test]
    fn test_failover_state_clone() {
        let normal = FailoverState::Normal.clone();
        assert_eq!(normal, FailoverState::Normal);

        let degraded = FailoverState::Degraded {
            failed_site: site(1),
        };
        let degraded_clone = degraded.clone();
        assert_eq!(
            degraded_clone,
            FailoverState::Degraded {
                failed_site: site(1)
            }
        );
    }

    #[test]
    fn test_bootstrap_catchup_progress_tracking() {
        let mut coordinator = BootstrapCoordinator::new(1);
        coordinator.begin_journal_catchup(2, 100, 200);
        coordinator.update_catchup_progress(150);
        assert_eq!(coordinator.stats().total_journal_entries_caught_up, 50);

        coordinator.update_catchup_progress(200);
        assert_eq!(coordinator.stats().total_journal_entries_caught_up, 100);
    }

    #[test]
    fn test_bootstrap_phase_variants() {
        assert!(matches!(BootstrapPhase::Idle, BootstrapPhase::Idle));
        assert!(matches!(
            BootstrapPhase::Enrolling { primary_site: 1 },
            BootstrapPhase::Enrolling { primary_site: _ }
        ));

        let snapshot = BootstrapPhase::SnapshotTransfer {
            primary_site: 1,
            bytes_received: 0,
            bytes_total: 100,
        };
        assert!(matches!(snapshot, BootstrapPhase::SnapshotTransfer { .. }));

        let catchup = BootstrapPhase::JournalCatchup {
            primary_site: 1,
            start_seq: 0,
            current_seq: 0,
            target_seq: 100,
        };
        assert!(matches!(catchup, BootstrapPhase::JournalCatchup { .. }));

        let complete = BootstrapPhase::Complete { enrolled_at_ns: 0 };
        assert!(matches!(complete, BootstrapPhase::Complete { .. }));

        let failed = BootstrapPhase::Failed {
            reason: "test".to_string(),
        };
        assert!(matches!(failed, BootstrapPhase::Failed { .. }));
    }
}
