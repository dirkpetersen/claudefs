//! Split-brain and network partition tests for ClaudeFS distributed consensus.

#[cfg(test)]
mod tests {
    use crate::chaos::{FaultInjector, FaultType};
    use crate::jepsen::{JepsenChecker, JepsenHistory, JepsenOpType, RegisterModel};

    #[test]
    fn test_empty_history_is_linearizable() {
        let history = JepsenHistory::new();
        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_single_write_is_linearizable() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(42));
        history.complete_ok(1, "x", Some(42));
        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_single_read_is_linearizable() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", None);
        history.complete_ok(1, "x", Some(0));
        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_well_formed_history_single_process() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.complete_ok(1, "x", Some(1));
        assert!(history.is_well_formed());
    }

    #[test]
    fn test_well_formed_history_multi_process() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        history.complete_ok(1, "x", Some(1));
        history.complete_ok(2, "y", Some(2));
        assert!(history.is_well_formed());
    }

    #[test]
    fn test_history_with_uncompleted_invoke_is_not_well_formed() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        assert!(!history.is_well_formed());
    }

    #[test]
    fn test_register_model_initial_value() {
        let model = RegisterModel::new(5);
        assert_eq!(model.apply_read(), 5);
    }

    #[test]
    fn test_register_model_write_then_read() {
        let mut model = RegisterModel::new(0);
        model.apply_write(42);
        assert_eq!(model.apply_read(), 42);
    }

    #[test]
    fn test_register_model_multiple_writes() {
        let mut model = RegisterModel::new(0);
        model.apply_write(10);
        model.apply_write(20);
        model.apply_write(30);
        assert_eq!(model.apply_read(), 30);
    }

    #[test]
    fn test_fault_injector_starts_empty() {
        let injector = FaultInjector::new();
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_fault_injector_inject_partition() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });
        assert_eq!(injector.active_faults(), 1);
        assert!(injector.has_fault(&FaultType::NetworkPartition { from: 1, to: 2 }));
        let _ = handle;
    }

    #[test]
    fn test_fault_injector_clear_removes_fault() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });
        injector.clear(handle);
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_fault_injector_multiple_partitions() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });
        injector.inject(FaultType::NetworkPartition { from: 3, to: 4 });
        assert_eq!(injector.active_faults(), 2);
    }

    #[test]
    fn test_fault_injector_clear_all() {
        let mut injector = FaultInjector::new();
        injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });
        injector.inject(FaultType::NetworkPartition { from: 3, to: 4 });
        injector.inject(FaultType::NodeCrash(5));
        injector.clear_all();
        assert_eq!(injector.active_faults(), 0);
    }

    #[test]
    fn test_jepsen_checker_linearizable_write_read() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(42));
        history.complete_ok(1, "x", Some(42));
        history.invoke(2, "x", None);
        history.complete_ok(2, "x", Some(42));
        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_jepsen_checker_concurrent_reads_consistent() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(100));
        history.complete_ok(1, "x", Some(100));
        history.invoke(2, "x", None);
        history.complete_ok(2, "x", Some(100));
        history.invoke(3, "x", None);
        history.complete_ok(3, "x", Some(100));
        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_jepsen_ops_by_process() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        history.complete_ok(1, "x", Some(1));
        history.complete_ok(2, "y", Some(2));

        let p1_ops = history.ops_by_process(1);
        let p2_ops = history.ops_by_process(2);

        assert_eq!(p1_ops.len(), 2);
        assert_eq!(p2_ops.len(), 2);
        assert_eq!(p1_ops[0].process, 1);
        assert_eq!(p2_ops[0].process, 2);
    }

    #[test]
    fn test_jepsen_invocations_count() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        history.complete_ok(1, "x", Some(1));
        history.complete_ok(2, "y", Some(2));

        let invocations = history.invocations();
        assert_eq!(invocations.len(), 2);
        for op in invocations {
            assert!(matches!(op.op_type, JepsenOpType::Invoke));
        }
    }

    #[test]
    fn test_jepsen_completions_count() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        history.complete_ok(1, "x", Some(1));
        history.complete_ok(2, "y", Some(2));

        let completions = history.completions();
        assert_eq!(completions.len(), 2);
    }

    #[test]
    fn test_jepsen_history_empty_is_well_formed() {
        let history = JepsenHistory::new();
        assert!(history.is_well_formed());
    }

    #[test]
    fn test_partition_simulation_inject_and_heal() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NetworkPartition { from: 1, to: 2 });

        assert_eq!(injector.active_faults(), 1);
        assert!(injector.has_fault(&FaultType::NetworkPartition { from: 1, to: 2 }));

        injector.clear(handle);

        assert_eq!(injector.active_faults(), 0);
        assert!(!injector.has_fault(&FaultType::NetworkPartition { from: 1, to: 2 }));
    }

    #[test]
    fn test_multiple_processes_write_same_key() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "counter", Some(1));
        history.invoke(2, "counter", Some(2));
        history.invoke(3, "counter", Some(3));
        history.complete_ok(1, "counter", Some(1));
        history.complete_ok(2, "counter", Some(2));
        history.complete_ok(3, "counter", Some(3));

        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_process_crash_fault_type() {
        let mut injector = FaultInjector::new();
        let handle = injector.inject(FaultType::NodeCrash(5));
        assert_eq!(injector.active_faults(), 1);
        assert!(injector.has_fault(&FaultType::NodeCrash(5)));
        let _ = handle;
    }

    #[test]
    fn test_register_model_read_after_no_write() {
        let model = RegisterModel::new(123);
        assert_eq!(model.apply_read(), 123);
    }

    #[test]
    fn test_history_with_multiple_keys() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "a", Some(10));
        history.invoke(2, "b", Some(20));
        history.invoke(1, "c", Some(30));
        history.complete_ok(1, "a", Some(10));
        history.complete_ok(2, "b", Some(20));
        history.complete_ok(1, "c", Some(30));

        let checker = JepsenChecker::new();
        let result = checker.check_register(&history);
        assert!(result.valid);
        assert!(history.is_well_formed());
    }
}
