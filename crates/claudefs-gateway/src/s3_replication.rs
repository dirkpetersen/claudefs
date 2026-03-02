//! S3 Cross-Region Replication

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Replication status of an object
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicationStatus {
    /// Replication pending
    Pending,
    /// Replication completed successfully
    Completed,
    /// Replication failed
    Failed,
    /// This object IS a replica (don't re-replicate)
    Replica,
    /// Not subject to any replication rule
    NotApplicable,
}

/// Delete marker replication options
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteMarkerReplication {
    /// Enable delete marker replication
    Enabled,
    /// Disable delete marker replication
    Disabled,
}

/// Filter for which objects a rule applies to
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationFilter {
    /// Object key prefix filter
    pub prefix: Option<String>,
    /// Key-value tag filter
    pub tags: HashMap<String, String>,
}

impl Default for ReplicationFilter {
    fn default() -> Self {
        Self {
            prefix: None,
            tags: HashMap::new(),
        }
    }
}

/// Destination for replication
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationDestination {
    /// Target bucket name
    pub bucket: String,
    /// Target region (optional)
    pub region: Option<String>,
    /// Target storage class (e.g., "STANDARD_IA", "GLACIER")
    pub storage_class: Option<String>,
    /// Replicate changes to replicas?
    pub replica_modifications: bool,
}

impl ReplicationDestination {
    /// Creates a new replication destination
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            region: None,
            storage_class: None,
            replica_modifications: false,
        }
    }

    /// Sets the region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Sets the storage class
    pub fn with_storage_class(mut self, class: impl Into<String>) -> Self {
        self.storage_class = Some(class.into());
        self
    }

    /// Sets replica modifications flag
    pub fn with_replica_modifications(mut self, enabled: bool) -> Self {
        self.replica_modifications = enabled;
        self
    }
}

/// A single replication rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationRule {
    /// Unique rule identifier
    pub id: String,
    /// Higher priority rules evaluated first
    pub priority: u32,
    /// Whether the rule is active
    pub enabled: bool,
    /// Object filter (prefix and tags)
    pub filter: ReplicationFilter,
    /// Replication destination
    pub destination: ReplicationDestination,
    /// Delete marker replication setting
    pub delete_marker_replication: DeleteMarkerReplication,
    /// Rule creation timestamp
    pub create_time: Option<std::time::SystemTime>,
}

impl ReplicationRule {
    /// Creates a new replication rule
    pub fn new(id: impl Into<String>, destination: ReplicationDestination) -> Self {
        Self {
            id: id.into(),
            priority: 0,
            enabled: true,
            filter: ReplicationFilter::default(),
            destination,
            delete_marker_replication: DeleteMarkerReplication::Enabled,
            create_time: Some(std::time::SystemTime::now()),
        }
    }

    /// Sets the priority
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the prefix filter
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.filter.prefix = Some(prefix.into());
        self
    }

    /// Sets the tag filter
    pub fn with_filter_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.filter.tags = tags;
        self
    }

    /// Sets enabled status
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets delete marker replication
    pub fn with_delete_marker_replication(mut self, dmr: DeleteMarkerReplication) -> Self {
        self.delete_marker_replication = dmr;
        self
    }

    /// Check if this rule matches an object key and tags
    pub fn matches(&self, key: &str, tags: &HashMap<String, String>) -> bool {
        if !self.enabled {
            return false;
        }

        // Check prefix
        if let Some(ref prefix) = self.filter.prefix {
            if !key.starts_with(prefix) {
                return false;
            }
        }

        // Check tags (all rule tags must be present in object tags)
        for (k, v) in &self.filter.tags {
            match tags.get(k) {
                Some(obj_v) if obj_v == v => {}
                _ => return false,
            }
        }

        true
    }
}

/// Replication configuration for a bucket
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketReplicationConfig {
    /// IAM role ARN for replication
    pub role: String,
    /// Replication rules
    pub rules: Vec<ReplicationRule>,
}

