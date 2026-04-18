# A10 Phase 36: Security Tests for Storage, FUSE, and Metadata Subsystems

You are implementing comprehensive security tests for ClaudeFS Phase 36. This document provides all context needed to generate 5 security test modules.

## Architecture Context

ClaudeFS is a distributed POSIX filesystem with 8 crates: storage, metadata, reduction, transport, FUSE client, replication, gateways, and management.

**A10 (Security Audit)** responsibilities:
- Unsafe code review (io_uring/FUSE/RDMA FFI boundaries)
- Fuzzing (network protocol, FUSE, NFS)
- Crypto implementation audit
- Authentication/authorization security
- Dependency CVE scanning
- Management API pen-testing

## Phase 36 Goal

Generate 95-100 new security tests across 5 specialized modules, bringing total to ~2480 tests.

---

## MODULE 1: Storage Background Subsystems Security Tests

**File:** `crates/claudefs-security/src/storage_background_subsystems_security_tests.rs`

**Target Modules:** `background_scheduler`, `device_health_monitor`, `prefetch_engine`, `wear_leveling`, `node_rebalance` from `claudefs-storage` crate

**Task:** Generate 30 comprehensive security tests covering concurrency, resource bounds, state machines, and DoS resilience.

### Background Scheduler (8 tests):
- `test_background_scheduler_concurrent_submit_no_race`: Concurrent task submission from multiple threads without lost updates or duplicates
- `test_background_scheduler_priority_enforcement`: High-priority tasks execute before normal/low-priority tasks
- `test_background_scheduler_task_deadline_respected`: Tasks with deadlines abort if not executed by deadline
- `test_background_scheduler_memory_bounded`: Task queue has maximum size; exceeding it returns error (no unbounded growth DoS)
- `test_background_scheduler_graceful_shutdown`: Shutdown cancels pending tasks and waits for in-flight tasks to complete
- `test_background_scheduler_starvation_prevention`: Low-priority tasks eventually execute (no starvation)
- `test_background_scheduler_reentrant_submission`: Task can submit subtasks without deadlock
- `test_background_scheduler_error_recovery`: Failed task doesn't crash scheduler; errors are propagated

### Device Health Monitor (7 tests):
- `test_device_health_monitor_smart_metrics_overflow_safe`: Large SMART values don't overflow internal counters
- `test_device_health_monitor_state_transition_rules`: Health transitions correctly Good→Warning→Failed (no skip)
- `test_device_health_monitor_concurrent_updates`: Concurrent metric updates without lost updates
- `test_device_health_monitor_alert_suppression_no_leak`: Alert suppression doesn't leak timing information
- `test_device_health_monitor_metric_timestamp_monotonic`: Metric timestamps never go backwards (prevent time-travel attacks)
- `test_device_health_monitor_health_score_concurrency`: Concurrent score calculations without race conditions
- `test_device_health_monitor_dashmap_consistency`: DashMap updates are atomic and visible to all readers

### Prefetch Engine (8 tests):
- `test_prefetch_engine_pattern_detection_memory_bounded`: Pattern detection uses bounded memory (no unbounded allocation)
- `test_prefetch_engine_access_history_lru_eviction`: Old access history properly evicted (LRU working correctly)
- `test_prefetch_engine_speculative_io_cancellation`: Speculative I/O properly cancelled when user I/O arrives
- `test_prefetch_engine_priority_over_user_io`: User I/O gets priority over prefetch requests
- `test_prefetch_engine_prediction_accuracy`: Prediction accuracy within expected bounds (measure hit rate)
- `test_prefetch_engine_concurrent_prefetch_requests`: Concurrent prefetch requests handled without interference
- `test_prefetch_engine_cache_eviction_correctness`: Least-recently-used entries evicted first
- `test_prefetch_engine_io_submission_ordering`: Prefetch I/Os submitted in correct order to storage

### Wear Leveling (5 tests):
- `test_wear_leveling_block_wear_tracking_overflow_safe`: Wear counts don't overflow (use saturating arithmetic)
- `test_wear_leveling_erase_count_distribution_fair`: Wear distribution is fair (check entropy/variance)
- `test_wear_leveling_hot_spot_rebalancing_safe`: Rebalancing doesn't lose data (all blocks remain readable)
- `test_wear_leveling_concurrent_wear_updates`: Concurrent wear updates use atomic operations (no lost updates)
- `test_wear_leveling_ssd_type_detection`: Detects SSD type and applies correct tier-specific wear policy

