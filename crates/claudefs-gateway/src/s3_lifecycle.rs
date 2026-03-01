//! S3 Bucket Lifecycle Configuration management.
//!
//! Implements S3-compatible lifecycle rules for object expiration and storage class
//! transitions. Integrates with ClaudeFS tiered storage (D5) to move objects between
//! NVMe flash tier and S3 capacity tier based on access patterns and age.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, warn};

/// S3 storage class for transition targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageClass {
    Standard,
    IntelligentTiering,
    StandardIa,
    GlacierIr,
    Glacier,
    DeepArchive,
    CfsFlash,
    CfsCapacity,
}

/// Filter for which objects a lifecycle rule applies to
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleFilter {
    pub prefix: Option<String>,
    pub tag: Option<(String, String)>,
    pub object_size_greater_than: Option<u64>,
    pub object_size_less_than: Option<u64>,
}

impl LifecycleFilter {
    pub fn new() -> Self {
        Self {
            prefix: None,
            tag: None,
            object_size_greater_than: None,
            object_size_less_than: None,
        }
    }

    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            tag: None,
            object_size_greater_than: None,
            object_size_less_than: None,
        }
    }

    pub fn matches(&self, key: &str, size: u64, tags: &[(String, String)]) -> bool {
        if let Some(ref prefix) = self.prefix {
            if !key.starts_with(prefix) {
                debug!("filter rejects key {}: no prefix match", key);
                return false;
            }
        }

        if let Some(ref tag_filter) = self.tag {
            let (ref filter_key, ref filter_value) = tag_filter;
            let has_tag = tags
                .iter()
                .any(|(k, v)| k == filter_key && v == filter_value);
            if !has_tag {
                debug!("filter rejects key {}: tag not present", key);
                return false;
            }
        }

        if let Some(min_size) = self.object_size_greater_than {
            if size <= min_size {
                debug!(
                    "filter rejects key {}: size {} not greater than {}",
                    key, size, min_size
                );
                return false;
            }
        }

        if let Some(max_size) = self.object_size_less_than {
            if size >= max_size {
                debug!(
                    "filter rejects key {}: size {} not less than {}",
                    key, size, max_size
                );
                return false;
            }
        }

        true
    }
}

impl Default for LifecycleFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// A transition action — move objects to a storage class after N days
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionAction {
    pub days: u32,
    pub storage_class: StorageClass,
}

/// An expiration action — delete objects after N days
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpirationAction {
    pub days: u32,
    pub expire_object_delete_markers: bool,
}

impl ExpirationAction {
    pub fn new(days: u32) -> Self {
        Self {
            days,
            expire_object_delete_markers: false,
        }
    }

    pub fn with_delete_markers(mut self, expire: bool) -> Self {
        self.expire_object_delete_markers = expire;
        self
    }
}

/// Status of a lifecycle rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleStatus {
    Enabled,
    Disabled,
}

/// A single lifecycle rule
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleRule {
    pub id: String,
    pub filter: LifecycleFilter,
    pub status: RuleStatus,
    pub transitions: Vec<TransitionAction>,
    pub expiration: Option<ExpirationAction>,
}

impl LifecycleRule {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            filter: LifecycleFilter::new(),
            status: RuleStatus::Enabled,
            transitions: Vec::new(),
            expiration: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.status == RuleStatus::Enabled
    }

    pub fn next_transition(&self, days_old: u32) -> Option<&TransitionAction> {
        self.transitions
            .iter()
            .filter(|t| days_old >= t.days)
            .max_by_key(|t| t.days)
    }

    pub fn is_expired(&self, days_old: u32) -> bool {
        match self.expiration {
            Some(exp) => days_old >= exp.days,
            None => false,
        }
    }
}

/// Lifecycle configuration for a bucket — up to 1000 rules
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleConfiguration {
    pub rules: Vec<LifecycleRule>,
}

impl LifecycleConfiguration {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: LifecycleRule) -> Result<(), LifecycleError> {
        if self.rules.len() >= 1000 {
            warn!("too many lifecycle rules: {}", self.rules.len() + 1);
            return Err(LifecycleError::TooManyRules(self.rules.len() + 1));
        }

        if self.rules.iter().any(|r| r.id == rule.id) {
            warn!("duplicate rule id: {}", rule.id);
            return Err(LifecycleError::DuplicateRuleId(rule.id));
        }

        if rule.transitions.is_empty() && rule.expiration.is_none() {
            warn!("rule {} has no actions", rule.id);
            return Err(LifecycleError::NoActions(rule.id));
        }

        for t in &rule.transitions {
            if t.days == 0 {
                warn!("invalid transition days: 0");
                return Err(LifecycleError::InvalidDays(0));
            }
        }

