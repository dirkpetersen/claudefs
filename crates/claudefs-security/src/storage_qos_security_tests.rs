//! Storage QoS/scheduling/capacity security tests.
//!
//! Part of A10 Phase 13: Storage QoS & scheduling security audit

#[cfg(test)]
mod tests {
    use claudefs_storage::capacity::{
        CapacityLevel, CapacityTracker, CapacityTrackerStats, SegmentTracker, TierOverride,
        WatermarkConfig,
    };
    use claudefs_storage::io_scheduler::{
        IoPriority, IoScheduler, IoSchedulerConfig, IoSchedulerStats, ScheduledIo,
    };
    use claudefs_storage::qos_storage::{
        BandwidthTracker, IoRequest, IoType, QosDecision, QosEnforcer, QosEnforcerStats, QosPolicy,
        TokenBucket, WorkloadClass,
    };
    use claudefs_storage::{BlockRef, BlockSize, IoOpType, IoRequestId};

    fn now_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn current_time_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn make_block_ref(offset: u64) -> BlockRef {
        BlockRef {
            id: claudefs_storage::BlockId::new(0, offset),
            size: BlockSize::B4K,
        }
    }

    fn make_scheduled_io(id: u64, priority: IoPriority, op_type: IoOpType) -> ScheduledIo {
        ScheduledIo::new(
            IoRequestId(id),
            priority,
            op_type,
            make_block_ref(id),
            now_ns(),
        )
    }

    fn make_io_request(
        tenant_id: &str,
        class: WorkloadClass,
        op_type: IoType,
        bytes: u64,
        timestamp_ns: u64,
    ) -> IoRequest {
        IoRequest {
            tenant_id: tenant_id.into(),
            class,
            op_type,
            bytes,
            timestamp_ns,
        }
    }

    fn make_segment_tracker(
        segment_id: u64,
        size_bytes: u64,
        last_access_age_secs: u64,
        s3_confirmed: bool,
        tier_override: TierOverride,
    ) -> SegmentTracker {
        let now = current_time_secs();
        SegmentTracker {
            segment_id,
            size_bytes,
            created_at_secs: now.saturating_sub(last_access_age_secs * 2),
            last_access_secs: now.saturating_sub(last_access_age_secs),
            s3_confirmed,
            tier_override,
        }
    }

    mod qos_token_bucket {
        use super::*;

        #[test]
        fn test_token_bucket_consume() {
            let mut bucket = TokenBucket::new(100, 10.0);
            assert_eq!(bucket.available(), 100);

            let result = bucket.try_consume(50, 0);
            assert!(result, "Should be able to consume 50 tokens");
            assert_eq!(bucket.available(), 50);

            let result = bucket.try_consume(60, 0);
            assert!(
                !result,
                "Should not be able to consume 60 tokens (only 50 available)"
            );
            assert_eq!(bucket.available(), 50);
        }

        #[test]
        fn test_token_bucket_refill() {
            let mut bucket = TokenBucket::new(10, 10.0);
            let consumed = bucket.try_consume(10, 0);
            assert!(consumed);
            assert_eq!(bucket.available(), 0);

            bucket.refill(1_000_000_000);
            let available = bucket.available();
            assert!(
                available >= 9 && available <= 11,
                "After 1 second at 10/sec, should have ~10 tokens"
            );
        }

        #[test]
        fn test_bandwidth_tracker_current() {
            let mut tracker = BandwidthTracker::new(1_000_000_000);
            tracker.record(1024 * 1024, 0);
            let mbps = tracker.current_mbps(0);
            assert!(
                (mbps - 1.0).abs() < 0.1,
                "1MB should be ~1 MB/s in 1s window"
            );

            tracker.record(1024 * 1024, 500_000_000);
            let mbps = tracker.current_mbps(500_000_000);
            assert!(
                (mbps - 2.0).abs() < 0.2,
                "2MB should be ~2 MB/s in ~0.5s window"
            );
        }

        #[test]
        fn test_qos_policy_default() {
            let policy = QosPolicy::default();
            assert_eq!(policy.class, WorkloadClass::BestEffort);
            assert_eq!(policy.max_iops, None);
            assert_eq!(policy.max_bandwidth_mbps, None);
            assert_eq!(policy.priority, 128);
            assert!(policy.burst_iops.is_none());
        }

        #[test]
        fn test_workload_class_display() {
            assert_eq!(format!("{}", WorkloadClass::AiTraining), "AiTraining");
            assert_eq!(format!("{}", WorkloadClass::Database), "Database");
            assert_eq!(format!("{}", WorkloadClass::Streaming), "Streaming");
            assert_eq!(format!("{}", WorkloadClass::Backup), "Backup");
            assert_eq!(format!("{}", WorkloadClass::Interactive), "Interactive");
            assert_eq!(format!("{}", WorkloadClass::BestEffort), "BestEffort");
        }
    }

