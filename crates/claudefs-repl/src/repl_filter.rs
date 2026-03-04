//! Selective replication filter for per-site replication policies.
//!
//! Allows include/exclude of journal entries based on operation type, path prefix,
//! or metadata attributes to reduce bandwidth for non-critical metadata changes.

use std::collections::HashMap;
use thiserror::Error;

/// Operation types that can be filtered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OpType {
    /// File creation.
    Create,
    /// File deletion.
    Delete,
    /// Rename operation.
    Rename,
    /// Set attributes.
    SetAttr,
    /// Set extended attributes.
    SetXattr,
    /// Create hard link.
    Link,
    /// Remove hard link.
    Unlink,
    /// Create directory.
    MkDir,
    /// Remove directory.
    RmDir,
    /// Write data.
    Write,
    /// Truncate file.
    Truncate,
    /// Other operation.
    Other,
}

/// A single filter rule.
#[derive(Debug, Clone)]
pub struct FilterRule {
    /// Unique rule ID.
    pub rule_id: u64,
    /// Human-readable description.
    pub description: String,
    /// Action: Include or Exclude.
    pub action: FilterAction,
    /// If Some, only match entries for these op types.
    pub op_types: Option<Vec<OpType>>,
    /// If Some, only match entries whose path starts with this prefix.
    pub path_prefix: Option<String>,
    /// If Some, only match if inode is in this range (inclusive).
    pub inode_range: Option<(u64, u64)>,
    /// Rule priority (lower number = higher priority).
    pub priority: u32,
    /// Whether this rule is enabled.
    pub enabled: bool,
}

/// Whether a rule includes or excludes matching entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterAction {
    /// Replicate matching entries.
    Include,
    /// Do NOT replicate matching entries.
    Exclude,
}

/// What the filter decided for an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterDecision {
    /// Entry should be replicated.
    Replicate,
    /// Entry should be dropped (not replicated).
    Drop,
    /// No rule matched; use default policy.
    Default,
}

/// An entry to be evaluated by the filter.
#[derive(Debug, Clone)]
pub struct FilterEntry {
    /// Entry sequence number.
    pub seq: u64,
    /// Operation type.
    pub op_type: OpType,
    /// Path of the affected inode (may be empty for some op types).
    pub path: String,
    /// Inode number.
    pub inode: u64,
    /// Size in bytes (for writes).
    pub size: u64,
}

/// Statistics for filter evaluation.
#[derive(Debug, Default)]
pub struct FilterStats {
    /// Total entries evaluated.
    pub total_evaluated: u64,
    /// Total entries replicated.
    pub total_replicated: u64,
    /// Total entries dropped.
    pub total_dropped: u64,
    /// Total entries that used default policy.
    pub total_default: u64,
    /// Rule match counts (rule_id -> match count).
    pub rules_matched: HashMap<u64, u64>,
}

/// Error types for filter operations.
#[derive(Debug, Error)]
pub enum FilterError {
    /// Rule with this ID already exists.
    #[error("duplicate rule id: {rule_id}")]
    DuplicateRuleId {
        /// The duplicate rule ID.
        rule_id: u64,
    },
    /// Rule with this ID not found.
    #[error("rule not found: {rule_id}")]
    RuleNotFound {
        /// The rule ID that was not found.
        rule_id: u64,
    },
    /// Too many rules (limit: 256).
    #[error("too many rules (limit: 256)")]
    TooManyRules,
}

/// Selective replication filter.
#[derive(Debug)]
pub struct ReplFilter {
    rules: Vec<FilterRule>,
    stats: FilterStats,
    /// Default policy when no rule matches.
    default_policy: FilterAction,
}

impl ReplFilter {
    /// Create a new filter. Default policy applies when no rule matches.
    pub fn new(default_policy: FilterAction) -> Self {
        Self {
            rules: Vec::new(),
            stats: FilterStats::default(),
            default_policy,
        }
    }

    /// Add a filter rule. Rules are sorted by priority after insertion.
    pub fn add_rule(&mut self, rule: FilterRule) -> Result<(), FilterError> {
        if self.rules.len() >= 256 {
            return Err(FilterError::TooManyRules);
        }

        if self.rules.iter().any(|r| r.rule_id == rule.rule_id) {
            return Err(FilterError::DuplicateRuleId {
                rule_id: rule.rule_id,
            });
        }

        self.rules.push(rule);
        self.rules.sort_by_key(|r| r.priority);
        Ok(())
    }

    /// Remove a rule by ID.
    pub fn remove_rule(&mut self, rule_id: u64) -> Result<FilterRule, FilterError> {
        let idx = self
            .rules
            .iter()
            .position(|r| r.rule_id == rule_id)
            .ok_or(FilterError::RuleNotFound { rule_id })?;
        Ok(self.rules.remove(idx))
    }

