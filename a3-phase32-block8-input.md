# A3: Phase 32 Block 8 — Disaster Recovery
## OpenCode Implementation Prompt

**Agent:** A3 (Data Reduction)
**Date:** 2026-04-18
**Task:** Implement 10-14 disaster recovery and recovery procedures tests
**Target File:** `crates/claudefs-reduce/tests/cluster_disaster_recovery.rs`
**Target LOC:** 650-800
**Target Tests:** 10-14

---

## High-Level Specs

### Block 8 Purpose
Validate **disaster recovery procedures** and recovery objectives (RPO/RTO): metadata backup/restore, point-in-time recovery, site failover, data integrity after recovery, and operational runbook accuracy.

### Key Tests
1. `test_cluster_dr_metadata_backup_and_restore` — Backup metadata, restore from backup, verify consistency
2. `test_cluster_dr_s3_backup_integrity` — S3 backup complete, checksums match
3. `test_cluster_dr_point_in_time_recovery` — Recover to specific point in time
4. `test_cluster_dr_site_a_complete_failure` — Site A down, Site B takes over (cross-site replication)
5. `test_cluster_dr_cross_site_replication_lag_recovery` — Replication lag > RTO, data loss assessment
6. `test_cluster_dr_metadata_shard_loss_recovery` — Metadata shard lost, recovered from replica
7. `test_cluster_dr_s3_bucket_loss_recovery` — S3 bucket unavailable, fallback tier
8. `test_cluster_dr_client_snapshot_recovery` — Client data restored from snapshot
9. `test_cluster_dr_cascading_failures_recovery` — Multiple failures, recovery order validation
10. `test_cluster_dr_rpo_rto_metrics_measured` — RPO <10min, RTO <30min verified
11. `test_cluster_dr_recovery_performance_degradation` — Recovery performance acceptable
12. `test_cluster_dr_data_integrity_after_recovery` — No data loss, no corruption post-recovery
13. `test_cluster_dr_automated_failover_trigger` — Automatic failover on detection
14. `test_cluster_dr_runbooks_documented_and_tested` — Runbooks exist, tested

### Prerequisites
- Block 1 ✅ (cluster health)
- Block 2 ✅ (single-node dedup)
- Block 3 ✅ (multi-node coordination)
- Block 6 ✅ (chaos, failure scenarios)

### Helper Functions to Create
- `backup_metadata() -> Result<BackupId, String>` — Create backup, return ID
- `restore_metadata(backup_id: &str) -> Result<(), String>` — Restore from backup
- `verify_backup_integrity(backup_id: &str) -> Result<(), String>` — Verify checksums
- `get_point_in_time_snapshots() -> Result<Vec<TimestampSnapshot>, String>` — List available snapshots
- `restore_to_point_in_time(timestamp: u64) -> Result<(), String>` — Restore to specific time
- `trigger_site_failover(from_site: &str, to_site: &str) -> Result<(), String>` — Manual failover
- `verify_data_integrity_after_restore(files: &[&str]) -> Result<(), String>` — Checksum verify
- `measure_rpo_rto() -> Result<(Duration, Duration), String>` — Return (RPO, RTO)
- `get_recovery_runbook(scenario: &str) -> Result<String, String>` — Get runbook text

### Error Handling
- Use `Result<(), String>` for tests
- Backup/restore timeouts: 15 minutes
- Recovery verification: must complete successfully
- Data integrity: 100% (assert no loss)

### Assertions
- RPO: <10 minutes (acceptable data loss)
- RTO: <30 minutes (acceptable downtime per node)
- Data integrity: 100% (zero data loss, zero corruption)
- Failover completion: <5 minutes
- Runbooks exist and are accurate
- Performance degradation during recovery: <50%

### Test Execution
- Depends on Block 1-6 passing
- Sequential execution (recovery scenarios are time-intensive)
- Total runtime target: 30-45 minutes (includes backup, restore, failover)

---

## Full Implementation Details

See `a3-phase32-blocks-3-8-plan.md` section "Block 8: Disaster Recovery (10-14 tests)" for complete specifications including:
- Detailed DR implementations for all 14 tests
- Backup strategies (metadata, S3 chunks, journals)
- Point-in-time recovery mechanisms
- Cross-site failover procedures
- Recovery verification (checksums, refcounts, integrity)
- RPO/RTO targets and measurement
- Operational runbooks (automated + manual)

---

## Success Criteria

✅ All 10-14 tests compile without errors
✅ All tests marked `#[ignore]` (require real cluster with cross-site setup)
✅ Zero clippy warnings in new code
✅ All recovery procedures complete successfully
✅ RPO <10 minutes verified
✅ RTO <30 minutes verified
✅ Data integrity 100% (zero loss, zero corruption)
✅ All runbooks exist and tested
✅ Phase 32 complete! (88-120 tests across 8 blocks)

---

## Output Specification

Generate a complete, production-ready Rust test file:
- Use existing helper functions from cluster_helpers.rs where applicable
- Create new helpers as specified above
- All tests marked `#[ignore]`
- Follow conventions from other cluster tests
- Use `std::time::Instant` for timing measurements
- Store runbook content as string constants or external files
- Result<(), String> for error handling
- Comprehensive assertions with thresholds

Expected line count: 650-800 LOC
Expected test count: 10-14

---

## Success Criteria for Phase 32 Completion

**Phase 32 Goals:**
- ✅ Block 1: Cluster setup (12-15 tests)
- ✅ Block 2: Single-node dedup (14-18 tests)
- ✅ Block 3: Multi-node coordination (16-20 tests)
- ✅ Block 4: Tiering with S3 (12-16 tests)
- ✅ Block 5: Multi-client workloads (14-18 tests)
- ✅ Block 6: Chaos & resilience (16-20 tests)
- ✅ Block 7: Performance benchmarks (10-14 tests)
- ✅ Block 8: Disaster recovery (10-14 tests)

**Total Phase 32:**
- 88-120 real cluster integration tests ✅
- ~5,000-6,000 LOC test code ✅
- Production readiness: 95%+ ✅
- Baseline (Phase 31): 2,284 tests
- Final (Phase 32): 2,370-2,400 tests ✅

---

## Runbooks to Document

1. **Metadata Backup Procedure** (automated + manual steps)
2. **Site Failover Procedure** (automatic trigger + manual override)
3. **Point-in-Time Recovery Procedure** (select timestamp, restore, verify)
4. **S3 Bucket Loss Procedure** (fallback tier, tiering policy, cleanup)
5. **Cascading Failure Recovery** (node recovery ordering, consistency checks)
6. **Client Data Recovery** (from snapshots, verification, validation)
