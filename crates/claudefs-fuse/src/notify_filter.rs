#![warn(missing_docs)]

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterType {
    #[default]
    Inode,
    Path,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterAction {
    #[default]
    Notify,
    Suppress,
    Throttle,
}

#[derive(Debug, Default)]
pub struct NotifyFilterStats {
    pub matched_count: AtomicU64,
    pub suppressed_count: AtomicU64,
    pub throttled_count: AtomicU64,
    pub total_checked: AtomicU64,
}

impl NotifyFilterStats {
    pub fn matched(&self) -> u64 {
        self.matched_count.load(Ordering::Relaxed)
    }

    pub fn suppressed(&self) -> u64 {
        self.suppressed_count.load(Ordering::Relaxed)
    }

    pub fn throttled(&self) -> u64 {
        self.throttled_count.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total_checked.load(Ordering::Relaxed)
    }

    pub fn increment_matched(&self) {
        self.matched_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_suppressed(&self) {
        self.suppressed_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_throttled(&self) {
        self.throttled_count.fetch_add(1, Ordering::Relaxed);
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct NotifyFilter {
    pub filter_type: FilterType,
    pub action: FilterAction,
    pub pattern: Option<String>,
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
    pub fn new(filter_type: FilterType) -> Self {
        Self {
            filter_type,
            ..Default::default()
        }
    }

    pub fn with_pattern(mut self, pattern: String) -> Self {
        self.pattern = Some(pattern);
        self
    }

    pub fn with_action(mut self, action: FilterAction) -> Self {
        self.action = action;
        self
    }

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
