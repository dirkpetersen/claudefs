[0m> build · accounts/fireworks/models/minimax-m2p5

[0m
[0m→ [0mRead crates/claudefs-tests/src/lib.rs
[0m[0m← [0mEdit crates/claudefs-tests/src/lib.rs
Index: /home/cfs/claudefs/crates/claudefs-tests/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-tests/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-tests/src/lib.rs
@@ -47,8 +47,16 @@
 pub mod system_invariants;
 pub mod transport_resilience;
 pub mod worm_delegation_tests;
 
+pub mod crash_consistency_tests;
+pub mod endurance_tests;
+pub mod fuse_coherence_policy_tests;
+pub mod mgmt_topology_audit_tests;
+pub mod performance_suite;
+pub mod storage_new_modules_tests;
+pub mod transport_new_modules_tests;
+
 pub use bench::{detect_fio_binary, parse_fio_json, FioConfig, FioResult, FioRunner, FioRwMode};
 pub use chaos::{FaultHandle, FaultInjector, FaultType, NetworkTopology, NodeId};
 pub use connectathon::{ConnectathonReport, ConnectathonResult, ConnectathonRunner};
 pub use crash::{CrashConsistencyTest, CrashError, CrashPoint, CrashReport, CrashSimulator};



[0m[0m→ [0mRead crates/claudefs-tests/src/lib.rs[90m [offset=45, limit=20][0m
Done. Added the 7 missing module declarations to `lib.rs` at lines 51-57, after `worm_delegation_tests` and before the `pub use` re-exports.
