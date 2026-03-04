//! Replication QoS, journal GC, and checkpoint security tests.
//!
//! Part of A10 Phase 17: Replication QoS & GC security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::checkpoint::{CheckpointManager, ReplicationCheckpoint};
    use claudefs_repl::journal_gc::{
        GcCandidate, GcPolicy, GcStats, JournalGcScheduler, JournalGcState,
    };
    use claudefs_repl::repl_qos::{BandwidthAllocation, QosPolicy, QosScheduler, WorkloadClass};
    use claudefs_repl::wal::ReplicationCursor;

    macro_rules! finding {
        ($id:expr, $msg:expr) => {
            eprintln!("FINDING-REPL-QOS-{}: {}", $id, $msg)
        };
    }

    fn now_us() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }

    fn make_qos_scheduler(total_bw: u64) -> QosScheduler {
        QosScheduler::new(QosPolicy::new(total_bw))
    }

    fn make_gc_candidate(seq: u64, timestamp_us: u64, size_bytes: usize) -> GcCandidate {
        GcCandidate {
            shard_id: 0,
            seq,
            timestamp_us,
            size_bytes,
        }
    }

    fn make_checkpoint(
        site_id: u64,
        checkpoint_id: u64,
        cursors: Vec<ReplicationCursor>,
    ) -> ReplicationCheckpoint {
        ReplicationCheckpoint::new(site_id, checkpoint_id, now_us(), cursors)
    }

    // Category 1: QoS Bandwidth Scheduling (5 tests)

    #[test]
    fn test_qos_priority_ordering() {
        assert!(WorkloadClass::Critical.priority() > WorkloadClass::High.priority());
        assert!(WorkloadClass::High.priority() > WorkloadClass::Normal.priority());
        assert!(WorkloadClass::Normal.priority() > WorkloadClass::Background.priority());

        assert_eq!(WorkloadClass::Critical.priority(), 100);
        assert_eq!(WorkloadClass::High.priority(), 75);
        assert_eq!(WorkloadClass::Normal.priority(), 50);
        assert_eq!(WorkloadClass::Background.priority(), 25);

        finding!("01", "priority ordering correctly distinguishes all tiers");
    }

    #[test]
    fn test_qos_default_allocation_ratios() {
        let policy = QosPolicy::new(10000);

        let critical = policy.get_allocation(&WorkloadClass::Critical).unwrap();
        assert_eq!(critical.max_bytes_per_sec, 4000);
        assert_eq!(critical.burst_bytes, 8000);

        let high = policy.get_allocation(&WorkloadClass::High).unwrap();
        assert_eq!(high.max_bytes_per_sec, 3000);
        assert_eq!(high.burst_bytes, 6000);

        let normal = policy.get_allocation(&WorkloadClass::Normal).unwrap();
        assert_eq!(normal.max_bytes_per_sec, 2000);
        assert_eq!(normal.burst_bytes, 4000);

        let background = policy.get_allocation(&WorkloadClass::Background).unwrap();
        assert_eq!(background.max_bytes_per_sec, 1000);
        assert_eq!(background.burst_bytes, 2000);

        assert_eq!(policy.total_bandwidth_limit, 10000);
    }

    #[test]
    fn test_qos_scheduler_caps_at_budget() {
        let mut scheduler = make_qos_scheduler(1000);

        let token1 = scheduler.request_bandwidth(WorkloadClass::Normal, 500, 0);
        let normal_allocation = scheduler
            .policy()
            .get_allocation(&WorkloadClass::Normal)
            .unwrap();
        let expected_allowed = normal_allocation.max_bytes_per_sec.min(500);
        assert_eq!(token1.bytes_allowed, expected_allowed);

        let token2 = scheduler.request_bandwidth(WorkloadClass::Normal, 100, 0);
        assert_eq!(token2.bytes_allowed, 0);

        finding!("02", "QoS prevents bandwidth hogging");
    }

    #[test]
    fn test_qos_window_reset() {
        let mut scheduler = make_qos_scheduler(250);

        let _ = scheduler.request_bandwidth(WorkloadClass::Normal, 100, 0);
        let token = scheduler.request_bandwidth(WorkloadClass::Normal, 50, 1_000_000_001);
        assert_eq!(token.bytes_allowed, 50);
    }

    #[test]
    fn test_qos_utilization_tracking() {
        let mut scheduler = make_qos_scheduler(1000);

        let _ = scheduler.request_bandwidth(WorkloadClass::Critical, 400, 0);
        let crit_util = scheduler.utilization(WorkloadClass::Critical);
        assert!(crit_util > 0.0);

        let bg_util = scheduler.utilization(WorkloadClass::Background);
        assert_eq!(bg_util, 0.0);

        let _ = scheduler.request_bandwidth(WorkloadClass::Background, 2000, 0);
        let bg_util_capped = scheduler.utilization(WorkloadClass::Background);
        assert!(bg_util_capped <= 1.0);
    }

    // Category 2: QoS Edge Cases (5 tests)

    #[test]
    fn test_qos_set_custom_allocation() {
        let mut policy = QosPolicy::new(1000);

        let original_high = policy
            .get_allocation(&WorkloadClass::High)
            .unwrap()
            .max_bytes_per_sec;

        let new_critical = BandwidthAllocation {
            max_bytes_per_sec: 600,
            burst_bytes: 1200,
            workload_class: WorkloadClass::Critical,
        };
        policy.set_allocation(WorkloadClass::Critical, new_critical);

        let updated_critical = policy.get_allocation(&WorkloadClass::Critical).unwrap();
        assert_eq!(updated_critical.max_bytes_per_sec, 600);

        let unchanged_high = policy.get_allocation(&WorkloadClass::High).unwrap();
        assert_eq!(unchanged_high.max_bytes_per_sec, original_high);
    }

    #[test]
    fn test_qos_token_fields() {
        let mut scheduler = make_qos_scheduler(1000);
        let token = scheduler.request_bandwidth(WorkloadClass::Critical, 400, 1234567890);

        assert_eq!(token.bytes_allowed, 400);
        assert_eq!(token.class, WorkloadClass::Critical);
        assert_eq!(token.issued_at_ns, 1234567890);
    }

    #[test]
    fn test_qos_classes_independent() {
        let mut scheduler = make_qos_scheduler(1000);

        let _ = scheduler.request_bandwidth(WorkloadClass::Critical, 400, 0);
        let _ = scheduler.request_bandwidth(WorkloadClass::Background, 100, 0);

        let crit_util = scheduler.utilization(WorkloadClass::Critical);
        let bg_util = scheduler.utilization(WorkloadClass::Background);
        let normal_util = scheduler.utilization(WorkloadClass::Normal);

        assert!(crit_util > 0.0);
        assert!(bg_util > 0.0);
        assert_eq!(normal_util, 0.0);
    }

    #[test]
    fn test_qos_zero_bandwidth() {
        let policy = QosPolicy::new(0);

        let critical = policy.get_allocation(&WorkloadClass::Critical).unwrap();
        assert_eq!(critical.max_bytes_per_sec, 0);

        let mut scheduler = make_qos_scheduler(0);
        let token = scheduler.request_bandwidth(WorkloadClass::Critical, 100, 0);
        assert_eq!(token.bytes_allowed, 0);
    }

    #[test]
    fn test_qos_critical_gets_more_than_background() {
        let mut scheduler = make_qos_scheduler(1000);

        let critical_token = scheduler.request_bandwidth(WorkloadClass::Critical, 1000, 0);

        let mut scheduler2 = make_qos_scheduler(1000);
        let bg_token = scheduler2.request_bandwidth(WorkloadClass::Background, 1000, 0);

        assert!(critical_token.bytes_allowed > bg_token.bytes_allowed);
    }

    // Category 3: Journal GC State (5 tests)

    #[test]
    fn test_gc_state_record_and_get_ack() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now_us());

        let ack = state.get_ack(1).expect("ack should exist");
        assert_eq!(ack.acked_through_seq, 100);

        assert!(state.get_ack(2).is_none());
        assert_eq!(state.site_count(), 1);
    }

    #[test]
    fn test_gc_min_acked_seq() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now_us());
        state.record_ack(2, 50, now_us());

        let min_seq = state.min_acked_seq(&[1, 2]);
        assert_eq!(min_seq, Some(50));

        let min_with_missing = state.min_acked_seq(&[1, 2, 3]);
        assert_eq!(min_with_missing, None);

        finding!("03", "missing site blocks GC — safety guarantee");
    }

    #[test]
    fn test_gc_all_sites_acked() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now_us());
        state.record_ack(2, 80, now_us());

        assert!(state.all_sites_acked(50, &[1, 2]));
        assert!(!state.all_sites_acked(90, &[1, 2]));
    }

    #[test]
    fn test_gc_retain_all_policy() {
        let mut scheduler = JournalGcScheduler::new(GcPolicy::RetainAll, vec![1, 2]);

        let candidates = vec![
            make_gc_candidate(1, now_us() - 1_000_000, 1024),
            make_gc_candidate(2, now_us() - 2_000_000, 2048),
        ];
        let result = scheduler.run_gc(&candidates, now_us());

        assert!(result.is_empty());
        assert_eq!(scheduler.stats().gc_runs, 1);
    }

    #[test]
    fn test_gc_retain_by_age() {
        let mut scheduler = JournalGcScheduler::new(
            GcPolicy::RetainByAge {
                max_age_us: 500_000,
            },
            vec![],
        );

        let old_ts = now_us() - 1_000_000;
        let new_ts = now_us() - 100_000;
        let candidates = vec![
            make_gc_candidate(1, old_ts, 1024),
            make_gc_candidate(2, new_ts, 2048),
        ];
        let result = scheduler.run_gc(&candidates, now_us());

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].seq, 1);
    }

    // Category 4: Journal GC Scheduling (5 tests)

    #[test]
    fn test_gc_retain_by_count() {
        let mut scheduler =
            JournalGcScheduler::new(GcPolicy::RetainByCount { max_entries: 2 }, vec![]);

        let now = now_us();
        let candidates = vec![
            make_gc_candidate(1, now, 1024),
            make_gc_candidate(2, now, 2048),
            make_gc_candidate(3, now, 4096),
            make_gc_candidate(4, now, 8192),
            make_gc_candidate(5, now, 16384),
        ];
        let result = scheduler.run_gc(&candidates, now);

        assert_eq!(result.len(), 3);
        assert_eq!(scheduler.stats().entries_gc_collected, 3);
    }

    #[test]
    fn test_gc_stats_tracking() {
        let mut scheduler =
            JournalGcScheduler::new(GcPolicy::RetainByAge { max_age_us: 1 }, vec![]);

        let now = now_us();
        let candidates1 = vec![make_gc_candidate(1, now - 1_000_000, 1024)];
        let candidates2 = vec![make_gc_candidate(2, now - 1_000_000, 2048)];

        scheduler.run_gc(&candidates1, now);
        scheduler.run_gc(&candidates2, now);

        assert_eq!(scheduler.stats().gc_runs, 2);
        assert_eq!(scheduler.stats().entries_gc_collected, 2);
        assert_eq!(scheduler.stats().bytes_gc_collected, 3072);
        assert!(scheduler.stats().last_gc_us > 0);
    }

    #[test]
    fn test_gc_stats_default() {
        let stats = GcStats::default();
        assert_eq!(stats.entries_gc_collected, 0);
        assert_eq!(stats.bytes_gc_collected, 0);
        assert_eq!(stats.gc_runs, 0);
        assert_eq!(stats.last_gc_us, 0);
    }

    #[test]
    fn test_gc_should_gc_retain_all_false() {
        let scheduler = JournalGcScheduler::new(GcPolicy::RetainAll, vec![]);

        let old_candidate = make_gc_candidate(1, now_us() - 1_000_000, 1024);
        assert!(!scheduler.should_gc_entry(&old_candidate, now_us()));
    }

    #[test]
    fn test_gc_should_gc_retain_by_age() {
        let scheduler = JournalGcScheduler::new(
            GcPolicy::RetainByAge {
                max_age_us: 500_000,
            },
            vec![],
        );

        let old_candidate = make_gc_candidate(1, now_us() - 1_000_000, 1024);
        let new_candidate = make_gc_candidate(2, now_us() - 100_000, 2048);

        assert!(scheduler.should_gc_entry(&old_candidate, now_us()));
        assert!(!scheduler.should_gc_entry(&new_candidate, now_us()));
    }

    // Category 5: Checkpoint Management (5 tests)

    #[test]
    fn test_checkpoint_create_and_fingerprint() {
        let cursors = vec![
            ReplicationCursor::new(2, 0, 100),
            ReplicationCursor::new(2, 1, 200),
        ];
        let cp1 = make_checkpoint(1, 1, cursors.clone());
        let cp2 = make_checkpoint(1, 1, cursors.clone());

        assert_eq!(cp1.fingerprint, cp2.fingerprint);

        let mut different_cursors = cursors.clone();
        different_cursors[0] = ReplicationCursor::new(2, 0, 101);
        let cp3 = make_checkpoint(1, 1, different_cursors);

        assert_ne!(cp1.fingerprint, cp3.fingerprint);
    }

    #[test]
    fn test_checkpoint_serialize_roundtrip() {
        let original = make_checkpoint(
            1,
            42,
            vec![
                ReplicationCursor::new(2, 0, 100),
                ReplicationCursor::new(2, 1, 200),
            ],
        );

        let bytes = original.to_bytes().expect("serialization should succeed");
        let restored =
            ReplicationCheckpoint::from_bytes(&bytes).expect("deserialization should succeed");

        assert_eq!(original, restored);

        finding!("04", "serialization round-trip preserves all fields");
    }

    #[test]
    fn test_checkpoint_manager_pruning() {
        let mut manager = CheckpointManager::new(1, 3);

        for i in 1..=5u64 {
            let cursors = vec![ReplicationCursor::new(2, 0, i * 100)];
            manager.create(cursors, now_us());
        }

        assert_eq!(manager.all().len(), 3);
        assert_eq!(manager.all()[0].checkpoint_id, 3);
        assert_eq!(manager.latest().unwrap().checkpoint_id, 5);
    }

    #[test]
    fn test_checkpoint_lag_calculation() {
        let cp1 = make_checkpoint(1, 1, vec![ReplicationCursor::new(2, 0, 200)]);
        let cp2 = make_checkpoint(1, 2, vec![ReplicationCursor::new(2, 0, 150)]);

        assert_eq!(cp1.lag_vs(&cp2), 50);
        assert_eq!(cp2.lag_vs(&cp1), 0);

        finding!("05", "lag calculation saturates to prevent underflow");
    }

    #[test]
    fn test_checkpoint_find_and_clear() {
        let mut manager = CheckpointManager::new(1, 10);

        for i in 1..=3u64 {
            let cursors = vec![ReplicationCursor::new(2, 0, i * 100)];
            manager.create(cursors, now_us());
        }

        assert!(manager.find_by_id(2).is_some());
        assert!(manager.find_by_id(999).is_none());

        manager.clear();
        assert!(manager.all().is_empty());
        assert!(manager.latest().is_none());
    }
}
