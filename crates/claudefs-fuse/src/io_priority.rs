//! I/O Priority Classification for QoS
//!
//! I/O priority classification enables quality-of-service in the FUSE client.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkloadClass {
    Interactive,
    Foreground,
    Background,
    Idle,
}

impl WorkloadClass {
    pub fn priority(&self) -> u8 {
        match self {
            WorkloadClass::Interactive => 3,
            WorkloadClass::Foreground => 2,
            WorkloadClass::Background => 1,
            WorkloadClass::Idle => 0,
        }
    }

    pub fn ioprio_class(&self) -> u8 {
        match self {
            WorkloadClass::Interactive => 1,
            WorkloadClass::Foreground => 1,
            WorkloadClass::Background => 2,
            WorkloadClass::Idle => 3,
        }
    }

    pub fn target_latency_us(&self) -> u64 {
        match self {
            WorkloadClass::Interactive => 1000,
            WorkloadClass::Foreground => 10000,
            WorkloadClass::Background => 1000000,
            WorkloadClass::Idle => 60000000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoPriorityClassifier {
    pid_overrides: HashMap<u32, WorkloadClass>,
    uid_overrides: HashMap<u32, WorkloadClass>,
    default_class: WorkloadClass,
}

impl IoPriorityClassifier {
    pub fn new(default_class: WorkloadClass) -> Self {
        Self {
            pid_overrides: HashMap::new(),
            uid_overrides: HashMap::new(),
            default_class,
        }
    }

    pub fn set_pid_class(&mut self, pid: u32, class: WorkloadClass) {
        self.pid_overrides.insert(pid, class);
    }

    pub fn set_uid_class(&mut self, uid: u32, class: WorkloadClass) {
        self.uid_overrides.insert(uid, class);
    }

    pub fn remove_pid_override(&mut self, pid: u32) {
        self.pid_overrides.remove(&pid);
    }

    pub fn remove_uid_override(&mut self, uid: u32) {
        self.uid_overrides.remove(&uid);
    }

    pub fn classify(&self, pid: u32, uid: u32) -> WorkloadClass {
        if let Some(&class) = self.pid_overrides.get(&pid) {
            return class;
        }
        if let Some(&class) = self.uid_overrides.get(&uid) {
            return class;
        }
        self.default_class
    }

    pub fn classify_by_op(
        &self,
        _pid: u32,
        _uid: u32,
        op_size_bytes: u64,
        is_metadata: bool,
    ) -> WorkloadClass {
        if is_metadata {
            return WorkloadClass::Interactive;
        }

        if op_size_bytes < 4096 {
            WorkloadClass::Interactive
        } else if op_size_bytes < 65536 {
            WorkloadClass::Foreground
        } else if op_size_bytes < 1048576 {
            WorkloadClass::Background
        } else {
            WorkloadClass::Idle
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct IoClassStats {
    pub ops_submitted: u64,
    pub ops_completed: u64,
    pub bytes_submitted: u64,
    pub bytes_completed: u64,
    pub total_latency_us: u64,
}

impl IoClassStats {
    pub fn avg_latency_us(&self) -> u64 {
        if self.ops_completed == 0 {
            0
        } else {
            self.total_latency_us / self.ops_completed
        }
    }

    pub fn record_op(&mut self, bytes: u64, latency_us: u64) {
        self.ops_submitted += 1;
        self.ops_completed += 1;
        self.bytes_submitted += bytes;
        self.bytes_completed += bytes;
        self.total_latency_us += latency_us;
    }
}

pub struct IoPriorityStats {
    pub by_class: HashMap<WorkloadClass, IoClassStats>,
}

impl Default for IoPriorityStats {
    fn default() -> Self {
        Self::new()
    }
}

impl IoPriorityStats {
    pub fn new() -> Self {
        Self {
            by_class: HashMap::new(),
        }
    }

    pub fn record(&mut self, class: WorkloadClass, bytes: u64, latency_us: u64) {
        self.by_class
            .entry(class)
            .or_default()
            .record_op(bytes, latency_us);
    }

    pub fn total_ops(&self) -> u64 {
        self.by_class.values().map(|s| s.ops_completed).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.by_class.values().map(|s| s.bytes_completed).sum()
    }

    pub fn class_share(&self, class: WorkloadClass) -> f64 {
        let total = self.total_ops() as f64;
        if total == 0.0 {
            return 0.0;
        }
        let class_ops = self
            .by_class
            .get(&class)
            .map(|s| s.ops_completed)
            .unwrap_or(0) as f64;
        class_ops / total
    }
}

#[derive(Debug)]
pub struct PriorityBudget {
    budgets: HashMap<WorkloadClass, u64>,
    limits: HashMap<WorkloadClass, u64>,
    window_ms: u64,
    last_reset_ms: u64,
}

impl PriorityBudget {
    pub fn new(window_ms: u64) -> Self {
        let limits = HashMap::from([
            (WorkloadClass::Interactive, 1000),
            (WorkloadClass::Foreground, 500),
            (WorkloadClass::Background, 100),
            (WorkloadClass::Idle, 10),
        ]);

        let budgets = limits.clone();

        Self {
            budgets,
            limits,
            window_ms,
            last_reset_ms: 0,
        }
    }

    pub fn set_limit(&mut self, class: WorkloadClass, limit: u64) {
        self.limits.insert(class, limit);
        self.budgets.insert(class, limit);
    }

    pub fn try_consume(&mut self, class: WorkloadClass, tokens: u64, now_ms: u64) -> bool {
        if now_ms - self.last_reset_ms >= self.window_ms {
            self.reset_window(now_ms);
        }

        let remaining = self.budgets.entry(class).or_insert(1000);
        if *remaining >= tokens {
            *remaining -= tokens;
            true
        } else {
            false
        }
    }

    pub fn remaining(&self, class: WorkloadClass) -> u64 {
        *self.budgets.get(&class).unwrap_or(&1000)
    }

    pub fn reset_window(&mut self, now_ms: u64) {
        self.last_reset_ms = now_ms;
        for (class, &limit) in &self.limits {
            self.budgets.insert(*class, limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workload_class_priority_ordering() {
        assert!(WorkloadClass::Interactive.priority() > WorkloadClass::Foreground.priority());
        assert!(WorkloadClass::Foreground.priority() > WorkloadClass::Background.priority());
        assert!(WorkloadClass::Background.priority() > WorkloadClass::Idle.priority());
    }

    #[test]
    fn test_target_latency_values() {
        assert_eq!(WorkloadClass::Interactive.target_latency_us(), 1000);
        assert_eq!(WorkloadClass::Foreground.target_latency_us(), 10000);
        assert_eq!(WorkloadClass::Background.target_latency_us(), 1000000);
        assert_eq!(WorkloadClass::Idle.target_latency_us(), 60000000);
    }

    #[test]
    fn test_classify_returns_pid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(123, WorkloadClass::Interactive);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Interactive));
    }

    #[test]
    fn test_classify_returns_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Foreground);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Foreground));
    }

    #[test]
    fn test_classify_returns_default() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Background));
    }

    #[test]
    fn test_pid_override_beats_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Foreground);
        classifier.set_pid_class(123, WorkloadClass::Idle);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Idle));
    }

    #[test]
    fn test_remove_pid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(123, WorkloadClass::Interactive);
        classifier.remove_pid_override(123);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Background));
    }

    #[test]
    fn test_classify_by_op_metadata() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify_by_op(123, 1000, 100, true);
        assert!(matches!(class, WorkloadClass::Interactive));
    }

    #[test]
    fn test_classify_by_op_small() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify_by_op(123, 1000, 1000, false);
        assert!(matches!(class, WorkloadClass::Interactive));
    }

    #[test]
    fn test_classify_by_op_medium() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify_by_op(123, 1000, 32768, false);
        assert!(matches!(class, WorkloadClass::Foreground));
    }

    #[test]
    fn test_classify_by_op_large() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify_by_op(123, 1000, 524288, false);
        assert!(matches!(class, WorkloadClass::Background));
    }

    #[test]
    fn test_classify_by_op_very_large() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);

        let class = classifier.classify_by_op(123, 1000, 2097152, false);
        assert!(matches!(class, WorkloadClass::Idle));
    }

    #[test]
    fn test_record_op_accumulates() {
        let mut stats = IoClassStats::default();

        stats.record_op(4096, 500);
        stats.record_op(8192, 600);

        assert_eq!(stats.ops_completed, 2);
        assert_eq!(stats.bytes_completed, 12288);
        assert_eq!(stats.total_latency_us, 1100);
    }

    #[test]
    fn test_avg_latency_calculation() {
        let mut stats = IoClassStats::default();

        stats.record_op(1000, 100);
        stats.record_op(1000, 200);

        assert_eq!(stats.avg_latency_us(), 150);
    }

    #[test]
    fn test_avg_latency_zero_ops() {
        let stats = IoClassStats::default();

        assert_eq!(stats.avg_latency_us(), 0);
    }

    #[test]
    fn test_total_ops() {
        let mut priority_stats = IoPriorityStats::new();

        priority_stats.record(WorkloadClass::Interactive, 1000, 100);
        priority_stats.record(WorkloadClass::Foreground, 2000, 200);
        priority_stats.record(WorkloadClass::Interactive, 3000, 300);

        assert_eq!(priority_stats.total_ops(), 3);
    }

    #[test]
    fn test_total_bytes() {
        let mut priority_stats = IoPriorityStats::new();

        priority_stats.record(WorkloadClass::Interactive, 1000, 100);
        priority_stats.record(WorkloadClass::Foreground, 2000, 200);

        assert_eq!(priority_stats.total_bytes(), 3000);
    }

    #[test]
    fn test_class_share() {
        let mut priority_stats = IoPriorityStats::new();

        priority_stats.record(WorkloadClass::Interactive, 1000, 100);
        priority_stats.record(WorkloadClass::Foreground, 1000, 100);

        let share = priority_stats.class_share(WorkloadClass::Interactive);
        assert!((share - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_class_share_zero_total() {
        let priority_stats = IoPriorityStats::new();

        let share = priority_stats.class_share(WorkloadClass::Interactive);
        assert_eq!(share, 0.0);
    }

    #[test]
    fn test_try_consume_allows_within_limit() {
        let mut budget = PriorityBudget::new(1000);

        let result = budget.try_consume(WorkloadClass::Interactive, 100, 100);
        assert!(result);
    }

    #[test]
    fn test_try_consume_rejects_when_exhausted() {
        let mut budget = PriorityBudget::new(1000);
        budget.set_limit(WorkloadClass::Interactive, 10);

        let result = budget.try_consume(WorkloadClass::Interactive, 100, 100);
        assert!(!result);
    }

    #[test]
    fn test_reset_window_refreshes_budgets() {
        let mut budget = PriorityBudget::new(1000);

        budget.try_consume(WorkloadClass::Interactive, 500, 100);
        assert_eq!(budget.remaining(WorkloadClass::Interactive), 500);

        budget.reset_window(2000);
        assert_eq!(budget.remaining(WorkloadClass::Interactive), 1000);
    }

    #[test]
    fn test_set_limit_overrides_default() {
        let mut budget = PriorityBudget::new(1000);

        budget.set_limit(WorkloadClass::Background, 50);

        assert_eq!(budget.remaining(WorkloadClass::Background), 50);
    }

    #[test]
    fn test_budget_window_reset_on_boundary() {
        let mut budget = PriorityBudget::new(1000);

        budget.try_consume(WorkloadClass::Foreground, 400, 100);

        let result = budget.try_consume(WorkloadClass::Foreground, 200, 1500);
        assert!(result);
    }

    #[test]
    fn test_remove_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Foreground);
        classifier.remove_uid_override(1000);

        let class = classifier.classify(123, 1000);
        assert!(matches!(class, WorkloadClass::Background));
    }
}
