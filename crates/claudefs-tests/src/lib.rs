//! ClaudeFS Test & Validation Infrastructure
//!
//! This crate provides comprehensive testing utilities for the ClaudeFS distributed filesystem.
//! It includes property-based tests, integration test scaffolding, POSIX test runners,
//! crash consistency testing, linearizability checking, and performance benchmarking.

pub mod bench;
pub mod chaos;
pub mod ci_matrix;
pub mod concurrency_tests;
pub mod connectathon;
pub mod crash;
pub mod distributed_tests;
pub mod fuzz_helpers;
pub mod harness;
pub mod integration;
pub mod jepsen;
pub mod linearizability;
pub mod meta_tests;
pub mod posix;
pub mod posix_compliance;
pub mod proptest_reduce;
pub mod proptest_storage;
pub mod proptest_transport;
pub mod reduce_tests;
pub mod regression;
pub mod report;
pub mod snapshot_tests;
pub mod soak;
pub mod storage_tests;
pub mod transport_tests;
pub mod write_path_e2e;

pub mod acl_integration;
pub mod fault_recovery_tests;
pub mod fuse_tests;
pub mod gateway_integration;
pub mod io_priority_qos_tests;
pub mod mgmt_integration;
pub mod perf_regression;
pub mod pipeline_integration;
pub mod quota_integration;
pub mod repl_integration;
pub mod security_integration;
pub mod storage_resilience;
pub mod system_invariants;
pub mod transport_resilience;
pub mod worm_delegation_tests;

pub use bench::{detect_fio_binary, parse_fio_json, FioConfig, FioResult, FioRunner, FioRwMode};
pub use chaos::{FaultHandle, FaultInjector, FaultType, NetworkTopology, NodeId};
pub use connectathon::{ConnectathonReport, ConnectathonResult, ConnectathonRunner};
pub use crash::{CrashConsistencyTest, CrashError, CrashPoint, CrashReport, CrashSimulator};
pub use harness::{TestCluster, TestEnv};
pub use integration::{IntegrationReport, IntegrationTestSuite};
pub use jepsen::{
    CheckResult, JepsenChecker, JepsenHistory, JepsenOp, JepsenOpType, JepsenTestConfig, Nemesis,
    RegisterModel, RegisterOp,
};
pub use linearizability::{History, LinearizabilityReport2, Model, Operation};
pub use posix::{
    detect_fsx_binary, detect_pjdfstest_binary, FsxResult, FsxRunner, PjdfsResult, PjdfsRunner,
    XfstestsResult, XfstestsRunner,
};
pub use posix_compliance::{PosixComplianceSuite, PosixSuiteReport, PosixTestResult};
pub use regression::{
    RegressionCase, RegressionRegistry, RegressionResult, RegressionRunner, RegressionSummary,
    Severity,
};
pub use report::{AggregateReport, ReportBuilder, TestCaseResult, TestStatus, TestSuiteReport};
pub use soak::{
    generate_task_sequence, FileSoakTest, SoakConfig, SoakSnapshot, SoakStats, WorkerOp, WorkerTask,
};