impl BucketReplicationConfig {
    /// Creates a new bucket replication config
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            rules: Vec::new(),
        }
    }

    /// Adds a rule to the config
    pub fn with_rule(mut self, rule: ReplicationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Find matching rules for an object key and tags, sorted by priority desc
    pub fn matching_rules(
        &self,
        key: &str,
        tags: &HashMap<String, String>,
    ) -> Vec<&ReplicationRule> {
        let mut matching: Vec<&ReplicationRule> =
            self.rules.iter().filter(|r| r.matches(key, tags)).collect();

        // Sort by priority descending (higher priority first)
        matching.sort_by(|a, b| b.priority.cmp(&a.priority));
        matching
    }

    /// Check if key should be replicated; returns destinations
    pub fn destinations_for(
        &self,
        key: &str,
        tags: &HashMap<String, String>,
    ) -> Vec<&ReplicationDestination> {
        self.matching_rules(key, tags)
            .iter()
            .map(|r| &r.destination)
            .collect()
    }
}

/// Per-object replication tracking entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectReplicationEntry {
    /// Object key
    pub object_key: String,
    /// Rule ID that triggered this replication
    pub rule_id: String,
    /// Destination bucket
    pub destination_bucket: String,
    /// Current replication status
    pub status: ReplicationStatus,
    /// Last replication attempt time
    pub last_attempt: Option<std::time::SystemTime>,
    /// Number of retry attempts
    pub retry_count: u32,
    /// Error message if failed
    pub error_message: Option<String>,
}

impl ObjectReplicationEntry {
    /// Creates a new replication entry
    pub fn new(
        object_key: impl Into<String>,
        rule_id: impl Into<String>,
        destination_bucket: impl Into<String>,
    ) -> Self {
        Self {
            object_key: object_key.into(),
            rule_id: rule_id.into(),
            destination_bucket: destination_bucket.into(),
            status: ReplicationStatus::Pending,
            last_attempt: None,
            retry_count: 0,
            error_message: None,
        }
    }

    /// Sets the status
    pub fn with_status(mut self, status: ReplicationStatus) -> Self {
        self.status = status;
        self
    }

    fn entry_key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.object_key, self.rule_id, self.destination_bucket
        )
    }
}

/// Manager for tracking replication queue
#[derive(Debug)]
pub struct ReplicationQueue {
    entries: HashMap<String, ObjectReplicationEntry>,
    max_retry: u32,
}

impl ReplicationQueue {
    /// Creates a new replication queue
    pub fn new(max_retry: u32) -> Self {
        Self {
            entries: HashMap::new(),
            max_retry,
        }
    }

    /// Add an entry to the queue
    pub fn enqueue(&mut self, entry: ObjectReplicationEntry) {
        let key = entry.entry_key();
        debug!("Enqueueing replication entry: {}", key);
        self.entries.insert(key, entry);
    }

    /// Mark a replication as completed
    pub fn mark_completed(&mut self, object_key: &str, rule_id: &str, dest_bucket: &str) {
        let key = format!("{}/{}/{}", object_key, rule_id, dest_bucket);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.status = ReplicationStatus::Completed;
            info!("Marked replication as completed: {}", key);
        }
    }

    /// Mark a replication as failed
    pub fn mark_failed(&mut self, object_key: &str, rule_id: &str, dest_bucket: &str) {
        let key = format!("{}/{}/{}", object_key, rule_id, dest_bucket);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.status = ReplicationStatus::Failed;
            entry.retry_count += 1;
            warn!(
                "Marked replication as failed: {} (retry {})",
                key, entry.retry_count
            );
        }
    }

    /// Get count of pending replications
    pub fn pending_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.status == ReplicationStatus::Pending)
            .count()
    }

    /// Get count of failed replications
    pub fn failed_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.status == ReplicationStatus::Failed)
            .count()
    }

    /// Get all pending entries
    pub fn get_pending(&self) -> Vec<&ObjectReplicationEntry> {
        self.entries
            .values()
            .filter(|e| e.status == ReplicationStatus::Pending)
            .collect()
    }

    /// Get failed entries that can be retried
    pub fn get_retryable(&self) -> Vec<&ObjectReplicationEntry> {
        self.entries
            .values()
            .filter(|e| e.status == ReplicationStatus::Failed && e.retry_count < self.max_retry)
            .collect()
    }

    /// Remove an entry from the queue
    pub fn remove(&mut self, object_key: &str, rule_id: &str, dest_bucket: &str) -> bool {
        let key = format!("{}/{}/{}", object_key, rule_id, dest_bucket);
        self.entries.remove(&key).is_some()
    }

    /// Number of entries in the queue
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Replication errors
#[derive(Debug, Error)]
pub enum ReplicationError {
    #[error("Rule not found: {0}")]
    RuleNotFound(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Queue error: {0}")]
    QueueError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_replication_rule_matches_prefix() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest-bucket"))
            .with_prefix("logs/");

        assert!(rule.matches("logs/2024/file.txt", &make_tags(&[])));
        assert!(rule.matches("logs/app.log", &make_tags(&[])));
        assert!(!rule.matches("data/file.txt", &make_tags(&[])));
    }