    mod qos_enforcer {
        use super::*;

        #[test]
        fn test_qos_enforcer_allow_within_limits() {
            let mut enforcer = QosEnforcer::new();
            let policy = QosPolicy {
                class: WorkloadClass::Interactive,
                max_iops: Some(100),
                ..Default::default()
            };
            enforcer.set_policy("t1".to_string(), policy);

            let request = make_io_request("t1", WorkloadClass::Interactive, IoType::Read, 4096, 0);
            let decision = enforcer.check_request(&request);

            assert!(
                matches!(decision, QosDecision::Allow),
                "Request within limits should be allowed"
            );
        }

        #[test]
        fn test_qos_enforcer_throttle_when_exceeded() {
            let mut enforcer = QosEnforcer::new();
            let policy = QosPolicy {
                class: WorkloadClass::Interactive,
                max_iops: Some(1),
                burst_iops: Some(1),
                ..Default::default()
            };
            enforcer.set_policy("t1".to_string(), policy);

            let request1 = make_io_request("t1", WorkloadClass::Interactive, IoType::Read, 4096, 0);
            let decision1 = enforcer.check_request(&request1);
            assert!(matches!(decision1, QosDecision::Allow));

            let request2 =
                make_io_request("t1", WorkloadClass::Interactive, IoType::Read, 4096, 1000);
            let decision2 = enforcer.check_request(&request2);

            assert!(
                matches!(decision2, QosDecision::Throttle { .. })
                    || matches!(decision2, QosDecision::Reject { .. }),
                "Second request when tokens exhausted should be throttled or rejected"
            );
        }

        #[test]
        fn test_qos_enforcer_no_policy_rejects() {
            let mut enforcer = QosEnforcer::new();

            let request =
                make_io_request("unknown", WorkloadClass::Interactive, IoType::Read, 4096, 0);
            let decision = enforcer.check_request(&request);

            assert!(
                matches!(decision, QosDecision::Reject { .. }),
                "Request for unknown tenant should be rejected"
            );
        }

        #[test]
        fn test_qos_enforcer_stats_tracking() {
            let mut enforcer = QosEnforcer::new();
            let policy = QosPolicy {
                max_iops: Some(100),
                ..Default::default()
            };
            enforcer.set_policy("t1".to_string(), policy);

            for i in 0..3 {
                let request = make_io_request(
                    "t1",
                    WorkloadClass::BestEffort,
                    IoType::Read,
                    4096,
                    i * 1000,
                );
                enforcer.check_request(&request);
            }

            let stats = enforcer.stats();
            assert_eq!(stats.total_requests, 3, "Should track 3 total requests");

            enforcer.record_completion("t1", 4096, 1000);
            let stats = enforcer.stats();
            assert!(
                stats.total_bytes_processed >= 4096,
                "Should track bytes processed"
            );
        }

        #[test]
        fn test_qos_enforcer_remove_policy() {
            let mut enforcer = QosEnforcer::new();
            let policy = QosPolicy::default();
            enforcer.set_policy("t1".to_string(), policy);

            assert!(enforcer.get_policy("t1").is_some(), "Policy should exist");

            enforcer.remove_policy("t1");
            assert!(
                enforcer.get_policy("t1").is_none(),
                "Policy should be removed"
            );

            let request = make_io_request("t1", WorkloadClass::BestEffort, IoType::Read, 4096, 0);
            let decision = enforcer.check_request(&request);
            assert!(
                matches!(decision, QosDecision::Reject { .. }),
                "Removed policy tenant should be rejected"
            );
        }
    }

    mod io_scheduler {
        use super::*;

        #[test]
        fn test_io_scheduler_priority_ordering() {
            assert!(IoPriority::Critical < IoPriority::High, "Critical < High");
            assert!(IoPriority::High < IoPriority::Normal, "High < Normal");
            assert!(IoPriority::Normal < IoPriority::Low, "Normal < Low");

            assert!(IoPriority::Critical.is_high());
            assert!(IoPriority::High.is_high());
            assert!(!IoPriority::Normal.is_high());
            assert!(!IoPriority::Low.is_high());
        }

        #[test]
        fn test_io_scheduler_dequeue_priority() {
            let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

            scheduler
                .enqueue(make_scheduled_io(1, IoPriority::Normal, IoOpType::Read))
                .unwrap();
            scheduler
                .enqueue(make_scheduled_io(2, IoPriority::Critical, IoOpType::Read))
                .unwrap();
            scheduler
                .enqueue(make_scheduled_io(3, IoPriority::Low, IoOpType::Read))
                .unwrap();

            let first = scheduler.dequeue().unwrap();
            assert_eq!(first.id, IoRequestId(2), "Critical should come first");

            let second = scheduler.dequeue().unwrap();
            assert_eq!(second.id, IoRequestId(1), "Normal should come second");

            let third = scheduler.dequeue().unwrap();
            assert_eq!(third.id, IoRequestId(3), "Low should come last");
        }