### Node Rebalance (2 tests):
- `test_node_rebalance_segment_distribution_fair`: Segments distributed fairly across nodes (no single node overloaded)
- `test_node_rebalance_during_node_failure`: Rebalance continues correctly if node fails mid-operation

## Module 1 Test File Structure

```rust
#[cfg(test)]
mod storage_background_subsystems_security_tests {
    use claudefs_storage::background_scheduler::BackgroundScheduler;
    use claudefs_storage::device_health_monitor::{DeviceHealthMonitor, HealthStatus};
    use claudefs_storage::prefetch_engine::PrefetchEngine;
    use claudefs_storage::wear_leveling::WearLevelingManager;
    use claudefs_storage::node_rebalance::NodeRebalancer;
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;
    use std::time::Duration;

    // 30 test functions here

    #[test]
    fn test_background_scheduler_concurrent_submit_no_race() {
        // Test implementation
    }
    // ... more tests
}
```

---

## MODULE 2: FUSE Cache Coherence Security Tests

**File:** `crates/claudefs-security/src/fuse_cache_coherence_security_tests.rs`

**Target Modules:** `readdir_cache`, `writeback_cache`, `mmap`, `otel_tracing_integration`, `distributed_session_manager`, `worm_enforcement`, `quota_client_tracker` from `claudefs-fuse` crate

**Task:** Generate 35 comprehensive security tests covering cache coherence, crash consistency, and session isolation.

### Readdir Cache (7 tests):
- `test_readdir_cache_invalidation_on_mkdir`: Cache invalidated when directory modified
- `test_readdir_cache_invalidation_on_create`: Cache invalidated when files added
- `test_readdir_cache_concurrent_readdir_consistency`: Concurrent readdirs from multiple clients show consistent entries (no stale)
- `test_readdir_cache_negative_entry_deleted_recreated`: Negative entries (non-existent files) properly invalidated when file recreated
- `test_readdir_cache_size_bounded`: Cache size never exceeds configured limit (no DoS via unlimited cache)
- `test_readdir_cache_ttl_expiration`: Expired cache entries refreshed on next access
- `test_readdir_cache_symlink_handling`: Symlinks in cached directories handled correctly

### Writeback Cache (8 tests):
- `test_writeback_cache_write_ordering_preserved`: Writes executed in order submitted
- `test_writeback_cache_fsync_flushes_all`: fsync() flushes all pending writes to storage
- `test_writeback_cache_no_data_loss_on_fsync`: Data visible on disk after fsync returns
- `test_writeback_cache_mmap_coherence`: Changes via mmap visible to read(); changes via write() visible to mmap
- `test_writeback_cache_crash_recovery_consistency`: After power loss, filesystem recovers consistently (no corruption)
- `test_writeback_cache_memory_pressure_eviction_safe`: Cache eviction under memory pressure doesn't lose data
- `test_writeback_cache_concurrent_write_flush`: Concurrent writes and flush don't race (flush sees all prior writes)
- `test_writeback_cache_partial_flush_safety`: Partial flush correctly syncs subset of writes

### Mmap (6 tests):
- `test_mmap_write_coherence`: Changes via write() visible to mmap; changes via mmap visible to read()
- `test_mmap_multiprocess_coherence`: mmap changes visible across processes
- `test_mmap_concurrent_region_access`: Concurrent mmap regions don't overlap or interfere
- `test_mmap_truncate_safety`: Truncate while mmap active doesn't crash or corrupt
- `test_mmap_anonymous_isolation`: Anonymous mmap doesn't leak data across processes
- `test_mmap_page_fault_during_unlink`: Page fault during concurrent unlink handled safely (no use-after-free)

### Otel Tracing Integration (5 tests):
- `test_otel_trace_id_uniqueness`: Every trace has unique TraceId (no collisions)
- `test_otel_span_parent_child_propagation`: Parent-child relationships correctly propagated across threads
- `test_otel_trace_context_no_leakage`: Trace context doesn't leak across security boundaries
- `test_otel_concurrent_span_recording`: Concurrent span recording without data races
- `test_otel_trace_export_batching_bounds`: Export batching respects memory bounds (no unbounded buffering)

### Distributed Session Manager (5 tests):
- `test_session_manager_affinity_consistency`: Client always routes to same metadata server (session affinity)
- `test_session_manager_expiry_enforcement`: Expired sessions can't be used (requests rejected)
- `test_session_manager_concurrent_ops`: Concurrent ops from same client ordered correctly
- `test_session_manager_tenant_isolation`: Session data from one tenant not visible to another
- `test_session_manager_failover_new_session`: When metadata node fails, client gets new session

