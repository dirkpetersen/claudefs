//! I/O Priority and QoS Tests
//!
//! Tests for A5 I/O priority classifier and QoS budget.

#[cfg(test)]
mod tests {
    use claudefs_fuse::io_priority::{
        IoClassStats, IoPriorityClassifier, IoPriorityStats, PriorityBudget, WorkloadClass,
    };

    #[test]
    fn test_workload_class_interactive_priority() {
        assert_eq!(WorkloadClass::Interactive.priority(), 3);
    }

    #[test]
    fn test_workload_class_foreground_priority() {
        assert_eq!(WorkloadClass::Foreground.priority(), 2);
    }

    #[test]
    fn test_workload_class_background_priority() {
        assert_eq!(WorkloadClass::Background.priority(), 1);
    }

    #[test]
    fn test_workload_class_idle_priority() {
        assert_eq!(WorkloadClass::Idle.priority(), 0);
    }

    #[test]
    fn test_workload_class_interactive_ioprio() {
        assert_eq!(WorkloadClass::Interactive.ioprio_class(), 1);
    }

    #[test]
    fn test_workload_class_idle_ioprio() {
        assert_eq!(WorkloadClass::Idle.ioprio_class(), 3);
    }

    #[test]
    fn test_workload_class_interactive_latency() {
        assert_eq!(WorkloadClass::Interactive.target_latency_us(), 1000);
    }

    #[test]
    fn test_workload_class_idle_latency() {
        assert_eq!(WorkloadClass::Idle.target_latency_us(), 60000000);
    }

    #[test]
    fn test_workload_class_ordering() {
        assert!(WorkloadClass::Interactive.priority() > WorkloadClass::Foreground.priority());
        assert!(WorkloadClass::Foreground.priority() > WorkloadClass::Background.priority());
        assert!(WorkloadClass::Background.priority() > WorkloadClass::Idle.priority());
    }