    /// Enable or disable a rule.
    pub fn set_rule_enabled(&mut self, rule_id: u64, enabled: bool) -> Result<(), FilterError> {
        let rule = self
            .rules
            .iter_mut()
            .find(|r| r.rule_id == rule_id)
            .ok_or(FilterError::RuleNotFound { rule_id })?;
        rule.enabled = enabled;
        Ok(())
    }

    /// Evaluate an entry against all rules (highest priority first).
    /// Returns the filter decision and the matching rule ID (if any).
    pub fn evaluate(&mut self, entry: &FilterEntry) -> (FilterDecision, Option<u64>) {
        self.stats.total_evaluated += 1;

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            if self.rule_matches(rule, entry) {
                let decision = match rule.action {
                    FilterAction::Include => FilterDecision::Replicate,
                    FilterAction::Exclude => FilterDecision::Drop,
                };

                *self.stats.rules_matched.entry(rule.rule_id).or_insert(0) += 1;

                match &decision {
                    FilterDecision::Replicate => self.stats.total_replicated += 1,
                    FilterDecision::Drop => self.stats.total_dropped += 1,
                    FilterDecision::Default => {}
                }

                return (decision, Some(rule.rule_id));
            }
        }

        self.stats.total_default += 1;
        let decision = match self.default_policy {
            FilterAction::Include => FilterDecision::Replicate,
            FilterAction::Exclude => FilterDecision::Drop,
        };
        (decision, None)
    }

    fn rule_matches(&self, rule: &FilterRule, entry: &FilterEntry) -> bool {
        if let Some(ref op_types) = rule.op_types {
            if !op_types.contains(&entry.op_type) {
                return false;
            }
        }

        if let Some(ref prefix) = rule.path_prefix {
            if !entry.path.starts_with(prefix) {
                return false;
            }
        }

        if let Some((lo, hi)) = rule.inode_range {
            if entry.inode < lo || entry.inode > hi {
                return false;
            }
        }

        true
    }

    /// Get current statistics.
    pub fn stats(&self) -> &FilterStats {
        &self.stats
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.stats = FilterStats::default();
    }

    /// List all rules sorted by priority.
    pub fn rules(&self) -> &[FilterRule] {
        &self.rules
    }

    /// Count of currently enabled rules.
    pub fn enabled_rule_count(&self) -> usize {
        self.rules.iter().filter(|r| r.enabled).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(op: OpType, path: &str, inode: u64) -> FilterEntry {
        FilterEntry {
            seq: 1,
            op_type: op,
            path: path.to_string(),
            inode,
            size: 0,
        }
    }

    #[test]
    fn test_no_rules_default_include_returns_replicate() {
        let mut filter = ReplFilter::new(FilterAction::Include);
        let entry = make_entry(OpType::Create, "/test", 1);
        let (decision, rule_id) = filter.evaluate(&entry);

        assert_eq!(decision, FilterDecision::Replicate);
        assert!(rule_id.is_none());
    }

    #[test]
    fn test_no_rules_default_exclude_returns_drop() {
        let mut filter = ReplFilter::new(FilterAction::Exclude);
        let entry = make_entry(OpType::Create, "/test", 1);
        let (decision, rule_id) = filter.evaluate(&entry);

        assert_eq!(decision, FilterDecision::Drop);
        assert!(rule_id.is_none());
    }

    #[test]
    fn test_exclude_by_path_prefix() {
        let mut filter = ReplFilter::new(FilterAction::Include);
        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Exclude /tmp".to_string(),
                action: FilterAction::Exclude,
                op_types: None,
                path_prefix: Some("/tmp".to_string()),
                inode_range: None,
                priority: 10,
                enabled: true,
            })
            .unwrap();

        let entry = make_entry(OpType::Create, "/tmp/file", 1);
        let (decision, _) = filter.evaluate(&entry);

        assert_eq!(decision, FilterDecision::Drop);
    }

    #[test]
    fn test_include_by_op_type() {
        let mut filter = ReplFilter::new(FilterAction::Exclude);
        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Include creates".to_string(),
                action: FilterAction::Include,
                op_types: Some(vec![OpType::Create, OpType::MkDir]),
                path_prefix: None,
                inode_range: None,
                priority: 10,
                enabled: true,
            })
            .unwrap();

        let create_entry = make_entry(OpType::Create, "/test", 1);
        let (decision, _) = filter.evaluate(&create_entry);
        assert_eq!(decision, FilterDecision::Replicate);

        let write_entry = make_entry(OpType::Write, "/test", 2);
        let (decision2, _) = filter.evaluate(&write_entry);
        assert_eq!(decision2, FilterDecision::Drop);
    }

    #[test]
    fn test_rule_priority_ordering_lower_priority_wins() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Low priority exclude".to_string(),
                action: FilterAction::Exclude,
                path_prefix: Some("/data".to_string()),
                priority: 100,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        filter
            .add_rule(FilterRule {
                rule_id: 2,
                description: "High priority include".to_string(),
                action: FilterAction::Include,
                path_prefix: Some("/data/important".to_string()),
                priority: 1,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let entry = make_entry(OpType::Create, "/data/important/file", 1);
        let (decision, rule_id) = filter.evaluate(&entry);

        assert_eq!(decision, FilterDecision::Replicate);
        assert_eq!(rule_id, Some(2));
    }

    #[test]
    fn test_disabled_rule_is_skipped() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Disabled exclude".to_string(),
                action: FilterAction::Exclude,
                path_prefix: Some("/tmp".to_string()),
                priority: 10,
                enabled: false,
                ..Default::default()
            })
            .unwrap();

        let entry = make_entry(OpType::Create, "/tmp/file", 1);
        let (decision, _) = filter.evaluate(&entry);

        assert_eq!(decision, FilterDecision::Replicate);
    }

    #[test]
    fn test_add_duplicate_rule_id_returns_error() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "First".to_string(),
                action: FilterAction::Include,
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let result = filter.add_rule(FilterRule {
            rule_id: 1,
            description: "Duplicate".to_string(),
            action: FilterAction::Exclude,
            priority: 20,
            enabled: true,
            ..Default::default()
        });

        assert!(matches!(result, Err(FilterError::DuplicateRuleId { .. })));
    }

    #[test]
    fn test_remove_rule_works() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Test".to_string(),
                action: FilterAction::Exclude,
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let removed = filter.remove_rule(1).unwrap();
        assert_eq!(removed.rule_id, 1);

        assert_eq!(filter.rules().len(), 0);
    }

    #[test]
    fn test_enable_disable_rule() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Test".to_string(),
                action: FilterAction::Exclude,
                priority: 10,
                enabled: false,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(filter.enabled_rule_count(), 0);

        filter.set_rule_enabled(1, true).unwrap();
        assert_eq!(filter.enabled_rule_count(), 1);

        filter.set_rule_enabled(1, false).unwrap();
        assert_eq!(filter.enabled_rule_count(), 0);
    }

    #[test]
    fn test_multiple_conditions_and_logic() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Large writes only".to_string(),
                action: FilterAction::Exclude,
                op_types: Some(vec![OpType::Write]),
                path_prefix: Some("/data".to_string()),
                inode_range: Some((1000, 2000)),
                priority: 10,
                enabled: true,
            })
            .unwrap();

        let entry1 = FilterEntry {
            seq: 1,
            op_type: OpType::Write,
            path: "/data/file".to_string(),
            inode: 1500,
            size: 1000,
        };
        let (decision1, _) = filter.evaluate(&entry1);
        assert_eq!(decision1, FilterDecision::Drop);

        let entry2 = FilterEntry {
            seq: 2,
            op_type: OpType::Write,
            path: "/data/file".to_string(),
            inode: 500,
            size: 1000,
        };
        let (decision2, _) = filter.evaluate(&entry2);
        assert_eq!(decision2, FilterDecision::Replicate);
    }

    #[test]
    fn test_stats_track_correctly() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Exclude".to_string(),
                action: FilterAction::Exclude,
                path_prefix: Some("/drop".to_string()),
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        filter.evaluate(&make_entry(OpType::Create, "/keep", 1));
        filter.evaluate(&make_entry(OpType::Create, "/drop", 2));
        filter.evaluate(&make_entry(OpType::Create, "/keep2", 3));

        let stats = filter.stats();
        assert_eq!(stats.total_evaluated, 3);
        assert_eq!(stats.total_replicated, 2);
        assert_eq!(stats.total_dropped, 1);
        assert_eq!(stats.rules_matched.get(&1), Some(&1));
    }

    #[test]
    fn test_inode_range_matching() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Exclude high inodes".to_string(),
                action: FilterAction::Exclude,
                inode_range: Some((1000, u64::MAX)),
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let (decision1, _) = filter.evaluate(&make_entry(OpType::Create, "/test", 500));
        assert_eq!(decision1, FilterDecision::Replicate);

        let (decision2, _) = filter.evaluate(&make_entry(OpType::Create, "/test", 1500));
        assert_eq!(decision2, FilterDecision::Drop);
    }

    #[test]
    fn test_path_prefix_matching_empty_path() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "Include /".to_string(),
                action: FilterAction::Include,
                path_prefix: Some("/".to_string()),
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let entry = make_entry(OpType::Create, "/anything", 1);
        let (decision, _) = filter.evaluate(&entry);
        assert_eq!(decision, FilterDecision::Replicate);
    }

    #[test]
    fn test_rule_count_methods() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "1".to_string(),
                action: FilterAction::Include,
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        filter
            .add_rule(FilterRule {
                rule_id: 2,
                description: "2".to_string(),
                action: FilterAction::Include,
                priority: 20,
                enabled: false,
                ..Default::default()
            })
            .unwrap();
        filter
            .add_rule(FilterRule {
                rule_id: 3,
                description: "3".to_string(),
                action: FilterAction::Include,
                priority: 30,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(filter.rules().len(), 3);
        assert_eq!(filter.enabled_rule_count(), 2);
    }

    #[test]
    fn test_reset_stats() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter.evaluate(&make_entry(OpType::Create, "/test", 1));

        filter.reset_stats();

        let stats = filter.stats();
        assert_eq!(stats.total_evaluated, 0);
        assert_eq!(stats.total_replicated, 0);
    }

    #[test]
    fn test_rules_sorted_by_priority() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        filter
            .add_rule(FilterRule {
                rule_id: 3,
                description: "c".to_string(),
                action: FilterAction::Include,
                priority: 30,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        filter
            .add_rule(FilterRule {
                rule_id: 1,
                description: "a".to_string(),
                action: FilterAction::Include,
                priority: 10,
                enabled: true,
                ..Default::default()
            })
            .unwrap();
        filter
            .add_rule(FilterRule {
                rule_id: 2,
                description: "b".to_string(),
                action: FilterAction::Include,
                priority: 20,
                enabled: true,
                ..Default::default()
            })
            .unwrap();

        let rules = filter.rules();
        assert_eq!(rules[0].rule_id, 1);
        assert_eq!(rules[1].rule_id, 2);
        assert_eq!(rules[2].rule_id, 3);
    }

    #[test]
    fn test_too_many_rules_returns_error() {
        let mut filter = ReplFilter::new(FilterAction::Include);

        for i in 0u32..256u32 {
            filter
                .add_rule(FilterRule {
                    rule_id: i as u64,
                    description: format!("Rule {}", i),
                    action: FilterAction::Include,
                    priority: i,
                    enabled: true,
                    ..Default::default()
                })
                .unwrap();
        }

        let result = filter.add_rule(FilterRule {
            rule_id: 999,
            description: "One more".to_string(),
            action: FilterAction::Include,
            priority: 999,
            enabled: true,
            ..Default::default()
        });

        assert!(matches!(result, Err(FilterError::TooManyRules)));
    }

    #[test]
    fn test_proptest_random_entries_with_no_rules() {
        use proptest::prelude::*;
        use proptest::sample::select;

        const OP_TYPES: [OpType; 12] = [
            OpType::Create,
            OpType::Delete,
            OpType::Rename,
            OpType::SetAttr,
            OpType::SetXattr,
            OpType::Link,
            OpType::Unlink,
            OpType::MkDir,
            OpType::RmDir,
            OpType::Write,
            OpType::Truncate,
            OpType::Other,
        ];

        proptest! {
            #[test]
            fn test_random_entries_default_policy(
                op in select(&OP_TYPES),
                path in "[a-z/]*",
                inode in 0u64..1000u64
            ) {
                let mut filter = ReplFilter::new(FilterAction::Include);
                let entry = FilterEntry {
                    seq: 1,
                    op_type: op,
                    path: path.to_string(),
                    inode,
                    size: 0,
                };
                let (decision, _) = filter.evaluate(&entry);
                assert_eq!(decision, FilterDecision::Replicate);

                let mut filter2 = ReplFilter::new(FilterAction::Exclude);
                let entry2 = entry.clone();
                let (decision2, _) = filter2.evaluate(&entry2);
                assert_eq!(decision2, FilterDecision::Drop);
            }
        }
    }

    #[test]
    fn test_remove_nonexistent_rule_returns_error() {
        let mut filter = ReplFilter::new(FilterAction::Include);
        let result = filter.remove_rule(999);
        assert!(matches!(result, Err(FilterError::RuleNotFound { .. })));
    }

    #[test]
    fn test_set_enabled_nonexistent_rule_returns_error() {
        let mut filter = ReplFilter::new(FilterAction::Include);
        let result = filter.set_rule_enabled(999, true);
        assert!(matches!(result, Err(FilterError::RuleNotFound { .. })));
    }

    #[test]
    fn test_filter_decision_derives_partial_eq() {
        assert_eq!(FilterDecision::Replicate, FilterDecision::Replicate);
        assert_eq!(FilterDecision::Drop, FilterDecision::Drop);
        assert_eq!(FilterDecision::Default, FilterDecision::Default);
        assert_ne!(FilterDecision::Replicate, FilterDecision::Drop);
    }
}

impl Default for FilterRule {
    fn default() -> Self {
        Self {
            rule_id: 0,
            description: String::new(),
            action: FilterAction::Include,
            op_types: None,
            path_prefix: None,
            inode_range: None,
            priority: 100,
            enabled: true,
        }
    }
}
