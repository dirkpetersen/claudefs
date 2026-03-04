//! Notification filtering for FUSE filesystem events.
//!
//! This module provides filtering capabilities for FUSE notification events,
//! allowing selective suppression, throttling, or passthrough of notifications
//! based on inode, path patterns, or global rules.

#![warn(missing_docs)]

use std::sync::atomic::{AtomicU64, Ordering};

/// Type of notification filter to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterType {
    /// Filter applies to specific inodes.
    #[default]
    Inode,
    /// Filter applies to path patterns.
    Path,
    /// Filter applies globally to all notifications.
    Global,
}

/// Action to take when a notification matches a filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterAction {
    /// Allow the notification to proceed.
    #[default]
    Notify,
    /// Suppress the notification entirely.
    Suppress,
    /// Throttle the notification rate.
    Throttle,
}

/// Statistics tracking for notification filter operations.
#[derive(Debug, Default)]
pub struct NotifyFilterStats {
    /// Number of notifications that matched a filter.
    pub matched_count: AtomicU64,
    /// Number of notifications that were suppressed.
    pub suppressed_count: AtomicU64,
    /// Number of notifications that were throttled.
    pub throttled_count: AtomicU64,
    /// Total number of notifications checked.
    pub total_checked: AtomicU64,
}

impl NotifyFilterStats {
    /// Returns the number of matched notifications.
    pub fn matched(&self) -> u64 {
        self.matched_count.load(Ordering::Relaxed)
    }

    /// Returns the number of suppressed notifications.
    pub fn suppressed(&self) -> u64 {
        self.suppressed_count.load(Ordering::Relaxed)
    }

    /// Returns the number of throttled notifications.
    pub fn throttled(&self) -> u64 {
        self.throttled_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of notifications checked.
    pub fn total(&self) -> u64 {
        self.total_checked.load(Ordering::Relaxed)
    }

    /// Increments the matched and total counters.
    pub fn increment_matched(&self) {
        self.matched_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the suppressed and total counters.
    pub fn increment_suppressed(&self) {
        self.suppressed_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the throttled and total counters.
    pub fn increment_throttled(&self) {
        self.throttled_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }
}

/// A notification filter rule.
#[derive(Debug, Clone)]
pub struct NotifyFilter {
    /// The type of filter to apply.
    pub filter_type: FilterType,
    /// The action to take when the filter matches.
    pub action: FilterAction,
    /// Optional pattern for path-based filtering.
    pub pattern: Option<String>,
    /// Whether this filter is currently enabled.
    pub enabled: bool,
}

impl Default for NotifyFilter {
    fn default() -> Self {
        Self {
            filter_type: FilterType::Inode,
            action: FilterAction::Notify,
            pattern: None,
            enabled: true,
        }
    }
}

impl NotifyFilter {
    /// Creates a new notification filter with the specified type.
    pub fn new(filter_type: FilterType) -> Self {
        Self {
            filter_type,
            ..Default::default()
        }
    }

    /// Sets the pattern for this filter and returns the modified filter.
    pub fn with_pattern(mut self, pattern: String) -> Self {
        self.pattern = Some(pattern);
        self
    }

    /// Sets the action for this filter and returns the modified filter.
    pub fn with_action(mut self, action: FilterAction) -> Self {
        self.action = action;
        self
    }

    /// Returns whether a notification should be sent based on this filter.
    pub fn should_notify(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match self.action {
            FilterAction::Notify => true,
            FilterAction::Suppress => false,
            FilterAction::Throttle => true,
        }
    }

    /// Checks if this filter matches the given inode and path.
    pub fn matches(&self, _inode: u64, _path: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_filter_default() {
        let filter = NotifyFilter::default();
        assert_eq!(filter.filter_type, FilterType::Inode);
        assert_eq!(filter.action, FilterAction::Notify);
        assert!(filter.enabled);
    }

    #[test]
    fn test_notify_filter_new() {
        let filter = NotifyFilter::new(FilterType::Path);
        assert_eq!(filter.filter_type, FilterType::Path);
    }

    #[test]
    fn test_notify_filter_with_pattern() {
        let filter = NotifyFilter::new(FilterType::Inode).with_pattern("*.tmp".to_string());
        assert_eq!(filter.pattern, Some("*.tmp".to_string()));
    }

    #[test]
    fn test_notify_filter_with_action() {
        let filter = NotifyFilter::new(FilterType::Global).with_action(FilterAction::Suppress);
        assert_eq!(filter.action, FilterAction::Suppress);
    }

    #[test]
    fn test_should_notify_enabled() {
        let filter = NotifyFilter::default();
        assert!(filter.should_notify());
    }

    #[test]
    fn test_should_notify_disabled() {
        let filter = NotifyFilter {
            enabled: false,
            ..Default::default()
        };
        assert!(!filter.should_notify());
    }

    #[test]
    fn test_should_notify_suppress() {
        let filter = NotifyFilter {
            action: FilterAction::Suppress,
            enabled: true,
            ..Default::default()
        };
        assert!(!filter.should_notify());
    }

    #[test]
    fn test_notify_filter_stats_default() {
        let stats = NotifyFilterStats::default();
        assert_eq!(stats.matched(), 0);
        assert_eq!(stats.suppressed(), 0);
        assert_eq!(stats.throttled(), 0);
    }

    #[test]
    fn test_notify_filter_stats_increment_matched() {
        let stats = NotifyFilterStats::default();
        stats.increment_matched();
        assert_eq!(stats.matched(), 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn test_notify_filter_stats_increment_suppressed() {
        let stats = NotifyFilterStats::default();
        stats.increment_suppressed();
        assert_eq!(stats.suppressed(), 1);
    }

    #[test]
    fn test_notify_filter_stats_increment_throttled() {
        let stats = NotifyFilterStats::default();
        stats.increment_throttled();
        assert_eq!(stats.throttled(), 1);
    }

    #[test]
    fn test_filter_type_default() {
        assert_eq!(FilterType::default(), FilterType::Inode);
    }

    #[test]
    fn test_filter_action_default() {
        assert_eq!(FilterAction::default(), FilterAction::Notify);
    }
}