### WORM Enforcement (2 tests):
- `test_worm_lock_enforced`: Write after WORM lock fails with EPERM
- `test_worm_concurrent_lock_transition`: Concurrent write attempts during WORM transition handled correctly

### Quota Client Tracker (2 tests):
- `test_quota_tracking_accuracy`: Quota usage accurately tracked per client
- `test_quota_violation_enforcement`: Requests rejected when quota exceeded

## Module 2 Test File Structure

```rust
#[cfg(test)]
mod fuse_cache_coherence_security_tests {
    use claudefs_fuse::readdir_cache::ReaddirCache;
    use claudefs_fuse::writeback_cache::WritebackCache;
    use claudefs_fuse::mmap::MmapTracker;
    use claudefs_fuse::otel_tracing_integration::TraceContext;
    use claudefs_fuse::distributed_session_manager::SessionManager;
    use claudefs_fuse::worm_enforcement::WormLock;
    use claudefs_fuse::quota_client_tracker::QuotaTracker;
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;

    // 35 test functions here

    #[test]
    fn test_readdir_cache_invalidation_on_mkdir() {
        // Test implementation
    }
    // ... more tests
}
```

---

## MODULE 3: Metadata Multi-Tenancy and Isolation Security Tests

**File:** `crates/claudefs-security/src/meta_multitenancy_isolation_security_tests.rs`

**Target Modules:** `concurrent_inode_ops`, `cross_shard`, `fingerprint_index_integration`, `hardlink`, `tenant_isolator`, `qos_coordinator`, `space_accounting`, `quota_tracker`, `lazy_delete` from `claudefs-meta` crate

**Task:** Generate 25 comprehensive security tests covering tenant isolation, concurrent inode operations, and resource accounting.

### Concurrent Inode Ops (5 tests):
- `test_concurrent_inode_create_delete_no_corruption`: Concurrent file create/delete without inode corruption
- `test_concurrent_hardlink_creation_link_count_accurate`: Concurrent hardlink creation maintains correct link count
- `test_concurrent_xattr_updates_isolation`: Concurrent xattr updates don't interfere between files
- `test_concurrent_timestamp_updates`: Concurrent mtime/atime/ctime updates don't lose updates
- `test_concurrent_permission_changes`: Concurrent chmod/chown updates safely without race conditions

### Cross Shard (4 tests):
- `test_cross_shard_move_atomicity`: Rename across shards is atomic (no partial failure)
- `test_cross_shard_directory_consistency`: Directory entries consistent across shards after move
- `test_cross_shard_link_count_updates`: Link count correctly updated in distributed scenario
- `test_cross_shard_deadlock_prevention`: Multi-shard operations don't deadlock (no circular lock waits)

### Fingerprint Index Integration (3 tests):
- `test_fingerprint_index_consistency_distributed_dedup`: Fingerprint index consistent across shards during dedup
- `test_fingerprint_concurrent_lookups_under_dedup`: Concurrent fingerprint lookups work during concurrent dedup
- `test_fingerprint_eviction_doesnt_break_refcount`: Fingerprint eviction doesn't break reference counting

### Hardlink (3 tests):
- `test_hardlink_count_accuracy_concurrent`: Concurrent link/unlink maintains accurate link count
- `test_hardlink_deletion_correctness`: File deleted when link count reaches zero
- `test_hardlink_cross_directory_safety`: Cross-directory hardlinks created/deleted safely

### Tenant Isolator (5 tests):
- `test_tenant_quota_no_spillover`: One tenant's quota enforcement doesn't affect another
- `test_tenant_concurrent_operations_no_interference`: Operations from different tenants don't interfere
- `test_tenant_data_isolation`: Tenant A can't read/write Tenant B's data
- `test_tenant_deletion_cleanup`: Tenant deletion cleans up all orphaned inodes
- `test_tenant_rate_limiting_qos`: QoS rate limiting enforced per tenant

### QoS Coordinator (3 tests):
- `test_qos_priority_enforcement`: RealtimeMeta > Interactive > Batch priority respected
- `test_qos_bandwidth_shaping`: Bandwidth limited per QoS class
- `test_qos_starvation_prevention`: Low-priority requests eventually execute

### Space Accounting (1 test):
- `test_space_accounting_accuracy`: Space usage correctly tracked; deleted files reclaim space

