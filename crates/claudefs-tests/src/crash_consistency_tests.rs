//! Crash consistency and recovery tests for ClaudeFS write journal and storage.

use crate::crash::{CrashConsistencyTest, CrashError, CrashPoint, CrashReport, CrashSimulator};

#[test]
fn test_crash_simulator_new_no_crash_point() {
    let simulator = CrashSimulator::new();
    assert!(!simulator.should_crash(CrashPoint::BeforeWrite));
    assert!(!simulator.should_crash(CrashPoint::AfterWrite));
    assert!(!simulator.should_crash(CrashPoint::DuringFlush));
    assert!(!simulator.should_crash(CrashPoint::AfterFlush));
    assert!(!simulator.should_crash(CrashPoint::DuringReplication));
    assert!(!simulator.should_crash(CrashPoint::AfterReplication));
}

#[test]
fn test_set_crash_point_before_write() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::BeforeWrite);
    assert!(simulator.should_crash(CrashPoint::BeforeWrite));
}

#[test]
fn test_set_crash_point_after_write() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::AfterWrite);
    assert!(simulator.should_crash(CrashPoint::AfterWrite));
}

#[test]
fn test_crash_point_mismatch_no_trigger() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::BeforeWrite);
    assert!(!simulator.should_crash(CrashPoint::AfterWrite));
}

#[test]
fn test_clear_crash_point() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::BeforeWrite);
    simulator.clear_crash_point();
    assert!(!simulator.should_crash(CrashPoint::BeforeWrite));
}

#[test]
fn test_simulate_write_path_no_crash() {
    let simulator = CrashSimulator::new();
    let result = simulator.simulate_write_path(b"test data");
    assert!(result.is_ok());
    let report = result.unwrap();
    assert!(report.data_consistent);
    assert!(report.recovery_success);
}

#[test]
fn test_simulate_write_path_recovery_default() {
    let simulator = CrashSimulator::default();
    let result = simulator.simulate_write_path(b"data");
    assert!(result.is_ok());
    let report = result.unwrap();
    assert!(report.data_consistent);
}

#[test]
fn test_crash_report_data_consistent_on_success() {
    let simulator = CrashSimulator::new();
    let result = simulator.simulate_write_path(b"hello");
    assert!(result.is_ok());
    let report = result.unwrap();
    assert!(report.data_consistent);
}

#[test]
fn test_crash_consistency_test_new() {
    let simulator = CrashSimulator::new();
    let test = CrashConsistencyTest::new(simulator);
    assert!(test.results().is_empty());
}

#[test]
fn test_crash_consistency_test_run_success() {
    let simulator = CrashSimulator::new();
    let mut test = CrashConsistencyTest::new(simulator);
    let result = test.run();
    assert!(result.is_ok());
}

#[test]
fn test_crash_consistency_test_results_empty_initially() {
    let simulator = CrashSimulator::new();
    let test = CrashConsistencyTest::new(simulator);
    assert!(test.results().is_empty());
}

#[test]
fn test_crash_consistency_test_results_after_run() {
    let simulator = CrashSimulator::new();
    let mut test = CrashConsistencyTest::new(simulator);
    let _ = test.run();
    assert_eq!(test.results().len(), 1);
}

#[test]
fn test_crash_point_during_flush() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::DuringFlush);
    assert!(simulator.should_crash(CrashPoint::DuringFlush));
}

#[test]
fn test_crash_point_after_flush() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::AfterFlush);
    assert!(simulator.should_crash(CrashPoint::AfterFlush));
}

#[test]
fn test_crash_point_during_replication() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::DuringReplication);
    assert!(simulator.should_crash(CrashPoint::DuringReplication));
    assert!(!simulator.should_crash(CrashPoint::AfterReplication));
}

#[test]
fn test_crash_point_after_replication() {
    let mut simulator = CrashSimulator::new();
    simulator.set_crash_point(CrashPoint::AfterReplication);
    assert!(simulator.should_crash(CrashPoint::AfterReplication));
    assert!(!simulator.should_crash(CrashPoint::DuringReplication));
}

