//! S3-compatible event notification routing

use std::collections::HashMap;
use thiserror::Error;

/// S3 notification event types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationEvent {
    ObjectCreated,
    ObjectRemoved,
    ObjectRestored,
    ReducedRedundancyLostObject,
}

impl NotificationEvent {
    /// Returns the S3-style event name string (e.g. "s3:ObjectCreated:*")
    pub fn event_name(&self) -> &'static str {
        match self {
            NotificationEvent::ObjectCreated => "s3:ObjectCreated:*",
            NotificationEvent::ObjectRemoved => "s3:ObjectRemoved:*",
            NotificationEvent::ObjectRestored => "s3:ObjectRestored:*",
            NotificationEvent::ReducedRedundancyLostObject => "s3:ReducedRedundancyLostObject",
        }
    }
}

/// Notification destination type
#[derive(Debug, Clone)]
pub enum NotificationDestination {
    Webhook { url: String, secret: Option<String> },
    InProcess { queue_name: String },
}

/// A notification filter — matches by key prefix/suffix
#[derive(Debug, Clone, Default)]
pub struct NotificationFilter {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
}

impl NotificationFilter {
    pub fn new() -> Self {
        Self {
            prefix: None,
            suffix: None,
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    pub fn with_suffix(mut self, suffix: &str) -> Self {
        self.suffix = Some(suffix.to_string());
        self
    }

    /// Returns true if the given key matches this filter
    pub fn matches(&self, key: &str) -> bool {
        if let Some(ref prefix) = self.prefix {
            if !key.starts_with(prefix) {
                return false;
            }
        }
        if let Some(ref suffix) = self.suffix {
            if !key.ends_with(suffix) {
                return false;
            }
        }
        true
    }
}

/// A notification configuration (one subscription)
pub struct NotificationConfig {
    pub id: String,
    pub events: Vec<NotificationEvent>,
    pub filter: NotificationFilter,
    pub destination: NotificationDestination,
    pub enabled: bool,
}

impl NotificationConfig {
    pub fn new(
        id: &str,
        events: Vec<NotificationEvent>,
        filter: NotificationFilter,
        destination: NotificationDestination,
    ) -> Self {
        Self {
            id: id.to_string(),
            events,
            filter,
            destination,
            enabled: true,
        }
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }
}

/// An event that was triggered
pub struct NotificationRecord {
    pub event: NotificationEvent,
    pub bucket: String,
    pub key: String,
    pub size_bytes: u64,
    pub etag: Option<String>,
}

/// Manages notification subscriptions per bucket
pub struct NotificationManager {
    configs: HashMap<String, Vec<NotificationConfig>>,
    delivered_count: u64,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            delivered_count: 0,
        }
    }

    /// Register a notification config for a bucket
    pub fn register(&mut self, bucket: &str, config: NotificationConfig) {
        self.configs
            .entry(bucket.to_string())
            .or_insert_with(Vec::new)
            .push(config);
    }

    /// Remove a notification config by ID from a bucket
    pub fn remove(&mut self, bucket: &str, id: &str) -> Result<(), NotificationError> {
        let configs = self
            .configs
            .get_mut(bucket)
            .ok_or_else(|| NotificationError::NoBucketConfig(bucket.to_string()))?;

        let initial_len = configs.len();
        configs.retain(|c| c.id != id);

        if configs.len() == initial_len {
            return Err(NotificationError::NotFound(id.to_string()));
        }

        Ok(())
    }