### Quota Tracker (1 test):
- `test_quota_enforcement_correctness`: Requests rejected when quota exceeded

## Module 3 Test File Structure

```rust
#[cfg(test)]
mod meta_multitenancy_isolation_security_tests {
    use claudefs_meta::concurrent_inode_ops::{InodeOps, InodeHandle};
    use claudefs_meta::tenant_isolator::TenantContext;
    use claudefs_meta::qos_coordinator::QosClass;
    use claudefs_meta::quota_tracker::QuotaTracker;
    use claudefs_meta::space_accounting::SpaceAccounting;
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;

    // 25 test functions here

    #[test]
    fn test_concurrent_inode_create_delete_no_corruption() {
        // Test implementation
    }
    // ... more tests
}
```

---

## MODULE 4: Protocol Fuzzing Infrastructure Security Tests

**File:** `crates/claudefs-security/src/protocol_fuzzing_infrastructure_security_tests.rs`

**Target:** Extends existing `fuzz_fuse.rs` with new fuzzing targets for RPC protocol, FUSE message parser, NFS gateway

**Task:** Generate 20 comprehensive fuzzing-infrastructure tests and additional fuzz targets.

### RPC Protocol Fuzzing Harness (8 tests):
- `test_rpc_malformed_message_no_panic`: Truncated/oversized RPC messages don't cause panic
- `test_rpc_invalid_type_handling`: Invalid enum variants handled gracefully (no panic)
- `test_rpc_buffer_overflow_prevention`: Oversized buffer fields rejected before processing
- `test_rpc_integer_overflow_detection`: Integer overflow in message size fields caught
- `test_rpc_message_validation_strictness`: Invalid messages rejected; valid messages accepted
- `test_rpc_fuzz_corpus_growth`: Fuzzing corpus grows with new interesting inputs
- `test_rpc_crash_deduplication`: Unique crashes identified correctly (not duplicated)
- `test_rpc_fuzzer_coverage_reporting`: Fuzzing engine reports code coverage metrics

### FUSE Message Parser Robustness (7 tests):
- `test_fuse_truncated_message_no_panic`: Truncated FUSE messages don't cause panic
- `test_fuse_oversized_array_rejection`: FUSE arrays larger than MAX_SIZE rejected
- `test_fuse_invalid_opcode_handling`: Invalid FUSE opcodes handled gracefully
- `test_fuse_zero_copy_buffer_safety`: Zero-copy buffer handling under fuzz (no use-after-free)
- `test_fuse_symlink_traversal_attacks`: Fuzzing with "../" sequences handled safely
- `test_fuse_permission_checking_fuzzed`: FUSE permission checks work correctly under fuzz
- `test_fuse_concurrent_message_processing`: Concurrent FUSE message processing under fuzz

### NFS Gateway Protocol Security (5 tests):
- `test_nfs_xdr_malformed_parsing_safe`: Malformed XDR doesn't cause panic
- `test_nfs_oversized_array_detection`: NFS arrays larger than reasonable limits rejected
- `test_nfs_handle_forgery_detection`: Random NFS handles not accepted as valid
- `test_nfs_export_validation_boundaries`: Export list validation respects boundaries
- `test_nfs_s3_bucket_name_validation`: S3 bucket names validated (special chars, length)

## Module 4 Test File Structure

```rust
#[cfg(test)]
mod protocol_fuzzing_infrastructure_security_tests {
    use claudefs_transport::rpc::RpcMessage;
    use claudefs_fuse::protocol::{FuseOp, FuseRequest};
    use claudefs_gateway::nfs::NfsHandle;
    use std::sync::{Arc, Mutex};

    // 20 test functions here

    #[test]
    fn test_rpc_malformed_message_no_panic() {
        // Test implementation
    }
    // ... more tests
}
```

---

## MODULE 5: Emerging Threats and Compliance Audit

**File:** `crates/claudefs-security/src/emerging_threats_compliance_security_tests.rs`

**Target:** Cross-cutting security audit of new threat models and compliance requirements

**Task:** Generate 15 comprehensive compliance and threat-model tests.

### Supply Chain Security (3 tests):
- `test_dependency_lock_integrity`: cargo.lock file integrity check (no unauthorized changes)
- `test_transitive_dependency_vulnerability_scan`: Recursive dependency CVE scanning works
- `test_unsigned_dependency_detection`: Detects builds from untrusted sources (no unsigned binaries)