        self.rules.push(rule);
        Ok(())
    }

    pub fn remove_rule(&mut self, id: &str) -> bool {
        let len_before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        let removed = self.rules.len() < len_before;
        if removed {
            debug!("removed lifecycle rule: {}", id);
        }
        removed
    }

    pub fn get_rule(&self, id: &str) -> Option<&LifecycleRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn enabled_rules(&self) -> impl Iterator<Item = &LifecycleRule> {
        self.rules.iter().filter(|r| r.is_enabled())
    }

    pub fn applicable_transitions(
        &self,
        key: &str,
        size: u64,
        tags: &[(String, String)],
        days_old: u32,
    ) -> Vec<&TransitionAction> {
        self.enabled_rules()
            .filter(|rule| rule.filter.matches(key, size, tags))
            .flat_map(|rule| {
                rule.transitions
                    .iter()
                    .filter(|t| days_old >= t.days)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn is_object_expired(
        &self,
        key: &str,
        size: u64,
        tags: &[(String, String)],
        days_old: u32,
    ) -> bool {
        self.enabled_rules()
            .any(|rule| rule.filter.matches(key, size, tags) && rule.is_expired(days_old))
    }
}

/// Registry mapping bucket names to their lifecycle configurations
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LifecycleRegistry {
    configs: HashMap<String, LifecycleConfiguration>,
}

impl LifecycleRegistry {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    pub fn set_config(&mut self, bucket: &str, config: LifecycleConfiguration) {
        debug!("setting lifecycle config for bucket: {}", bucket);
        self.configs.insert(bucket.to_string(), config);
    }

    pub fn get_config(&self, bucket: &str) -> Option<&LifecycleConfiguration> {
        self.configs.get(bucket)
    }

    pub fn delete_config(&mut self, bucket: &str) -> bool {
        let removed = self.configs.remove(bucket).is_some();
        if removed {
            debug!("deleted lifecycle config for bucket: {}", bucket);
        }
        removed
    }

    pub fn bucket_count(&self) -> usize {
        self.configs.len()
    }
}

/// Errors for lifecycle operations
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("too many rules: maximum 1000, got {0}")]
    TooManyRules(usize),
    #[error("duplicate rule id: {0}")]
    DuplicateRuleId(String),
    #[error("rule {0} has no actions (needs at least one transition or expiration)")]
    NoActions(String),
    #[error("invalid transition: days must be > 0, got {0}")]
    InvalidDays(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_matches_all_objects() {
        let filter = LifecycleFilter::new();
        assert!(filter.matches("any/key", 100, &[]));
        assert!(filter.matches("", 0, &[]));
        assert!(filter.matches("test.txt", 1000000, &[]));
    }

    #[test]
    fn test_filter_prefix_match() {
        let filter = LifecycleFilter::with_prefix("logs/");
        assert!(filter.matches("logs/2024/access.log", 100, &[]));
        assert!(filter.matches("logs/", 0, &[]));
        assert!(!filter.matches("archive/logs/file.txt", 100, &[]));
    }

    #[test]
    fn test_filter_prefix_no_match() {
        let filter = LifecycleFilter::with_prefix("documents/");
        assert!(!filter.matches("images/photo.jpg", 100, &[]));
        assert!(!filter.matches("doc.pdf", 100, &[]));
    }

    #[test]
    fn test_filter_size_range() {
        let mut filter = LifecycleFilter::new();
        filter.object_size_greater_than = Some(1000);
        filter.object_size_less_than = Some(10000);

        assert!(filter.matches("file.txt", 5000, &[]));
        assert!(!filter.matches("file.txt", 500, &[]));
        assert!(!filter.matches("file.txt", 15000, &[]));
    }

    #[test]
    fn test_filter_tag_match() {
        let mut filter = LifecycleFilter::new();
        filter.tag = Some(("env".to_string(), "prod".to_string()));

        assert!(filter.matches("file.txt", 100, &[("env".to_string(), "prod".to_string())]));
        assert!(filter.matches(
            "file.txt",
            100,
            &[
                ("other".to_string(), "value".to_string()),
                ("env".to_string(), "prod".to_string())
            ]
        ));
    }

    #[test]
    fn test_filter_tag_no_match() {
        let mut filter = LifecycleFilter::new();
        filter.tag = Some(("env".to_string(), "prod".to_string()));

        assert!(!filter.matches("file.txt", 100, &[("env".to_string(), "dev".to_string())]));
        assert!(!filter.matches("file.txt", 100, &[]));
    }

    #[test]
    fn test_rule_enabled_disabled() {
        let mut rule = LifecycleRule::new("test");
        assert!(rule.is_enabled());

        rule.status = RuleStatus::Disabled;
        assert!(!rule.is_enabled());

        rule.status = RuleStatus::Enabled;
        assert!(rule.is_enabled());
    }

    #[test]
    fn test_rule_next_transition_none() {
        let rule = LifecycleRule::new("test");
        assert!(rule.next_transition(30).is_none());
    }

    #[test]
    fn test_rule_next_transition_first() {
        let mut rule = LifecycleRule::new("test");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        rule.transitions.push(TransitionAction {
            days: 90,
            storage_class: StorageClass::Glacier,
        });

        assert_eq!(
            rule.next_transition(30).unwrap().storage_class,
            StorageClass::StandardIa
        );
        assert_eq!(
            rule.next_transition(100).unwrap().storage_class,
            StorageClass::Glacier
        );
    }

    #[test]
    fn test_rule_next_transition_not_yet() {
        let mut rule = LifecycleRule::new("test");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });

        assert!(rule.next_transition(29).is_none());
        assert!(rule.next_transition(0).is_none());
    }

    #[test]
    fn test_rule_is_expired_true() {
        let mut rule = LifecycleRule::new("test");
        rule.expiration = Some(ExpirationAction::new(30));

        assert!(rule.is_expired(30));
        assert!(rule.is_expired(100));
    }

    #[test]
    fn test_rule_is_expired_false() {
        let mut rule = LifecycleRule::new("test");
        rule.expiration = Some(ExpirationAction::new(30));

        assert!(!rule.is_expired(29));
        assert!(!rule.is_expired(0));
    }

    #[test]
    fn test_config_add_rule() {
        let mut config = LifecycleConfiguration::new();
        let mut rule = LifecycleRule::new("test-rule");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });

        assert!(config.add_rule(rule).is_ok());
        assert_eq!(config.rules.len(), 1);
    }

    #[test]
    fn test_config_duplicate_id_error() {
        let mut config = LifecycleConfiguration::new();

        let mut rule1 = LifecycleRule::new("test-rule");
        rule1.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        config.add_rule(rule1).unwrap();

        let mut rule2 = LifecycleRule::new("test-rule");
        rule2.expiration = Some(ExpirationAction::new(90));

        assert!(matches!(
            config.add_rule(rule2),
            Err(LifecycleError::DuplicateRuleId(_))
        ));
    }

    #[test]
    fn test_config_remove_rule() {
        let mut config = LifecycleConfiguration::new();

        let mut rule = LifecycleRule::new("test-rule");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        config.add_rule(rule).unwrap();

        assert!(config.remove_rule("test-rule"));
        assert!(!config.remove_rule("test-rule"));
        assert!(config.get_rule("test-rule").is_none());
    }

    #[test]
    fn test_config_enabled_rules_filter() {
        let mut config = LifecycleConfiguration::new();

        let mut rule1 = LifecycleRule::new("enabled");
        rule1.status = RuleStatus::Enabled;
        rule1.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });

        let mut rule2 = LifecycleRule::new("disabled");
        rule2.status = RuleStatus::Disabled;
        rule2.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::Glacier,
        });

        config.add_rule(rule1).unwrap();
        config.add_rule(rule2).unwrap();

        let enabled: Vec<_> = config.enabled_rules().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "enabled");
    }

    #[test]
    fn test_config_applicable_transitions() {
        let mut config = LifecycleConfiguration::new();

        let mut rule = LifecycleRule::new("rule1");
        rule.filter = LifecycleFilter::with_prefix("logs/");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        config.add_rule(rule).unwrap();

        let transitions = config.applicable_transitions("logs/2024/access.log", 1000, &[], 60);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].storage_class, StorageClass::StandardIa);

        let no_transitions = config.applicable_transitions("other/file.txt", 1000, &[], 60);
        assert!(no_transitions.is_empty());
    }

    #[test]
    fn test_config_is_object_expired() {
        let mut config = LifecycleConfiguration::new();

        let mut rule = LifecycleRule::new("expire-rule");
        rule.expiration = Some(ExpirationAction::new(30));
        config.add_rule(rule).unwrap();

        assert!(config.is_object_expired("any/file.txt", 1000, &[], 30));
        assert!(config.is_object_expired("any/file.txt", 1000, &[], 100));
        assert!(!config.is_object_expired("any/file.txt", 1000, &[], 29));
    }

    #[test]
    fn test_registry_set_get() {
        let mut registry = LifecycleRegistry::new();

        let mut config = LifecycleConfiguration::new();
        let mut rule = LifecycleRule::new("test");
        rule.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::CfsCapacity,
        });
        config.add_rule(rule).unwrap();

        registry.set_config("my-bucket", config);

        let retrieved = registry.get_config("my-bucket");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().rules.len(), 1);
    }

    #[test]
    fn test_registry_delete_config() {
        let mut registry = LifecycleRegistry::new();

        let config = LifecycleConfiguration::new();
        registry.set_config("my-bucket", config);

        assert!(registry.delete_config("my-bucket"));
        assert!(!registry.delete_config("my-bucket"));
        assert!(registry.get_config("my-bucket").is_none());
    }

    #[test]
    fn test_storage_class_variants() {
        assert_eq!(StorageClass::StandardIa, StorageClass::StandardIa);
        assert_ne!(StorageClass::StandardIa, StorageClass::Standard);
        assert_ne!(StorageClass::CfsFlash, StorageClass::CfsCapacity);
        assert_ne!(StorageClass::IntelligentTiering, StorageClass::Glacier);
        let sc = StorageClass::DeepArchive;
        assert_eq!(sc, sc.clone());
    }

    #[test]
    fn test_lifecycle_rule_no_actions_error() {
        let rule = LifecycleRule::new("no-actions");
        let mut config = LifecycleConfiguration::new();

        assert!(matches!(
            config.add_rule(rule),
            Err(LifecycleError::NoActions(_))
        ));
    }
}