    /// Get all configs for a bucket
    pub fn configs_for(&self, bucket: &str) -> &[NotificationConfig] {
        self.configs
            .get(bucket)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Find matching configs for an event+key
    pub fn matching_configs(
        &self,
        bucket: &str,
        event: &NotificationEvent,
        key: &str,
    ) -> Vec<&NotificationConfig> {
        let configs = match self.configs.get(bucket) {
            Some(c) => c,
            None => return Vec::new(),
        };

        configs
            .iter()
            .filter(|c| c.enabled && c.events.contains(event) && c.filter.matches(key))
            .collect()
    }

    /// Record that an event was delivered (increments counter)
    pub fn record_delivery(&mut self) {
        self.delivered_count += 1;
    }

    /// Total delivered count
    pub fn delivered_count(&self) -> u64 {
        self.delivered_count
    }

    /// Count enabled configs across all buckets
    pub fn enabled_config_count(&self) -> usize {
        self.configs
            .values()
            .map(|configs| configs.iter().filter(|c| c.enabled).count())
            .sum()
    }
}

/// Error type
#[derive(Debug, Error)]
pub enum NotificationError {
    #[error("config not found: {0}")]
    NotFound(String),
    #[error("bucket has no notification configs: {0}")]
    NoBucketConfig(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_event_event_name_object_created() {
        assert_eq!(
            NotificationEvent::ObjectCreated.event_name(),
            "s3:ObjectCreated:*"
        );
    }

    #[test]
    fn test_notification_event_event_name_object_removed() {
        assert_eq!(
            NotificationEvent::ObjectRemoved.event_name(),
            "s3:ObjectRemoved:*"
        );
    }

    #[test]
    fn test_notification_event_event_name_object_restored() {
        assert_eq!(
            NotificationEvent::ObjectRestored.event_name(),
            "s3:ObjectRestored:*"
        );
    }

    #[test]
    fn test_notification_event_event_name_reduced_redundancy() {
        assert_eq!(
            NotificationEvent::ReducedRedundancyLostObject.event_name(),
            "s3:ReducedRedundancyLostObject"
        );
    }

    #[test]
    fn test_notification_filter_default_has_no_prefix_suffix() {
        let filter = NotificationFilter::default();
        assert!(filter.prefix.is_none());
        assert!(filter.suffix.is_none());
    }

    #[test]
    fn test_notification_filter_with_prefix_sets_prefix() {
        let filter = NotificationFilter::new().with_prefix("logs/");
        assert_eq!(filter.prefix, Some("logs/".to_string()));
    }

    #[test]
    fn test_notification_filter_with_suffix_sets_suffix() {
        let filter = NotificationFilter::new().with_suffix(".txt");
        assert_eq!(filter.suffix, Some(".txt".to_string()));
    }

    #[test]
    fn test_notification_filter_matches_returns_true_when_key_has_matching_prefix() {
        let filter = NotificationFilter::new().with_prefix("logs/");
        assert!(filter.matches("logs/app.log"));
        assert!(filter.matches("logs/"));
    }

    #[test]
    fn test_notification_filter_matches_returns_true_when_key_has_matching_suffix() {
        let filter = NotificationFilter::new().with_suffix(".log");
        assert!(filter.matches("app.log"));
        assert!(filter.matches("path/to/file.log"));
    }

    #[test]
    fn test_notification_filter_matches_returns_false_when_key_doesnt_match() {
        let filter = NotificationFilter::new().with_prefix("logs/");
        assert!(!filter.matches("data/file.txt"));
        assert!(!filter.matches("logs"));

        let filter2 = NotificationFilter::new().with_suffix(".txt");
        assert!(!filter2.matches("file.log"));
    }

    #[test]
    fn test_notification_filter_matches_returns_true_when_no_filter_set() {
        let filter = NotificationFilter::new();
        assert!(filter.matches("any/key/here"));
        assert!(filter.matches(""));
        assert!(filter.matches("simple"));
    }

    #[test]
    fn test_notification_config_new_creates_enabled_config() {
        let config = NotificationConfig::new(
            "test-id",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "default".to_string(),
            },
        );
        assert_eq!(config.id, "test-id");
        assert!(config.enabled);
    }

    #[test]
    fn test_notification_config_disable_sets_enabled_false() {
        let mut config = NotificationConfig::new(
            "test-id",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "default".to_string(),
            },
        );
        config.disable();
        assert!(!config.enabled);
    }

    #[test]
    fn test_notification_config_enable_sets_enabled_true() {
        let mut config = NotificationConfig::new(
            "test-id",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "default".to_string(),
            },
        );
        config.disable();
        config.enable();
        assert!(config.enabled);
    }

    #[test]
    fn test_notification_manager_new_is_empty() {
        let manager = NotificationManager::new();
        assert_eq!(manager.delivered_count(), 0);
        assert_eq!(manager.enabled_config_count(), 0);
    }

    #[test]
    fn test_register_adds_config_to_bucket() {
        let mut manager = NotificationManager::new();
        let config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("my-bucket", config);
        assert_eq!(manager.configs_for("my-bucket").len(), 1);
    }

    #[test]
    fn test_configs_for_returns_registered_configs() {
        let mut manager = NotificationManager::new();
        let config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("my-bucket", config);
        let configs = manager.configs_for("my-bucket");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].id, "config-1");
    }

    #[test]
    fn test_configs_for_returns_empty_slice_for_unknown_bucket() {
        let manager = NotificationManager::new();
        let configs = manager.configs_for("unknown-bucket");
        assert!(configs.is_empty());
    }

    #[test]
    fn test_matching_configs_finds_configs_matching_event_key() {
        let mut manager = NotificationManager::new();
        let config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("my-bucket", config);

        let matches = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "any/key.txt",
        );
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_matching_configs_does_not_return_disabled_configs() {
        let mut manager = NotificationManager::new();
        let mut config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        config.disable();
        manager.register("my-bucket", config);

        let matches = manager.matching_configs(
            "my-bucket",
            &NotificationEvent::ObjectCreated,
            "any/key.txt",
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn test_remove_deletes_config_by_id() {
        let mut manager = NotificationManager::new();
        let config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("my-bucket", config);

        manager.remove("my-bucket", "config-1").unwrap();
        assert!(manager.configs_for("my-bucket").is_empty());
    }

    #[test]
    fn test_remove_returns_not_found_for_unknown_id() {
        let mut manager = NotificationManager::new();
        let config = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("my-bucket", config);

        let result = manager.remove("my-bucket", "unknown-id");
        assert!(matches!(result, Err(NotificationError::NotFound(_))));
    }

    #[test]
    fn test_record_delivery_increments_counter() {
        let mut manager = NotificationManager::new();
        assert_eq!(manager.delivered_count(), 0);

        manager.record_delivery();
        assert_eq!(manager.delivered_count(), 1);

        manager.record_delivery();
        assert_eq!(manager.delivered_count(), 2);
    }

    #[test]
    fn test_enabled_config_count_counts_only_enabled_configs() {
        let mut manager = NotificationManager::new();

        let config1 = NotificationConfig::new(
            "config-1",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue1".to_string(),
            },
        );
        manager.register("bucket1", config1);

        let mut config2 = NotificationConfig::new(
            "config-2",
            vec![NotificationEvent::ObjectCreated],
            NotificationFilter::new(),
            NotificationDestination::InProcess {
                queue_name: "queue2".to_string(),
            },
        );
        config2.disable();
        manager.register("bucket1", config2);

        assert_eq!(manager.enabled_config_count(), 1);
    }
}
