# A3 Phase 30: Integration Testing & Production Hardening

**Phase Goal:** Integrate data reduction pipeline with other subsystems and add comprehensive integration tests. Target: 50-80 new tests, reaching 2120-2150 total.

**Current State:** 99 modules, 2071 tests passing. Phase 27-28 complete, Phase 29 maintenance done.

---

## Integration Context

- **A2 Metadata Service:** Inode operations, fingerprint index, quota tracking
- **A4 Transport:** RPC calls to dedup coordinator, bandwidth shaping
- **A5 FUSE Client:** Write path integration, client-side hints for tiering
- **A8 Management:** Prometheus metrics export, DuckDB schema for reduction stats

---

## Phase 30: Four Integration Test Blocks

### Block 1: Write Path Integration Tests (~18 tests)

**Purpose:** End-to-end integration testing of chunking → dedup → compression → encryption → storage.

**Coverage:**
- Write flow with all optional stages enabled (chunking + dedup + compress + encrypt)
- Distributed dedup coordination via `dedup_coordinator` + `stripe_coordinator`
- Segment packing and EC encoding via `erasure_codec`
- Tiering decisions via `tiering_advisor` under different workload patterns
- Quota enforcement via `multi_tenant_quotas` coordinating with A2
- QoS shaping via `bandwidth_throttle` coordinating with A4

**Test Examples:**
- `test_write_path_with_all_stages_enabled` — full pipeline
- `test_distributed_dedup_coordination` — multi-node fingerprint routing
- `test_tiering_advisor_recommendations` — hot/cold/archive decisions
- `test_quota_enforcement_multi_tenant` — tenant isolation under heavy load
- `test_bandwidth_throttle_under_load` — QoS enforcement
- `test_segment_packing_and_ec` — 2MB segment packing with 4+2 EC
- `test_write_amplification_tracking` — amplification ratio consistency

**Modules Tested:**
- `write_path.rs` — main integration point
- `dedup_coordinator.rs` — distributed coordination
- `stripe_coordinator.rs` — EC stripe placement
- `multi_tenant_quotas.rs` — quota coordination
- `bandwidth_throttle.rs` — QoS shaping
- `tiering_advisor.rs` — policy decisions

### Block 2: Read Path & Recovery Integration Tests (~16 tests)

**Purpose:** End-to-end read path, crash recovery, and consistency verification.

**Coverage:**
- Read flow with decompression, decryption, validation
- Segment reconstruction from EC stripes
- Crash recovery and incomplete operation detection
- Journal replay for write consistency
- Recovery from corrupted chunks (checksum verification)
- Reference counting consistency under concurrent operations

**Test Examples:**
- `test_read_path_full_pipeline` — decrypt → decompress → reassemble
- `test_ec_reconstruction_from_stripes` — read with 1-2 missing stripes
- `test_crash_recovery_incomplete_dedup` — recovery from partial dedup operation
- `test_journal_replay_consistency` — journal replay produces same state as live
- `test_checksum_verification_detects_corruption` — silent corruption detection
- `test_reference_counting_under_concurrent_ops` — refcount table consistency
- `test_gc_coordination_with_refcount` — GC waves respect refcount

**Modules Tested:**
- `read_path.rs` (if exists) or via `segment_reader.rs`
- `segment_reader.rs` — segment reading
- `erasure_codec.rs` — EC reconstruction
- `recovery_enhancer.rs` — crash recovery
- `journal_replay.rs` — write journal replay
- `checksum_store.rs` — data integrity
- `refcount_table.rs` — reference counting
- `gc_coordinator.rs` — garbage collection

### Block 3: Tier Migration & Lifecycle Integration Tests (~16 tests)

**Purpose:** S3 tiering, snapshot lifecycle, and data lifecycle management.

**Coverage:**
- Eviction policy under capacity pressure (D5 high/critical watermarks)
- S3 tiering with object assembler (64MB blobs)
- Snapshot lifecycle: creation, replication, aging, S3 tiering
- Delta compression for similar chunks (Tier 2)
- WORM compliance and retention enforcement
- Key rotation without full re-encryption

**Test Examples:**
- `test_eviction_policy_high_watermark_triggers` — flash at 80% evicts
- `test_eviction_policy_low_watermark_stops` — eviction stops at 60%
- `test_s3_blob_assembly_64mb_chunks` — 64MB blob packing
- `test_snapshot_creation_and_lifecycle` — snapshot → age → archive
- `test_delta_compression_similarity_detection` — Tier 2 pipeline
- `test_worm_retention_policy_enforcement` — immutable block enforcement
- `test_key_rotation_without_full_re_encryption` — envelope encryption rotation
- `test_tier_migration_consistency` — data integrity through tiering

**Modules Tested:**
- `eviction_policy.rs` — high/critical watermark handling
- `object_assembler.rs` — 64MB blob packing
- `snapshot.rs` + `snapshot_catalog.rs` — lifecycle
- `tier_migration.rs` — data movement
- `similarity_coordinator.rs` + `delta_index.rs` — Tier 2
- `worm_retention_enforcer.rs` — WORM compliance
- `key_rotation_orchestrator.rs` — key rotation

### Block 4: Performance & Consistency Integration Tests (~14 tests)

**Purpose:** Performance under various workloads and cross-system consistency.

**Coverage:**
- Performance metrics tracking across pipeline stages
- Write amplification under dedup + compression + EC
- Read amplification from EC reconstruction
- Consistency between replica instances (cross-site)
- Metrics export to Prometheus (A8 coordination)
- Tenant isolation performance (noisy neighbor test)

**Test Examples:**
- `test_write_amplification_ratio_tracking` — amplification consistency
- `test_read_amplification_from_ec_reconstruction` — amplification tracking
- `test_metrics_export_prometheus_compatible` — metrics format
- `test_tenant_isolation_performance_noisy_neighbor` — QoS isolation
- `test_similarity_detection_performance` — Tier 2 latency SLO
- `test_pipeline_backpressure_under_memory_pressure` — memory safety
- `test_pipeline_monitor_alert_thresholds` — alerting integration

**Modules Tested:**
- `pipeline_monitor.rs` — stage metrics and alerts
- `write_amplification.rs` — amplification tracking
- `read_amplification.rs` — read amplification
- `metrics.rs` — Prometheus export
- `similarity_tier_stats.rs` — Tier 2 performance
- `pipeline_backpressure.rs` — memory control
- `segment_pressure.rs` — flash pressure tracking

---

## Success Criteria

**All Blocks:**
- ✅ 50-80 new tests (18 + 16 + 16 + 14 = 64 tests)
- ✅ 2135 total tests passing (2071 + 64)
- ✅ 100% coverage of integration points with A2, A4, A5, A8
- ✅ Zero clippy warnings on new test code
- ✅ All tests deterministic and reproducible (no timing issues)
- ✅ Documentation of integration APIs with other subsystems

---

## Implementation Notes

1. **Write Path Integration:** Most critical — exercises entire pipeline.
2. **Read Path & Recovery:** Ensures durability and consistency guarantees.
3. **Tier Migration:** Validates S3 tiering decision logic and object assembly.
4. **Performance & Consistency:** Validates metrics and cross-system behavior.

---

## Delivery

- OpenCode prompt: `a3-phase30-input.md` (to be written after approval)
- Output: 5 new integration test files in `crates/claudefs-reduce/tests/`
- Commit: `[A3] Phase 30: Integration Testing & Production Hardening — 2135 tests`

---

## Next Phase (Phase 31)

- Benchmarking suite (FIO integration)
- Security hardening (fuzzing of reduction pipeline)
- Multi-node cluster testing (with A9, A10, A11)