    #[test]
    fn test_classifier_new_default_interactive() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Interactive);
        let class = classifier.classify(1, 1);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_classifier_new_default_background() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        let class = classifier.classify(999, 999);
        assert_eq!(class, WorkloadClass::Background);
    }

    #[test]
    fn test_classifier_classify_uses_default() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Interactive);
        let class = classifier.classify(1, 1);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_classifier_set_pid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(42, WorkloadClass::Interactive);
        let class = classifier.classify(42, 1);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_classifier_classify_uses_pid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(42, WorkloadClass::Interactive);
        let class = classifier.classify(42, 1);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_classifier_set_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Idle);
        let class = classifier.classify(1, 1000);
        assert_eq!(class, WorkloadClass::Idle);
    }

    #[test]
    fn test_classifier_classify_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Idle);
        let class = classifier.classify(1, 1000);
        assert_eq!(class, WorkloadClass::Idle);
    }

    #[test]
    fn test_classifier_pid_overrides_uid() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(42, WorkloadClass::Interactive);
        classifier.set_uid_class(1000, WorkloadClass::Idle);
        let class = classifier.classify(42, 1000);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_classifier_remove_pid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_pid_class(42, WorkloadClass::Interactive);
        classifier.remove_pid_override(42);
        let class = classifier.classify(42, 1);
        assert_eq!(class, WorkloadClass::Background);
    }

    #[test]
    fn test_classifier_remove_uid_override() {
        let mut classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        classifier.set_uid_class(1000, WorkloadClass::Idle);
        classifier.remove_uid_override(1000);
        let class = classifier.classify(1, 1000);
        assert_eq!(class, WorkloadClass::Background);
    }

    #[test]
    fn test_classifier_classify_by_op_read() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        let class = classifier.classify_by_op(1, 1, 4096, false);
        assert!(class.priority() >= WorkloadClass::Background.priority());
    }

    #[test]
    fn test_classifier_classify_by_op_sync() {
        let classifier = IoPriorityClassifier::new(WorkloadClass::Background);
        let class = classifier.classify_by_op(1, 1, 4096, true);
        assert!(class.priority() >= WorkloadClass::Foreground.priority());
    }

    #[test]
    fn test_io_class_stats_initial() {
        let stats = IoClassStats::default();
        assert_eq!(stats.ops_completed, 0);
        assert_eq!(stats.bytes_completed, 0);
    }

    #[test]
    fn test_io_class_stats_avg_latency_empty() {
        let stats = IoClassStats::default();
        assert_eq!(stats.avg_latency_us(), 0);
    }

    #[test]
    fn test_io_class_stats_record_op() {
        let mut stats = IoClassStats::default();
        stats.record_op(4096, 100);
        assert_eq!(stats.ops_completed, 1);
        assert_eq!(stats.bytes_completed, 4096);
    }

    #[test]
    fn test_io_class_stats_avg_latency_single() {
        let mut stats = IoClassStats::default();
        stats.record_op(4096, 100);
        assert_eq!(stats.avg_latency_us(), 100);
    }

    #[test]
    fn test_io_class_stats_avg_latency_multiple() {
        let mut stats = IoClassStats::default();
        stats.record_op(4096, 100);
        stats.record_op(4096, 200);
        stats.record_op(4096, 300);
        assert_eq!(stats.avg_latency_us(), 200);
    }

    #[test]
    fn test_io_class_stats_bytes_accumulate() {
        let mut stats = IoClassStats::default();
        stats.record_op(4096, 100);
        stats.record_op(8192, 200);
        assert_eq!(stats.bytes_completed, 4096 + 8192);
    }

    #[test]
    fn test_io_priority_stats_new() {
        let stats = IoPriorityStats::new();
        assert_eq!(stats.total_ops(), 0);
    }

    #[test]
    fn test_io_priority_stats_record_interactive() {
        let mut stats = IoPriorityStats::new();
        stats.record(WorkloadClass::Interactive, 4096, 100);
        assert_eq!(stats.total_ops(), 1);
    }

    #[test]
    fn test_io_priority_stats_total_ops() {
        let mut stats = IoPriorityStats::new();
        stats.record(WorkloadClass::Interactive, 4096, 100);
        stats.record(WorkloadClass::Background, 4096, 100);
        stats.record(WorkloadClass::Foreground, 4096, 100);
        assert_eq!(stats.total_ops(), 3);
    }

    #[test]
    fn test_io_priority_stats_total_bytes() {
        let mut stats = IoPriorityStats::new();
        stats.record(WorkloadClass::Interactive, 4096, 100);
        stats.record(WorkloadClass::Background, 8192, 100);
        assert_eq!(stats.total_bytes(), 4096 + 8192);
    }

    #[test]
    fn test_io_priority_stats_class_share() {
        let mut stats = IoPriorityStats::new();
        stats.record(WorkloadClass::Interactive, 4096, 100);
        let share = stats.class_share(WorkloadClass::Interactive);
        assert!((share - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_priority_budget_new() {
        let budget = PriorityBudget::new(1000);
        assert!(budget.remaining(WorkloadClass::Interactive) > 0);
    }

    #[test]
    fn test_priority_budget_no_limit_by_default() {
        let mut budget = PriorityBudget::new(1000);
        assert!(budget.try_consume(WorkloadClass::Interactive, 500, 0));
    }

    #[test]
    fn test_priority_budget_set_limit() {
        let mut budget = PriorityBudget::new(1000);
        budget.set_limit(WorkloadClass::Interactive, 1000);
        let remaining = budget.remaining(WorkloadClass::Interactive);
        assert!(remaining > 0);
    }

    #[test]
    fn test_priority_budget_consume_within_limit() {
        let mut budget = PriorityBudget::new(1000);
        budget.set_limit(WorkloadClass::Interactive, 1000);
        assert!(budget.try_consume(WorkloadClass::Interactive, 500, 0));
    }

    #[test]
    fn test_priority_budget_consume_exceeds_limit() {
        let mut budget = PriorityBudget::new(1000);
        budget.set_limit(WorkloadClass::Interactive, 1000);
        assert!(!budget.try_consume(WorkloadClass::Interactive, 1500, 0));
    }

    #[test]
    fn test_priority_budget_independent_classes() {
        let mut budget = PriorityBudget::new(1000);
        budget.set_limit(WorkloadClass::Background, 100);
        assert!(budget.try_consume(WorkloadClass::Interactive, 1000, 0));
    }
}
