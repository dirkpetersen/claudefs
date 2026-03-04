//! Gateway S3 notification, replication, and storage class security tests.
//!
//! Part of A10 Phase 20: Gateway S3 notification/replication/storage-class security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::s3_notification::{
        NotificationDestination, NotificationError, NotificationEvent, NotificationFilter,
        NotificationManager,
    };
    use claudefs_gateway::s3_replication::{
        BucketReplicationConfig, ObjectReplicationEntry, ReplicationDestination, ReplicationQueue,
        ReplicationRule, ReplicationStatus,
    };
    use claudefs_gateway::s3_storage_class::{
        evaluate_transitions, ObjectStorageState, RestoreTier, StorageClass, StorageClassTransition,
    };
    use std::collections::HashMap;

    fn make_notification_manager() -> NotificationManager {
        NotificationManager::new()
    }

    fn make_notification_config(
        id: &str,
        events: Vec<NotificationEvent>,
        filter: NotificationFilter,
    ) -> claudefs_gateway::s3_notification::NotificationConfig {
        claudefs_gateway::s3_notification::NotificationConfig::new(
            id,
            events,
            filter,
            NotificationDestination::InProcess {
                queue_name: "test".to_string(),
            },
        )
    }

    fn make_replication_destination(bucket: &str) -> ReplicationDestination {
        ReplicationDestination::new(bucket)
    }

    fn make_replication_rule(id: &str, bucket: &str) -> ReplicationRule {
        ReplicationRule::new(id, make_replication_destination(bucket))
    }

    fn make_tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // =========================================================================
    // Category 1: S3 Notification Events & Filters (5 tests)
    // =========================================================================

    #[test]
    fn test_notification_event_names() {
        assert_eq!(
            NotificationEvent::ObjectCreated.event_name(),
            "s3:ObjectCreated:*"
        );
        assert_eq!(
            NotificationEvent::ObjectRemoved.event_name(),
            "s3:ObjectRemoved:*"
        );
        assert!(
            NotificationEvent::ObjectRestored
                .event_name()
                .starts_with("s3:ObjectRestored"),
            "ObjectRestored event_name should start with s3:ObjectRestored"
        );
        assert!(
            NotificationEvent::ReducedRedundancyLostObject
                .event_name()
                .contains("ReducedRedundancy"),
            "ReducedRedundancyLostObject event_name should contain ReducedRedundancy"
        );
    }

    #[test]
    fn test_notification_filter_prefix_suffix() {
        let filter = NotificationFilter::new().with_prefix("logs/");
        assert!(
            filter.matches("logs/app.log"),
            "logs/app.log should match prefix logs/"
        );
        assert!(
            !filter.matches("data/file.txt"),
            "data/file.txt should NOT match prefix logs/"
        );

        let filter_suffix = NotificationFilter::new().with_suffix(".log");
        assert!(filter_suffix.matches("app.log"), ".log should match suffix");
        assert!(
            !filter_suffix.matches("file.txt"),
            "file.txt should NOT match suffix .log"
        );

        let filter_both = NotificationFilter::new()
            .with_prefix("logs/")
            .with_suffix(".log");
        assert!(
            filter_both.matches("logs/app.log"),
            "logs/app.log should match both prefix and suffix"
        );
        assert!(
            !filter_both.matches("logs/app.txt"),
            "logs/app.txt should NOT match (suffix mismatch)"
        );
        assert!(
            !filter_both.matches("data/app.log"),
            "data/app.log should NOT match (prefix mismatch)"
        );
    }

    #[test]
    fn test_notification_filter_empty_matches_all() {
        let filter = NotificationFilter::new();
        assert!(
            filter.matches("anything"),
            "Empty filter should match 'anything'"
        );
        assert!(filter.matches(""), "Empty filter should match empty string");
        assert!(
            filter.matches("data/file.txt"),
            "Empty filter should match 'data/file.txt'"
        );
    }

    #[test]
    fn test_notification_config_enable_disable() {
        let mut config = make_notification_config(
            "test-id",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        assert!(config.enabled, "Config should be enabled by default");

        config.disable();
        assert!(!config.enabled, "Config should be disabled after disable()");

        config.enable();
        assert!(config.enabled, "Config should be enabled after enable()");
    }

    #[test]
    fn test_notification_manager_register_and_query() {
        let mut manager = make_notification_manager();
        let config = make_notification_config(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        manager.register("my-bucket", config);

        assert_eq!(
            manager.configs_for("my-bucket").len(),
            1,
            "Should have 1 config for my-bucket"
        );
        assert!(
            manager.configs_for("other-bucket").is_empty(),
            "Should have no configs for other-bucket"
        );

        let matches = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "any/key.txt",
        );
        assert_eq!(matches.len(), 1, "ObjectCreated should match 1 config");

        let wrong_event = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectRemoved,
            "any/key.txt",
        );
        assert_eq!(
            wrong_event.len(),
            0,
            "ObjectRemoved should match 0 configs (wrong event)"
        );
    }

    // =========================================================================
    // Category 2: S3 Notification — Matching & Delivery (5 tests)
    // =========================================================================

    #[test]
    fn test_notification_matching_disabled_skipped() {
        let mut manager = make_notification_manager();
        let mut config = make_notification_config(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        config.disable();
        manager.register("my-bucket", config);

        let matches = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "any/key.txt",
        );
        assert!(matches.is_empty(), "Disabled configs should never match");
    }

    #[test]
    fn test_notification_matching_filter_applied() {
        let mut manager = make_notification_manager();
        let config = make_notification_config(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new().with_prefix("logs/"),
        );
        manager.register("my-bucket", config);

        let matches = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "logs/file.txt",
        );
        assert_eq!(matches.len(), 1, "logs/file.txt should match (prefix fits)");

        let no_match = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "data/file.txt",
        );
        assert_eq!(
            no_match.len(),
            0,
            "data/file.txt should NOT match (prefix mismatch)"
        );
    }

    #[test]
    fn test_notification_remove_config() {
        let mut manager = make_notification_manager();
        let config = make_notification_config(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        manager.register("my-bucket", config);

        manager.remove("my-bucket", "config-1").unwrap();
        assert!(
            manager.configs_for("my-bucket").is_empty(),
            "Config should be removed"
        );

        let result = manager.remove("my-bucket", "non-existent");
        assert!(
            matches!(result, Err(NotificationError::NotFound(_))),
            "Remove non-existent ID should return NotFound"
        );

        let result_bucket = manager.remove("non-existent-bucket", "config-1");
        assert!(
            matches!(result_bucket, Err(NotificationError::NoBucketConfig(_))),
            "Remove from non-existent bucket should return NoBucketConfig"
        );
    }

    #[test]
    fn test_notification_delivery_counter() {
        let mut manager = make_notification_manager();
        assert_eq!(manager.delivered_count(), 0, "Initial count should be 0");

        manager.record_delivery();
        manager.record_delivery();
        manager.record_delivery();

        assert_eq!(manager.delivered_count(), 3, "Delivered count should be 3");
    }

    #[test]
    fn test_notification_enabled_config_count() {
        let mut manager = make_notification_manager();

        let config1 = make_notification_config(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        manager.register("bucket1", config1);

        let mut config2 = make_notification_config(
            "config-2",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
        );
        config2.disable();
        manager.register("bucket1", config2);

        assert_eq!(
            manager.enabled_config_count(),
            1,
            "Should have 1 enabled config"
        );
    }

    // =========================================================================
    // Category 3: S3 Replication Rules & Config (5 tests)
    // =========================================================================

    #[test]
    fn test_replication_rule_prefix_match() {
        let rule = make_replication_rule("rule1", "dest-bucket").with_prefix("logs/");

        assert!(
            rule.matches("logs/2024/file.txt", &make_tags(&[])),
            "logs/2024/file.txt should match prefix logs/"
        );
        assert!(
            !rule.matches("data/file.txt", &make_tags(&[])),
            "data/file.txt should NOT match prefix logs/"
        );
    }

    #[test]
    fn test_replication_rule_tag_match() {
        let rule = make_replication_rule("rule1", "dest-bucket")
            .with_filter_tags(make_tags(&[("env", "prod")]));

        assert!(
            rule.matches("data/file.txt", &make_tags(&[("env", "prod")])),
            "Matching tags should return true"
        );
        assert!(
            !rule.matches("data/file.txt", &make_tags(&[("env", "dev")])),
            "Different tag value should return false"
        );
        assert!(
            !rule.matches("data/file.txt", &make_tags(&[])),
            "Missing tag should return false"
        );
    }

    #[test]
    fn test_replication_disabled_rule_no_match() {
        let rule = make_replication_rule("rule1", "dest-bucket")
            .with_enabled(false)
            .with_prefix("logs/");

        assert!(
            !rule.matches("logs/file.txt", &make_tags(&[])),
            "Disabled rule should never match regardless of key"
        );
        assert!(
            !rule.matches("data/file.txt", &make_tags(&[])),
            "Disabled rule should never match regardless of tags"
        );
    }

    #[test]
    fn test_replication_config_priority_ordering() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication")
            .with_rule(make_replication_rule("rule1", "dest").with_priority(10))
            .with_rule(make_replication_rule("rule2", "dest").with_priority(50))
            .with_rule(make_replication_rule("rule3", "dest").with_priority(30));

        let matches = config.matching_rules("data/file.txt", &make_tags(&[]));
        assert_eq!(matches.len(), 3, "Should match all 3 rules");
        assert_eq!(
            matches[0].id, "rule2",
            "Highest priority (50) should be first"
        );
        assert_eq!(
            matches[1].id, "rule3",
            "Second priority (30) should be second"
        );
        assert_eq!(
            matches[2].id, "rule1",
            "Lowest priority (10) should be third"
        );
    }

    #[test]
    fn test_replication_destinations_for() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication")
            .with_rule(make_replication_rule("rule-us", "bucket-us").with_prefix("us/"))
            .with_rule(make_replication_rule("rule-eu", "bucket-eu").with_prefix("eu/"));

        let dests_us = config.destinations_for("us/data.txt", &make_tags(&[]));
        assert_eq!(dests_us.len(), 1, "us/data.txt should have 1 destination");
        assert_eq!(
            dests_us[0].bucket, "bucket-us",
            "us/data.txt should go to bucket-us"
        );

        let dests_eu = config.destinations_for("eu/data.txt", &make_tags(&[]));
        assert_eq!(dests_eu.len(), 1, "eu/data.txt should have 1 destination");
        assert_eq!(
            dests_eu[0].bucket, "bucket-eu",
            "eu/data.txt should go to bucket-eu"
        );
    }

    // =========================================================================
    // Category 4: S3 Replication Queue (5 tests)
    // =========================================================================

    #[test]
    fn test_replication_queue_enqueue_pending() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));
        queue.enqueue(ObjectReplicationEntry::new("key2", "rule2", "bucket2"));

        assert_eq!(queue.pending_count(), 2, "Should have 2 pending entries");
        assert_eq!(queue.len(), 2, "Queue length should be 2");

        let pending = queue.get_pending();
        assert_eq!(pending.len(), 2, "get_pending should return 2 entries");
    }

    #[test]
    fn test_replication_queue_mark_completed() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        queue.mark_completed("key1", "rule1", "bucket1");

        assert_eq!(
            queue.pending_count(),
            0,
            "Should have 0 pending after completion"
        );
        assert_eq!(
            queue.failed_count(),
            0,
            "Should have 0 failed after completion"
        );
    }

    #[test]
    fn test_replication_queue_retry_limit() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        queue.mark_failed("key1", "rule1", "bucket1");
        queue.mark_failed("key1", "rule1", "bucket1");
        queue.mark_failed("key1", "rule1", "bucket1");

        let retryable = queue.get_retryable();
        assert!(
            retryable.is_empty(),
            "Entry with retry_count >= max_retry should not be retryable"
        );

        let mut queue2 = ReplicationQueue::new(3);
        queue2.enqueue(ObjectReplicationEntry::new("key2", "rule2", "bucket2"));
        queue2.mark_failed("key2", "rule2", "bucket2");
        queue2.mark_failed("key2", "rule2", "bucket2");

        let retryable2 = queue2.get_retryable();
        assert_eq!(
            retryable2.len(),
            1,
            "Entry with 2 failures should still be retryable"
        );
    }

    #[test]
    fn test_replication_queue_remove() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        let removed = queue.remove("key1", "rule1", "bucket1");
        assert!(removed, "Remove should return true for existing entry");

        assert_eq!(queue.len(), 0, "Queue should be empty after remove");

        let removed_again = queue.remove("key1", "rule1", "bucket1");
        assert!(!removed_again, "Remove non-existent should return false");
    }

    #[test]
    fn test_replication_status_variants() {
        assert!(matches!(
            ReplicationStatus::Pending,
            ReplicationStatus::Pending
        ));
        assert!(matches!(
            ReplicationStatus::Completed,
            ReplicationStatus::Completed
        ));
        assert!(matches!(
            ReplicationStatus::Failed,
            ReplicationStatus::Failed
        ));
        assert!(matches!(
            ReplicationStatus::Replica,
            ReplicationStatus::Replica
        ));
        assert!(matches!(
            ReplicationStatus::NotApplicable,
            ReplicationStatus::NotApplicable
        ));

        let entry = ObjectReplicationEntry::new("key", "rule", "bucket");
        assert!(
            matches!(entry.status, ReplicationStatus::Pending),
            "New entry should have Pending status by default"
        );
    }

    // =========================================================================
    // Category 5: Storage Class Management (5 tests)
    // =========================================================================

    #[test]
    fn test_storage_class_from_str_roundtrip() {
        assert_eq!(
            StorageClass::from_str("STANDARD"),
            Some(StorageClass::Standard),
            "STANDARD should parse to Standard"
        );
        assert_eq!(
            StorageClass::from_str("GLACIER"),
            Some(StorageClass::Glacier),
            "GLACIER should parse to Glacier"
        );
        assert_eq!(
            StorageClass::from_str("INVALID"),
            None,
            "INVALID should return None"
        );
        assert_eq!(
            StorageClass::Standard.as_str(),
            "STANDARD",
            "Standard.as_str() should return STANDARD"
        );
        assert_eq!(
            StorageClass::from_str("EXPRESS"),
            Some(StorageClass::Express),
            "EXPRESS should parse to Express"
        );
        assert_eq!(
            StorageClass::from_str("EXPRESS_ONEZONE"),
            Some(StorageClass::Express),
            "EXPRESS_ONEZONE should also parse to Express"
        );
    }

    #[test]
    fn test_storage_class_properties() {
        assert!(
            StorageClass::Standard.is_realtime(),
            "Standard should be realtime"
        );
        assert!(
            !StorageClass::Glacier.is_realtime(),
            "Glacier should NOT be realtime"
        );
        assert!(
            StorageClass::Glacier.requires_restore(),
            "Glacier should require restore"
        );
        assert!(
            !StorageClass::Standard.requires_restore(),
            "Standard should NOT require restore"
        );
        assert_eq!(
            StorageClass::GlacierDeepArchive.min_storage_days(),
            180,
            "GlacierDeepArchive min_storage_days should be 180"
        );
        assert_eq!(
            StorageClass::Standard.min_storage_days(),
            0,
            "Standard min_storage_days should be 0"
        );
        assert!(
            StorageClass::Express.cost_tier() > StorageClass::Standard.cost_tier(),
            "Express cost tier should be higher than Standard"
        );
        assert!(
            StorageClass::Standard.cost_tier() > StorageClass::GlacierDeepArchive.cost_tier(),
            "Standard cost tier should be higher than GlacierDeepArchive"
        );
    }

    #[test]
    fn test_evaluate_transitions() {
        let transitions = vec![
            StorageClassTransition::new(30, StorageClass::StandardIa),
            StorageClassTransition::new(90, StorageClass::Glacier),
            StorageClassTransition::new(180, StorageClass::GlacierDeepArchive),
        ];

        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 29, &transitions),
            None,
            "Age 29 should not trigger any transition"
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 30, &transitions),
            Some(StorageClass::StandardIa),
            "Age 30 should transition to StandardIa"
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 100, &transitions),
            Some(StorageClass::Glacier),
            "Age 100 should transition to Glacier"
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 200, &transitions),
            Some(StorageClass::GlacierDeepArchive),
            "Age 200 should transition to GlacierDeepArchive"
        );

        assert_eq!(
            evaluate_transitions(&StorageClass::Glacier, 100, &transitions),
            None,
            "Non-Standard source class should return None"
        );
    }

    #[test]
    fn test_object_storage_state_restore() {
        let mut state = ObjectStorageState::new(StorageClass::Glacier);
        assert!(state.needs_restore(), "Glacier state should need restore");

        state.start_restore(std::time::Duration::from_secs(3600));
        assert!(
            state.is_restoring,
            "Should be restoring after start_restore"
        );
        assert!(
            state.needs_restore(),
            "Should still need restore while restoring"
        );

        state.complete_restore();
        assert!(
            state.is_restored(),
            "Should be restored after complete_restore"
        );
        assert!(
            !state.needs_restore(),
            "Should NOT need restore after restore complete"
        );

        let standard_state = ObjectStorageState::new(StorageClass::Standard);
        assert!(
            !standard_state.needs_restore(),
            "Standard state should NOT need restore"
        );
    }

    #[test]
    fn test_restore_tier_properties() {
        assert_eq!(
            RestoreTier::Expedited.as_str(),
            "Expedited",
            "Expedited as_str should be Expedited"
        );
        assert_eq!(
            RestoreTier::Expedited.restore_duration().as_secs(),
            5 * 60,
            "Expedited restore should be 5 minutes"
        );
        assert_eq!(
            RestoreTier::Standard.restore_duration().as_secs(),
            5 * 60 * 60,
            "Standard restore should be 5 hours"
        );
        assert_eq!(
            RestoreTier::Bulk.restore_duration().as_secs(),
            12 * 60 * 60,
            "Bulk restore should be 12 hours"
        );
    }
}