### Metadata Service Byzantine Tolerance (4 tests):
- `test_split_brain_detection_and_resolution`: Split-brain scenario detected and resolved correctly
- `test_byzantine_fault_tolerance_bounds`: System tolerates up to 1 compromised metadata node (f=1, 3 nodes)
- `test_consensus_log_integrity`: Leader can't inject false entries into log
- `test_follower_read_validation`: Followers can't serve stale reads

### Encryption Key Lifecycle (3 tests):
- `test_key_rotation_correctness`: Old keys still decrypt old data after rotation
- `test_key_derivation_determinism`: Same input produces same derived key
- `test_key_material_not_in_logs`: Key material never appears in debug output or metrics

### Audit Logging & Forensics (3 tests):
- `test_security_events_logged`: Auth failures, key rotations, CRL updates logged
- `test_audit_log_immutability`: Can't tamper with historical audit entries
- `test_correlation_ids_for_tracing`: Correlation IDs enable tracing audit chains

### Rate Limiting & Brute-Force Prevention (2 tests):
- `test_management_api_rate_limiting`: Management API rate limiting prevents credential bruteforce
- `test_enrollment_endpoint_rate_limiting`: Enrollment endpoint rate limiting prevents spam

## Module 5 Test File Structure

```rust
#[cfg(test)]
mod emerging_threats_compliance_security_tests {
    use std::path::Path;
    use std::fs;

    // 15 test functions here

    #[test]
    fn test_dependency_lock_integrity() {
        // Test implementation
    }
    // ... more tests
}
```

---

## IMPLEMENTATION REQUIREMENTS

1. **Existing Test Patterns:** Reference these Phase 35 test files for patterns and conventions:
   - `crates/claudefs-security/src/storage_io_depth_limiter_security_tests.rs` (35 tests, excellent model)
   - `crates/claudefs-security/src/meta_client_session_security_tests.rs` (38 tests, multi-tenancy patterns)

2. **Module Dependencies:** Import from actual crates (already compiled):
   - `claudefs_storage::*` for Module 1
   - `claudefs_fuse::*` for Module 2
   - `claudefs_meta::*` for Module 3
   - `claudefs_transport::*`, `claudefs_gateway::*` for Module 4

3. **Test Framework:** Use standard Rust testing framework:
   ```rust
   #[tokio::test]  // for async tests
   #[test]         // for sync tests
   ```

4. **Assertion Patterns:**
   - Use `assert!()`, `assert_eq!()`, `assert_ne!()` for basic assertions
   - Use `panic!()` for unrecoverable errors in test setup
   - Use `.unwrap()` for expected-safe operations
   - Properly handle `Result<T, E>` types

5. **Concurrency Testing:**
   - Use `std::thread::spawn()` for thread-based tests
   - Use `tokio::task::spawn()` for async tests
   - Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state
   - Use `std::sync::Barrier` for synchronization points

6. **Code Quality:**
   - No clippy warnings
   - No panics in production code (panics only in tests)
   - All tests passing with `cargo test --release`

---

## DELIVERABLES

1. `storage_background_subsystems_security_tests.rs` (30 tests) — 1200-1500 lines
2. `fuse_cache_coherence_security_tests.rs` (35 tests) — 1400-1700 lines
3. `meta_multitenancy_isolation_security_tests.rs` (25 tests) — 1000-1300 lines
4. `protocol_fuzzing_infrastructure_security_tests.rs` (20 tests) — 800-1100 lines
5. `emerging_threats_compliance_security_tests.rs` (15 tests) — 600-900 lines

**Total: 125 tests, 5000-6600 lines of test code**

Update `crates/claudefs-security/src/lib.rs` to include:
```rust
pub mod storage_background_subsystems_security_tests;
pub mod fuse_cache_coherence_security_tests;
pub mod meta_multitenancy_isolation_security_tests;
pub mod protocol_fuzzing_infrastructure_security_tests;
pub mod emerging_threats_compliance_security_tests;
```

---

## TESTING VALIDATION

After code generation:

```bash
cd /home/cfs/claudefs
cargo test --release -p claudefs-security --lib 2>&1 | tail -20
```

All tests must pass with 0 errors.

---

## REFERENCE: Phase 35 Test Counts

Phase 35 delivered:
- storage_io_depth_limiter: 35 tests
- storage_command_queueing: 32 tests
- meta_client_session: 38 tests
- transport_trace_aggregator: 28 tests
- transport_bandwidth_shaper: 30 tests
- **Total Phase 35: 163 tests** (✅ Merged)

Phase 36 Target: 125 tests (total project: 2383 + 125 = **2508 tests**)