    #[test]
    fn test_replication_rule_matches_tags() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest-bucket"))
            .with_filter_tags(make_tags(&[("env", "prod"), ("type", "archive")]));

        assert!(rule.matches(
            "data/file.txt",
            &make_tags(&[("env", "prod"), ("type", "archive")])
        ));
        assert!(!rule.matches("data/file.txt", &make_tags(&[("env", "prod")])));
        assert!(!rule.matches(
            "data/file.txt",
            &make_tags(&[("env", "dev"), ("type", "archive")])
        ));
    }

    #[test]
    fn test_replication_rule_priority_ordering() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication")
            .with_rule(
                ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
                    .with_priority(10),
            )
            .with_rule(
                ReplicationRule::new("rule2", ReplicationDestination::new("dest"))
                    .with_priority(50),
            )
            .with_rule(
                ReplicationRule::new("rule3", ReplicationDestination::new("dest"))
                    .with_priority(30),
            );

        let matches = config.matching_rules("data/file.txt", &make_tags(&[]));
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].id, "rule2");
        assert_eq!(matches[1].id, "rule3");
        assert_eq!(matches[2].id, "rule1");
    }

    #[test]
    fn test_multiple_destinations() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication")
            .with_rule(
                ReplicationRule::new("rule1", ReplicationDestination::new("bucket-us"))
                    .with_prefix("us/"),
            )
            .with_rule(
                ReplicationRule::new("rule2", ReplicationDestination::new("bucket-eu"))
                    .with_prefix("eu/"),
            );

        let dests = config.destinations_for("us/data.txt", &make_tags(&[]));
        assert_eq!(dests.len(), 1);
        assert_eq!(dests[0].bucket, "bucket-us");

        let dests2 = config.destinations_for("eu/data.txt", &make_tags(&[]));
        assert_eq!(dests2.len(), 1);
        assert_eq!(dests2[0].bucket, "bucket-eu");
    }

    #[test]
    fn test_delete_marker_replication() {
        let rule_enabled = ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
            .with_delete_marker_replication(DeleteMarkerReplication::Enabled);
        let rule_disabled = ReplicationRule::new("rule2", ReplicationDestination::new("dest"))
            .with_delete_marker_replication(DeleteMarkerReplication::Disabled);

        assert!(matches!(
            rule_enabled.delete_marker_replication,
            DeleteMarkerReplication::Enabled
        ));
        assert!(matches!(
            rule_disabled.delete_marker_replication,
            DeleteMarkerReplication::Disabled
        ));
    }

    #[test]
    fn test_queue_enqueue_and_pending_count() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));
        queue.enqueue(ObjectReplicationEntry::new("key2", "rule2", "bucket2"));

        assert_eq!(queue.pending_count(), 2);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_queue_mark_completed() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        queue.mark_completed("key1", "rule1", "bucket1");

        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.failed_count(), 0);
    }

    #[test]
    fn test_queue_mark_failed_and_retry() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        queue.mark_failed("key1", "rule1", "bucket1");
        assert_eq!(queue.failed_count(), 1);

        // Should still be retryable (retry_count=1 < max_retry=3)
        let retryable = queue.get_retryable();
        assert_eq!(retryable.len(), 1);

        queue.mark_failed("key1", "rule1", "bucket1");
        queue.mark_failed("key1", "rule1", "bucket1");

        // Now retry_count=3, should not be retryable
        let retryable = queue.get_retryable();
        assert_eq!(retryable.len(), 0);
    }

    #[test]
    fn test_get_pending() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));
        queue.enqueue(ObjectReplicationEntry::new("key2", "rule2", "bucket2"));

        let pending = queue.get_pending();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_filter_without_prefix_or_tags_matches_all() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest"));

        assert!(rule.matches("anything.txt", &make_tags(&[])));
        assert!(rule.matches("foo/bar/baz", &make_tags(&[])));
    }

    #[test]
    fn test_disabled_rule_does_not_match() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
            .with_enabled(false)
            .with_prefix("data/");

        assert!(!rule.matches("data/file.txt", &make_tags(&[])));
    }

    #[test]
    fn test_destination_builder() {
        let dest = ReplicationDestination::new("my-bucket")
            .with_region("us-west-2")
            .with_storage_class("STANDARD_IA")
            .with_replica_modifications(true);

        assert_eq!(dest.bucket, "my-bucket");
        assert_eq!(dest.region, Some("us-west-2".to_string()));
        assert_eq!(dest.storage_class, Some("STANDARD_IA".to_string()));
        assert!(dest.replica_modifications);
    }

    #[test]
    fn test_replication_status_values() {
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
    }

    #[test]
    fn test_config_matching_rules_filters_disabled() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication")
            .with_rule(
                ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
                    .with_enabled(true)
                    .with_prefix("enabled/"),
            )
            .with_rule(
                ReplicationRule::new("rule2", ReplicationDestination::new("dest"))
                    .with_enabled(false)
                    .with_prefix("disabled/"),
            );

        let matches = config.matching_rules("enabled/file.txt", &make_tags(&[]));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "rule1");

        let matches2 = config.matching_rules("disabled/file.txt", &make_tags(&[]));
        assert!(matches2.is_empty());
    }

    #[test]
    fn test_combined_prefix_and_tag_filter() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
            .with_prefix("logs/")
            .with_filter_tags(make_tags(&[("env", "prod")]));

        // Matches prefix AND tags
        assert!(rule.matches("logs/app.log", &make_tags(&[("env", "prod")])));

        // Matches prefix but not tags
        assert!(!rule.matches("logs/app.log", &make_tags(&[("env", "dev")])));

        // Matches tags but not prefix
        assert!(!rule.matches("data/app.log", &make_tags(&[("env", "prod")])));
    }

    #[test]
    fn test_remove_entry() {
        let mut queue = ReplicationQueue::new(3);
        queue.enqueue(ObjectReplicationEntry::new("key1", "rule1", "bucket1"));

        assert!(queue.remove("key1", "rule1", "bucket1"));
        assert_eq!(queue.len(), 0);

        // Remove non-existent should return false
        assert!(!queue.remove("key1", "rule1", "bucket1"));
    }

    #[test]
    fn test_empty_config() {
        let config = BucketReplicationConfig::new("arn:aws:iam::123:role/replication");

        let matches = config.matching_rules("anything", &make_tags(&[]));
        assert!(matches.is_empty());

        let dests = config.destinations_for("anything", &make_tags(&[]));
        assert!(dests.is_empty());
    }

    #[test]
    fn test_multiple_tags_require_all() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
            .with_filter_tags(make_tags(&[("a", "1"), ("b", "2"), ("c", "3")]));

        // All 3 tags present
        assert!(rule.matches(
            "file.txt",
            &make_tags(&[("a", "1"), ("b", "2"), ("c", "3")])
        ));

        // Only 2 tags present
        assert!(!rule.matches("file.txt", &make_tags(&[("a", "1"), ("b", "2")])));

        // All 3 but wrong value
        assert!(!rule.matches(
            "file.txt",
            &make_tags(&[("a", "1"), ("b", "2"), ("c", "99")])
        ));
    }

    #[test]
    fn test_rule_with_create_time() {
        let rule = ReplicationRule::new("rule1", ReplicationDestination::new("dest"))
            .with_prefix("data/")
            .with_priority(5);

        assert!(rule.create_time.is_some());
    }
}
