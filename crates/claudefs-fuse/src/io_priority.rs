//! I/O Priority Classification for QoS
//!
//! I/O priority classification enables quality-of-service in the FUSE client.
//! Operations are classified into workload classes based on process identity,
//! operation size, or metadata vs data distinctions. This enables the storage
//! layer to provide differentiated service levels.

use std::collections::HashMap;

/// Workload classification for I/O operations.
///
/// Determines priority level, scheduling behavior, and latency targets
/// for storage operations. Higher priority classes get preferential
/// scheduling and lower latency targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkloadClass {
    /// User-interactive operations requiring minimal latency.
    /// Examples: UI responsiveness, small metadata operations.
    Interactive,
    /// Foreground operations with moderate latency requirements.
    /// Examples: normal file reads/writes, application data access.
    Foreground,
    /// Background operations tolerant of higher latency.
    /// Examples: backups, indexing, large sequential scans.
    Background,
    /// Idle operations with no latency constraints.
    /// Examples: prefetching, speculative reads, cleanup.
    Idle,
}

impl WorkloadClass {
    /// Returns the numeric priority value for this workload class.
    ///
    /// Higher values indicate higher priority. Used for queue ordering
    /// and scheduling decisions.
    pub fn priority(&self) -> u8 {
        match self {
            WorkloadClass::Interactive => 3,
            WorkloadClass::Foreground => 2,
            WorkloadClass::Background => 1,
            WorkloadClass::Idle => 0,
        }
    }

    /// Returns the Linux `ioprio` class value for this workload class.
    ///
    /// Maps to `IOPRIO_CLASS_RT` (1), `IOPRIO_CLASS_BE` (2), and `IOPRIO_CLASS_IDLE` (3).
    pub fn ioprio_class(&self) -> u8 {
        match self {
            WorkloadClass::Interactive => 1,
            WorkloadClass::Foreground => 1,
            WorkloadClass::Background => 2,
            WorkloadClass::Idle => 3,
        }
    }

    /// Returns the target latency in microseconds for this workload class.
    ///
    /// Used for latency-aware scheduling and admission control.
    /// Interactive operations target 1ms, idle operations have 60s target.
    pub fn target_latency_us(&self) -> u64 {
        match self {
            WorkloadClass::Interactive => 1000,
            WorkloadClass::Foreground => 10000,
            WorkloadClass::Background => 1000000,
            WorkloadClass::Idle => 60000000,
        }
    }
}

/// Classifier for determining I/O priority based on process identity.
///
/// Supports per-PID and per-UID overrides, with a fallback default class.
/// PID overrides take precedence over UID overrides.
#[derive(Debug, Clone)]
pub struct IoPriorityClassifier {
    pid_overrides: HashMap<u32, WorkloadClass>,
    uid_overrides: HashMap<u32, WorkloadClass>,
    default_class: WorkloadClass,
}

impl IoPriorityClassifier {
    /// Creates a new classifier with the specified default workload class.
    ///
    /// Operations without PID or UID overrides will be classified as
    /// the default class.
    pub fn new(default_class: WorkloadClass) -> Self {
        Self {
            pid_overrides: HashMap::new(),
            uid_overrides: HashMap::new(),
            default_class,
        }
    }

    /// Sets a workload class override for a specific process ID.
    ///
    /// PID overrides take precedence over UID overrides.
    pub fn set_pid_class(&mut self, pid: u32, class: WorkloadClass) {
        self.pid_overrides.insert(pid, class);
    }

    /// Sets a workload class override for a specific user ID.
    ///
    /// PID overrides take precedence over UID overrides.
    pub fn set_uid_class(&mut self, uid: u32, class: WorkloadClass) {
        self.uid_overrides.insert(uid, class);
    }

    /// Removes the workload class override for a specific process ID.
    ///
    /// After removal, classification for this PID will fall back to
    /// UID override or default class.
    pub fn remove_pid_override(&mut self, pid: u32) {
        self.pid_overrides.remove(&pid);
    }

    /// Removes the workload class override for a specific user ID.
    ///
    /// After removal, classification for this UID will fall back to
    /// the default class.
    pub fn remove_uid_override(&mut self, uid: u32) {
        self.uid_overrides.remove(&uid);
    }

    /// Classifies an I/O operation based on process and user identity.
    ///
    /// Checks PID override first, then UID override, then falls back
    /// to the default class.
    pub fn classify(&self, pid: u32, uid: u32) -> WorkloadClass {
        if let Some(&class) = self.pid_overrides.get(&pid) {
            return class;
        }
        if let Some(&class) = self.uid_overrides.get(&uid) {
            return class;
        }
        self.default_class
    }