        #[test]
        fn test_io_scheduler_max_queue_depth() {
            let config = IoSchedulerConfig {
                max_queue_depth: 3,
                ..Default::default()
            };
            let mut scheduler = IoScheduler::new(config);

            assert!(scheduler
                .enqueue(make_scheduled_io(1, IoPriority::Normal, IoOpType::Read))
                .is_ok());
            assert!(scheduler
                .enqueue(make_scheduled_io(2, IoPriority::Normal, IoOpType::Read))
                .is_ok());
            assert!(scheduler
                .enqueue(make_scheduled_io(3, IoPriority::Normal, IoOpType::Read))
                .is_ok());

            let result =
                scheduler.enqueue(make_scheduled_io(4, IoPriority::Normal, IoOpType::Read));
            assert!(result.is_err(), "Queue full should reject new request");
        }

        #[test]
        fn test_io_scheduler_inflight_tracking() {
            let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

            scheduler
                .enqueue(make_scheduled_io(1, IoPriority::High, IoOpType::Read))
                .unwrap();
            scheduler
                .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Read))
                .unwrap();

            assert_eq!(scheduler.inflight_count(), 0, "Not yet dequeued");

            scheduler.dequeue();
            scheduler.dequeue();

            assert_eq!(scheduler.inflight_count(), 2, "Should track 2 in-flight");

            scheduler.complete(IoRequestId(1));
            assert_eq!(scheduler.inflight_count(), 1, "After completing one");

