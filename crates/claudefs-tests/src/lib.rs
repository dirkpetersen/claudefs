//! ClaudeFS Test & Validation Infrastructure
//!
//! This crate provides comprehensive testing utilities for the ClaudeFS distributed filesystem.
//! It includes property-based tests, integration test scaffolding, POSIX test runners,
//! crash consistency testing, linearizability checking, and performance benchmarking.

pub mod bench;
pub mod chaos;
pub mod connectathon;
pub mod crash;
pub mod harness;
pub mod integration;
pub mod linearizability;
pub mod posix;
pub mod proptest_reduce;
pub mod proptest_storage;
pub mod proptest_transport;

pub use bench::{detect_fio_binary, parse_fio_json, FioConfig, FioResult, FioRunner, FioRwMode};
pub use chaos::{FaultHandle, FaultInjector, FaultType, NetworkTopology, NodeId};
pub use connectathon::{ConnectathonReport, ConnectathonResult, ConnectathonRunner};
pub use crash::{CrashConsistencyTest, CrashError, CrashPoint, CrashReport, CrashSimulator};
pub use harness::{TestCluster, TestEnv};
pub use integration::{IntegrationReport, IntegrationTestSuite};
pub use linearizability::{History, LinearizabilityReport2, Model, Operation};
pub use posix::{
    detect_fsx_binary, detect_pjdfstest_binary, FsxResult, FsxRunner, PjdfsResult, PjdfsRunner,
    XfstestsResult, XfstestsRunner,
};