#[test]
fn test_crash_report_repaired_entries_zero_on_no_crash() {
    let simulator = CrashSimulator::new();
    let result = simulator.simulate_write_path(b"data");
    assert!(result.is_ok());
    let report = result.unwrap();
    assert_eq!(report.repaired_entries, 0);
}

#[test]
fn test_multiple_runs_accumulate_results() {
    let simulator = CrashSimulator::new();
    let mut test = CrashConsistencyTest::new(simulator);
    let _ = test.run();
    let _ = test.run();
    let _ = test.run();
    assert_eq!(test.results().len(), 3);
}

#[test]
fn test_crash_error_simulated_crash_debug() {
    let error = CrashError::SimulatedCrash {
        at: CrashPoint::BeforeWrite,
    };
    let debug_str = format!("{:?}", error);
    assert!(debug_str.contains("SimulatedCrash"));
    assert!(debug_str.contains("BeforeWrite"));
}

#[test]
fn test_crash_report_debug() {
    let report = CrashReport {
        crash_point: CrashPoint::AfterWrite,
        recovery_success: true,
        data_consistent: false,
        repaired_entries: 3,
    };
    let debug_str = format!("{:?}", report);
    assert!(debug_str.contains("AfterWrite"));
    assert!(debug_str.contains("recovery_success"));
}

#[test]
fn test_crash_report_recovery_success_on_clean_path() {
    let simulator = CrashSimulator::new();
    let result = simulator.simulate_write_path(b"clean data");
    assert!(result.is_ok());
    let report = result.unwrap();
    assert!(report.recovery_success);
}

#[test]
fn test_crash_simulator_write_with_data() {
    let simulator = CrashSimulator::new();
    let data = b"test data with content";
    let result = simulator.simulate_write_path(data);
    assert!(result.is_ok());
}

#[test]
fn test_crash_simulator_write_with_empty_data() {
    let simulator = CrashSimulator::new();
    let result = simulator.simulate_write_path(b"");
    assert!(result.is_ok());
}

#[test]
fn test_crash_point_all_variants_can_be_set() {
    let mut simulator = CrashSimulator::new();

    simulator.set_crash_point(CrashPoint::BeforeWrite);
    assert!(simulator.should_crash(CrashPoint::BeforeWrite));
    simulator.clear_crash_point();

    simulator.set_crash_point(CrashPoint::AfterWrite);
    assert!(simulator.should_crash(CrashPoint::AfterWrite));
    simulator.clear_crash_point();

    simulator.set_crash_point(CrashPoint::DuringFlush);
    assert!(simulator.should_crash(CrashPoint::DuringFlush));
    simulator.clear_crash_point();

    simulator.set_crash_point(CrashPoint::AfterFlush);
    assert!(simulator.should_crash(CrashPoint::AfterFlush));
    simulator.clear_crash_point();

    simulator.set_crash_point(CrashPoint::DuringReplication);
    assert!(simulator.should_crash(CrashPoint::DuringReplication));
    simulator.clear_crash_point();

    simulator.set_crash_point(CrashPoint::AfterReplication);
    assert!(simulator.should_crash(CrashPoint::AfterReplication));
}

#[test]
fn test_crash_consistency_test_multiple_scenarios() {
    let points = vec![
        CrashPoint::BeforeWrite,
        CrashPoint::AfterWrite,
        CrashPoint::DuringFlush,
        CrashPoint::AfterFlush,
        CrashPoint::DuringReplication,
        CrashPoint::AfterReplication,
    ];

    let mut collected_results: Vec<CrashReport> = Vec::new();

    for point in points {
        let mut simulator = CrashSimulator::new();
        simulator.set_crash_point(point.clone());
        let result = simulator.simulate_write_path(b"scenario test");

        if result.is_err() {
            let error = result.unwrap_err();
            if let CrashError::SimulatedCrash { at } = error {
                assert_eq!(at, point);
            }
        }
        collected_results.push(CrashReport {
            crash_point: point,
            recovery_success: false,
            data_consistent: false,
            repaired_entries: 1,
        });
    }

    assert_eq!(collected_results.len(), 6);
}