            scheduler.complete(IoRequestId(2));
            assert_eq!(scheduler.inflight_count(), 0, "After completing all");
        }

        #[test]
        fn test_io_scheduler_drain_priority() {
            let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

            for i in 1..=3 {
                scheduler
                    .enqueue(make_scheduled_io(i, IoPriority::Normal, IoOpType::Read))
                    .unwrap();
            }
            for i in 4..=5 {
                scheduler
                    .enqueue(make_scheduled_io(i, IoPriority::Critical, IoOpType::Read))
                    .unwrap();
            }

            let drained = scheduler.drain_priority(IoPriority::Normal);
            assert_eq!(drained.len(), 3, "Should drain 3 Normal requests");
            assert_eq!(
                scheduler.priority_depth(IoPriority::Normal),
                0,
                "Normal queue should be empty"
            );
            assert_eq!(
                scheduler.priority_depth(IoPriority::Critical),
                2,
                "Critical should still have 2"
            );
        }
    }

    mod capacity_watermarks {
        use super::*;

        #[test]
        fn test_capacity_level_normal() {
            let config = WatermarkConfig::default();
            let tracker = CapacityTracker::new(config, 1000);

            tracker.update_usage(500);
            assert_eq!(tracker.level(), CapacityLevel::Normal);
            assert_eq!(tracker.usage_pct(), 50);
        }

        #[test]
        fn test_capacity_level_transitions() {
            let config = WatermarkConfig {
                high_watermark_pct: 80,
                critical_watermark_pct: 95,
                low_watermark_pct: 60,
            };
            let tracker = CapacityTracker::new(config, 1000);

            tracker.update_usage(500);
            assert_eq!(tracker.level(), CapacityLevel::Normal);

            tracker.update_usage(700);
            assert_eq!(tracker.level(), CapacityLevel::Elevated);

            tracker.update_usage(810);
            assert_eq!(tracker.level(), CapacityLevel::High);

            tracker.update_usage(960);
            assert_eq!(tracker.level(), CapacityLevel::Critical);

            tracker.update_usage(1000);
            assert_eq!(tracker.level(), CapacityLevel::Full);
        }

        #[test]
        fn test_capacity_eviction_trigger() {
            let config = WatermarkConfig {
                high_watermark_pct: 80,
                ..Default::default()
            };
            let tracker = CapacityTracker::new(config, 1000);

            tracker.update_usage(790);
            assert!(
                !tracker.should_evict(),
                "Below high watermark should not trigger eviction"
            );

            tracker.update_usage(810);
            assert!(
                tracker.should_evict(),
                "Above high watermark should trigger eviction"
            );
        }

        #[test]
        fn test_capacity_segment_registration() {
            let config = WatermarkConfig::default();
            let tracker = CapacityTracker::new(config, 10000);

            let now = current_time_secs();
            for i in 1..=3 {
                tracker.register_segment(SegmentTracker {
                    segment_id: i,
                    size_bytes: 1024,
                    created_at_secs: now,
                    last_access_secs: now,
                    s3_confirmed: false,
                    tier_override: TierOverride::Auto,
                });
            }

            let stats = tracker.stats();
            assert_eq!(stats.tracked_segments, 3, "Should track 3 segments");

            tracker.mark_s3_confirmed(1);
            let stats = tracker.stats();
            assert_eq!(
                stats.s3_confirmed_segments, 1,
                "Should have 1 S3-confirmed segment"
            );
        }

        #[test]
        fn test_capacity_eviction_candidates() {
            let config = WatermarkConfig::default();
            let tracker = CapacityTracker::new(config, 10000);

            let now = current_time_secs();
            tracker.register_segment(SegmentTracker {
                segment_id: 1,
                size_bytes: 1000,
                created_at_secs: now - 1000,
                last_access_secs: now - 1000,
                s3_confirmed: true,
                tier_override: TierOverride::Auto,
            });
            tracker.register_segment(SegmentTracker {
                segment_id: 2,
                size_bytes: 2000,
                created_at_secs: now - 500,
                last_access_secs: now - 500,
                s3_confirmed: true,
                tier_override: TierOverride::Auto,
            });
            tracker.register_segment(SegmentTracker {
                segment_id: 3,
                size_bytes: 500,
                created_at_secs: now - 2000,
                last_access_secs: now - 2000,
                s3_confirmed: true,
                tier_override: TierOverride::Auto,
            });
            tracker.register_segment(SegmentTracker {
                segment_id: 4,
                size_bytes: 3000,
                created_at_secs: now,
                last_access_secs: now,
                s3_confirmed: false,
                tier_override: TierOverride::Auto,
            });

            let candidates = tracker.eviction_candidates(10);
            assert_eq!(
                candidates.len(),
                3,
                "Should only return S3-confirmed segments"
            );
        }
    }

    mod config_defaults {
        use super::*;

        #[test]
        fn test_io_scheduler_config_defaults() {
            let config = IoSchedulerConfig::default();
            assert!(
                config.max_queue_depth > 0,
                "max_queue_depth should be positive"
            );
            assert!(config.max_inflight > 0, "max_inflight should be positive");
            assert!(
                config.starvation_threshold_ms > 0,
                "starvation_threshold_ms should be positive"
            );
            assert!(
                config.critical_reservation > 0.0,
                "critical_reservation should be positive"
            );
            assert!(
                config.critical_reservation <= 1.0,
                "critical_reservation should be <= 1.0"
            );
        }

        #[test]
        fn test_watermark_config_defaults() {
            let config = WatermarkConfig::default();
            assert!(
                config.high_watermark_pct < config.critical_watermark_pct,
                "high_watermark should be less than critical_watermark"
            );
            assert!(
                config.low_watermark_pct < config.high_watermark_pct,
                "low_watermark should be less than high_watermark"
            );
            assert!(
                config.high_watermark_pct <= 100,
                "high_watermark should be <= 100"
            );
            assert!(
                config.critical_watermark_pct <= 100,
                "critical_watermark should be <= 100"
            );
            assert!(
                config.low_watermark_pct <= 100,
                "low_watermark should be <= 100"
            );
        }

        #[test]
        fn test_capacity_tracker_zero_total() {
            let config = WatermarkConfig::default();
            let tracker = CapacityTracker::new(config, 0);

            tracker.update_usage(0);
            let level = tracker.level();
            assert!(
                level == CapacityLevel::Normal || level == CapacityLevel::Full,
                "Zero capacity should return Normal or Full"
            );

            let usage = tracker.usage_pct();
            assert!(usage <= 100, "Usage percentage should not panic");
        }

        #[test]
        fn test_io_scheduler_empty_dequeue() {
            let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

            let result = scheduler.dequeue();
            assert!(
                result.is_none(),
                "Dequeue on empty scheduler should return None"
            );

            assert!(
                scheduler.is_empty(),
                "Empty scheduler should report is_empty"
            );
        }

        #[test]
        fn test_qos_enforcer_reset_stats() {
            let mut enforcer = QosEnforcer::new();
            let policy = QosPolicy {
                max_iops: Some(100),
                ..Default::default()
            };
            enforcer.set_policy("t1".to_string(), policy);

            let request = make_io_request("t1", WorkloadClass::BestEffort, IoType::Read, 4096, 0);
            enforcer.check_request(&request);
            enforcer.check_request(&request);

            enforcer.reset_stats();

            let stats = enforcer.stats();
            assert_eq!(stats.total_requests, 0, "total_requests should be reset");
            assert_eq!(
                stats.allowed_requests, 0,
                "allowed_requests should be reset"
            );
            assert_eq!(
                stats.throttled_requests, 0,
                "throttled_requests should be reset"
            );
            assert_eq!(
                stats.rejected_requests, 0,
                "rejected_requests should be reset"
            );
        }
    }
}