    /// Classifies an I/O operation based on operation characteristics.
    ///
    /// Uses operation size and metadata flag to determine appropriate
    /// workload class. Metadata operations always classify as Interactive.
    /// Small operations (<4KB) are Interactive, medium (4KB-64KB) are
    /// Foreground, large (64KB-1MB) are Background, and very large (>1MB)
    /// are Idle.
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

/// Statistics for a single I/O workload class.
///
/// Tracks operation counts, byte totals, and latency accumulation
/// for monitoring and reporting.
#[derive(Debug, Default, Clone)]
pub struct IoClassStats {
    /// Number of operations submitted to this class.
    pub ops_submitted: u64,
    /// Number of operations completed in this class.
    pub ops_completed: u64,
    /// Total bytes submitted in this class.
    pub bytes_submitted: u64,
    /// Total bytes completed in this class.
    pub bytes_completed: u64,
    /// Cumulative latency in microseconds for completed operations.
    pub total_latency_us: u64,
}

impl IoClassStats {
    /// Computes the average latency per completed operation.
    ///
    /// Returns 0 if no operations have been completed.
    pub fn avg_latency_us(&self) -> u64 {
        if self.ops_completed == 0 {
            0
        } else {
            self.total_latency_us / self.ops_completed
        }
    }

    /// Records a completed operation with its byte count and latency.
    ///
    /// Increments operation and byte counters, and accumulates latency.
    pub fn record_op(&mut self, bytes: u64, latency_us: u64) {
        self.ops_submitted += 1;
        self.ops_completed += 1;
        self.bytes_submitted += bytes;
        self.bytes_completed += bytes;
        self.total_latency_us += latency_us;
    }
}

/// Aggregate statistics across all I/O workload classes.
///
/// Maintains per-class statistics and provides aggregate calculations.
pub struct IoPriorityStats {
    /// Per-class statistics, indexed by `WorkloadClass`.
    pub by_class: HashMap<WorkloadClass, IoClassStats>,
}

impl Default for IoPriorityStats {
    fn default() -> Self {
        Self::new()
    }
}

impl IoPriorityStats {
    /// Creates a new empty statistics container.
    pub fn new() -> Self {
        Self {
            by_class: HashMap::new(),
        }
    }

    /// Records a completed operation for the specified workload class.
    ///
    /// Creates per-class statistics on first access if needed.
    pub fn record(&mut self, class: WorkloadClass, bytes: u64, latency_us: u64) {
        self.by_class
            .entry(class)
            .or_default()
            .record_op(bytes, latency_us);
    }

    /// Returns the total number of completed operations across all classes.
    pub fn total_ops(&self) -> u64 {
        self.by_class.values().map(|s| s.ops_completed).sum()
    }

    /// Returns the total bytes completed across all classes.
    pub fn total_bytes(&self) -> u64 {
        self.by_class.values().map(|s| s.bytes_completed).sum()
    }

    /// Computes the share of operations for a specific workload class.
    ///
    /// Returns a value between 0.0 and 1.0 representing the proportion
    /// of total operations in the specified class. Returns 0.0 if no
    /// operations have been recorded.
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

/// Token bucket budget for per-class rate limiting.
///
/// Implements a replenishing token bucket for each workload class,
/// with configurable limits and a time-based window for replenishment.
#[derive(Debug)]
pub struct PriorityBudget {
    budgets: HashMap<WorkloadClass, u64>,
    limits: HashMap<WorkloadClass, u64>,
    window_ms: u64,
    last_reset_ms: u64,
}

impl PriorityBudget {
    /// Creates a new budget manager with the specified window duration.
    ///
    /// Initializes with default limits: Interactive (1000), Foreground (500),
    /// Background (100), Idle (10).
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

    /// Sets the token limit for a specific workload class.
    ///
    /// Also resets the current budget to the new limit.
    pub fn set_limit(&mut self, class: WorkloadClass, limit: u64) {
        self.limits.insert(class, limit);
        self.budgets.insert(class, limit);
    }

    /// Attempts to consume tokens from a workload class budget.
    ///
    /// Automatically resets the window if enough time has passed.
    /// Returns `true` if tokens were available and consumed, `false` otherwise.
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

    /// Returns the remaining token budget for a workload class.
    ///
    /// Returns 1000 as a default if the class has no budget entry.
    pub fn remaining(&self, class: WorkloadClass) -> u64 {
        *self.budgets.get(&class).unwrap_or(&1000)
    }

    /// Resets all budgets to their configured limits.
    ///
    /// Called automatically when the time window expires during
    /// `try_consume`, or can be called manually.
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
